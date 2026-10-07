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
| Core ([`guardian`](../../../guardian)) | **Implemented** | Detectors, thermal state machine, monitoring status, events |
| Input adapter ([`guardian-service`](../../../guardian-service)) | **Implemented** | Subscribe to `BatteryTemperature` over uProtocol, decode the Protobuf payload, call the core |
| Tick ([`guardian-service`](../../../guardian-service)) | **Implemented** | Call the core every 50 ms, so that missing samples are detected |
| Output adapter ([`guardian-service`](../../../guardian-service)) | **Implemented** | Publish every core event as a `GuardianEvent` over uProtocol |
| DFM adapter | **Implemented / tested** | Write Guardian fault events to the DFM and expose them through OpenSOVD (FSR-D.1, FSR-D.2) |
| Independent Guardian supervisor | **Missing** | Detect termination or loss of evaluation progress and request a monitoring-unavailable warning independently of the Guardian (HARA DFR-5) |

The HARA item boundary includes Guardian evaluation and its output requests. The
physical sensor, CAN decoding, KUKSA components, occupant interface, and physical
mitigation actuators are external unless system-level architecture allocates them
to the item. A `MitigationRequested` event is therefore a request, not proof that
an occupant was warned or an actuator acted.

The messages and topics are defined in the
[Battery Thermal Contract](../../../contracts/README.md). A payload the Guardian
cannot decode is rejected before it reaches the core; if only such payloads
arrive, the core reports the loss of fresh data. An unknown quality value is
passed to the core as `UNDEFINED`, so the core reports it instead of using the
value.

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
| Freshness monitor | FSR-2.2, FSR-2.3, FSR-2.8 | Ignore samples that are not fresh. After `T_stale` without fresh data, report freshness loss; when repeated unchanged alive counters satisfy FSR-2.3, report counter-stuck instead. FSR-2.8 rejects samples older than `T_age` only when source and Guardian clocks are synchronized. Checked on each sample and 50 ms tick. |
| Quality check | FSR-3.4 | A fresh sample whose quality is not `VALID` is discarded, reported, and sets monitoring to `DEGRADED` without debounce. The sample indicates source activity but cannot support thermal assessment. |
| Stuck detector | FSR-2.4 | If maximum temperature stays unchanged for `T_stuck` while average or minimum moves by at least `Δ_stuck`, report a stuck-signal fault and set monitoring to `DEGRADED`. It does not detect all temperature channels frozen together. |
| Order and range checks | FSR-3.1, FSR-3.2 | Reject a sample that violates `Min ≤ Avg ≤ Max` or lies outside `[θ_min, θ_max]`; report the fault and set monitoring to `DEGRADED`. An above-range maximum also raises thermal state to at least `WARNING`. |
| Rate plausibility | FSR-3.3, FSR-3.5, FSR-3.6; HARA DFR-4 | Reject a rate-implausible sample and preserve at least `WARNING` when the rise could be real. HARA requires an isolated spike to set `SUSPECT` and repeated spikes (`N_suspect` within `T_suspect`) to set `DEGRADED`; invalid spikes alone must not cause `CRITICAL` or overtemperature mitigation. The current Safety Concept instead specifies immediate `DEGRADED` in FSR-3.3 and marks FSR-3.5 planned, so this is an implementation/specification gap. |
| Thermal thresholds and trends | FSR-1.1 to FSR-1.4 | On valid samples, threshold criteria raise the state to `WARNING` or `CRITICAL`. Trend and hot-spot criteria are planned. Invalid samples do not lower the thermal state. |
| Independent supervision | HARA DFR-5 | An independent in-vehicle supervisor must detect Guardian termination or evaluation hang and request the defined monitoring-unavailable warning. This supervisor is not implemented; the Evidence Collector and runtime restart do not satisfy this occupant-protection requirement. |

