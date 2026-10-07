<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Guardian Watchdog

The Guardian Watchdog detects when the
[Battery Thermal Guardian](battery-thermal-guardian.md) itself stops working,
because it crashed or because it hangs, and reports this to DFM, so that
OpenSOVD shows it next to the Guardian's own faults. Its required behavior is
FSR-2.7 in the [Safety Concept](../../explanation/safety-concept.md). This
document explains how the watchdog is built. How to run it is in the
[watchdog README](../../../watchdog/README.md).

## Heartbeat and watchdog

A crashed or hung Guardian cannot report its own failure. The Guardian therefore
publishes a heartbeat, and a separate process watches it.

```text
 ┌─────────── Guardian process ───────────┐          ┌──────── watchdog process ────────┐
 │ select loop                            │          │                                  │
 │  ├─ sample ──► core                    │          │ listener ──► HeartbeatMonitor    │
 │  ├─ tick (50 ms) ──► core              │  uProto- │ check (100 ms) ──►   │           │
 │  └─ heartbeat (500 ms) ────────────────┼── col ──►│                      ▼           │
 │                                        │  (Zenoh) │            Failed / Passed       │
 └────────────────────────────────────────┘          │                      │           │
                                                     └──────────────────────┼───────────┘
                                                                            ▼
                                                                    DFM ──► OpenSOVD
```

| Part | Status | Responsibility |
|------|--------|----------------|
| Heartbeat ([`guardian-service`](../../../guardian-service/src/runtime.rs)) | **Implemented** | Publish a `Heartbeat` every `T_hb_period` (500 ms) from the loop that also runs the core |
| Heartbeat monitor ([`watchdog/src/lib.rs`](../../../watchdog/src/lib.rs)) | **Implemented** | Decide from the arrival times whether the Guardian is healthy or lost |
| Listener and timeout check ([`watchdog/src/main.rs`](../../../watchdog/src/main.rs)) | **Implemented** | Subscribe to the heartbeat over uProtocol, check the timeout every 100 ms |
| DFM reporter ([`watchdog/src/diagnostics.rs`](../../../watchdog/src/diagnostics.rs)) | **Implemented** | Report `BTG_GuardianHeartbeatLoss` to DFM without blocking the watchdog |
| Restart of a crashed Guardian | **Implemented** (by Docker) | `restart: unless-stopped` in [`docker-compose.yml`](../../../docker-compose.yml) |
| Restart of a hung Guardian | Not implemented | A hung Guardian is only reported |
| Occupant warning (HARA DFR-5) | Not implemented | The watchdog does not request `DRIVER_WARNING_MONITORING_UNAVAILABLE` |

### Why this design

- **Hangs are detected, not only crashes.** The heartbeat is sent from the same
  `select` loop that feeds samples and ticks into the core. A loop that stops
  making progress stops the heartbeat too. Docker notices a crash, but it does
  not notice a hang: a paused Guardian still counts as running.
- **Independent of the Guardian.** The watchdog is its own process and container.
  It shares nothing with the Guardian except the heartbeat topic and the DFM
  entity.
- **No change to the core.** The heartbeat is an adapter concern. The core
  stays free of I/O and clocks.
- **Same diagnostic interface as the Guardian.** The watchdog reports through
  the same `fault_lib` interface and under the same SOVD entity
  (`battery_guardian`), so all Guardian faults are in one place in OpenSOVD.
  `fault_lib` allows one reporter per process, which is another reason the
  watchdog is a separate process.

## Heartbeat interface

