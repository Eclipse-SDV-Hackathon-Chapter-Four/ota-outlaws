<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Scenario Generator

> **Status: design, partly implemented.** The traces exist
> ([`Fault_Injection_CAN_Logs/`](../../../Fault_Injection_CAN_Logs/README.txt)), and
> [`diagnostics/smoke_test.py`](../../../diagnostics/smoke_test.py) already runs
> campaigns, but with a test publisher instead of the CAN chain.

The Scenario Generator fills the *campaign runner* role of the
[Safety Concept](../../explanation/safety-concept.md#test-roles): it generates
the faulted CAN traces and executes the scenarios. The
[Evidence Collector](evidence-collector.md) judges them. One command runs one
scenario or the whole campaign:

```sh
python3 campaigns/run.py counter_stuck
python3 campaigns/run.py --all
```

The challenge asks for this to be automated: it rejects one-off manual runs,
because they cannot be replayed, and remote reruns through openDUT need a
single command to trigger.

## Scenario catalog

`campaigns/scenarios.toml` is the single description of every scenario. Three
programs read it:

| Reader | Uses |
|--------|------|
| Trace generator (`generate_asc_logs.py`) | How to build the faulted trace |
| Campaign runner (`campaigns/run.py`) | Which trace to replay, which extra injection to do, how long to wait |
| Evidence Collector | The fault onset, the expected and forbidden reactions, the traceability |

Today the scenario definitions are hard-coded in `generate_asc_logs.py`; the
generator will read them from the catalog instead.

```toml
[[scenario]]
id = "counter_stuck"
description = "AliveCounter remains fixed during the middle fault segment."
fault_class = "Source"
hazard = "H-2"
safety_goal = "SG-2"
requirements = ["FSR-2.3", "FSR-2.5", "FSR-2.6", "FSR-D.1", "FSR-D.2"]
trace = "Fault_Injection_CAN_Logs/counter_stuck.asc"
onset = "alive_counter_repeats"

[scenario.generate]              # read by the trace generator
lead_in_frames = 40
fault_frames = 20
recovery_frames = 40
counter = "stuck"

[[scenario.expect]]
fault = "BTG_TempCounterStuck"
budget = "T_stale + T_react"     # resolved from config/guardian/safety-params.toml

[[scenario.expect]]
monitoring = "DEGRADED"
caused_by = "fault"

[[scenario.expect]]
mitigation = "DRIVER_WARNING_MONITORING_UNAVAILABLE"
caused_by = "monitoring"

[[scenario.expect]]
sovd = "BTG_TempCounterStuck"
budget = "T_diag"

[[scenario.expect]]
recovery = "BTG_TempCounterStuck"
```

An optional `[scenario.inject]` table describes an injection beyond the trace,
for example `pause = ["opensovd-dfm", "opensovd-gateway"]` for the diagnostics
outage.

## Running a scenario

For each scenario, the runner:

1. starts a fresh Compose project: Data Broker, Zenoh, VSS Publisher, Guardian,
   DFM, and OpenSOVD, so that every scenario has its own Guardian session (A-4);
2. starts `evidence-collector record` and writes `manifest.json`;
3. starts the KUKSA CAN Provider with the scenario's trace, replayed **once**
   (`CANDUMP_FILE`, without `--infinite`);
4. performs the extra injection, if any, and logs it with its time;
5. waits until the trace has ended plus a recovery margin, then stops the
   recording;
6. runs `evidence-collector evaluate`, collects the service logs, and removes
   the Compose project.

The evaluation window ends with the last recorded frame. The loss of data after
the replay ends is expected and is not judged.

`manifest.json` holds the run's facts; the expectations stay in the catalog:

```json
{
  "run_id": "20261007-101203-ab12",
  "scenario": "counter_stuck",
  "trace": {"path": "Fault_Injection_CAN_Logs/counter_stuck.asc", "sha256": "3f2a…"},
  "git_revision": "2b1f82c",
  "images": {"guardian": "sha256:…", "vss-publisher": "sha256:…"},
  "injections": [{"action": "start_can_provider", "runner_time": "2026-10-07T10:12:05.120Z"}]
}
```

The runner's times are only a cross-check; the Evidence Collector takes the
fault onset from its own observation.

## Scenarios and expected reactions

What the Guardian should do with each trace, by the current requirements:

| Scenario | Fault | Requirements | Expected | Status of the requirement |
|----------|-------|--------------|----------|---------------------------|
| `normal` | none | — | no fault, MONITORING | — |
| `heating` (new, from `can/BMS_MSG1_CAN.asc`) | none, 40 °C to 55 °C | FSR-1.1, FSR-1.2 | WARNING, then CRITICAL with overtemperature warning | implemented |
| `timeout` | 1.8 s gap | FSR-2.2 | `BTG_TempFreshnessLost`, DEGRADED, warning, OpenSOVD, recovery | tested |
| `counter_stuck` | counter fixed for 2 s | FSR-2.3 | `BTG_TempCounterStuck`, … | tested |
| `invalid_quality` | quality 00/FF for 2 s | FSR-3.4 | `BTG_TempQualityInvalid`, … | tested |
| `temp_stuck` | all temperatures fixed for 2 s | FSR-2.4 | see below | tested |
| `diagnostics_outage` (from `smoke_test.py`) | DFM and OpenSOVD paused | FSR-D.1 | safety reaction on time; OpenSOVD catches up after resume | tested |
| `counter_error` | counter skips 39 → 45 | FSR-3.7 | counter error, SUSPECT | planned |
| `out_of_range` | maximum 250 °C | FSR-3.2 | signal fault, at least WARNING | planned |
| `implausible_jump` | jump to 160 °C | FSR-3.3 | signal fault, at least WARNING | planned |
| `min_gt_avg`, `avg_gt_max`, `min_gt_max` | Min ≤ Avg ≤ Max violated | FSR-3.1 | signal fault | planned |
| `high_delta` | Max 100 °C, Min 10 °C | FSR-1.4 | WARNING (hot spot) | planned |

Scenarios for planned requirements run as well and are reported honestly: they
fail until the requirement is implemented. Some fail in an instructive way: the
Guardian takes 250 °C and 160 °C as valid samples and reaches CRITICAL without a
signal fault, which FSR-3.6 forbids.

### Two traces need a change

- **`temp_stuck`** does not test FSR-2.4. It freezes the maximum, average, and
  minimum together, which looks exactly like a battery at constant temperature;
  FSR-2.4 only detects a frozen maximum while the others move. Its fault segment
  of 2 s is also shorter than `T_stuck` (3 s). For FSR-2.4 the trace needs the
  maximum frozen while the average and minimum rise by at least `Δ_stuck`, for
  more than `T_stuck` plus `T_react`. The current trace stays useful as a
  negative test of a known limitation: expected is *no* fault.
- **A heating trace is missing.** `normal` stays at 40 °C, so no trace reaches
  `θ_warn` or `θ_crit`. `can/BMS_MSG1_CAN.asc` heats from 40 °C to 55 °C and can
  serve until the generator produces one.

## Relation to the existing campaign script

`campaigns/run.py` grows out of `diagnostics/smoke_test.py`: it keeps the fresh
Compose project per scenario, the evidence directory, the pause and resume for
the diagnostics outage, the log collection, and the rule that a failure is never
overwritten. It replaces the test publisher with the CAN chain and the
hard-coded scenario list with the catalog.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
