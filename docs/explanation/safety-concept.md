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
> This is an illustrative safety analysis for the hackathon. It is not a full
> hazard analysis and risk assessment: we do not assign integrity levels, because
> we have no real exposure or controllability data.

## Summary

- **Three safety goals:** warn in time (SG-1), never lose monitoring silently
  (SG-2), never let implausible data lower caution (SG-3).
- **Guiding principle:** when in doubt, fail toward warning.
- **Two outputs:** the Guardian reports a *thermal state* (how hot) and a
  *monitoring status* (how trustworthy the input is) separately, so a signal loss
  never hides a hot battery.
- **Demo scope:** the 8 **Must** requirements cover the challenge's recommended
  demo: baseline thresholds, a transport fault, a stuck signal, and diagnostics
  in DFM and OpenSOVD. The 14 **Should** and **Could** requirements show where
  the design goes next. Their status says honestly whether they are done.

## Structure

The concept has three levels. Each level refines the one above it:

1. **Hazards** describe what can go wrong at vehicle level.
2. **Safety goals** say what must never happen. They are abstract on purpose:
   they name no states, components, or numbers, so they stay valid when the
   design changes.
3. **Functional safety requirements (FSR)** refine each goal into concrete,
   testable behavior, using the parameters in [Parameters](#parameters).

Fault campaigns reference FSR IDs, FSRs reference safety goals, and safety goals
reference hazards. This gives the evidence chain
hazard → safety goal → fault → detection → mitigation → verdict.

## Hazards

| ID | Hazard | Example cause |
|----|--------|---------------|
| H-1 | A dangerous battery thermal condition develops without a timely occupant warning. | Over-temperature, a fast temperature rise, or a local hot spot is not detected. |
| H-2 | Thermal monitoring is lost or corrupted, and nobody notices. | Stale, stuck, or missing temperature data; no data after startup; the Guardian itself crashes or hangs. |
| H-3 | A spurious critical warning occurs. | A single corrupted sample triggers the highest warning level. Frequent false alarms also teach drivers to ignore real ones. |

## Safety goals

| ID | Safety goal | Hazard |
|----|-------------|--------|
| SG-1 | Occupants shall be warned in time when the battery develops toward a dangerous thermal condition. | H-1 |
| SG-2 | A loss or degradation of thermal monitoring shall never go unnoticed. | H-2 |
| SG-3 | Implausible or corrupted data shall never lead to a less cautious thermal assessment. | H-2, H-3 |

**Guiding principle for all goals:** when in doubt, fail toward warning, never
toward a lower thermal assessment. An implausible high reading may come from a
broken sensor or from a real thermal runaway. Either way, the system keeps
warning.

H-3 has no safety goal of its own, because a false alarm is less severe than a
missed one. FSR-3.6 limits its impact: invalid data may raise a warning, but
never the critical level.

### Diagnostic goal

This goal is not a safety goal, but the challenge's Definition of Done requires it.

| ID | Diagnostic goal |
|----|-----------------|
| DG-1 | Every detected fault shall be traceable in the vehicle diagnostics, linked to the fault that caused it. |

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
| MITIGATING | Same severity as CRITICAL. The mitigation request was acknowledged. |

**Monitoring status:** whether the Guardian can currently trust its input.

| Monitoring status | Meaning |
|-------------------|---------|
| OK | The input is fresh, in order, and plausible. |
| SUSPECT | Isolated invalid samples were seen and discarded. No state change yet (debounce). |
| DEGRADED | The input is lost or persistently invalid. The thermal state cannot be assessed reliably. |

**Safe states** and the mitigation events the Guardian publishes for them:

| Safe state | Entered when | Mitigation event |
|------------|--------------|------------------|
| Thermal state CRITICAL | A critical criterion is exceeded. | `DRIVER_WARNING_OVERTEMP` |
| Monitoring status DEGRADED | Monitoring is lost or the input is persistently invalid. | `DRIVER_WARNING_MONITORING_UNAVAILABLE` |

Both can be active at the same time.

## Timing and verdicts

### Timing reference

All times are measured by the **evidence collector on its own clock**. The
collector subscribes passively to the Guardian's input topic (the tap point) and
to all Guardian outputs. No clock synchronization between hosts is required.

- **t0**: the first observation of the fault at the tap point. This is either the
  first faulty sample, or the moment the first expected sample is missing (the
  last valid arrival plus one signal cycle). The injection timestamp recorded by
  the campaign runner serves only as a cross-check, because the runner cannot
  know exactly when a fault takes effect (for example, when a prepared CAN trace
  reaches the faulty frame).
- **t1**: the arrival of the corresponding Guardian event at the collector.

Each FSR has a **latency budget** of the observation window it needs (for
example `T_stuck`) plus the reaction time `T_react`. A requirement that needs no
observation window has a budget of `T_react`.

### Scenario verdicts

| Verdict | Condition |
|---------|-----------|
| PASS | The fault was observed at the tap point, every expected reaction occurred within its latency budget, no forbidden reaction occurred, and the evidence is complete. |
| FAIL | An expected reaction is missing or late, or a forbidden reaction occurred (for example, the thermal state was lowered while monitoring was DEGRADED). |
| INCONCLUSIVE | The fault never reached the tap point (for example, the CAN provider resampled a one-frame spike away), or the evidence is incomplete (for example, a DFM record is missing in a scenario that does not target diagnostics). |

Each campaign defines its expected reactions. For a **diagnostics** fault
campaign, the expected reaction is that the collector detects the missing or
late diagnostic record. If it does, the verdict is PASS.

## Functional safety requirements

Every FSR has a priority for the hackathon (**Must**, **Should**, **Could**) and
a status (**planned**, **implemented**, **tested**). A requirement only counts as
fulfilled when its status is **tested** and it links to the test or campaign
that proves it.

| Priority | Meaning | Requirements |
|----------|---------|--------------|
| Must | Needed for the demo. Implemented and tested first. | FSR-1.1, FSR-1.2, FSR-2.2, FSR-2.4, FSR-2.5, FSR-3.6, FSR-D.1, FSR-D.2 |
| Should | Next, when the Musts are tested. FSR-2.3 comes first: it lets the Guardian tell a source fault from a transport fault. | FSR-1.5, FSR-2.1, FSR-2.3, FSR-2.6, FSR-3.2, FSR-3.3, FSR-3.5 |
| Could | Only if time is left. | FSR-1.3, FSR-1.4, FSR-1.6, FSR-1.7, FSR-2.7, FSR-3.1, FSR-3.4 |

### SG-1: Timely thermal warning

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-1.1 | When the maximum cell temperature of a valid sample reaches `θ_warn`, the thermal state shall be WARNING or more severe. | `T_react` | Nominal heating profile | Must | planned |
| FSR-1.2 | When the maximum cell temperature of a valid sample reaches `θ_crit`, the thermal state shall be CRITICAL and the Guardian shall publish `DRIVER_WARNING_OVERTEMP`. | `T_react` | Nominal heating profile to the critical limit | Must | planned |
| FSR-1.3 | When valid samples show a temperature rise of at least `r_trend` sustained for `T_trend`, the thermal state shall be WARNING or more severe, even below `θ_warn`. | `T_trend` + `T_react` | Fast heating profile below `θ_warn` | Could | planned |
| FSR-1.4 | When the maximum cell temperature exceeds the average by more than `Δ_hotspot`, the thermal state shall be WARNING or more severe. A large spread is treated as a real local hot spot, never as a sensor fault. | `T_react` | Single-cell hot spot, upward drift of the maximum | Could | planned |
| FSR-1.5 | The thermal state shall be lowered only when the triggering criterion has been undercut by the hysteresis `θ_hyst` for `N_recover` consecutive valid samples. | — | Temperature oscillating around `θ_warn` | Should | planned |
| FSR-1.6 | When the mitigation consumer acknowledges a mitigation request, the thermal state shall change from CRITICAL to MITIGATING. Without an acknowledgement within `T_ack`, the Guardian shall stay CRITICAL, repeat the request, and report a fault. | `T_ack` | Nominal critical profile; mitigation consumer stopped | Could | planned |
| FSR-1.7 | When the temperature keeps rising for `T_mitigation` while MITIGATING, the Guardian shall return to CRITICAL and repeat the mitigation request ("mitigation failed"). | `T_mitigation` + `T_react` | Heating profile that continues after the mitigation request | Could | planned |

### SG-2: No silent loss of monitoring

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-2.1 | When no valid sample arrives within `T_startup` after the Guardian starts, the monitoring status shall be DEGRADED and the Guardian shall report a startup fault. | `T_startup` + `T_react` | Guardian started without a source | Should | planned |
| FSR-2.2 | When no message arrives for longer than `T_stale`, the monitoring status shall be DEGRADED and the Guardian shall report a **transport** fault. | `T_stale` + `T_react` | Transport delay, transport outage, publisher stopped | Must | planned |
| FSR-2.3 | When publisher heartbeats keep arriving but the source timestamp does not advance for longer than `T_stale`, the monitoring status shall be DEGRADED and the Guardian shall report a **source** fault. | `T_stale` + `T_react` | Source dropout, replay interruption | Should | planned |
| FSR-2.4 | When the maximum cell temperature stays unchanged for longer than `T_stuck` while the average or minimum temperature changes, the monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (stuck). | `T_stuck` + `T_react` | Stuck maximum value | Must | planned |
| FSR-2.5 | While the monitoring status is DEGRADED, the thermal state shall not be lowered. It may still be raised by FSR-3.2 and FSR-3.3. | — | Every SG-2 fault injected during WARNING and during CRITICAL | Must | planned |
| FSR-2.6 | The monitoring status shall return from DEGRADED to OK only after `N_recover` consecutive valid samples. The thermal state shall then be reassessed from fresh data, following FSR-1.5. | — | Recovery after each SG-2 fault ends | Should | planned |
| FSR-2.7 | The Guardian shall publish a heartbeat every `T_hb_period`. When the heartbeat is missing for longer than `T_hb`, the mitigation consumer shall warn the occupants that monitoring is unavailable, independently of the Guardian. The runtime shall restart a terminated Guardian. | `T_hb` + `T_react` | Guardian killed, Guardian paused | Could | planned |

### SG-3: Implausible data never lowers caution

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-3.1 | A sample that violates `Min ≤ Avg ≤ Max` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (implausible). | `T_react` | Maximum below average (downward drift), swapped values | Could | planned |
| FSR-3.2 | A sample outside `[θ_min, θ_max]` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (out of range). If the value is above `θ_max`, the thermal state shall also be WARNING or more severe, because the cause may be a real fire. | `T_react` | Out-of-range high, out-of-range low | Should | planned |
| FSR-3.3 | A sample that implies a rise faster than `r_max` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (implausible), and the thermal state shall be WARNING or more severe, because the cause may be a real thermal runaway. | `T_react` | Spike | Should | planned |
| FSR-3.4 | A duplicated or out-of-order sample shall be discarded. The Guardian shall report a **transport** fault (sequence). A gap in the sequence shall also be reported as a transport fault. | `T_react` | Duplicate, reorder, drop | Could | planned |
| FSR-3.5 | An isolated invalid sample shall set the monitoring status to SUSPECT and be discarded. `N_suspect` invalid samples within `T_suspect` shall set the monitoring status to DEGRADED. | `T_suspect` + `T_react` | Single spike versus repeated spikes | Should | planned |
| FSR-3.6 | Invalid input shall never lower the thermal state. Invalid input alone shall never raise the thermal state to CRITICAL; only valid samples can do that. | — | Every SG-3 fault, injected during WARNING | Must | planned |

### DG-1: Diagnostic traceability

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-D.1 | Every fault the Guardian reports shall be written to the DFM. The fault code shall identify the fault model entry. A failed or delayed write shall not delay the safety reaction. Once the DFM is reachable again, the Guardian shall report the failed write as a diagnostic fault. | `T_report` | Delayed DFM write | Must | planned |
| FSR-D.2 | Every DFM fault record shall be visible through OpenSOVD. The evidence collector checks this. | `T_diag` | Partial OpenSOVD visibility | Must | planned |

## Parameters

These values are proposals. We will tune them after measuring real latencies in
the end-to-end setup. The Guardian's parameter file will become the single source
of truth; this table then only explains the values.

| Parameter | Proposed value | Meaning |
|-----------|---------------:|---------|
| `θ_warn` | 45 °C | Warning threshold for the maximum cell temperature |
| `θ_crit` | 55 °C | Critical threshold for the maximum cell temperature |
| `θ_hyst` | 2 °C | Hysteresis below a threshold before the thermal state is lowered |
| `θ_min`, `θ_max` | −40 °C, 125 °C | Physical sensor range, matching the CAN signal definition |
| `Δ_hotspot` | 10 °C | Spread between maximum and average that indicates a local hot spot |
| `r_trend` | 1 °C/s | Rise rate that indicates a dangerous trend |
| `T_trend` | 5 s | Minimum duration of a dangerous trend |
| `r_max` | 20 °C/s | Rise rate above which a sample counts as implausible |
| `T_stuck` | 3 s | Maximum time the maximum may stay frozen while other signals change |
| `T_stale` | 300 ms | Freshness timeout, three times the 100 ms signal cycle |
| `T_startup` | 5 s | Maximum time after start until the first valid sample |
| `N_suspect`, `T_suspect` | 3 samples in 1 s | Debounce before invalid samples lead to DEGRADED |
| `N_recover` | 10 samples | Consecutive valid samples required to recover or to lower the thermal state |
| `T_react` | 500 ms | Reaction time of the Guardian once a condition is observable |
| `T_ack` | 1000 ms | Maximum time until the mitigation consumer acknowledges a request |
| `T_mitigation` | 10 s | Time after which a continued rise counts as failed mitigation |
| `T_hb_period` | 500 ms | Guardian heartbeat period |
| `T_hb` | 1500 ms | Heartbeat timeout |
| `T_report` | 200 ms | Maximum latency of a DFM write |
| `T_diag` | 2000 ms | Maximum latency until a DFM record is visible through OpenSOVD |

## Traceability overview

| Fault class | Fault | Requirements | Safety goal |
|-------------|-------|--------------|-------------|
| Thermal (nominal) | Over-temperature, fast rise, hot spot | FSR-1.1 to FSR-1.5 | SG-1 |
| Thermal (nominal) | Mitigation not acknowledged, mitigation failed | FSR-1.6, FSR-1.7 | SG-1 |
| Transport | Delay, outage | FSR-2.2 | SG-2 |
| Transport | Duplicate, reorder, drop | FSR-3.4 | SG-3 |
| Signal | Stuck value | FSR-2.4 | SG-2 |
| Signal | Drift upward | FSR-1.4 (treated as a real hot spot) | SG-1 |
| Signal | Drift downward | FSR-3.1 (once the maximum falls below the average) | SG-3 |
| Signal | Spike | FSR-3.3, FSR-3.5 | SG-3 |
| Signal | Out-of-range | FSR-3.2, FSR-3.5 | SG-3 |
| Source | Dropout, replay interruption | FSR-2.3 | SG-2 |
| Source | No data after startup | FSR-2.1 | SG-2 |
| Guardian | Crash, hang | FSR-2.7 | SG-2 |
| Diagnostics | Delayed DFM write, partial OpenSOVD visibility | FSR-D.1, FSR-D.2 | DG-1 |

FSR-2.5, FSR-2.6, and FSR-3.6 apply to every fault in SG-2 and SG-3.

## Known limitations

- **Aggregated signals.** The Guardian receives the maximum, average, and minimum
  computed over all cells. If the sensor of the hottest cell drifts low, the
  maximum shows the next-hottest cell, and nothing looks implausible. A drift
  that affects all sensors equally is not detectable either.
- **Frozen source.** If the source keeps sending the same frame with all values
  constant, the timestamps stay fresh and no value changes. This looks like a
  battery at constant temperature. Detecting it needs an alive counter inside the
  source frame, which our current signal definition does not have.
- **Spike or runaway.** A single sample that rises faster than `r_max` cannot be
  told apart from a real thermal runaway. FSR-3.3 therefore raises a warning,
  accepting a possible false alarm.
- **Mitigation effect.** With a replayed temperature profile, the temperature does
  not react to mitigation. FSR-1.7 can only be shown with a profile that is
  prepared accordingly, or with a simulated thermal model.
- **Hang detection.** It is not yet verified whether the runtime can detect a hung
  Guardian, as opposed to a terminated one. FSR-2.7 therefore relies on the
  mitigation consumer for the safety reaction.
- **Diagnostic link.** It is not yet verified whether a DFM record can carry an ID
  per occurrence. Until then, the link from a Guardian event to a DFM record uses
  the fault code and the time window.
- **Timing values** are not derived from a thermal model of a real battery pack.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
