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

### Fault Type

Fault Type classifies what kind of thing is wrong, independently of how severe
it is. It is the DFM fault catalog's `category` field
([`FaultType`](../../third-party/fault-lib/README.md), from `fault_lib`):

```rust
pub enum FaultType {
    /// Hardware fault (sensor, actuator, etc.).
    Hardware,
    /// Software fault (assertion, logic error, etc.).
    Software,
    /// Communication fault (bus timeout, CRC mismatch, etc.).
    Communication,
    /// Configuration fault (invalid parameter, schema mismatch, etc.).
    Configuration,
    /// Timing fault (deadline miss, watchdog, etc.).
    Timing,
    /// Power-related fault (undervoltage, brownout, etc.).
    Power,
    /// Escape hatch for domain-specific groupings until the enum grows.
    Custom(ShortString),
}
```

For the four implemented faults, the value is taken verbatim from
[`diagnostics/catalog/battery_guardian.json`](../../diagnostics/catalog/battery_guardian.json):
all four are `Communication`, because freshness, a stuck counter, a stuck
value, and an invalid quality flag are all about whether the signal path can
be trusted, not about the battery itself. The remaining rows follow the same
line: `Hardware` for a genuine physical battery/sensor condition,
`Communication` for signal-path integrity, `Configuration` for a plausibility
or validation rule, `Timing` for an internal deadline the Guardian itself
owns (not the bus), and `Software` for the Guardian's own evaluation logic.
None of the faults below use `Power` or `Custom`.

### Mitigation Type

Mitigation Type classifies the mitigation the same way Fault Type classifies
the fault, so the two can be read side by side. It groups the specific
mitigation values used in the table (`DriverWarningOvertemp`,
`DriverWarningMonitoringUnavailable`, and the proposed ones) into the kind of
response they are:

```rust
pub enum MitigationType {
    /// Record the event for diagnostics; no change to Guardian output.
    Log,
    /// Drop the untrustworthy sample from further evaluation.
    DiscardSample,
    /// Mark monitoring degraded/unavailable (`DriverWarningMonitoringUnavailable`).
    DegradeMonitoring,
    /// Occupant-facing advisory warning, no physical actuation requested.
    DriverWarning,
    /// A physical protective response is requested (e.g. `DriverWarningOvertemp`,
    /// blocking charge).
    ActiveProtection,
    /// A prior mitigation did not resolve the condition; request a stronger one.
    Escalation,
    /// The runtime/supervisor recovers the faulty component itself (e.g.
    /// restart), independent of any occupant warning.
    SelfRecovery,
    /// Escape hatch for domain-specific mitigations until the enum grows.
    Custom(ShortString),
}
```

Severity and Mitigation Type track each other: `Warn` faults map to `Log`,
`DriverWarning`, or the advisory half of a dual mitigation; `Error` faults map
to `DiscardSample` and `DegradeMonitoring`; `Fatal` faults map to
`ActiveProtection`, `Escalation`, or `SelfRecovery` — the categories that
change what the vehicle or the Guardian itself does, not just what the driver
is told.

| Fault | HARA ID | Fault Type | Description | Severity | Mitigation | Mitigation Type |
|---|---|---|---|---|---|---|
| Overtemperature Warning *(implemented)* | HE-1, HE-2, HE-3 | Hardware | Max cell temperature crossed the WARNING threshold (≥45°C) | Warn | Log and raise driver attention, no active intervention yet | DriverWarning |
| Overtemperature Critical *(implemented)* | HE-1, HE-2, HE-3 | Hardware | Max cell temperature crossed the CRITICAL threshold (≥55°C) | Fatal | `DriverWarningOvertemp` | ActiveProtection |
| Undertemperature | — | Hardware | Temperature drops below a safe charging/operating threshold (no dedicated threshold implemented yet) | Warn | Log and raise driver attention, block charging if applicable | ActiveProtection |
| FreshnessLost *(implemented)* | F-4, F-9 | Communication | No fresh data arriving anymore | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| CounterStuck *(implemented)* | F-1 | Communication | ECU frozen, keeps sending the same frame | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| SignalStuck *(implemented)* | F-1 | Communication | Max frozen while Avg/Min keep moving | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| QualityInvalid *(implemented)* | — | Communication | Source marks the value as unusable | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| OrderImplausible | — | Configuration | Min/Avg/Max not in the correct order | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| OutOfRange | F-6 | Configuration | Value outside the plausible range | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| RateImplausible | F-8 | Configuration | Rise faster than physically plausible | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Fast-Heating Trend | F-7 | Hardware | Rise below θ_warn sustained over time, early warning | Warn | Log and raise driver attention, pre-arm thermal mitigation | DriverWarning |
| Hot Spot | — | Hardware | Max significantly above Avg, single cell overheating | Warn | Log and raise driver attention, pre-arm thermal mitigation | DriverWarning |
| Mitigation Failed | — | Timing | Temperature keeps rising despite requested mitigation | Fatal | Escalate to a stronger protective action (e.g. request power limiting or shutdown) | Escalation |
| Startup Without Source | — | Communication | No valid sample since Guardian start | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Isolated vs. Repeated Spike | F-8 | Configuration | Single occurrence sets SUSPECT, repeated sets DEGRADED | Warn / Error | Discard sample only / escalate to `DriverWarningMonitoringUnavailable` | DiscardSample / DegradeMonitoring |
| Counter Error, Isolated vs. Repeated | F-3, F-5 | Communication | Counter jump, not advancing by exactly one | Warn / Error | Discard sample only / escalate to `DriverWarningMonitoringUnavailable` | DiscardSample / DegradeMonitoring |
| Heartbeat Loss | F-10 | Timing | Guardian itself stops reporting | Fatal | Runtime restarts the Guardian, `DriverWarningMonitoringUnavailable` until recovered | SelfRecovery |
| Stale Timestamp | F-2 | Communication | Constant transport delay despite synchronized clocks | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Min Stuck | F-1 | Communication | Mirror of SignalStuck, but on the cold side | Error | `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Avg Deviates from (Min+Max)/2 | — | Configuration | Internal aggregation error in the BMS | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Chattering Between Warning/Critical | — | Software | Unstable thermal process despite hysteresis | Warn | Log only, no mitigation change | Log |
| Upper-Scale Saturation | — | Hardware | Sensor pegged at its limit rather than a plausible value | Error | Discard sample, `DriverWarningMonitoringUnavailable` | DegradeMonitoring |
| Rapid Cooling Faster Than Physically Plausible | F-7 | Hardware | Coolant leak or sensor fault | Warn | Log and raise driver attention | DriverWarning |

## AI Assistance

This document was revised with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The section "Battery Thermal Guardian Fault Catalog" was added with the
assistance of **Claude Code** using the model **Claude Sonnet 5**
(`claude-sonnet-5`).
