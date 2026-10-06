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
- **Demo scope:** the 7 **Must** requirements form a self-contained set that
  covers the challenge's recommended demo: baseline thresholds, loss of fresh
  data, a stuck signal, and diagnostics in DFM and OpenSOVD. The 16 **Should** and
  **Could** requirements show where the design goes next. Their status says
  honestly whether they are done.

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

## Test roles

Two roles take part in every fault campaign. One acts on the system, the other
only observes it.

| Role | What it does | Who fills it |
|------|--------------|--------------|
| **Campaign runner** | Executes a campaign. It prepares the system (for example, starts a fresh Guardian, see A-4), starts the temperature source, injects the faults the campaign defines, and logs what it injected and when, under a run ID. | At first a **person** who follows the written campaign procedure. Later an **automation script** that executes the same procedure, and finally a **remote trigger** (openDUT) for remote reruns. |
| **Evidence collector** | Observes the Guardian's input (the tap point), all Guardian outputs, and OpenSOVD. It measures latencies, attributes each fault to the layer that caused it (see [Evidence collector requirements](#evidence-collector-requirements)), checks the expected reactions, and writes the verdict report. It never influences the system. | Always an automated component. |

The two roles stay separate, so that the one who injects a fault never judges
its own claims. The collector takes the fault onset from its own observation,
not from the runner's log (see [Timing reference](#timing-reference)). This also
means that a person can act as the campaign runner: imprecise manual timing does
not affect the measured latencies.

A manual run still has to follow a written campaign, and the collector has to
record it. The challenge rejects one-off manual runs, because they cannot be
replayed. Writing every campaign down from the start lets an automation script
take over the procedure unchanged.

## Assumptions

The Guardian's safety behavior relies on these properties of other components.
Each component's own documentation must state how it fulfills them.

| ID | Assumption | Component |
|----|------------|-----------|
| A-1 | Every sample carries the source timestamp (the time the value was captured at its origin) and a sequence number that increases by one per published message. The Guardian uses the source timestamp; the evidence collector uses the sequence number. | VSS uProtocol Publisher |
| A-2 | A sample is published every signal cycle, even if the values have not changed. Otherwise a constant temperature would look like stale data. | VSS uProtocol Publisher, KUKSA CAN Provider configuration |
| A-4 | Each scenario starts with a freshly started Guardian. The campaign runner restarts it through Ankaios, so scenarios cannot influence each other. | Campaign runner, Ankaios |
| A-5 | The evidence collector observes exactly the stream the Guardian receives. All faults are injected upstream of the collector's tap point. | Evidence collector, fault injection |
| A-6 | The mitigation consumer acknowledges mitigation requests and monitors the Guardian heartbeat. In the demo, it is a mock. | Mitigation consumer |

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

**Valid sample:** a sample that passes every check the Guardian implements. A
sample can be valid while the monitoring status is DEGRADED.

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

All times are measured by the **evidence collector on its own clock**. The
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
example `T_stuck`) plus the reaction time `T_react`. A requirement that needs no
observation window has a budget of `T_react`.

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
that proves it.

| Priority | Meaning | Requirements |
|----------|---------|--------------|
| Must | Needed for the demo. A self-contained set: it is safe without the other requirements. Implemented and tested first. | FSR-1.1, FSR-1.2, FSR-2.2, FSR-2.4, FSR-2.5, FSR-D.1, FSR-D.2 |
| Should | Next, when the Musts are tested. | FSR-1.5, FSR-2.1, FSR-2.6, FSR-3.2, FSR-3.3, FSR-3.5, FSR-3.6, FSR-D.3, FSR-D.4 |
| Could | Only if time is left. | FSR-1.3, FSR-1.4, FSR-1.6, FSR-1.7, FSR-2.7, FSR-2.8, FSR-3.1 |

