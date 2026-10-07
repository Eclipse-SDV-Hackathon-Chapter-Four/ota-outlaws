<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Guardian watchdog

Watches the Battery Thermal Guardian and reports when it stops working
(FSR-2.7). The Guardian publishes a `Heartbeat` every 500 ms
([contract](../contracts/README.md#heartbeat)). When no heartbeat arrives for
longer than `T_hb` (1500 ms), the watchdog

1. requests the occupant warning `DRIVER_WARNING_MONITORING_UNAVAILABLE` on its
   own uProtocol topic (HARA DFR-5), and
2. reports `BTG_GuardianHeartbeatLoss` to DFM; OpenSOVD then shows it next to
   the Guardian's own faults.

The watchdog is a separate process and container because a crashed or hung
Guardian cannot report its own failure. The Guardian sends its heartbeat from
the loop that runs its core, so the watchdog detects both:

- **Crash:** the process ends. Docker restarts it (`restart: unless-stopped`);
  the new Guardian's first heartbeat recovers the fault.
- **Hang:** the process stays alive but stops evaluating. Docker does not
  notice this; the watchdog does.

## Occupant warning (HARA DFR-5)

A failed Guardian cannot warn the occupants that thermal monitoring is gone,
so the watchdog does it. It publishes `SupervisorEvent` messages on
`//guardian-watchdog/9003/1/8001`
([contract](../contracts/README.md#supervisorevent)), a uEntity of its own:

| Event | When | Cause |
|-------|------|-------|
| `GuardianLost` | No heartbeat for longer than `T_hb`, once per outage | — |
| `MitigationRequested` `DRIVER_WARNING_MONITORING_UNAVAILABLE` | Together with `GuardianLost` | `GuardianLost` |
| `GuardianRestored` | First heartbeat after the loss; the Guardian's own warnings apply again | `GuardianLost` |

The warning is published before the DFM report, so diagnostics never delay it
(HARA DFR-6). The path depends neither on the Guardian nor on the Evidence
Collector. The logic is in [`src/supervisor.rs`](src/supervisor.rs), with no IO
and no clock, like the heartbeat monitor. The dashboard shows the topic, and the
campaign checks it in the `guardian_crash` and `guardian_hang` scenarios
(`supervisor_warning`, `supervisor_restored`).

## Fault

Reported through the same `fault_lib` interface as the Guardian's faults
([`src/diagnostics.rs`](src/diagnostics.rs), compare
[`guardian-service/src/diagnostics.rs`](../guardian-service/src/diagnostics.rs)),
under the same SOVD entity `battery_guardian`. The catalog entry is in
[`diagnostics/catalog/battery_guardian.json`](../diagnostics/catalog/battery_guardian.json).

| Attribute | Value |
|-----------|-------|
| Fault ID | `BTG_GuardianHeartbeatLoss` |
| Name | Heartbeat Loss |
| Summary | `FSR-2.7: Guardian itself stops reporting` |
| Severity | `Error` |
| Category | `Software` |
| Mitigation | `RestartGuardian` |

`RestartGuardian` is what the fault calls for; the watchdog does not restart
the Guardian itself. A crashed Guardian is restarted by Docker. A hung one is
only reported: restarting it would need access to the container runtime, which
the watchdog deliberately does not have.

| Stage | When | Environment data |
|-------|------|------------------|
| `NotTested` | Watchdog start | — |
| `Passed` | First heartbeat, and the first heartbeat after a loss | `guardian_session_id`, `heartbeat_sequence` |
| `Failed` | No heartbeat for longer than `T_hb`, reported once per outage | last `guardian_session_id` and `heartbeat_sequence` if any, `silence_ms` |

Every record also carries `requirement` (`FSR-2.7`) and `watchdog_session_id`.
A Guardian that never starts is reported too: the timeout counts from the
watchdog's own start.

## Run

Part of `docker compose up --build -d`. On its own, from the repository root,
with a Zenoh router and DFM running:

```sh
ZENOH_CONNECT=tcp/127.0.0.1:7447 cargo run -p watchdog
```

| Variable | Default | Meaning |
|----------|---------|---------|
| `HEARTBEAT_TIMEOUT_MS` | `1500` | `T_hb` |
| `FAULT_CATALOG` | `diagnostics/catalog/battery_guardian.json` | DFM fault catalog |
| `SOVD_ENTITY` | `battery_guardian` | SOVD entity to report under |
| `ZENOH_CONNECT`, `ZENOH_LISTEN` | — | Comma-separated Zenoh endpoints |
| `RUST_LOG` | `info` | Log filter |

## Test

```sh
cargo test -p watchdog
```

The timeout logic in [`src/lib.rs`](src/lib.rs) and the warning logic in
[`src/supervisor.rs`](src/supervisor.rs) take the time as a parameter and do no
IO, so they are tested without a network or DFM.
[`tests/supervisor.rs`](tests/supervisor.rs) starts the real binary without a
Guardian, with `T_hb` = 300 ms, and checks over Zenoh that it requests the
warning and withdraws it once heartbeats arrive.

Checked by hand against the running stack, reading
`/sovd/v1/apps/battery_guardian/faults/BTG_GuardianHeartbeatLoss`:

| Injection | Reported `Failed` | Recovery |
|-----------|-------------------|----------|
| SIGKILL to the Guardian process (`docker run --rm --pid=host debian:bookworm-slim sh -c "kill -9 <pid>"`, `<pid>` from `docker top guardian`) | 1.4 s after the kill (1592 ms of silence) | Docker restarted it; `Passed` on the new session's first heartbeat 0.7 s later. The new Guardian reached DFM again |
| `docker pause guardian` (hang; Docker reports the container as running) | 1.8 s after the pause (1524 ms of silence) | `Passed` 0.4 s after `docker unpause`, same session |

`docker kill guardian` is reported too, but Docker treats it as a manual stop
and does not restart the container; use the SIGKILL above to simulate a crash.
No campaign scenario automates these runs yet.

## Limits

- Nothing watches the watchdog. If it dies, Docker restarts it, and it starts
  again with `NotTested`.
- **False alarms under load.** `T_hb` leaves little margin against scheduling
  stalls. While Rust builds loaded the Docker Desktop host for about 10 minutes,
  the watchdog reported 19 losses for a Guardian that was running, each
  followed by `Passed` within seconds. At least once the Guardian itself saw its
  input go stale at the same time, so the whole host stalled. Without load, no
  false alarm was seen. Raising `T_hb` (`HEARTBEAT_TIMEOUT_MS`) trades detection
  time for fewer false alarms; that is a Safety Concept decision.
- The Guardian and the watchdog share DFM's PID namespace
  (`pid: "service:opensovd-dfm"` in `docker-compose.yml`). iceoryx2 treats every
  node with the caller's own PID as alive; with every container process being
  PID 1, a killed reporter's DFM publisher slot was never freed, and the
  restarted Guardian could not report to DFM at all. `fault_lib` now also
  removes dead nodes before it connects
  ([patch notes](../third-party/fault-lib/README.md)).
- **The warning is only published.** No HMI in this project displays it; the
  dashboard shows it. It is sent once per outage and uProtocol publish keeps no
  history, so a consumer that subscribes during an outage does not see it.
- **Shared transport.** The warning travels over the same Zenoh router as the
  Guardian's messages. A failed router stops both; the Guardian's own
  freshness monitoring does not cover that case either.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
