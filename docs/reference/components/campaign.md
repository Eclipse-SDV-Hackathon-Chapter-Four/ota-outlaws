<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Campaign Tool

The campaign tool (`campaign/`) runs fault campaigns against the real signal
chain and judges them. It fills both the *fault campaign runner* and the
*evidence collector* building blocks of the challenge, as two parts of one
tool, like a test case on a HIL bench that holds stimulus and verification
together.

```sh
cargo run -p campaign -- run counter_stuck     # one scenario
cargo run -p campaign -- run --all             # every scenario with a CAN trace
cargo run -p campaign -- observe source_dropout --seconds 60   # hardware demo
cargo run -p campaign -- evaluate runs/<campaign>/<scenario>   # judge again
```

Every run writes a report, failed and inconclusive ones included. The exit
code is 0 when every scenario with status `implemented` passed.

## The two parts

| Part | Challenge building block | Attaches to the system at |
|------|--------------------------|----------------------------|
| **Stimulus** | Fault campaign runner | the injection point of the scenario: the CAN source (trace replay), or services it pauses |
| **Evidence** | Evidence collector | the tap points: the Guardian's input and output topics on uProtocol, and OpenSOVD |

Both parts live in one tool, but the verdict uses only what the evidence part
observed, never what the stimulus part believes it injected. The fault onset t0
is the first observation of the fault at the Guardian's input
([Timing reference](../../explanation/safety-concept.md#timing-reference)). So a
fault that is lost on the way is reported as INCONCLUSIVE ("fault not
delivered"), not as PASS, and the same evidence part can judge runs it did not
inject, such as the hardware demo.

## Scenario catalog

[`campaign/scenarios.toml`](../../../campaign/scenarios.toml) holds one test
case per scenario:

```toml
[[scenario]]
id = "counter_stuck"
description = "AliveCounter remains fixed for 2 s while frames keep arriving"
fault_class = "Source"
hazard = "HE-1"
safety_goal = "SG-2"
status = "implemented"            # or "planned": expected to fail until implemented
stimulus = { type = "can_trace", trace = "Fault_Injection_CAN_Logs/counter_stuck.asc" }
onset = "alive_counter_repeats"

[[scenario.expect]]
kind = "fault"
dtc = "BTG_TempCounterStuck"
budget = "T_stale + T_react"      # T_stale from safety-params.toml, T_react from [budgets]
requirement = "FSR-2.3"
```

| Stimulus | Effect |
|----------|--------|
| `can_trace` | Replays the trace once through the KUKSA CAN Provider; optionally pauses services (`pause`, `pause_after_ms`, `pause_for_ms`) |
| `external` | No injection; someone else injects (for example by unplugging the hardware source). Only with `observe`. |

| Onset | t0 = first recorded sample … |
|-------|------------------------------|
| `none` | of the run (nominal scenarios) |
| `gap` | before a gap longer than `T_stale`, plus one cycle |
| `alive_counter_repeats` | whose alive counter equals the previous one |
| `alive_counter_jumps` | whose alive counter jumps by 3 or more (a single lost frame is not a fault) |
| `quality_not_valid` | with a quality other than VALID |
| `max_frozen_while_reference_moves` | that starts a plateau of the maximum while the average or minimum moves by `Δ_stuck` |
| `order_violated` | that violates `Min ≤ Avg ≤ Max` |
| `max_at_least:<°C>` | with a maximum of at least this value |

| Expectation | Met when |
|-------------|----------|
| `fault` | The Guardian reports the DTC within the budget after t0 |
| `degraded` | That fault causes DEGRADED, which causes the monitoring-unavailable warning |
| `sovd` | OpenSOVD shows the DTC failed, with this run's session and event ID, within the budget after the Guardian event |
| `recovery` | The fault recovers (cause: the fault), monitoring returns to OK, and OpenSOVD shows the DTC passed with its history kept |
| `thermal` | The thermal state reaches the state (or a more severe one) within the budget after t0, or after `after` |
| `overtemp_warning` | The change to CRITICAL causes the overtemperature warning |
| `not_thermal` | The thermal state never reaches the state after t0 |
| `no_fault` | No fault is reported during the scenario |

In every scenario these reactions are **forbidden**: lowering the thermal state
while monitoring is DEGRADED (FSR-2.5), a fault before the onset (a false
alarm, SG-4), and events of more than one Guardian session (A-4).

## A run

1. A fresh Docker Compose project per scenario: Data Broker, Zenoh router, VSS
   Publisher, Guardian, DFM, and OpenSOVD. Every scenario gets its own Guardian
   session (A-4), its own ports, and its own container names, so a campaign can
   run while the development stack is up. The DFM and OpenSOVD image is built
   once beforehand, as described in the [README](../../../README.md#build-from-clean-committed-source);
   the Guardian and VSS Publisher images are built by `campaign run` unless
   `--no-build` is given.
2. The taps start: uProtocol listeners on `BatteryTemperature` and
   `GuardianEvent`, and OpenSOVD polling every 25 ms. Every observation goes
   into `recording.jsonl`, stamped with the tool's own clock.
3. The KUKSA CAN Provider replays the scenario's trace **once**.
4. When the samples have stopped after the trace's duration, the tool records
   3 s more, so OpenSOVD can catch up, then removes the project.
5. The evaluation judges the recording. The judged window ends with the last
   sample plus one cycle: the loss of data after the replay ends is not judged.
   Latencies use the tool's arrival times; the order of cause and effect uses
   the Guardian's event IDs, because events can arrive a few milliseconds
   swapped over the network.

A run directory holds the evidence:

| File | Content |
|------|---------|
| `manifest.json` | run ID, scenario, stimulus, git revision, SHA-256 of the catalog, the parameters, and the trace |
| `recording.jsonl` | every observation |
| `report.json`, `report.md` | verdict, onset, evidence chain, result per requirement |
| `services.log` | logs of all services |
| `error.txt` | only if the run itself failed; the scenario is still judged |

`campaign.md` in the campaign directory lists every scenario with its verdict.

## Verdicts

As defined in the [Safety Concept](../../explanation/safety-concept.md#scenario-verdicts):

| Verdict | When |
|---------|------|
| PASS | Onset observed, every expectation met within its budget, nothing forbidden |
| FAIL | An expectation missed or late, or a forbidden reaction |
| INCONCLUSIVE | Onset not observed, or evidence missing (for example, OpenSOVD never answered) |

## Hardware demo

The hardware source is not part of the automated campaigns: it cannot be
replayed or triggered remotely. Its demo is recorded and judged with
`observe`, against a stack started with `docker compose up`. The tool taps the
Zenoh router on `127.0.0.1:7447` (`ZENOH_HOST_PORT`) and OpenSOVD on
`127.0.0.1:7690`:

```sh
cargo run -p campaign -- observe source_dropout --seconds 60
# unplug the board, wait, plug it back in
```

## Traces

Most traces come from
[`Fault_Injection_CAN_Logs/`](../../../Fault_Injection_CAN_Logs/README.txt).
[`campaign/traces/generate_traces.py`](../../../campaign/traces/generate_traces.py)
adds the ones the safety concept needs and that do not exist there yet:
`heating` (FSR-1.1, FSR-1.2), `max_stuck` (FSR-2.4), and `spike` (FSR-3.3, an
in-range spike; `implausible_jump` goes beyond the plausible range and tests
FSR-3.2).

`temp_stuck` freezes all temperatures together, which looks like a battery at
constant temperature; it is kept as a test of that known limitation and expects
no fault.

## Not covered yet

- **Transport faults** (delay, duplicate, reorder on the uProtocol channel):
  needs an injection point between the VSS Publisher and the Guardian.
- **Diagnostics outage**: still covered by
  [`diagnostics/smoke_test.py`](../../../diagnostics/smoke_test.py). The
  `pause` stimulus exists, but its OpenSOVD budget would have to count from the
  resume.
- **EC-1 to EC-3** of the Safety Concept (attribution, sequence diagnosis,
  delay measurement) and the Guardian heartbeat.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