The `Heartbeat` message and its topic are defined in the
[Battery Thermal Contract](../../../contracts/README.md#heartbeat).

| Field | Meaning |
|-------|---------|
| `session_id` | The Guardian's session ID. A new value means the Guardian restarted |
| `sequence` | Counts heartbeats within a session, starting at 1 |
| `guardian_time_ms` | The Guardian's local monotonic time when it sent the heartbeat |

The heartbeat is not a `GuardianEvent`. It carries no safety decision and is not
part of the evidence chain.

## Heartbeat monitor

The decision logic is a small state machine with no I/O and no clock. The caller
passes the time into every call, as for the Guardian core, so it is tested
without a network or DFM.

```rust
let mut monitor = HeartbeatMonitor::new(timeout_ms);
let transition = monitor.on_heartbeat(now_ms); // for every received heartbeat
let transition = monitor.on_tick(now_ms);      // every 100 ms
```

| Transition | When | Reported to DFM |
|------------|------|-----------------|
| `FirstHeartbeat` | First heartbeat after the watchdog started | `Passed` |
| `BecameLost` | No heartbeat for longer than `T_hb` (1500 ms) while healthy | `Failed`, once per outage |
| `Recovered` | First heartbeat after a loss | `Passed` |

The timeout counts from the watchdog's own start, so a Guardian that never
starts is reported too. Until the first heartbeat arrives, the fault stays
`NotTested`: a fresh watchdog does not claim a healthy Guardian.

A loss is reported between `T_hb` and `T_hb` + 100 ms after the last heartbeat.
Since the Guardian fails at some point between two heartbeats, that is
`T_hb` − `T_hb_period` to `T_hb` + 100 ms after the failure, plus transport
delay.

## Fault

| Attribute | Value |
|-----------|-------|
| Fault ID | `BTG_GuardianHeartbeatLoss` |
| Name | Heartbeat Loss |
| Summary | `FSR-2.7: Guardian itself stops reporting` |
| Severity | `Error` |
| Category | `Software` |
| Mitigation | `RestartGuardian` |

The catalog entry is in
[`diagnostics/catalog/battery_guardian.json`](../../../diagnostics/catalog/battery_guardian.json).
The [Faults to Be Detected](../faults-to-be-detected.md) list has it as F-10.

Every record carries `requirement` (`FSR-2.7`) and `watchdog_session_id` as
environment data. `Passed` records add the Guardian's `guardian_session_id` and
`heartbeat_sequence`. `Failed` records add the last seen session and sequence, if
any, and `silence_ms`. Together with the Guardian's own records, which carry the
same `guardian_session_id`, this shows which Guardian session was lost.

DFM delivery runs in its own thread with a queue. A slow or unavailable DFM
never delays the timeout check. While DFM is unavailable, the watchdog keeps
watching and retries.

## Deployment

The watchdog runs as the `watchdog` service in
[`docker-compose.yml`](../../../docker-compose.yml). Like the Guardian, it
shares DFM's IPC namespace (iceoryx2 shared memory) and also DFM's PID
namespace.

The PID namespace matters. iceoryx2 treats every node with the caller's own PID
as alive. In separate PID namespaces every process is PID 1, so a killed
Guardian's DFM connection was never cleaned up, and the restarted Guardian could
not report to DFM at all. With a shared PID namespace, PIDs are unique, and the
[patched `fault_lib`](../../../third-party/fault-lib/README.md) removes dead
nodes before it connects.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `HEARTBEAT_TIMEOUT_MS` | `1500` | `T_hb` |
| `FAULT_CATALOG` | `diagnostics/catalog/battery_guardian.json` | DFM fault catalog |
| `SOVD_ENTITY` | `battery_guardian` | SOVD entity to report under |
| `ZENOH_CONNECT`, `ZENOH_LISTEN` | — | Zenoh endpoints |

`T_hb_period` is a constant in the Guardian service. Both values come from the
[Safety Concept](../../explanation/safety-concept.md#parameters).

## Verification

The heartbeat monitor and the DFM record content have unit tests
(`cargo test -p watchdog`). One test checks that the shipped catalog contains the
fault.

The crash and hang cases were verified by hand against the running stack,
reading `/sovd/v1/apps/battery_guardian/faults/BTG_GuardianHeartbeatLoss`:

| Injection | Reported `Failed` | Recovery |
|-----------|-------------------|----------|
| SIGKILL to the Guardian process | 1.4 s after the kill (1592 ms of silence) | Docker restarted it; `Passed` 0.7 s later, new session; the new Guardian reached DFM again |
| `docker pause guardian` (hang) | 1.8 s after the pause (1524 ms of silence) | `Passed` 0.4 s after `docker unpause`, same session |

No campaign scenario automates these runs yet, so FSR-2.7 has the status
**implemented**, not **tested**. The HARA scenarios TS-12 and TS-13 record the
coverage in detail.

## Limits

- **No occupant warning.** HARA DFR-5 asks for a monitoring-unavailable warning
  when the Guardian fails. The watchdog only reports to diagnostics.
- **No restart of a hung Guardian.** That would need access to the container
  runtime, which the watchdog does not have. `docker kill` is not restarted
  either: Docker treats it as a manual stop.
- **False alarms under load.** While Rust builds loaded the developer machine
  for about 10 minutes, the watchdog reported 19 losses for a Guardian that was
  running, each followed by `Passed` within seconds. Without load, no false
  alarm was seen. Raising `T_hb` trades detection time for fewer false alarms.
  That is a Safety Concept decision.
- **Nothing watches the watchdog.** If it dies, Docker restarts it, and the
  fault starts again as `NotTested`.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