### SG-1: Timely thermal warning

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-1.1 | When the maximum cell temperature of a valid sample reaches `θ_warn`, the thermal state shall be WARNING or more severe. | `T_react` | Nominal heating profile | Must | planned |
| FSR-1.2 | When the maximum cell temperature of a valid sample reaches `θ_crit`, the thermal state shall be CRITICAL and the Guardian shall publish `DRIVER_WARNING_OVERTEMP`. | `T_react` | Nominal heating profile to the critical limit | Must | planned |
| FSR-1.3 | When valid samples show a temperature rise of at least `r_trend` sustained for `T_trend`, the thermal state shall be WARNING or more severe, even below `θ_warn`. | `T_trend` + `T_react` | Fast heating profile below `θ_warn` | Could | planned |
| FSR-1.4 | When the maximum cell temperature exceeds the average by more than `Δ_hotspot`, the thermal state shall be WARNING or more severe. A large spread is treated as a real local hot spot, never as a sensor fault. | `T_react` | Single-cell hot spot, upward drift of the maximum | Could | planned |
| FSR-1.5 | The thermal state shall be lowered only when the triggering criterion has been undercut by the hysteresis `θ_hyst` for `N_recover` consecutive valid samples, and only while the monitoring status is not DEGRADED. | — | Temperature oscillating around `θ_warn` | Should | planned |
| FSR-1.6 | When the mitigation consumer acknowledges a mitigation request, the thermal state shall change from CRITICAL to MITIGATING. Without an acknowledgement within `T_ack`, the Guardian shall stay CRITICAL, repeat the request, and report a fault. | `T_ack` | Nominal critical profile; mitigation consumer stopped | Could | planned |
| FSR-1.7 | When the temperature keeps rising for `T_mitigation` while MITIGATING, the Guardian shall return to CRITICAL and repeat the mitigation request ("mitigation failed"). | `T_mitigation` + `T_react` | Heating profile that continues after the mitigation request | Could | planned |

