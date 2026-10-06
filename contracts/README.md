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

The uProtocol interface between the KUKSA Proxy, the Battery Thermal Guardian,
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
| Battery temperature | `//battery-vss/9001/1/9001` | KUKSA Proxy (`vss-publisher`) | `BatteryTemperature` |
| Guardian events | `//guardian/9002/1/8001` | Battery Thermal Guardian (`guardian-service`) | `GuardianEvent` |

## BatteryTemperature

One message per Data Broker update of the battery temperature signals. The
Guardian relies on the assumptions A-1 to A-3 of the
[Safety Concept](../docs/explanation/safety-concept.md#assumptions). This is how
the KUKSA Proxy fulfills them today:

| Field | Assumption | Source in the KUKSA Proxy |
|-------|------------|---------------------------|
| `max_c`, `avg_c`, `min_c` | — | `Vehicle.Powertrain.TractionBattery.Temperature.{Max,Average,Min}` |
| `source_timestamp_ms` | A-1 | Newest Data Broker timestamp of the update. Publish time if the Data Broker sends none. |
| `sequence` | A-1 | Counts the published messages, starting at 1. |
| `alive_counter` | A-3 | VSS signal set in `VSS_ALIVE_COUNTER_PATH`. **MOCK** while it is not set: the proxy counts its own messages, so a frozen CAN source cannot be detected (FSR-2.3). |
| `quality` | A-3 | VSS signal set in `VSS_QUALITY_PATH`. **MOCK** while it is not set: always `QUALITY_OK`, so FSR-3.4 never triggers. |

The two mocks end when the CAN frame definition (DBC) and the VSS mapping carry
the alive counter and the quality flag.

A-2 (one message per CAN frame) holds only if the KUKSA CAN Provider writes all
signals of a frame in one Data Broker update. This is not verified yet.

## GuardianEvent

One message per event of the Guardian core: a thermal state change, a monitoring
status change, a detected fault, or a mitigation request. `event_id` and
`cause_event_id` link the events into the evidence chain, for example
fault → monitoring status change → mitigation request. A `FaultDetected` event
carries the diagnostic trouble code and the requirement that detected the fault.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
