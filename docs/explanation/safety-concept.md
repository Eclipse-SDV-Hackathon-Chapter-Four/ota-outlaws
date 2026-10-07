<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Safety Concept – Battery Thermal Guardian

> **Status: draft for team review.**
> The [HARA](../reference/hara.md) is the source of truth for hazardous events,
> safety goals, and their ratings. This concept refines its safety goals into
> functional safety requirements for the Guardian.

## Summary

- **Four safety goals, from the HARA:** warn in time (SG-1), never treat lost or
  invalid monitoring as safe (SG-2), never let invalid data cause an unintended
  mitigation (SG-3), never let an invalid sample cause an unsafe response while
  keeping real warnings (SG-4).
- **Guiding principle:** when in doubt, fail toward warning.
- **Two outputs:** the Guardian reports a *thermal state* (how hot) and a
  *monitoring status* (how trustworthy the input is) separately, so a signal loss
  never hides a hot battery.
- **Demo scope:** the **Must** requirements form a self-contained set that
  covers the challenge's recommended demo: baseline thresholds, loss of fresh
  data, a stuck alive counter, a stuck signal, an invalid quality flag, and
  diagnostics in DFM and OpenSOVD. The **Should** and **Could** requirements show
  where the design goes next. Their status says honestly whether they are done.

## Structure

The concept has three levels. Each level refines the one above it:

1. **Hazardous events** (HE-1 to HE-6) describe what can go wrong at vehicle
   level. They are defined and rated in the [HARA](../reference/hara.md).
2. **Safety goals** (SG-1 to SG-4) say what must never happen. They are also
   defined in the HARA.
