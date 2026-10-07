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

Mitigation follows severity the same way: `Warn` faults are logged for
awareness only; `Error` faults discard the sample and request
`DriverWarningMonitoringUnavailable`; `Fatal` faults request an active
protective mitigation such as `DriverWarningOvertemp`, or escalate further if
the first mitigation did not help.

Faults marked *(implemented)* exist as a `FaultCode` and DTC in the Guardian
today. The rest are derivable from the same four signals and are either
specified in the Safety Concept as `planned`, or proposed here as not yet
specified at all; none of those are implemented.

The HARA ID column traces each fault to the
[preliminary HARA](hara.md#candidate-faults-and-malfunctions)'s candidate faults
(`F-*`) or, where a row is a correct escalation rather than an injected fault,
to the hazardous events it guards against (`HE-*`). `—` means the HARA does not
currently define a matching item; that is a gap to close there, not evidence
the fault is unimportant here.

| Fault | HARA ID | Description | Severity | Mitigation |
|---|---|---|---|---|
| Overtemperature Warning *(implemented)* | HE-1, HE-2, HE-3 | Max cell temperature crossed the WARNING threshold (≥45°C) | Warn | Log and raise driver attention, no active intervention yet |
| Overtemperature Critical *(implemented)* | HE-1, HE-2, HE-3 | Max cell temperature crossed the CRITICAL threshold (≥55°C) | Fatal | `DriverWarningOvertemp` |
| Undertemperature | — | Temperature drops below a safe charging/operating threshold (no dedicated threshold implemented yet) | Warn | Log and raise driver attention, block charging if applicable |
| FreshnessLost *(implemented)* | F-4, F-9 | No fresh data arriving anymore | Error | `DriverWarningMonitoringUnavailable` |
| CounterStuck *(implemented)* | F-1 | ECU frozen, keeps sending the same frame | Error | `DriverWarningMonitoringUnavailable` |
| SignalStuck *(implemented)* | F-1 | Max frozen while Avg/Min keep moving | Error | `DriverWarningMonitoringUnavailable` |
| QualityInvalid *(implemented)* | — | Source marks the value as unusable | Error | `DriverWarningMonitoringUnavailable` |
| OrderImplausible | — | Min/Avg/Max not in the correct order | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| OutOfRange | F-6 | Value outside the plausible range | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| RateImplausible | F-8 | Rise faster than physically plausible | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| Fast-Heating Trend | F-7 | Rise below θ_warn sustained over time, early warning | Warn | Log and raise driver attention, pre-arm thermal mitigation |
| Hot Spot | — | Max significantly above Avg, single cell overheating | Warn | Log and raise driver attention, pre-arm thermal mitigation |
| Mitigation Failed | — | Temperature keeps rising despite requested mitigation | Fatal | Escalate to a stronger protective action (e.g. request power limiting or shutdown) |
| Startup Without Source | — | No valid sample since Guardian start | Error | `DriverWarningMonitoringUnavailable` |
| Isolated vs. Repeated Spike | F-8 | Single occurrence sets SUSPECT, repeated sets DEGRADED | Warn / Error | Discard sample only / escalate to `DriverWarningMonitoringUnavailable` |
| Counter Error, Isolated vs. Repeated | F-3, F-5 | Counter jump, not advancing by exactly one | Warn / Error | Discard sample only / escalate to `DriverWarningMonitoringUnavailable` |
| Heartbeat Loss | F-10 | Guardian itself stops reporting | Fatal | Runtime restarts the Guardian, `DriverWarningMonitoringUnavailable` until recovered |
| Stale Timestamp | F-2 | Constant transport delay despite synchronized clocks | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| Min Stuck | F-1 | Mirror of SignalStuck, but on the cold side | Error | `DriverWarningMonitoringUnavailable` |
| Avg Deviates from (Min+Max)/2 | — | Internal aggregation error in the BMS | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| Chattering Between Warning/Critical | — | Unstable thermal process despite hysteresis | Warn | Log only, no mitigation change |
| Upper-Scale Saturation | — | Sensor pegged at its limit rather than a plausible value | Error | Discard sample, `DriverWarningMonitoringUnavailable` |
| Rapid Cooling Faster Than Physically Plausible | F-7 | Coolant leak or sensor fault | Warn | Log and raise driver attention |

## AI Assistance

This document was revised with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The section "Battery Thermal Guardian Fault Catalog" was added with the
assistance of **Claude Code** using the model **Claude Sonnet 5**
(`claude-sonnet-5`).
