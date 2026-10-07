<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Battery Thermal Contract

The uProtocol interface between the VSS Publisher, the Battery Thermal Guardian,
and the Evidence Collector. The payloads are defined in
[`battery_thermal.proto`](battery_thermal.proto) and encoded as Protobuf
(`UPAYLOAD_FORMAT_PROTOBUF`).

The Rust crate in this folder (`thermal-contract`) generates the types and holds
the topic addresses. It uses a vendored `protoc`, so no installation is needed.
Other languages, such as the Python Evidence Collector, generate their types from
the same `.proto` file.

## Topics

| Topic | uProtocol URI | Publisher | Payload |
|-------|---------------|-----------|---------|
| Battery temperature | `//battery-vss/9001/1/9001` | VSS Publisher (`vss-publisher`) | `BatteryTemperature` |
| Guardian events | `//guardian/9002/1/8001` | Battery Thermal Guardian (`guardian-service`) | `GuardianEvent` |
| Guardian heartbeat | `//guardian/9002/1/8002` | Battery Thermal Guardian (`guardian-service`) | `Heartbeat` |

## BatteryTemperature

One message per Data Broker update of the battery signals. The Guardian relies on
the assumptions A-1 to A-3 of the
[Safety Concept](../docs/reference/hara.md#assumptions). This is how
the VSS Publisher fulfills them:

| Field | Assumption | Source in the VSS Publisher |
|-------|------------|-----------------------------|
| `max_c`, `avg_c`, `min_c` | — | `Vehicle.Powertrain.TractionBattery.Temperature.{Max,Average,Min}` |
| `source_timestamp_ms` | A-1 | Newest Data Broker timestamp of the update. Publish time if the Data Broker sends none. |
| `sequence` | A-1 | Counts the published messages, starting at 1. |
| `alive_counter` | A-3 | `Vehicle.Powertrain.TractionBattery.BMS.AliveCounter` |
| `quality` | A-3 | `Vehicle.Powertrain.TractionBattery.BMS.SignalQuality`, translated as below |

The VSS paths are those of [`can/vss_dbc.json`](../can/vss_dbc.json); the
environment variables `VSS_ALIVE_COUNTER_PATH` and `VSS_QUALITY_PATH` override the
last two. The publisher sends a message only once it knows all five signals.

The contract does not carry the raw CAN quality value. The VSS Publisher
translates it ([Quality Enum](../docs/reference/architecture.md#quality-enum)):

| CAN value | Contract value |
|-----------|----------------|
| `0x80` (VALID) | `QUALITY_VALID` |
| `0x00` (INVALID) | `QUALITY_INVALID` |
| `0xFF` (ERROR_NOT_AVAILABLE) | `QUALITY_NOT_AVAILABLE` |
| anything else | `QUALITY_UNSPECIFIED` |

The Guardian evaluates only `QUALITY_VALID` samples. Every other value is
reported as a quality fault (FSR-3.4).

A-2 (one message per CAN frame): the KUKSA CAN Provider writes the signals of a
frame one by one, in DBC order, so the alive counter arrives last. The VSS
Publisher therefore publishes only when the alive counter is updated, which
gives one complete message per frame. The mapping in `can/vss_dbc.json` lets
the provider forward each signal at most every 50 ms, below the 100 ms frame
cycle; with 100 ms, more than half of the frames were dropped. About 7 % of the
frames are still lost on the way (measured in
[Run the Signal Chain](../docs/how-to/run-signal-chain.md)).

## GuardianEvent

Each service session has a fresh UUID `session_id`; event IDs and Guardian
monotonic timestamps are scoped to that session. The same session/event ID pair
is written to DFM environment data and read back through OpenSOVD.

One message per event of the Guardian core: a thermal state change, a monitoring
status change, a detected or recovered fault, or a mitigation request. `event_id` and
`cause_event_id` link the events into the evidence chain, for example
fault → monitoring status change → mitigation request. A `FaultDetected` event
carries the diagnostic trouble code and the requirement that detected the fault.
`FaultRecovered` carries the same code and requirement, the healthy trigger sample,
and the original detection event as its cause. DFM receives `Passed`; individual
OpenSOVD readback must show `testFailed=false` while retaining the original
failure metadata. The recovery event links to that detection through its cause ID.
Failure history and occurrence counts remain available.

After startup, monitors remain NotTested until sustained healthy input produces
`FaultTestPassed`. This also clears a previous session's current DFM failure
without deleting its history or inventing a detection cause in the new session.
The first stuck-signal monitor test observes more than the configured three-second
stuck interval. Subsequent detected faults recover through `FaultRecovered`.
Only `testFailed` clearing is required; historical confirmation and warning bits
follow DFM's lifecycle/reset policy.

## Heartbeat

Published every `T_hb_period` (500 ms) whether or not anything changed, so the
[watchdog](../watchdog/README.md) can tell "nothing to report" from "Guardian
crashed or hung" (FSR-2.7). `session_id` is the same as in the Guardian's
`GuardianEvent` messages; `sequence` counts heartbeats from 1. The heartbeat is
sent from the same loop that runs the Guardian core, so it stops when the core
hangs, not only when the process ends.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The GuardianEvent session field was added with assistance from **Codex** using
**GPT-6.1 Sol** (`gpt-6.1-sol`).

The Heartbeat topic and payload were added with the assistance of **Claude Code**
using the model **Claude Opus 5.5** (`claude-opus-5-5`).
