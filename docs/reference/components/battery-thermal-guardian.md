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

The Battery Thermal Guardian decides how dangerous the battery temperature is and
whether its input can be trusted. Its required behavior is defined in the
[Safety Concept](../../explanation/safety-concept.md). This document explains how the Guardian is
built.

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
| DFM adapter | Not implemented | Write fault events to the DFM without blocking the safety reaction (FSR-D.1) |

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
| Freshness monitor | FSR-2.2, FSR-2.3 | No [fresh sample](../../explanation/safety-concept.md#guardian-output-model) for longer than `T_stale` is a fault. If at least two frames with an unchanged alive counter arrived meanwhile, the fault is "counter stuck" (FSR-2.3), otherwise "freshness lost" (FSR-2.2). Checked on every tick. |
| Quality check | FSR-3.4 | A fresh sample whose quality flag is not `VALID` is reported and leads to DEGRADED. It is not evaluated, but it still shows that the source is alive. |
| Stuck detector | FSR-2.4 | The maximum keeps the same value for longer than `T_stuck` while the average or minimum moves by at least `Δ_stuck`. Checked on every valid sample. |
| Thresholds | FSR-1.1, FSR-1.2 | The maximum reaches `θ_warn` or `θ_crit`. Checked on every valid sample. |

The Guardian detects faults, but does not find out what caused them. Telling a
source fault from a transport fault, and diagnosing duplicated or reordered
messages, is the Evidence Collector's job (EC-1, EC-2). See
[Responsibilities](../architecture.md#responsibilities-guardian-and-evidence-collector).

Samples that are not fresh are ignored entirely. This is safe: if only such
samples arrive, the freshness monitor reports the loss of fresh data.

Every fault is reported once. The first fault sets the monitoring status to
DEGRADED and requests the "monitoring unavailable" warning. Later faults are
reported for the evidence chain, but do not repeat the warning.

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

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
