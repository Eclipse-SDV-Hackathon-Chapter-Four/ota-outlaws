<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Battery Thermal Guardian

The Battery Thermal Guardian evaluates battery temperatures and the trustworthiness
of their input, then publishes thermal, monitoring, fault, and mitigation events.
The [HARA](../hara.md) is authoritative for the item boundary, hazards, safety goals,
faults, and derived requirements. The [Safety Concept](../../explanation/safety-concept.md)
records the current implementation status. Where it conflicts with the HARA, this
design follows the HARA and records the implementation as a gap rather than changing
the safety requirement.

## Core and adapters

The Guardian is split into a **core** that contains all safety logic and
**adapters** that connect it to the outside world.

```text
                 ┌──────────────────────── Guardian process ───────────────────────┐
 uProtocol  ───► │ input adapter ──► Sample ──►┐                                    │
 (Zenoh)         │                             │   ┌──────────── core ──────────┐   │
 timer      ───► │ tick ──────────────────────►├──►│ detectors + state machine  │   │
                 │                             │   │ no I/O, no clock, no async │   │
                 │                             │   └─────────────┬──────────────┘   │
                 │                             │              Events                │
                 │ output adapter ◄──────────────────────────────┤                  │
                 │ DFM adapter    ◄──────────────────────────────┘                  │
                 └───────┬──────────────────┬─────────────────────────────────────┘
                         ▼                  ▼
                  uProtocol topics      DFM → OpenSOVD
```

| Part | Status | Responsibility |
|------|--------|----------------|
| Core ([`guardian`](../../../components/guardian)) | **Implemented** | Detectors, thermal state machine, monitoring status, events |
| Input adapter ([`guardian-service`](../../../components/guardian-service)) | **Implemented** | Subscribe to `BatteryTemperature` over uProtocol, decode the Protobuf payload, call the core |
| Tick ([`guardian-service`](../../../components/guardian-service)) | **Implemented** | Call the core every 50 ms, so that missing samples are detected |
| Output adapter ([`guardian-service`](../../../components/guardian-service)) | **Implemented** | Publish every core event as a `GuardianEvent` over uProtocol |
| DFM adapter | **Implemented / tested** | Write Guardian fault events to the DFM and expose them through OpenSOVD (FSR-D.1, FSR-D.2) |
| Independent Guardian supervisor | **Missing** | Detect termination or loss of evaluation progress and request a monitoring-unavailable warning independently of the Guardian (HARA DFR-5) |

The HARA item boundary includes Guardian evaluation and its output requests. The
physical sensor, CAN decoding, KUKSA components, occupant interface, and physical
mitigation actuators are external unless system-level architecture allocates them
to the item. A `MitigationRequested` event is therefore a request, not proof that
an occupant was warned or an actuator acted.

The messages and topics are defined in the
[Battery Thermal Contract](../../../components/contracts/README.md). A payload the Guardian
cannot decode is rejected before it reaches the core; if only such payloads
arrive, the core reports the loss of fresh data. An unknown quality value is
passed to the core as `UNDEFINED`, so the core reports it instead of using the
value.

## Diagnostics and SOVD interfaces