The Guardian detects faults, but does not find out what caused them. Telling a
source fault from a transport fault, and diagnosing duplicated or reordered
messages, is the Evidence Collector's job (EC-1, EC-2). See
[Responsibilities](../architecture.md#responsibilities-guardian-and-evidence-collector).

Samples that are not fresh are ignored entirely. If no fresh sample then arrives
for `T_stale`, the freshness monitor reports loss of monitoring. Fault events
carry event and cause IDs for diagnostic correlation. `DEGRADED` requests
`DRIVER_WARNING_MONITORING_UNAVAILABLE`; this degraded response is distinct from
the overtemperature mitigation `DRIVER_WARNING_OVERTEMP`. Per HARA DFR-4,
invalid data must never cause the latter or lower an active thermal state.

The thermal state (`CLEAR`, `MONITORING`, `WARNING`, `CRITICAL`, `MITIGATING`)
and monitoring status (`OK`, `SUSPECT`, `DEGRADED`) are independent. `SUSPECT`
means isolated rate-implausible samples have been discarded; `DEGRADED` means
input is lost or persistently invalid. HARA-required debounce for repeated
spikes is not yet implemented, so the current immediate-degradation behavior
must not be presented as satisfying the HARA's isolated/repeated distinction.

## HARA fault allocation

The HARA fault IDs and test cases remain authoritative. Status below follows the
Safety Concept; a test or design entry does not imply that a planned requirement
is implemented.

| HARA fault | Design response and current status | HARA verification |
|---|---|---|
| F-1: Temperature value frozen while messages continue | FSR-2.4 detects a frozen maximum only when average or minimum changes. All temperature channels frozen together are not detectable from current inputs. | TS-10 is a limitation probe; it does not demonstrate detection. |
| F-2: Message arrives after its allowed age/deadline | FSR-2.2 detects a freshness gap. FSR-2.8 rejects over-age timestamps only with synchronized clocks and is planned. | TS-05 tests a prolonged gap; proposed TS-27 covers explicit late arrival. |
| F-3: Same message delivered more than once | Non-fresh samples are ignored (FSR-2.2). Counter-stuck handling is FSR-2.3; collector-side duplicate attribution (EC-2) is planned. | TS-07. |
| F-4: Expected update dropped before reaching Guardian | FSR-2.2 reports freshness loss and requests the degraded response. | TS-05, TS-06, TS-18. |
| F-5: Messages arrive out-of-order | Samples with older source timestamps are ignored as not fresh. Collector attribution is planned under EC-2. | TS-08. |
| F-6: Isolated temperature sample outside configured interval | FSR-3.2 rejects the sample and sets monitoring to `DEGRADED`; high out-of-range input also keeps thermal state at least `WARNING`. | TS-11, TS-12, TS-14, TS-20, TS-21. |
| F-7: Temperature drifts over time | FSR-1.3 covers a sustained real upward trend and FSR-1.4 a hot spot; both are planned. Aggregate signals cannot identify every sensor drift. | Proposed TS-28 tests a rising thermal trend, not sensor-bias detection. |
| F-8: Isolated rate-implausible spike | FSR-3.3 rejects the sample and preserves a warning, but currently specifies immediate `DEGRADED`, contrary to HARA's isolated-spike debounce. | TS-22 and TS-23; TS-23 remains blocked until FSR-3.3/3.5 are reconciled. |
| F-9: Source disconnect or replay stops | FSR-2.2 reports freshness loss and requests the degraded response. | TS-03, TS-04, TS-18. |
| F-10: Guardian terminates or evaluation hangs | FSR-2.7 observes heartbeat loss and restarts a terminated process; independent occupant warning and verified hang detection are missing per DFR-5. | TS-25, TS-26; these do not prove occupant protection. |
| F-11: Source marks fresh sample invalid or unavailable | FSR-3.4 rejects the sample, sets `DEGRADED`, and reports the quality fault without debounce. | TS-13, TS-19. |
| F-12: Temperature saturates at 255 °C | FSR-3.2 treats it as high out-of-range and requires at least `WARNING`; no separate saturation diagnosis is available. | TS-14 is generic high-range coverage; proposed TS-29 uses 255 °C explicitly. |
| F-13: Repeated rate-implausible spikes | HARA requires `SUSPECT` for an isolated spike and `DEGRADED` after the configured repeat threshold. FSR-3.5 is planned and conflicts with immediate degradation in FSR-3.3. | TS-24 is blocked until the requirement conflict is resolved. |

## Timing and recovery

The core receives local monotonic time from the service adapter; it does not
read a wall clock. The service calls the core on each sample and every 50 ms.
`T_react` is 100 ms from the HARA and measures response after a condition is
observable. It is not a validated battery warning lead time. The warning
thresholds and their time-to-hazard basis still require battery-level validation.

Freshness uses source timestamps and the alive counter. `T_age` is applied only
when source/Guardian clocks are synchronized (FSR-2.8); otherwise the Guardian
can detect gaps but cannot detect a constant transport delay. Thermal state is
not lowered while monitoring is `DEGRADED`. Recovery requires `N_recover`
consecutive fresh, valid samples spanning `T_recover`, with each active fault
cleared, followed by the thermal hysteresis rules in FSR-1.5.

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
Which requirements are implemented is recorded in the status column of the
[Safety Concept](../../explanation/safety-concept.md#functional-safety-requirements).

## Verification strategy

The HARA owns the detailed scenario stimuli and expected results. This design
uses those scenarios to verify behavior through the uProtocol input and output,
and, for diagnostic requirements, DFM/OpenSOVD. Record the active configuration,
Guardian input, state/status and fault events, mitigation requests, diagnostics,
and response latency for each applicable run.

| HARA tests | Verification focus | Status / limitation |
|---|---|---|
| TS-01, TS-02, TS-15, TS-16 | Nominal monitoring and valid threshold warning/critical response | FSR-1.1/1.2 tested. |
| TS-03 to TS-06, TS-18 | Startup timeout, source loss, delayed/withheld updates, and transport dropout | FSR-2.1/2.2 tested; path attribution under EC-1 is planned. |
| TS-07, TS-08 | Duplicate and out-of-order inputs | Guardian ignores non-fresh samples; collector attribution under EC-2 is planned. |
| TS-09, TS-10 | Stuck maximum and all-temperature-frozen limitation | FSR-2.4 tested for a stuck maximum with reference movement; TS-10 is not a detection pass. |
| TS-11 to TS-14, TS-19 to TS-22 | Thermal-state retention, quality, range, and rate plausibility | FSR-3.2 to FSR-3.4 and FSR-3.6 are tested; isolated-spike status remains inconsistent. |
| TS-23, TS-24 | Isolated and repeated spikes | FSR-3.5 planned; both cases are blocked from a conformance verdict until it is reconciled with FSR-3.3. |
| TS-25, TS-26 | Guardian process termination and evaluation hang | FSR-2.7 is planned; DFR-5's independent in-vehicle response is missing. |
| Proposed TS-27 to TS-29 | Late timestamp, gradual trend, explicit upper-scale saturation | Proposed requirement-gap tests; not evidence of implemented behavior. |
| Diagnostic campaigns | DFM writes and OpenSOVD visibility | FSR-D.1/.2 are tested; diagnostic-path failures must not delay Guardian safety responses. |

TS-17 (undertemperature) is blocked in the HARA because no lower operating limit,
responsible vehicle component, or charging-control interface has been allocated.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The HARA alignment, fault allocation, and verification strategy were updated with
the assistance of **GitHub Copilot** using the model **GPT-6 Luna**.