3. **Functional safety requirements (FSR)** refine each goal into concrete,
   testable behavior of the Guardian, using the parameters in
   [Parameters](#parameters). The HARA's derived functional requirements
   (DFR-1 to DFR-6) say which FSRs cover which goal.

Fault campaigns reference FSR IDs, FSRs reference safety goals, and safety goals
reference hazardous events. This gives the evidence chain
hazardous event → safety goal → fault → detection → mitigation → verdict.

## Safety goals

The safety goals and the hazardous events they address are defined in the
[HARA](../reference/hara.md#risk-classification-and-safety-goals). In short:

| ID | Safety goal (summary) | Hazardous events | Refined by |
|----|-----------------------|------------------|------------|
| SG-1 | Effective occupant warning and defined mitigation before a thermal event becomes dangerous | HE-1 to HE-3 | FSR-1.x |
| SG-2 | Lost or invalid monitoring, including Guardian unavailability, is never treated as safe; a defined degraded response follows | HE-1 to HE-3 | FSR-2.x, FSR-3.4, FSR-3.5, FSR-3.7 |
| SG-3 | Invalid, repeated, or duplicated data never causes an unintended mitigation | HE-4, HE-5 | FSR-3.6 |
| SG-4 | An invalid sample never causes an unsafe response, while warnings that may indicate real danger are kept | HE-6, HE-1 | FSR-3.1 to FSR-3.3, FSR-3.6 |

**Guiding principle for all goals:** when in doubt, fail toward warning, never
toward a lower thermal assessment. An implausible high reading may come from a
broken sensor or from a real thermal runaway. Either way, the system keeps
warning.

**Warning versus mitigation.** Two kinds of output must be told apart for SG-3:

- the **overtemperature mitigation** (`DRIVER_WARNING_OVERTEMP`, requested on
  CRITICAL) can change the vehicle's response. Invalid data alone must never
  cause it (FSR-3.6).
- the **"monitoring unavailable" warning** (`DRIVER_WARNING_MONITORING_UNAVAILABLE`,
  requested on DEGRADED) is the defined degraded response of SG-2. Invalid data
  may cause it, and must, so that a broken sensor never looks safe.

This reading of the HARA's DFR-4 needs the team's confirmation.

### Diagnostic goal

This goal is not a safety goal, but the challenge's Definition of Done requires it.

| ID | Diagnostic goal |
|----|-----------------|
| DG-1 | Every detected fault shall be traceable in the vehicle diagnostics, linked to the fault that caused it. |

## Test roles

Two roles take part in every fault campaign. One acts on the system, the other
only observes it.

| Role | What it does | Who fills it |
|------|--------------|--------------|
| **Campaign runner** | Executes a campaign. It prepares the system (for example, starts a fresh Guardian, see A-4), starts the temperature source, injects the faults the campaign defines, and logs what it injected and when, under a run ID. | At first a **person** who follows the written campaign procedure. Later the **Scenario Generator**, which executes the same procedure automatically, and finally a **remote trigger** (openDUT) for remote reruns. |
| **Evidence Collector** | Observes the Guardian's input (the tap point), all Guardian outputs, and OpenSOVD. It measures latencies, attributes each fault to the layer that caused it (see [Evidence collector requirements](#evidence-collector-requirements)), checks the expected reactions, and writes the verdict report. It never influences the system. | Always an automated component. |

The two roles stay separate, so that the one who injects a fault never judges
its own claims. The collector takes the fault onset from its own observation,
not from the runner's log (see [Timing reference](#timing-reference)). This also
means that a person can act as the campaign runner: imprecise manual timing does
not affect the measured latencies.

A manual run still has to follow a written campaign, and the collector has to
record it. The challenge rejects one-off manual runs, because they cannot be
replayed. Writing every campaign down from the start lets an automation script
take over the procedure unchanged.

A *campaign* is a set of scenarios; each scenario injects one fault (or one
combination of faults) and has its own expected reactions.

## Assumptions

The Guardian's safety behavior relies on these properties of other components.
Each component's own documentation must state how it fulfills them.

| ID | Assumption | Component |
|----|------------|-----------|
| A-1 | Every sample carries the source timestamp (the time the value was captured at its origin) and a sequence number that increases by one per published message. The Guardian uses the source timestamp; the Evidence Collector uses the sequence number. | VSS Publisher |
| A-2 | Every CAN frame leads to exactly one sample, even if the values have not changed. Frames are not dropped, merged, or repeated on the way. Otherwise a constant temperature would look like stale data, and counter checks would report false gaps. | VSS Publisher, KUKSA CAN Provider configuration |
| A-3 | Every sample carries the alive counter and the quality flag of the CAN frame it was decoded from, unchanged. The source increments the counter by one per frame (0 to 255, then back to 0). The quality flag says whether the source vouches for the values: `VALID`, `INVALID`, or `ERROR_NOT_AVAILABLE` (see the [Quality Enum](../reference/architecture.md#quality-enum)). | Temperature source, KUKSA CAN Provider, VSS Publisher |
| A-4 | Each scenario starts with a freshly started Guardian, so that every scenario has its own session ID and its events and DFM records can be told apart. The Guardian recovers on its own (FSR-2.6), so this is not needed for safety. | Campaign runner |
| A-5 | The Evidence Collector observes exactly the stream the Guardian receives. All faults are injected upstream of the collector's tap point. | Evidence collector, fault injection |

## Guardian output model

The Guardian reports two independent dimensions. Mixing them into one scale would
lose information: if the signal is lost while the battery is hot, the occupants
must learn both facts.

**Thermal state:** how dangerous the battery temperature is. It follows the state
machine suggested in the challenge, ordered by severity:

```text
CLEAR < MONITORING < WARNING < CRITICAL = MITIGATING
```

| Thermal state | Meaning |
|---------------|---------|
| CLEAR | Initial state after start. No valid data stream has been received yet. This is **not** "all clear". |
| MONITORING | A valid stream is present and no thermal criterion is exceeded. |
| WARNING | A warning criterion is exceeded (threshold, trend, or hot spot), or invalid data indicates a possible high temperature. |
| CRITICAL | A critical criterion is exceeded. A mitigation is requested. |
| MITIGATING | Same severity as CRITICAL. The mitigation has been requested. |

**Monitoring status:** whether the Guardian can currently trust its input.

| Monitoring status | Meaning |
|-------------------|---------|
| OK | The input is fresh, in order, and plausible. |
| SUSPECT | Isolated invalid samples were seen and discarded. No state change yet (debounce). |
| DEGRADED | The input is lost or persistently invalid. The thermal state cannot be assessed reliably. |

**Fresh sample:** a sample whose source timestamp is later than that of the
previous fresh sample, whose alive counter differs from that of the previous
fresh sample, and whose values are finite numbers. Samples that are not fresh are
ignored (FSR-2.2, FSR-2.3).

**Valid sample:** a fresh sample with the quality flag `VALID` that passes every
other check the Guardian implements. A sample can be valid while the monitoring
status is DEGRADED.

**Raising and lowering:**

- The thermal state may be **raised** at any time, by any valid sample, whatever
  the monitoring status is. A plausible sample at a critical temperature always
  leads to CRITICAL at once.
- The thermal state may only be **lowered** under FSR-1.5, and never while the
  monitoring status is DEGRADED.
- The monitoring status returns to OK only under FSR-2.6.

**Safe states** and the mitigation events the Guardian publishes for them:

| Safe state | Entered when | Mitigation event |
|------------|--------------|------------------|
| Thermal state CRITICAL | A critical criterion is exceeded. | `DRIVER_WARNING_OVERTEMP` |
| Monitoring status DEGRADED | Monitoring is lost or the input is persistently invalid. | `DRIVER_WARNING_MONITORING_UNAVAILABLE` |

Both can be active at the same time.

## Timing and verdicts

### Timing reference

All times are measured by the **Evidence Collector on its own clock**. The
collector subscribes passively to the Guardian's input topic (the tap point) and
to all Guardian outputs. No clock synchronization between hosts is required.
Faults are injected upstream of the tap point (A-5), so the collector sees
exactly what the Guardian sees.

- **t0**: the first observation of the fault at the tap point. This is either the
  first faulty sample, or the moment the first expected sample is missing (the
  last valid arrival plus one signal cycle). The injection timestamp recorded by
  the campaign runner serves only as a cross-check, because the runner cannot
  know exactly when a fault takes effect (for example, when a prepared CAN trace
  reaches the faulty frame).
- **t1**: the arrival of the corresponding Guardian event at the collector.

Each FSR has a **latency budget** of the observation window it needs (for
example `T_stale`) plus the reaction time `T_react`. A requirement that needs no
observation window has a budget of `T_react`.

FSR-2.4 is the exception. A stuck value can only be told apart from a constant
temperature once the other temperatures move, so its detection time depends on
how fast the battery heats. Its budget is therefore a **hidden temperature
error**: how far the other temperatures moved between fault onset and detection.
The Evidence Collector measures it at its tap point.

### Scenario verdicts

| Verdict | Condition |
|---------|-----------|
| PASS | The fault was observed at the tap point, every expected reaction occurred within its latency budget, no forbidden reaction occurred, and the evidence is complete. |
| FAIL | An expected safety reaction is missing or late, or a forbidden reaction occurred (for example, the thermal state was lowered while monitoring was DEGRADED). |
| INCONCLUSIVE | The fault never reached the tap point (for example, the CAN provider resampled a one-frame spike away), or the evidence is incomplete (for example, a DFM record is missing in a scenario that does not target diagnostics). |

Each campaign defines its expected reactions. For a **diagnostics** fault
campaign, the expected reaction is that the collector detects the missing or
late diagnostic record. If it does, the verdict is PASS.

The evidence report also lists the result of each FSR separately. A missing DFM
record therefore shows as a failed FSR-D.2, even when the scenario verdict is
INCONCLUSIVE.

## Functional safety requirements

Every FSR has a priority for the hackathon (**Must**, **Should**, **Could**) and
a status (**planned**, **implemented**, **tested**). A requirement only counts as
fulfilled when its status is **tested** and it links to the test or campaign
that proves it. **Implemented** means the behavior exists in the Guardian core and is
covered by its unit tests; **tested** means an end-to-end campaign proves it.

Two kinds of end-to-end campaigns prove requirements:

- The [Campaign Tool](../reference/components/campaign.md) replays CAN traces
  through the real chain (CAN provider, Data Broker, VSS Publisher, uProtocol,
  Guardian, DFM, OpenSOVD) and judges every scenario from its own observations.
  Scenarios and expectations are in
  [`campaign/scenarios.toml`](../../campaign/scenarios.toml).
- The diagnostic campaigns ([`diagnostics/smoke_test.py`](../../diagnostics/smoke_test.py))
  run the Guardian service with a test publisher against the real DFM and
  OpenSOVD; they also cover the diagnostics outage (FSR-D.1).

| Priority | Meaning |
|----------|---------|
| Must | Needed for the demo. A self-contained set: it is safe without the other requirements. Implemented and tested first. |
| Should | Next, when the Musts are tested. |
| Could | Only if time is left. |

### SG-1: Timely thermal warning

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-1.1 | When the maximum cell temperature of a valid sample reaches `θ_warn`, the thermal state shall be WARNING or more severe. | `T_react` | Nominal heating profile | Must | tested |
| FSR-1.2 | When the maximum cell temperature of a valid sample reaches `θ_crit`, the thermal state shall be CRITICAL and the Guardian shall publish `DRIVER_WARNING_OVERTEMP`. | `T_react` | Nominal heating profile to the critical limit | Must | tested |
| FSR-1.3 | When valid samples show a temperature rise of at least `r_trend` sustained for `T_trend`, the thermal state shall be WARNING or more severe, even below `θ_warn`. | `T_trend` + `T_react` | Fast heating profile below `θ_warn` | Could | planned |
| FSR-1.4 | When the maximum cell temperature exceeds the average by more than `Δ_hotspot`, the thermal state shall be WARNING or more severe. A large spread is treated as a real local hot spot, never as a sensor fault. | `T_react` | Single-cell hot spot, upward drift of the maximum | Could | planned |
| FSR-1.5 | The thermal state shall be lowered only when the triggering criterion has been undercut by the hysteresis `θ_hyst` for `N_recover` consecutive valid samples spanning at least `T_recover`, and only while the monitoring status is OK. It is lowered one level at a time; each level needs its own window. | — | Temperature oscillating around `θ_warn` | Should | implemented |
| FSR-1.6 | Once the Guardian has published the mitigation request, the thermal state shall change from CRITICAL to MITIGATING. | `T_react` | Nominal critical profile | Could | planned |
| FSR-1.7 | When the temperature keeps rising for `T_mitigation` while MITIGATING, the Guardian shall return to CRITICAL and repeat the mitigation request ("mitigation failed"). | `T_mitigation` + `T_react` | Heating profile that continues after the mitigation request | Could | planned |

### SG-2: No silent loss of monitoring

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-2.1 | When no fresh sample arrives within `T_stale` after the Guardian starts, the monitoring status shall be DEGRADED and the Guardian shall report a startup fault. (HARA TS-03 sets the limit to `T_stale`.) | `T_stale` + `T_react`, from the Guardian's start, on its own clock | Guardian started without a source | Must | tested |
| FSR-2.2 | When no **fresh** sample arrives for longer than `T_stale`, the monitoring status shall be DEGRADED and the Guardian shall report a freshness fault. Fresh is defined in the [output model](#guardian-output-model); samples that are not fresh are ignored. | `T_stale` + `T_react` | Transport outage, transport delay longer than `T_stale`, VSS Publisher stopped, source dropout, duplicate, reorder | Must | tested |
| FSR-2.3 | When no fresh sample arrives for longer than `T_stale`, but at least two frames with an unchanged alive counter arrive meanwhile, the monitoring status shall be DEGRADED and the Guardian shall report a **source** fault (counter stuck) instead of a freshness fault. A single repeated frame, such as a duplicate, does not count. | `T_stale` + `T_react` | Source repeats the same frame (frozen ECU) | Must | tested |
| FSR-2.4 | When the maximum cell temperature stays unchanged while the average or minimum temperature moves by at least `Δ_stuck`, the monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (stuck), but not before the maximum has been unchanged for `T_stuck`. However slowly the battery heats, the fault shall be detected before the average or minimum has moved by more than `Δ_stuck` plus one CAN step (1 °C). | `T_react` after both conditions hold; hidden error ≤ `Δ_stuck` + 1 °C | Stuck maximum with fast heating and with very slow heating; slow nominal heating as a negative test | Must | tested |
| FSR-2.5 | While the monitoring status is DEGRADED, the thermal state shall not be lowered. It may still be raised as described in the [output model](#guardian-output-model). | — | Every SG-2 fault injected during WARNING and during CRITICAL | Must | implemented |
| FSR-2.6 | Each active fault shall recover only after `N_recover` consecutive fresh, valid samples spanning at least `T_recover` without the fault condition; a gap, an invalid sample, or a repeated frame restarts the window. The monitoring status shall return from DEGRADED to OK only when every active fault has recovered. The thermal state shall then be reassessed from fresh data, following FSR-1.5. | — | Recovery after each SG-2 fault ends | Should | tested |
| FSR-2.7 | The Guardian shall publish a heartbeat every `T_hb_period`. The Evidence Collector shall record a heartbeat missing for longer than `T_hb` as a Guardian failure. The runtime shall restart a terminated Guardian. | `T_hb` + `T_react` | Guardian killed, Guardian paused | Could | implemented |
| FSR-2.8 | When the clocks of source and Guardian are synchronized (enabled by configuration), a sample whose source timestamp is older than `T_age` shall not count as fresh. | `T_react` | Constant transport delay longer than `T_age` | Could | planned |

### SG-2, SG-3, SG-4: Invalid input

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-3.1 | A fresh sample that violates `Min ≤ Avg ≤ Max` shall not be used as a valid measurement. The monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (order implausible). | `T_react` | Minimum above average, average above maximum, minimum above maximum | Must | tested |
| FSR-3.2 | A fresh sample whose maximum, average, or minimum lies outside `[θ_min, θ_max]` shall not be used as a valid measurement. The monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (out of range). If the maximum is above `θ_max`, the thermal state shall also be WARNING or more severe, because the cause may be a real fire. | `T_react` | Out-of-range high | Must | tested |
| FSR-3.3 | A fresh sample whose maximum rose by more than `r_max` × Δt + `Δ_res` since the last valid sample, with Δt from the source timestamps, shall not be used as a valid measurement. The monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (rate implausible), and the thermal state shall be WARNING or more severe, because the cause may be a real thermal runaway. | `T_react` | Spike | Must | tested |
| FSR-3.4 | A fresh sample whose quality flag is not `VALID` shall not be used as a valid measurement. The monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (quality invalid). The source itself declares the value unusable, so no debounce applies. | `T_react` | Quality `INVALID`, quality `ERROR_NOT_AVAILABLE` | Must | tested |
| FSR-3.5 | An isolated invalid sample shall set the monitoring status to SUSPECT and be discarded. `N_suspect` invalid samples within `T_suspect` shall set the monitoring status to DEGRADED. | `T_suspect` + `T_react` | Single spike versus repeated spikes | Should | planned |
| FSR-3.6 | Invalid input shall never lower the thermal state. Invalid input alone shall never raise the thermal state to CRITICAL and never cause the overtemperature mitigation; only valid samples can. (HARA DFR-4, see [Warning versus mitigation](#safety-goals).) | — | Every invalid-input fault, injected below CRITICAL; a valid critical sample as positive control | Must | tested |
| FSR-3.7 | A fresh sample whose alive counter did not advance by exactly one shall be reported as a counter error. An isolated counter error shall set the monitoring status to SUSPECT. `N_suspect` counter errors within `T_suspect` shall set the monitoring status to DEGRADED. | `T_suspect` + `T_react` | Lost frames, counter jumps | Should | planned |

### DG-1: Diagnostic traceability

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-D.1 | Every fault the Guardian reports shall be written to the DFM. The fault code shall identify the requirement that detected the fault. A failed or delayed write shall not delay the safety reaction. | `T_report` | Every Must campaign | Must | tested |
| FSR-D.2 | Every DFM fault record shall be visible through OpenSOVD. The Evidence Collector checks this. | `T_diag` | Every Must campaign | Must | tested |
| FSR-D.3 | When a DFM write fails or is delayed beyond `T_report`, the Guardian shall report the failed write as a diagnostic fault once the DFM is reachable again. | — | Delayed DFM write | Should | planned |
| FSR-D.4 | When a DFM record is not visible through OpenSOVD within `T_diag`, the Evidence Collector shall report it. | `T_diag` | Partial OpenSOVD visibility | Should | planned |

## Evidence Collector requirements

These requirements explain faults; they do not protect the occupants. The
Guardian's safety reaction is the same whatever caused a fault, so these
requirements belong to the Evidence Collector, which sees several tap points on
one clock.

| ID | Requirement | Test with | Prio | Status |
|----|-------------|-----------|------|--------|
| EC-1 | When the Guardian reports a freshness fault (FSR-2.2), the Evidence Collector shall attribute it to the source, the VSS Publisher, or the transport, by comparing where the sample stream stopped: in the Data Broker, at the VSS Publisher output, or at the Guardian input. | Source dropout, VSS Publisher stopped, transport outage | Should | planned |
| EC-2 | The Evidence Collector shall detect duplicated, out-of-order, and missing samples at the Guardian input by their sequence numbers, and report them as transport faults. | Duplicate, reorder, drop | Could | planned |
| EC-3 | The Evidence Collector shall measure the transport delay between the Data Broker and the Guardian input on its own clock, and report it per scenario. This makes a constant delay visible, which the Guardian cannot detect without synchronized clocks. | Constant transport delay | Could | planned |

## Parameters

For parameters the Guardian already implements, the value is only in its
[config](../../config/guardian/safety-params.toml), which is the single source of truth. For the others, the
table proposes a value. All values will be tuned after measuring real latencies
in the end-to-end setup.

| Parameter | Value | Meaning | Used by |
|-----------|-------|---------|---------|
| `θ_warn` | [config](../../config/guardian/safety-params.toml) | Warning threshold for the maximum cell temperature | FSR-1.1 |
| `θ_crit` | [config](../../config/guardian/safety-params.toml) | Critical threshold for the maximum cell temperature | FSR-1.2 |
| `T_stale` | [config](../../config/guardian/safety-params.toml) | Freshness timeout, three times the 100 ms signal cycle | FSR-2.2, FSR-2.3 |
| `T_stuck` | [config](../../config/guardian/safety-params.toml) | Maximum time the maximum may stay frozen while other signals change | FSR-2.4 |
| `Δ_stuck` | [config](../../config/guardian/safety-params.toml) | Minimum change of average or minimum that makes a frozen maximum suspicious. Two steps of the 1 °C CAN resolution, so that a single-step flicker does not count | FSR-2.4 |
| `T_react` | 100 ms | Reaction time of the Guardian once a condition is observable, from the HARA (DFR-1) | most FSRs |
| `T_report` | 200 ms | Maximum latency of a DFM write | FSR-D.1, FSR-D.3 |
| `T_diag` | 2000 ms | Maximum latency until a DFM record is visible through OpenSOVD | FSR-D.2, FSR-D.4 |
| `θ_hyst` | [config](../../config/guardian/safety-params.toml) | Hysteresis below a threshold before the thermal state is lowered | FSR-1.5 |
| `θ_min`, `θ_max` | [config](../../config/guardian/safety-params.toml) | Plausible cell temperature range. The CAN signal (whole degrees, no offset, 0 to 255 °C) cannot represent values below 0 °C | FSR-3.2 |
| `Δ_hotspot` | 10 °C | Spread between maximum and average that indicates a local hot spot | FSR-1.4 |
| `r_trend` | 1 °C/s | Rise rate that indicates a dangerous trend | FSR-1.3 |
| `T_trend` | 5 s | Minimum duration of a dangerous trend | FSR-1.3 |
| `r_max` | [config](../../config/guardian/safety-params.toml) | Rise rate of the maximum above which a sample counts as implausible | FSR-3.3 |
| `Δ_res` | [config](../../config/guardian/safety-params.toml) | Resolution of the temperature signal; a rise of one step is always plausible, however close the source timestamps are | FSR-3.3 |
| `T_age` | 1000 ms | Maximum age of a sample when clocks are synchronized | FSR-2.8 |
| `N_suspect`, `T_suspect` | 3 samples in 1 s | Debounce before invalid samples or counter errors lead to DEGRADED | FSR-3.5, FSR-3.7 |
| `N_recover` | [config](../../config/guardian/safety-params.toml) | Consecutive valid samples required to recover or to lower the thermal state | FSR-1.5, FSR-2.6 |
| `T_recover` | [config](../../config/guardian/safety-params.toml) | Minimum duration of a recovery window, in addition to `N_recover` samples | FSR-1.5, FSR-2.6 |
| `T_mitigation` | 10 s | Time after which a continued rise counts as failed mitigation | FSR-1.7 |
| `T_hb_period` | 500 ms | Guardian heartbeat period | FSR-2.7 |
| `T_hb` | 1500 ms | Heartbeat timeout | FSR-2.7 |

## Traceability

Every fault class of the challenge, the name the [project plan](../reference/project-plan.md)
uses for it, and the requirements that handle it.

| Fault class | Fault | Name in the project plan | Requirements | Goal |
|-------------|-------|--------------------------|--------------|------|
| Thermal | Over-temperature, fast rise | — | FSR-1.1, FSR-1.2, FSR-1.3, FSR-1.5 | SG-1 |
| Thermal | Mitigation requested, mitigation failed | — | FSR-1.6, FSR-1.7 | SG-1 |
| Transport | Outage, delay at onset | CAN Message Timeout | FSR-2.2 (detection), EC-1 (attribution) | SG-2 |
| Transport | Constant delay | — | FSR-2.8 (only with synchronized clocks), EC-3 (measurement) | SG-2 |
| Transport | Duplicate, reorder, drop | CAN Counter Error | FSR-2.2 (ignored; persistent loss detected), FSR-3.7, EC-2 (diagnosis) | SG-2 |
| Source | Dropout, replay interruption | Missing or Outdated Temperature Data | FSR-2.2 (detection), EC-1 (attribution) | SG-2 |
| Source | Frozen source repeating the same frame | CAN Counter Stuck | FSR-2.3 | SG-2 |
| Source | No data after startup | — | FSR-2.1 | SG-2 |
| Signal | Stuck value | Temperature Sensor Stuck / No Temperature Change over Time | FSR-2.4 | SG-2 |
| Signal | Quality flag not `VALID` | Invalid Quality | FSR-3.4 | SG-2 |
| Signal | Drift upward, local hot spot | Excessive Temperature Difference | FSR-1.4 (treated as a real hot spot, not as a sensor fault) | SG-1 |
| Signal | Drift downward | CellTempMin > CellTempAvg, CellTempAvg > CellTempMax, CellTempMin > CellTempMax | FSR-3.1 (once the maximum falls below the average) | SG-4 |
| Signal | Spike | Implausible Temperature Value | FSR-3.3, FSR-3.6 | SG-3, SG-4 |
| Signal | Out-of-range | Temperature Value Out of Range | FSR-3.2, FSR-3.6 | SG-3, SG-4 |
| Guardian | Crash, hang | — | FSR-2.7 | SG-2 |
| Diagnostics | Delayed DFM write | — | FSR-D.1, FSR-D.3 | DG-1 |
| Diagnostics | Partial OpenSOVD visibility | — | FSR-D.2, FSR-D.4 | DG-1 |

FSR-2.5, FSR-2.6, and FSR-3.6 apply to every fault in SG-2, SG-3, and SG-4.

## Known limitations

- **Constant transport delay.** Without synchronized clocks, the Guardian only
  sees the gap when a delay starts. If every sample is delayed by the same
  amount, samples keep arriving at the normal rate, and the data is old without
  the Guardian noticing. FSR-2.8 closes this gap only when clocks are
  synchronized, for example when source and Guardian run on the same host.
- **Aggregated signals.** The Guardian receives the maximum, average, and minimum
  computed over all cells. If the sensor of the hottest cell drifts low, the
  maximum shows the next-hottest cell, and nothing looks implausible. A drift
  that affects all sensors equally is not detectable either.
- **CAN encoding.** The temperatures are sent in whole degrees with no offset, so
  the CAN signal covers 0 °C to 255 °C. Temperatures below 0 °C, which
  are normal for a parked vehicle in winter, cannot be represented and reach the
  Guardian as 0 °C. The 1 °C resolution also limits how small a change the
  detectors can see.
- **Dropped frames on the way.** The counter checks rely on every CAN frame
  reaching the Guardian (A-2). In the measured signal chain, about 7 % of the
  frames are lost between the CAN provider and the VSS Publisher (see
  [Run the Signal Chain](../how-to/run-signal-chain.md)). The Guardian then sees
  counter gaps that are not real CAN faults. This is why FSR-3.7 is debounced
  and not part of the Must scope.
- **No debounce yet.** FSR-3.5 is not implemented, so a single invalid sample
  already leads to DEGRADED and the "monitoring unavailable" warning. This fails
  toward warning; it may cause false warnings (HE-6) until FSR-3.5 debounces
  isolated samples.
- **Source timestamps are Data Broker times.** The VSS Publisher uses the time
  the Data Broker received a value, not the time the CAN frame was sent. In the
  real chain, the first frames of a replay reach the Data Broker in a burst, 2 ms
  apart instead of 100 ms. The rate check (FSR-3.3) therefore allows one
  resolution step (`Δ_res`) regardless of the time between samples; without it,
  a normal 1 °C step raised a false alarm.
- **A sustained high value becomes valid.** FSR-3.3 compares with the last valid
  sample. If an in-range high value persists, its implied rise rate falls below
  `r_max` after a few seconds, the sample becomes valid, and FSR-1.2 raises
  CRITICAL. A real thermal runaway is therefore escalated after a short delay,
  while a single spike is not.
- **Spike or runaway.** A single sample that rises faster than `r_max` cannot be
  told apart from a real thermal runaway. FSR-3.3 therefore raises a warning,
  accepting a possible false alarm.
- **Mitigation effect.** With a replayed temperature profile, the temperature does
  not react to mitigation. FSR-1.7 can only be shown with a profile that is
  prepared accordingly, or with a simulated thermal model.
- **Recovery.** FSR-1.5 and FSR-2.6 are implemented with configurable 2 °C
  hysteresis, ten consecutive fresh, valid, healthy samples, and a minimum
  one-second observation period. Interruptions reset recovery. Critical →
  Warning and Warning → Monitoring each require a separate period and never
  occur while DEGRADED. Monitoring returns to OK only after every active fault
  recovers. A stuck maximum must change before its recovery period starts.
  FaultRecovered events link to detection events and become DFM Passed records;
  OpenSOVD testFailed clears while history and occurrence counts are preserved.
  Diagnostic delivery is asynchronous; monitoring recovery does not wait for DFM.
  These parameters remain proposals, not validated vehicle safety values.
- **Guardian failure in the vehicle.** Nothing in the vehicle reacts to a dead
  Guardian: FSR-2.7 only records the failure and restarts a terminated
  Guardian. Warning the occupants would need an independent monitor, for
  example in the HMI.
- **FSR-2.7 as built.** The Guardian publishes its heartbeat from the loop that
  runs its core, so a hung core stops it too. A separate
  [watchdog](../../watchdog/README.md) process reports
  `BTG_GuardianHeartbeatLoss` to DFM when the heartbeat is missing for longer
  than `T_hb`, and Passed on the next one; Docker restarts a terminated
  Guardian. Verified by hand against OpenSOVD: SIGKILL reported 1.4 s after the
  kill, `docker pause` (a hang, which Docker itself does not notice) 1.8 s after
  the pause, both Passed again on the next heartbeat. The Evidence Collector
  does not read the heartbeat yet, and no campaign scenario covers it, so the
  status is **implemented**, not **tested**. Under heavy host load (Rust builds,
  about 10 minutes) the watchdog reported 19 false losses for a running
  Guardian: `T_hb` = 1500 ms has little margin against scheduling stalls on a
  developer machine.
- **Diagnostic link.** Guardian session/event IDs and last-sample references are
  stored in DFM environment data and verified in integration tests through the
  individual OpenSOVD fault endpoint. Guardian performs no OpenSOVD polling;
  deployed verification belongs in a future evidence collector. DFM exposes a current snapshot, not a complete occurrence
  history; the Evidence Collector must preserve snapshots per campaign.
- **Timing values** are not derived from a thermal model of a real battery pack.

After startup, monitors remain NotTested until sustained healthy input produces
`FaultTestPassed`. This also clears a previous session's current DFM failure
without deleting its history or inventing a detection cause in the new session.
The first stuck-signal monitor test observes more than the configured three-second
stuck interval. Subsequent detected faults recover through `FaultRecovered`.
Only `testFailed` clearing is required; historical confirmation and warning bits
follow DFM's lifecycle/reset policy.

Signal-stuck testing and recovery require the full three-second detector observation
period plus the one-second healthy confirmation period. A maximum that jumps once
and freezes again while reference temperatures move does not recover. The local
`third-party/fault-lib` patch blocks newer IPC records behind older retries;
Guardian still performs no OpenSOVD polling. Delivery remains best effort, with
bounded queues and the upstream retry limit. The outage campaign restores healthy
input while diagnostics are paused, then verifies the final Passed state after resume.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The diagnostics implementation status and correlation limitations were updated
with assistance from **Codex** using **GPT-6.1 Sol** (`gpt-6.1-sol`).

The FSR-2.7 status and watchdog results were updated with assistance from
**Claude Code** using **Claude Opus 5.5** (`claude-opus-5-5`).