### SG-2: No silent loss of monitoring

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-2.1 | When no valid sample arrives within `T_startup` after the Guardian starts, the monitoring status shall be DEGRADED and the Guardian shall report a startup fault. | `T_startup` + `T_react` | Guardian started without a source | Should | planned |
| FSR-2.2 | When no **fresh** sample arrives for longer than `T_stale`, the monitoring status shall be DEGRADED and the Guardian shall report a freshness fault. A sample is fresh if its source timestamp is later than that of the previous sample. Repeated, duplicated, and out-of-order samples do not count as fresh and are ignored. | `T_stale` + `T_react` | Transport outage, transport delay longer than `T_stale`, publisher stopped, source dropout, duplicate, reorder | Must | planned |
| FSR-2.4 | When the maximum cell temperature stays unchanged for longer than `T_stuck` while the average or minimum temperature changes by at least `Δ_stuck`, the monitoring status shall be DEGRADED and the Guardian shall report a **signal** fault (stuck). | `T_stuck` + `T_react` | Stuck maximum value; slow nominal heating profile as a negative test | Must | planned |
| FSR-2.5 | While the monitoring status is DEGRADED, the thermal state shall not be lowered. It may still be raised as described in the [output model](#guardian-output-model). | — | Every SG-2 fault injected during WARNING and during CRITICAL | Must | planned |
| FSR-2.6 | The monitoring status shall return from DEGRADED to OK only after `N_recover` consecutive valid samples. The thermal state shall then be reassessed from fresh data, following FSR-1.5. | — | Recovery after each SG-2 fault ends | Should | planned |
| FSR-2.7 | The Guardian shall publish a heartbeat every `T_hb_period`. When the heartbeat is missing for longer than `T_hb`, the mitigation consumer shall warn the occupants that monitoring is unavailable, independently of the Guardian. The runtime shall restart a terminated Guardian. | `T_hb` + `T_react` | Guardian killed, Guardian paused | Could | planned |
| FSR-2.8 | When the clocks of source and Guardian are synchronized (enabled by configuration), a sample whose source timestamp is older than `T_age` shall not count as fresh. | `T_react` | Constant transport delay longer than `T_age` | Could | planned |

### SG-3: Implausible data never lowers caution

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-3.1 | A sample that violates `Min ≤ Avg ≤ Max` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (implausible). | `T_react` | Maximum below average (downward drift), swapped values | Could | planned |
| FSR-3.2 | A sample outside `[θ_min, θ_max]` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (out of range). If the value is above `θ_max`, the thermal state shall also be WARNING or more severe, because the cause may be a real fire. | `T_react` | Out-of-range high, out-of-range low | Should | planned |
| FSR-3.3 | A sample that implies a rise faster than `r_max` shall not be used as a valid measurement. The Guardian shall report a **signal** fault (implausible), and the thermal state shall be WARNING or more severe, because the cause may be a real thermal runaway. | `T_react` | Spike | Should | planned |
| FSR-3.5 | An isolated invalid sample shall set the monitoring status to SUSPECT and be discarded. `N_suspect` invalid samples within `T_suspect` shall set the monitoring status to DEGRADED. | `T_suspect` + `T_react` | Single spike versus repeated spikes | Should | planned |
| FSR-3.6 | Invalid input shall never lower the thermal state. Invalid input alone shall never raise the thermal state to CRITICAL; only valid samples can do that. | — | Every SG-3 fault, injected during WARNING | Should | planned |

### DG-1: Diagnostic traceability

| ID | Requirement | Budget | Test with | Prio | Status |
|----|-------------|--------|-----------|------|--------|
| FSR-D.1 | Every fault the Guardian reports shall be written to the DFM. The fault code shall identify the requirement that detected the fault. A failed or delayed write shall not delay the safety reaction. | `T_report` | Every Must campaign | Must | planned |
| FSR-D.2 | Every DFM fault record shall be visible through OpenSOVD. The evidence collector checks this. | `T_diag` | Every Must campaign | Must | planned |
| FSR-D.3 | When a DFM write fails or is delayed beyond `T_report`, the Guardian shall report the failed write as a diagnostic fault once the DFM is reachable again. | — | Delayed DFM write | Should | planned |
| FSR-D.4 | When a DFM record is not visible through OpenSOVD within `T_diag`, the evidence collector shall report it. | `T_diag` | Partial OpenSOVD visibility | Should | planned |

## Evidence collector requirements

These requirements explain faults; they do not protect the occupants. The
Guardian's safety reaction is the same whatever caused a fault, so these
requirements belong to the evidence collector, which sees several tap points on
one clock. FSR-2.3 and FSR-3.4 of earlier drafts moved here as EC-1 and EC-2.

| ID | Requirement | Test with | Prio | Status |
|----|-------------|-----------|------|--------|
| EC-1 | When the Guardian reports a freshness fault (FSR-2.2), the evidence collector shall attribute it to the source, the publisher, or the transport, by comparing where the sample stream stopped: in the Data Broker, at the publisher output, or at the Guardian input. | Source dropout, publisher stopped, transport outage | Should | planned |
| EC-2 | The evidence collector shall detect duplicated, out-of-order, and missing samples at the Guardian input by their sequence numbers, and report them as transport faults. | Duplicate, reorder, drop | Could | planned |
| EC-3 | The evidence collector shall measure the transport delay between the Data Broker and the Guardian input on its own clock, and report it per scenario. This makes a constant delay visible, which the Guardian cannot detect without synchronized clocks. | Constant transport delay | Could | planned |

## Parameters

These values are proposals. We will tune them after measuring real latencies in
the end-to-end setup. The Guardian's parameter file will become the single source
of truth; this table then only explains the values. Parameters marked **Must**
are needed for the Must requirements.

| Parameter | Proposed value | Meaning | Used by | Must |
|-----------|---------------:|---------|---------|:----:|
| `θ_warn` | 45 °C | Warning threshold for the maximum cell temperature | FSR-1.1 | ✓ |
| `θ_crit` | 55 °C | Critical threshold for the maximum cell temperature | FSR-1.2 | ✓ |
| `T_stale` | 300 ms | Freshness timeout, three times the 100 ms signal cycle | FSR-2.2 | ✓ |
| `T_stuck` | 3 s | Maximum time the maximum may stay frozen while other signals change | FSR-2.4 | ✓ |
| `Δ_stuck` | 1 °C | Minimum change of average or minimum that makes a frozen maximum suspicious | FSR-2.4 | ✓ |
| `T_react` | 500 ms | Reaction time of the Guardian once a condition is observable | most FSRs | ✓ |
| `T_report` | 200 ms | Maximum latency of a DFM write | FSR-D.1, FSR-D.3 | ✓ |
| `T_diag` | 2000 ms | Maximum latency until a DFM record is visible through OpenSOVD | FSR-D.2, FSR-D.4 | ✓ |
| `θ_hyst` | 2 °C | Hysteresis below a threshold before the thermal state is lowered | FSR-1.5 | |
| `θ_min`, `θ_max` | −40 °C, 125 °C | Physical sensor range, matching the CAN signal definition | FSR-3.2 | |
| `Δ_hotspot` | 10 °C | Spread between maximum and average that indicates a local hot spot | FSR-1.4 | |
| `r_trend` | 1 °C/s | Rise rate that indicates a dangerous trend | FSR-1.3 | |
| `T_trend` | 5 s | Minimum duration of a dangerous trend | FSR-1.3 | |
| `r_max` | 20 °C/s | Rise rate above which a sample counts as implausible | FSR-3.3 | |
| `T_startup` | 5 s | Maximum time after start until the first valid sample | FSR-2.1 | |
| `T_age` | 1000 ms | Maximum age of a sample when clocks are synchronized | FSR-2.8 | |
| `N_suspect`, `T_suspect` | 3 samples in 1 s | Debounce before invalid samples lead to DEGRADED | FSR-3.5 | |
| `N_recover` | 10 samples | Consecutive valid samples required to recover or to lower the thermal state | FSR-1.5, FSR-2.6 | |
| `T_ack` | 1000 ms | Maximum time until the mitigation consumer acknowledges a request | FSR-1.6 | |
| `T_mitigation` | 10 s | Time after which a continued rise counts as failed mitigation | FSR-1.7 | |
| `T_hb_period` | 500 ms | Guardian heartbeat period | FSR-2.7 | |
| `T_hb` | 1500 ms | Heartbeat timeout | FSR-2.7 | |

## Traceability overview

| Fault class | Fault | Requirements | Safety goal |
|-------------|-------|--------------|-------------|
| Thermal (nominal) | Over-temperature, fast rise, hot spot | FSR-1.1 to FSR-1.5 | SG-1 |
| Thermal (nominal) | Mitigation not acknowledged, mitigation failed | FSR-1.6, FSR-1.7 | SG-1 |
| Transport | Outage, delay at onset | FSR-2.2 (detection), EC-1 (attribution) | SG-2 |
| Transport | Constant delay | FSR-2.8 (only with synchronized clocks), EC-3 (measurement) | SG-2 |
| Transport | Duplicate, reorder, drop | FSR-2.2 (ignored; persistent loss detected), EC-2 (diagnosis) | SG-2 |
| Signal | Stuck value | FSR-2.4 | SG-2 |
| Signal | Drift upward | FSR-1.4 (treated as a real hot spot) | SG-1 |
| Signal | Drift downward | FSR-3.1 (once the maximum falls below the average) | SG-3 |
| Signal | Spike | FSR-3.3, FSR-3.5 | SG-3 |
| Signal | Out-of-range | FSR-3.2, FSR-3.5 | SG-3 |
| Source | Dropout, replay interruption | FSR-2.2 (detection), EC-1 (attribution) | SG-2 |
| Source | No data after startup | FSR-2.1 | SG-2 |
| Guardian | Crash, hang | FSR-2.7 | SG-2 |
| Diagnostics | Delayed DFM write | FSR-D.1, FSR-D.3 | DG-1 |
| Diagnostics | Partial OpenSOVD visibility | FSR-D.2, FSR-D.4 | DG-1 |

FSR-2.5, FSR-2.6, and FSR-3.6 apply to every fault in SG-2 and SG-3.

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
- **No recovery in the Must scope.** Without FSR-1.5 and FSR-2.6, the Guardian
  never lowers its thermal state or leaves DEGRADED. This is safe, but it is only
  practical because each scenario starts with a fresh Guardian (A-4).
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
