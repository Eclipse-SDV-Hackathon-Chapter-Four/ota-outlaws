<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Guardian Architecture

The Battery Thermal Guardian decides how dangerous the battery temperature is and
whether its input can be trusted. Its required behavior is defined in the
[Safety Concept](safety-concept.md). This document explains how the Guardian is
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
| Core (`components/guardian`) | **Implemented** | Detectors, thermal state machine, monitoring status, events |
| Input adapter | Not implemented | Subscribe over uProtocol, decode the Protobuf payload, call the core |
| Tick | Not implemented | Call the core every 50 ms, so that missing samples are detected |
| Output adapter | Not implemented | Publish state, fault, and mitigation events over uProtocol |
| DFM adapter | Not implemented | Write fault events to the DFM without blocking the safety reaction (FSR-D.1) |

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

- **`Sample`**: maximum, average, and minimum cell temperature, plus the source
  timestamp and sequence number from the publisher (assumption A-1).
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
| Freshness monitor | FSR-2.2 | A sample is fresh if its source timestamp is later than the previous fresh one and all values are finite. No fresh sample for longer than `T_stale` is a fault. Checked on every tick. |
| Stuck detector | FSR-2.4 | The maximum keeps the same value for longer than `T_stuck` while the average or minimum moves by at least `Δ_stuck`. Checked on every fresh sample. |
| Thresholds | FSR-1.1, FSR-1.2 | The maximum reaches `θ_warn` or `θ_crit`. Checked on every fresh sample. |

Samples that are not fresh are ignored entirely. This is safe: if only such
samples arrive, the freshness monitor reports the loss of fresh data.

Every fault is reported once. The first fault sets the monitoring status to
DEGRADED and requests the "monitoring unavailable" warning. Later faults are
reported for the evidence chain, but do not repeat the warning.

## Configuration

The safety parameters are in
[`config/guardian/safety-params.toml`](../../config/guardian/safety-params.toml).
The core rejects a configuration that would silently disable a safety mechanism,
for example a warning threshold that is not below the critical threshold, or a
timeout of zero. Unknown parameters are rejected as well, so a typo cannot
switch a check off.

The requirement tests load the shipped file, so CI also verifies the
configuration.

## Not implemented yet

- All adapters (see the table above). The core is not yet connected to the
  running system.
- Recovery (FSR-1.5, FSR-2.6). The thermal state is never lowered, and DEGRADED
  is never left. This is safe, and each scenario starts with a new Guardian
  (assumption A-4).
- All Should and Could requirements of the safety concept.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
