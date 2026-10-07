<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Evidence Collector

> **Status: design, not implemented.** Today the diagnostic campaign test
> [`guardian-service/tests/diagnostics.rs`](../../../guardian-service/tests/diagnostics.rs)
> does part of this job. The Evidence Collector grows out of it.

The Evidence Collector observes a scenario run, records what it sees, and
decides whether the system reacted correctly: PASS, FAIL, or INCONCLUSIVE. It
never influences the system. Its role and its requirements (EC-1 to EC-3) are
defined in the [Safety Concept](../../explanation/safety-concept.md#test-roles);
the [Scenario Generator](scenario-generator.md) runs the scenarios it judges.

## Design decisions

| Decision | Reason |
|----------|--------|
| **Rust**, using `up-rust` and `up-transport-zenoh` | The real uProtocol API, as in the Guardian. Reuses the `thermal-contract` types and topics and the Guardian's parameter file. No maintained Python uProtocol package exists. |
| **Record first, evaluate afterwards** | Evaluation is a pure function of a recording. It is unit-testable without Docker, can judge any old run again, and makes reruns comparable. The run directory is the evidence. |
| **Judged only on observations** | The fault onset t0 is the first observation at the tap point, not the runner's injection time ([Timing reference](../../explanation/safety-concept.md#timing-reference)). A fault that never reached the Guardian is INCONCLUSIVE, not PASS. |
| **Expectations come from the scenario catalog** | No scenario names in the code. A new scenario is a catalog entry, not a code change. |
| **Every run writes a report, also a failed one** | The challenge forbids hiding failed scenarios. A failed expectation is a result, not a crash. |

## Structure

```text
evidence-collector/            Rust crate in the workspace
  src/lib.rs
    recording.rs   observation types, JSONL read/write
    catalog.rs     scenario catalog (campaigns/scenarios.toml)
    onset.rs       fault onset detectors over the recorded input stream
    evaluate.rs    expectations, budgets, verdicts       (pure, no I/O)
    report.rs      report.json and report.md
  src/main.rs      CLI: record, evaluate
  src/record.rs    uProtocol listeners and OpenSOVD polling
```

The uProtocol transport helpers move from `guardian-service` into
`thermal-contract`, so that the Guardian and the Evidence Collector share them
without depending on each other.

## record

```sh
evidence-collector record --run-dir runs/<run_id>
```

Runs until it receives SIGINT or SIGTERM from the Scenario Generator.

| Tap | How | Recorded as |
|-----|-----|-------------|
| Guardian input | uProtocol listener on `BatteryTemperature` | `battery_temperature` |
| Guardian output | uProtocol listener on `GuardianEvent` | `guardian_event` |
| Diagnostics | Polls `GET /sovd/v1/apps/battery_guardian/faults/<code>` every 25 ms for every fault code; records each change | `sovd_fault` |

Every observation is one JSON line in `recording.jsonl`, stamped with the
collector's own monotonic clock (`t_ms`, from the start of the recording). All
latencies are differences of these stamps, so no clock synchronization is
needed:

```json
{"t_ms": 4012, "tap": "battery_temperature", "sequence": 41, "alive_counter": 40, "quality": "VALID", "max_c": 40.0, "avg_c": 32.0, "min_c": 24.0, "source_timestamp_ms": 1791357970962}
{"t_ms": 4318, "tap": "guardian_event", "session_id": "…", "event_id": 5, "cause_event_id": 0, "guardian_time_ms": 4301, "kind": {"FaultDetected": {"dtc": "BTG_TempCounterStuck", "requirement": "FSR-2.3"}}}
{"t_ms": 4890, "tap": "sovd_fault", "code": "BTG_TempCounterStuck", "status": {"testFailed": true}, "environment_data": {"session_id": "…", "event_id": "5"}}
```

The OpenSOVD snapshots are kept per run, because the DFM only exposes the
current state, not a history of occurrences.

## evaluate

```sh
evidence-collector evaluate --run-dir runs/<run_id>
```

Reads `manifest.json` (written by the Scenario Generator), `recording.jsonl`,
the scenario catalog, and the safety parameters, and writes `report.json` and
`report.md`.

1. **Session.** All Guardian events of the run must carry one `session_id`.
   The OpenSOVD records count only if they carry the same one.
2. **Onset t0.** The scenario's `onset` detector finds the first observation of
   the fault in the recorded `BatteryTemperature` stream (see below). Not found
   → INCONCLUSIVE ("fault not delivered").
3. **Expectations.** Each expectation of the scenario is checked against the
   recording, with its budget resolved from `safety-params.toml`.
4. **Forbidden reactions.** Checked over the whole run.
5. **Verdict** and a result per requirement.

### Onset detectors

| `onset` | t0 = first recorded `BatteryTemperature` … |
|---------|---------------------------------------------|
| `none` | — (nominal scenario: no fault expected) |
| `gap` | … after which none arrives for longer than `T_stale`; t0 = that arrival plus one cycle (100 ms) |
| `alive_counter_repeats` | … whose alive counter equals the previous one |
| `alive_counter_jumps` | … whose alive counter does not follow the previous one |
| `quality_not_valid` | … with quality other than `VALID` |
| `max_frozen_while_reference_moves` | … that starts a plateau of the maximum, during which the average or minimum changes by at least `Δ_stuck` |
| `max_at_least` | … with a maximum of at least the given value |

### Expectations

| Expectation | Met when |
|-------------|----------|
| `fault` | A `FaultDetected` with this code arrives within its budget after t0 |
| `monitoring` | A `MonitoringStatusChanged` to this status, caused by the fault event |
| `mitigation` | A `MitigationRequested` with this action, caused by the status change or the thermal change |
| `thermal` | A `ThermalStateChanged` to this state within its budget after t0 |
| `sovd` | The fault appears in OpenSOVD with `testFailed: true` and this run's session and event ID, within `T_diag` after the Guardian event |
| `recovery` | `FaultRecovered` caused by the fault, then monitoring back to OK, and OpenSOVD `testFailed: false` with `testFailedSinceLastClear: true` |
| `no_fault` | No `FaultDetected` in the whole run |

### Forbidden reactions

Checked in every scenario:

- the thermal state was lowered while monitoring was DEGRADED (FSR-2.5);
- a Guardian event of another session appeared.

### Verdict

As defined in the [Safety Concept](../../explanation/safety-concept.md#scenario-verdicts):

| Verdict | When |
|---------|------|
| PASS | Onset observed, every expectation met within its budget, nothing forbidden happened, evidence complete |
| FAIL | An expectation missed or late, or a forbidden reaction |
| INCONCLUSIVE | Onset not observed, or evidence incomplete (for example, OpenSOVD never answered) |

The report also lists the result of every requirement the scenario covers, so
a missing OpenSOVD record shows as a failed FSR-D.2 without hiding the safety
result.

## Report

`report.json` is machine-readable, for comparing reruns. `report.md` shows the
evidence chain the challenge asks for:

```text
counter_stuck  run 20261007-101203-ab12   PASS
  hazard H-2 → SG-2 → injected: alive counter stuck (counter_stuck.asc, sha256 3f2a…)
  onset   t0 = 4.012 s   counter 40 repeated
  detect  +0.31 s  #5 FaultDetected BTG_TempCounterStuck (FSR-2.3)      budget 0.80 s  ✓
  status  +0.31 s  #6 MonitoringStatusChanged OK → DEGRADED (cause #5)                ✓
  warn    +0.31 s  #7 DRIVER_WARNING_MONITORING_UNAVAILABLE (cause #6)                ✓
  sovd    +0.88 s  BTG_TempCounterStuck testFailed, session and event #5  budget 2.00 s  ✓
  recover +2.40 s  #9 FaultRecovered (cause #5), #10 DEGRADED → OK, OpenSOVD passed   ✓
  inputs  safety-params.toml sha256 9c1e…  trace sha256 3f2a…  git 2b1f82c
```

A campaign report (`runs/<campaign_id>/campaign.md`) lists every scenario with
its verdict, including the failed and inconclusive ones.

## What happens to the diagnostic campaign test

`guardian-service/tests/diagnostics.rs` stays until the Evidence Collector
covers its checks: cause chain, session, OpenSOVD environment data, recovery,
and the diagnostics outage. Then it shrinks to a test of the Guardian service,
and its evidence part is gone. Two things are fixed on the way: its report
always says PASS (it is only written when every assertion passed), and its
`injected_fault` text is wrong for the `counter` and `quality` scenarios.

## Milestones

1. Library: recording format, catalog, onset detectors, evaluation, report.
   Unit tests with synthetic recordings.
2. `record`: uProtocol listeners and OpenSOVD polling.
3. First real verdicts for `normal`, `timeout`, `counter_stuck`,
   `invalid_quality` on the real CAN chain, started by the Scenario Generator.
4. EC-1 (attribution through a Data Broker tap), EC-2, EC-3, Guardian heartbeat.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