The Guardian does not call an OpenSOVD HTTP endpoint. Its diagnostics adapter
maps Guardian events to DTC lifecycle records and publishes them through the
local `fault_lib` Reporter/DFM interface. The DFM and OpenSOVD gateway expose
those records; the Campaign Tool's Evidence Collector reads them over HTTP (see
[Evidence Collector to OpenSOVD](campaign.md#evidence-collector-to-opensovd)).
Diagnostic delivery is asynchronous and does not gate thermal evaluation or
mitigation requests.

### Guardian to DFM

The DFM catalog entity is configured by `SOVD_ENTITY` (default:
`battery_guardian`) and must match the catalog ID. Each catalog DTC is initialized
as `NotTested`. Guardian events then map to DTC lifecycle updates:

| Guardian event | DFM lifecycle update |
|---|---|
| `FaultDetected` | Mark the matching input-fault DTC `Failed`. |
| `FaultRecovered` or `FaultTestPassed` | Mark the matching input-fault DTC `Passed`. |
| Thermal transition into `WARNING` or `CRITICAL` from a real temperature sample | Mark the corresponding overtemperature DTC `Failed`. A warning caused by an invalid sample does not create an overtemperature DTC. |
| Thermal transition below a previously failed warning/critical level | Mark that overtemperature DTC `Passed`. |

Each record carries environment data for correlation: `session_id`, `event_id`,
`guardian_time_ms`, detecting `requirement`, catalog `fault_type` and `severity`,
and, when available, the sample sequence, source timestamp, and alive counter.
The DFM reporter publishes these records locally; there is no Guardian-side
SOVD write endpoint.

### Why this split

- **Deterministic.** The core never reads a clock. The caller passes the current
  time into every call. The same inputs always produce the same events, so a
  recorded input stream can be replayed against the core and must give the same
  verdict. This supports the challenge's requirement for replayable campaigns.
- **Testable without infrastructure.** Every requirement can be tested in
  milliseconds, without containers, brokers, or a network.
- **Portable.** The core knows nothing about uProtocol, KUKSA, or CAN. The
  architecture rule of the challenge ("Guardian should not be tightly coupled to
  direct Data Broker reads") holds by construction.

## Core interface

```rust
let mut guardian = Guardian::new(&config);
let events = guardian.on_sample(sample, Millis(now)); // for every received sample
let events = guardian.on_tick(Millis(now));            // every 50 ms
```

- **`Sample`**: maximum, average, and minimum cell temperature, the source
  timestamp and sequence number from the VSS Publisher (assumption A-1), and the
  alive counter and quality flag of the CAN frame (assumption A-3).
- **`Millis`**: the Guardian's local monotonic time. The core only compares local
  times with local times, and source timestamps with source timestamps.
- **`Event`**: what the adapters publish. Every event has an ID and the ID of
  the event that caused it, which forms the evidence chain:

```text
FaultDetected ──cause──► MonitoringStatusChanged ──cause──► MitigationRequested
ThermalStateChanged (trigger: sample) ──cause──► MitigationRequested
```

## Detectors

| Detector | Requirement | Rule |
|----------|-------------|------|
| Freshness monitor | FSR-2.2, FSR-2.3, FSR-2.8; HARA DFR-7, DFR-8 | Do not evaluate samples that are not fresh. A frame with the alive counter of the last fresh sample and a source timestamp that is not older (a frozen source or an exact duplicate) counts as a repeated frame. Per DFR-7, two messages with the same counter (`N_suspect` = 1 repeat) set `SUSPECT`, and ten (`N_stuck` = 9 repeats) report counter-stuck and set `DEGRADED`. A sample with an older or equal source timestamp and another counter is out of order and sets `SUSPECT` at once. After `T_stale` without fresh data, report freshness loss. `SUSPECT` returns to `OK` only after `N_recover` (10) consecutive fresh, valid samples. FSR-2.8 (`T_age`, synchronized clocks) is not implemented. Checked on each sample and 50 ms tick. |
| Quality check | FSR-3.4 | A fresh sample whose quality is not `VALID` is discarded, reported, and sets monitoring to `DEGRADED` without debounce. The sample indicates source activity but cannot support thermal assessment. |
| Stuck detector | FSR-2.4 | If maximum temperature stays unchanged for `T_stuck` while average or minimum moves by at least `Δ_stuck`, report a stuck-signal fault and set monitoring to `DEGRADED`. It does not detect all temperature channels frozen together. |
| Order and range checks | FSR-3.1, FSR-3.2 | Reject a sample that violates `Min ≤ Avg ≤ Max` or lies outside `[θ_min, θ_max]`; report the fault and set monitoring to `DEGRADED`. An above-range maximum also raises thermal state to at least `WARNING`. |
| Rate plausibility | FSR-3.3, FSR-3.5, FSR-3.6; HARA DFR-4 | Discard a rate-implausible sample and raise thermal state to at least `WARNING`, because the rise could be real. An isolated spike sets `SUSPECT` only, without a fault or monitoring-unavailable warning (TS-20). `N_suspect` (3) spikes within `T_suspect` (1 s) report the rate fault and set `DEGRADED` (TS-21). Invalid spikes alone never cause `CRITICAL` or overtemperature mitigation. |
| Thermal thresholds and trends | FSR-1.1 to FSR-1.4 | On valid samples, threshold criteria raise the state to `WARNING` or `CRITICAL`. A valid maximum that rises by at least `r_trend` (1 °C/s) on average over `T_trend` (5 s) raises `WARNING` below `θ_warn` and keeps it while the trend holds (FSR-1.3, TS-25). The rate uses source timestamps over the actual span, so a data gap cannot fake a trend. The hot-spot criterion (FSR-1.4) is not implemented. Invalid samples do not lower the thermal state. |
| Independent supervision | HARA DFR-5 | An independent in-vehicle supervisor must detect Guardian termination or evaluation hang and request the defined monitoring-unavailable warning. This supervisor is not implemented; the Evidence Collector and runtime restart do not satisfy this occupant-protection requirement. |

The Guardian detects faults, but does not find out what caused them. Telling a
source fault from a transport fault, and diagnosing duplicated or reordered
messages, is the Evidence Collector's job (EC-1, EC-2). See
[Responsibilities](../architecture.md#responsibilities-guardian-and-evidence-collector).

Samples that are not fresh are never evaluated; duplicates and out-of-order
samples only move monitoring to `SUSPECT` or, if they persist, `DEGRADED`. If no
fresh sample then arrives for `T_stale`, the freshness monitor reports loss of
monitoring. Fault events
carry event and cause IDs for diagnostic correlation. `DEGRADED` requests
`DRIVER_WARNING_MONITORING_UNAVAILABLE`; this degraded response is distinct from
the overtemperature mitigation `DRIVER_WARNING_OVERTEMP`. Per HARA DFR-4,
invalid data must never cause the latter or lower an active thermal state.

## State and transition events

Thermal state describes battery danger; monitoring status describes input
trustworthiness. They are independent, so a monitoring fault does not erase an
existing thermal warning or critical state. Their transitions are published as
different event kinds.

### Monitoring status

`MonitoringStatusChanged` carries the previous and current status. A transition
to `DEGRADED` also reports the detected fault and requests the monitoring-
unavailable warning.

| Status | Meaning and transition condition | Event / response | Implementation status |
|---|---|---|---|
| `OK` | Input is fresh, in order, and plausible. | Published as the `current` status when monitoring returns to healthy operation. | Implemented. |
| `SUSPECT` | Repeated frames or duplicates (`N_suspect`), an out-of-order sample, or an isolated spike have been discarded; debounce has not reached the degraded threshold. Returns to `OK` after `N_recover` consecutive fresh, valid samples (DFR-8). | `MonitoringStatusChanged`; no mitigation. | Implemented (FSR-2.3, FSR-3.5, DFR-7, DFR-8, TS-07, TS-08, TS-20). |
| `DEGRADED` | Fresh data is lost or input is invalid/persistently faulty, so temperature cannot be assessed reliably. | `FaultDetected` and `MonitoringStatusChanged`; request `DRIVER_WARNING_MONITORING_UNAVAILABLE`. | Implemented for freshness, counter-stuck (repeated frames and duplicates), quality, order, range, stuck-signal, and repeated-spike checks. Independent Guardian supervision is the watchdog's job (HARA DFR-5). |

### Thermal state

`ThermalStateChanged` carries the previous and current state and the sample that
caused the transition. `CRITICAL` and `MITIGATING` have equal severity; the
latter records that a mitigation request has been made.

| State | Meaning and transition condition | Event / response | Implementation status |
|---|---|---|---|
| `CLEAR` | Initial state before a valid temperature stream has established monitoring. It does not mean the battery is proven safe. | Initial state; no mitigation. | Implemented. |
| `MONITORING` | Valid input is available and no thermal warning or critical criterion is met. | `ThermalStateChanged`; no mitigation. | Implemented. |
| `WARNING` | A warning threshold, trend, or hot-spot criterion is met, or invalid high input could indicate real danger. | `ThermalStateChanged`; no overtemperature mitigation is currently requested at this level. | Threshold and trend behavior implemented; hot-spot behavior planned. |
| `CRITICAL` | A valid critical criterion is met. Invalid input alone must not cause this state. | `ThermalStateChanged`; request `DRIVER_WARNING_OVERTEMP`. | Implemented for the critical temperature threshold. |
| `MITIGATING` | The overtemperature mitigation request has been published; severity remains equal to `CRITICAL`. | `ThermalStateChanged`; no distinct mitigation value beyond the critical request. | Defined by HARA/Safety Concept FSR-1.6; not currently reachable because FSR-1.6 is planned. |

An isolated rate-implausible spike raises `WARNING` with the `SUSPECT` status
change as its cause, so the evidence chain shows the warning comes from
untrusted input. It never causes `CRITICAL` or `DRIVER_WARNING_OVERTEMP`.
Repeated spikes escalate monitoring to `DEGRADED`; their `WARNING` is caused by
the rate fault. This resolves the FSR-3.3 / FSR-3.5 conflict in favor of the
HARA (DFR-4, TS-20, TS-21): FSR-3.3's immediate `DEGRADED` applies only once
the spike debounce is exceeded.

## HARA fault allocation

The HARA fault IDs and test cases remain authoritative. Status below follows the
Safety Concept; a test or design entry does not imply that a planned requirement
is implemented.

| HARA fault | Design response and current status | HARA verification |
|---|---|---|
| F-1: Temperature value frozen while messages continue | FSR-2.4 detects a frozen maximum only when average or minimum changes. All temperature channels frozen together are not detectable from current inputs. | TS-10 is a limitation probe; it does not demonstrate detection. |
| F-2: Message arrives after its allowed age/deadline | FSR-2.2 detects a freshness gap. FSR-2.8 rejects over-age timestamps only with synchronized clocks and is planned. | TS-05 tests a prolonged gap; proposed TS-24 covers explicit late arrival. |
| F-3: Same message delivered more than once | Duplicates are not evaluated and count as repeated frames: the first duplicate sets `SUSPECT` without mitigation, the tenth message with the same counter reports counter-stuck and sets `DEGRADED` (FSR-2.3, DFR-7). Collector-side duplicate attribution (EC-2) is planned. | TS-07. |
| F-4: Expected update dropped before reaching Guardian | FSR-2.2 reports freshness loss and requests the degraded response. | TS-05, TS-06, TS-15. |
| F-5: Messages arrive out-of-order | A sample with an older source timestamp is not evaluated and sets `SUSPECT`; recovery needs `N_recover` fresh samples (DFR-8). Collector attribution is planned under EC-2. | TS-08. |
| F-6: Isolated temperature sample outside configured interval | FSR-3.2 rejects the sample and sets monitoring to `DEGRADED`; high out-of-range input also keeps thermal state at least `WARNING`. | TS-12, TS-17, TS-18. |
| F-7: Temperature drifts over time | FSR-1.3 raises `WARNING` for a sustained upward trend below `θ_warn`; FSR-1.4 (hot spot) is planned. Aggregate signals cannot identify every sensor drift, and a downward drift is not detected. | TS-25 tests a rising thermal trend, not sensor-bias detection. |
| F-8: Isolated rate-implausible spike | FSR-3.3/3.5 discard the sample, set `SUSPECT`, and raise at least `WARNING`; no fault, no mitigation. | TS-19, TS-20. |
| F-9: Source disconnect or replay stops | FSR-2.2 reports freshness loss and requests the degraded response. | TS-03, TS-04, TS-15. |
| F-10: Guardian terminates or evaluation hangs | FSR-2.7 observes heartbeat loss and restarts a terminated process; independent occupant warning and verified hang detection are missing per DFR-5. | TS-22, TS-23; these do not prove occupant protection. |
| F-11: Source marks fresh sample invalid or unavailable | FSR-3.4 rejects the sample, sets `DEGRADED`, and reports the quality fault without debounce. | TS-11, TS-16. |
| F-12: Temperature saturates at 255 °C | FSR-3.2 treats it as high out-of-range and requires at least `WARNING`; no separate saturation diagnosis is available. | TS-12 is generic high-range coverage; TS-26 uses 255 °C explicitly (core test). |
| F-13: Repeated rate-implausible spikes | `N_suspect` spikes within `T_suspect` report the rate fault, set `DEGRADED`, and request the monitoring-unavailable warning; no overtemperature mitigation (FSR-3.5, DFR-4). | TS-21. |

## Timing and recovery

The core receives local monotonic time from the service adapter; it does not
read a wall clock. The service calls the core on each sample and every 50 ms.
`T_react` is 100 ms from the HARA and measures response after a condition is
observable. It is not a validated battery warning lead time. The warning
thresholds and their time-to-hazard basis still require battery-level validation.

Freshness uses source timestamps and the alive counter. FSR-2.8 (`T_age` with
synchronized clocks) is not implemented, so the Guardian can detect gaps but
cannot detect a constant transport delay. Thermal state is lowered only while
monitoring is `OK`. Recovery from `DEGRADED` requires `N_recover` consecutive
fresh, valid samples spanning `T_recover`, with each active fault cleared,
followed by the thermal hysteresis rules in FSR-1.5. Recovery from `SUSPECT`
requires `N_recover` consecutive fresh, valid samples (DFR-8).

## Configuration

The safety parameters are in
[`config/guardian/safety-params.toml`](../../../config/guardian/safety-params.toml).
The core rejects a configuration that would silently disable a safety mechanism,
for example a warning threshold that is not below the critical threshold, or a
timeout of zero. Unknown parameters are rejected as well, so a typo cannot
switch a check off.

The requirement tests load the shipped file, so CI also verifies the
configuration.

## Status

The table under [Core and adapters](#core-and-adapters) shows which parts exist.
Which requirements are implemented is recorded in the
[HARA fault allocation](#hara-fault-allocation) above and in the requirement
tests, [`components/guardian/tests/requirements.rs`](../../../components/guardian/tests/requirements.rs).

## Verification strategy

The HARA owns the detailed scenario stimuli and expected results. This design
uses those scenarios to verify behavior through the uProtocol input and output,
and, for diagnostic requirements, DFM/OpenSOVD. Record the active configuration,
Guardian input, state/status and fault events, mitigation requests, diagnostics,
and response latency for each applicable run.

| HARA tests | Verification focus | Status / limitation |
|---|---|---|
| TS-01, TS-02, TS-13, TS-14 | Nominal monitoring and valid threshold warning/critical response | FSR-1.1/1.2 tested. |
| TS-03 to TS-06, TS-15 | Startup timeout, source loss, delayed/withheld updates, and transport dropout | FSR-2.1/2.2 tested; path attribution under EC-1 is planned. |
| TS-07, TS-08 | Duplicate and out-of-order inputs | Guardian tests for DFR-7/DFR-8 and TS-08; collector attribution under EC-2 is planned. |
| TS-09, TS-10 | Stuck maximum and all-temperature-frozen limitation | FSR-2.4 tested for a stuck maximum with reference movement; TS-10 is not a detection pass. |
| TS-11, TS-12, TS-16 to TS-19 | Quality, mitigation gating, range, and rate plausibility | FSR-3.2 to FSR-3.4 and FSR-3.6 are tested. |
| TS-20, TS-21 | Isolated and repeated spikes | FSR-3.5 tested; campaign scenarios `isolated_spike` and `spike`. |
| TS-22, TS-23 | Guardian process termination and evaluation hang | FSR-2.7 is planned; DFR-5's independent in-vehicle response is missing. |
| TS-24 | Late-arriving stale message | Not implemented: needs synchronized clocks (FSR-2.8). |
| TS-25, TS-26 | Gradual trend, explicit upper-scale saturation (255 °C) | Tested in the core; campaign scenario `drift` for TS-25. TS-26 does not show that saturation can be told apart from a real extreme temperature. |
| Diagnostic campaigns | DFM writes and OpenSOVD visibility | FSR-D.1/.2 are tested; diagnostic-path failures must not delay Guardian safety responses. |

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The HARA alignment, fault allocation, and verification strategy were updated with
the assistance of **GitHub Copilot** using the model **GPT-6 Luna**.

The sync with the HARA's duplicate, out-of-order, spike-debounce, and trend
requirements was made with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
