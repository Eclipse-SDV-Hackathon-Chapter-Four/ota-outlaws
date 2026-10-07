<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Faults to Be Detected

The faults the Battery Thermal Guardian detects and who handles them. The
Guardian detects a fault and warns; the Evidence Collector finds out what
caused it (see
[Responsibilities](architecture.md#responsibilities-guardian-and-evidence-collector)).
The requirements for each fault are in the
[traceability table](../explanation/safety-concept.md#traceability) of the Safety
Concept.

## Battery Thermal Guardian Fault Catalog

The table below lists concrete faults for the Battery Thermal Guardian, derived
from the `BMS_MSG1` signals (`CellTempMax`, `CellTempMin`, `CellTempAvg`,
`Quality`, `AliveCounter`; see [Architecture](architecture.md#can-signals)).

Severity follows the DFM scale (`Warn` · `Error` · `Fatal`): `Warn` is an early
or advisory condition that raises attention without changing Guardian output;
`Error` means the sample can no longer be trusted, so it is discarded and
monitoring is marked unavailable; `Fatal` means the battery itself is in a
dangerous state and requires an active protective response.

Faults marked *(implemented)* exist as a DTC in the Guardian today: the input
faults as a `FaultCode` of the core, the two overtemperature faults as DTCs the
Guardian service derives from thermal state changes. The exception is Heartbeat
Loss (`BTG_GuardianHeartbeatLoss`): a crashed or hung Guardian cannot report its
own failure, so the separate [watchdog](../../watchdog/README.md) reports it, to
the same DFM entity. Every DTC carries its fault type and severity in the DFM's
environment data (the DFM keeps only the severity in its records). The campaign
checks both in OpenSOVD. The rest are derivable from the same four signals and
are either specified in the Safety Concept as `planned`, or proposed here as not
yet specified at all; none of those are implemented.

The HARA ID column traces each fault to the
[preliminary HARA](hara.md#candidate-faults-and-malfunctions)'s candidate faults
(`F-*`) or, where a row is a correct escalation rather than an injected fault,
to the hazardous events it guards against (`HE-*`). `—` means the HARA does not
currently define a matching item; that is a gap to close there, not evidence
the fault is unimportant here.

Fault Type classifies what kind of thing is wrong, independently of how severe
it is. It is the DFM fault catalog's existing `category` field
([`FaultType`](../../third-party/fault-lib/README.md), from `fault_lib`). For
the four implemented faults, the value is taken verbatim from
[`diagnostics/catalog/battery_guardian.json`](../../diagnostics/catalog/battery_guardian.json):
all four are `Communication`, because freshness, a stuck counter, a stuck
value, and an invalid quality flag are all about whether the signal path can
be trusted, not about the battery itself. The remaining rows follow the same
line: `Hardware` for a genuine physical battery/sensor condition,
`Communication` for signal-path integrity, `Configuration` for a plausibility
or validation rule, `Timing` for an internal deadline the Guardian itself owns
(not the bus), and `Software` for the Guardian's own evaluation logic. None of
the faults below use `Power` or `Custom`.

Mitigation is the actual Guardian `Mitigation` value, not a description of it.
Overtemperature Warning has none today: reaching WARNING requests no mitigation
(`DriverWarning` is proposed).
`DriverWarningOvertemp` and `DriverWarningMonitoringUnavailable` are the two
values that exist in the Guardian today. The rest are short names proposed in
the same style for faults that are not yet implemented.

| Fault | HARA ID | Fault Type | Description | Severity | Mitigation | Test Case |
|---|---|---|---|---|---|---|
| Overtemperature Warning *(implemented)* | HE-1, HE-2, HE-3 | Hardware | Max cell temperature crossed the WARNING threshold (≥45°C) | Warn | — | |
| Overtemperature Critical *(implemented)* | HE-1, HE-2, HE-3 | Hardware | Max cell temperature crossed the CRITICAL threshold (≥55°C) | Fatal | `DriverWarningOvertemp` | TS-02 |
| Undertemperature | — | Hardware | Temperature drops below a safe charging/operating threshold (no dedicated threshold implemented yet) | Warn | `BlockCharging` | |
| FreshnessLost *(implemented)* | F-4, F-9 | Communication | No fresh data arriving anymore | Error | `DriverWarningMonitoringUnavailable` | |
| CounterStuck *(implemented)* | F-1 | Communication | ECU frozen, keeps sending the same frame | Error | `DriverWarningMonitoringUnavailable` | |
| SignalStuck *(implemented)* | F-1 | Communication | All temperature values are stuck | Error | `DriverWarningMonitoringUnavailable` | TS-10 |
| QualityInvalid *(implemented)* | TS-13 | Communication | Source marks the value as unusable | Error | `DriverWarningMonitoringUnavailable` | TS-13, TS-28 |
| OutOfRange *(implemented)* | F-6, TS-11 | Configuration | Value outside the plausible range | Error | `DriverWarningMonitoringUnavailable` | |
| RateImplausible *(implemented)* | F-8, TS-11 | Configuration | Rise faster than physically plausible | Error | `DriverWarningMonitoringUnavailable` | |
| Fast-Heating Trend | F-7 | Hardware | Rise below θ_warn sustained over time, early warning | Warn | `DriverWarning` | |
| Startup Without Source *(implemented)* | F-9, TS-03 | Communication | No valid sample since Guardian start | Error | `DriverWarningMonitoringUnavailable` | TS-03 |
| Isolated vs. Repeated Spike | F-8 | Configuration | Single occurrence sets SUSPECT, repeated sets DEGRADED | Warn / Error | `DiscardSample` / `DriverWarningMonitoringUnavailable` | |
| Counter Error, Isolated vs. Repeated | F-3, F-5 | Communication | Counter jump, not advancing by exactly one | Warn / Error | `DiscardSample` / `DriverWarningMonitoringUnavailable` | TS-06, TS-08, TS-07 |
| Heartbeat Loss | F-10 | Timing | Guardian itself stops reporting | Fatal | `RestartGuardian` | |
| Stale Timestamp | F-2 | Communication | Constant transport delay despite synchronized clocks | Error | `DriverWarningMonitoringUnavailable` | |
| Min Stuck | F-1 | Communication | Signal source connection abort | Error | `DriverWarningMonitoringUnavailable` | TS-04 |
| Upper-Scale Saturation | — | Hardware | Sensor pegged at its limit rather than a plausible value | Error | `DriverWarningMonitoringUnavailable` | |

## AI Assistance

This document was revised with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The section "Battery Thermal Guardian Fault Catalog" was added with the
assistance of **Claude Code** using the model **Claude Sonnet 5**
(`claude-sonnet-5`).

The implementation markers and the overtemperature DTCs were updated with the
assistance of **Claude Code** using the model **Claude Opus 5.5**
(`claude-opus-5-5`).
