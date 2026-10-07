<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Dashboard

The dashboard is a web page for the whole stack. It shows the state, memory,
and settings of every component and starts and stops them. It also shows what
goes into each component and what comes out, the DTCs stored in OpenSOVD, and
the campaign reports while a campaign runs. It only observes and operates the
stack. It takes no part in the safety function, and the Guardian does not
depend on it. How to run it is in the [dashboard README](../../../dashboard/README.md).

## Structure

```text
                     ┌──────────────────── dashboard container ─────────────────────┐
 browser ◄── HTTP ──►│ web page (embedded)  ◄──  API                                 │
                     │                            ├─ components ── Docker socket ───┼──► all containers
                     │                            ├─ taps ─┬─ KUKSA gRPC ───────────┼──► Data Broker
                     │                            │        ├─ uProtocol (Zenoh) ────┼──► BatteryTemperature, GuardianEvent
                     │                            │        └─ HTTP poll ────────────┼──► OpenSOVD
                     │                            ├─ OpenSOVD: DTCs, clear ─────────┼──► OpenSOVD
                     │                            ├─ runs: campaign evidence ───────┼──► runs/ (read-only)
                     │                            └─ launcher ── Docker socket ─────┼──► campaign-runner container
                     └──────────────────────────────────────────────────────────────┘
```

| Part | Status | Responsibility |
|------|--------|----------------|
| Components ([`components.rs`](../../../dashboard/src/components.rs)) | **Implemented** | Every 2 s: state, health, memory, settings of each container in the Compose project. Start, stop, restart |
| Docker client ([`docker.rs`](../../../dashboard/src/docker.rs)) | **Implemented** | The Docker Engine API over the Unix socket: list, inspect, stats, start/stop, logs, files |
| Taps ([`taps.rs`](../../../dashboard/src/taps.rs)) | **Implemented** | Passive observers of the data between the components, for the input and output logs |
| OpenSOVD ([`sovd.rs`](../../../dashboard/src/sovd.rs)) | **Implemented** | DTC list joined with the fault catalog, one DTC with its environment data, clearing |
| Campaign evidence ([`runs.rs`](../../../dashboard/src/runs.rs)) | **Implemented** | Campaigns in `runs/`, which one is running, the reports of its scenarios |
| Web page ([`static/`](../../../dashboard/static)) | **Implemented** | Overview, one tab per component, campaign tab. Plain JavaScript, no build step |
| Launcher ([`launcher.rs`](../../../dashboard/src/launcher.rs)) | **Implemented** | Starts a campaign in a `campaign-runner` container, stops it, removes what it left behind |

## Overview

One row per component of the Compose project, in the order of the signal chain:

| Column | Content |
|--------|---------|
| Component | Name, role, container |
| Run state | Docker state (running, exited, paused, restarting), health check, uptime, restart count |
| Memory | Memory in use without the page cache, as `docker stats` shows it, and the limit |
| Settings | Image, published ports, restart policy, number of environment variables, shared namespaces |
| Actions | Start or stop, restart |

**Start all** starts the services in dependency order: the Zenoh router, the
Data Broker, DFM, the gateway, then the signal chain from the CAN provider to the
Guardian. **Stop all** goes the other way.

The Guardian, the gateway, and the watchdog (where it exists) share DFM's IPC
namespace. When DFM starts, the dashboard therefore restarts the running
services that share its namespace, as the [README](../../../README.md#lifecycle)
asks for. Otherwise they would stay in the namespace of the stopped DFM and
lose their connection to it.

The campaign tool has a row too, with the campaign that is running. **Start…**
opens the campaign tab; while a campaign runs, **Stop** stops it.

## Component tabs

Each component has a tab with its state, memory, start time, restart count,
and all settings. The settings come from `docker inspect`: image, entrypoint,
command, environment, ports, mounts, networks, and namespaces. The Guardian tab
also shows its safety parameters, read from the running container
(`/etc/guardian/safety-params.toml`). These are the values the Guardian
actually uses, not the copy in the repository.

Three logs per component:

| Log | Content |
|-----|---------|
| Input log | What the component receives |
| Output log | What it produces |
| Container log | Its own stdout and stderr |

Where data flows between components, the dashboard observes it on the way with
its own taps, like the campaign's evidence collector. Where it does not, it
filters the component's own log:

| Component | Input log | Output log |
|-----------|-----------|------------|
| KUKSA CAN Provider | its log: CAN trace replay | KUKSA tap: the battery signals in the Data Broker |
| KUKSA Data Broker | KUKSA tap | KUKSA tap, its warnings (for example slow subscribers) |
| VSS Publisher | KUKSA tap | uProtocol tap: `BatteryTemperature` |
| Zenoh Router | uProtocol taps | uProtocol taps |
| Guardian | uProtocol tap: `BatteryTemperature` | uProtocol tap: `GuardianEvent`; its log: DFM records |
| Guardian Watchdog | its log: heartbeats | its log: DFM records |
| OpenSOVD DFM | its log: received fault records | OpenSOVD tap: fault status changes |
| OpenSOVD Gateway | its log: HTTP requests | OpenSOVD tap |

| Tap | How | Line |
|-----|-----|------|
| KUKSA | gRPC subscription to the five battery signals; one line per CAN frame, when the alive counter arrives | `Temperature.Max 54.0 °C, Average 46.0 °C, Min 38.0 °C, BMS.SignalQuality 128, BMS.AliveCounter 14` |
| uProtocol | Listeners on the contract's topics, decoded with the campaign's recording types | `#7 MitigationRequested DRIVER_WARNING_OVERTEMP, cause #6 (session 01234567…, t=1200 ms)` |
| OpenSOVD | Polls the fault list every second and logs every change | `BTG_TempOutOfRange: testFailed (mask 0xAB, occurrences 3, confirmedDtc)` |

The taps keep the last 600 lines each, in memory. They only listen and never
publish. The KUKSA tap is a second subscriber of the Data Broker, next to the
VSS Publisher. The dashboard is not the Guardian, so the rule that the Guardian
reads VSS data only through uProtocol is not affected.

## OpenSOVD tab: DTCs

The DFM and gateway tabs show every DTC of the `battery_guardian` entity:

| Column | Content |
|--------|---------|
| Severity | From the fault catalog, colored: Fatal (dark red), Error (red), Warn (amber), Info (blue), Debug and Trace (gray) |
| DTC | Fault code |
| Description | Summary from the catalog |
| Category | Fault type from the catalog (Hardware, Communication, Software) |
| Status | ACTIVE (test failed now), CONFIRMED, PENDING, HISTORY (failed since the last clear), warning lamp, PASSED, NOT TESTED |
| Occurrences | Occurrence, aging, and healing counters |

Active DTCs come first, then by severity. **Details** shows a DTC's status bits
and environment data (Guardian session, event ID, requirement, triggering
sample). **Clear** clears one DTC, and **Clear all faults** clears the entity.
Both use OpenSOVD's `DELETE …/faults[/{code}]`, after a confirmation. **Print
report (PDF)** opens the browser's print dialog with a report of all DTCs and
the environment data of the failed ones; "Save as PDF" writes the file.

OpenSOVD reports severity as a number. The dashboard takes the name from the
DFM fault catalog, and from the number only for a code that is not in the
catalog.

## Campaign tab

The banner shows whether a campaign is running, which scenario, and how many
scenarios are done. Below it is the report of the selected campaign: by
default the running one, otherwise the newest. It shows the counts of PASS,
FAIL, and INCONCLUSIVE, a progress bar, and one card per scenario.

| Scenario state | Card |
|----------------|------|
| Judged | Verdict, reason, hazard → safety goal, onset, Guardian session, evidence chain (requirement, expectation, observation, latency, budget, result), forbidden reactions, result per requirement |
| Running | Number of observations recorded so far |
| To come | Listed from the campaign's plan |
| Not judged | The campaign tool stopped during the scenario |

**Print report (PDF)** prints the campaign summary and every judged scenario.

The dashboard reads what the campaign tool writes to `runs/`. To show what is
still to come, the campaign tool now writes `plan.json` with the campaign's
scenarios when it starts. A scenario counts as running while its Compose
project (`campaign-<scenario>`) has running containers, or while its recording
keeps growing. Campaigns from before `plan.json` existed are shown with the
scenarios that started.

### Running a campaign

The campaign tab starts campaigns: all scenarios, or the ones ticked in the
list (all except `external` ones, which need someone else to inject). The
option **rebuild the Guardian and VSS Publisher images first** runs the tool
without `--no-build`, so the campaign judges the current code; without it,
the campaign uses the images that exist. The output of the campaign tool
appears below the buttons, and its exit code when it ends (0: every
implemented scenario passed, 1: one did not, 2: the tool failed).

The campaign tool starts one Compose project per scenario, with bind mounts
relative to the repository and ports on the Docker host's `127.0.0.1`, which
it then connects to. So it does not run inside the dashboard. The dashboard
runs its own image, which also contains the campaign tool and the Docker CLI
with Compose and Buildx, as a separate `campaign-runner` container:

| Setting | Why |
|---------|-----|
| The repository, read-write, at the path the Docker daemon knows it by | Compose sends the bind-mount paths to the daemon. The path comes from the dashboard's own `/repo` mount; Docker Desktop on Windows reports `C:\…`, which the daemon knows as `/run/desktop/mnt/host/c/…`. `HOST_REPO_DIR` overrides it |
| Host network | `127.0.0.1` is the Docker host, where the scenario's ports are published |
| The Docker socket | The campaign tool runs `docker compose` |
| The repository owner's user ID, if not root | The evidence in `runs/` belongs to the user |

The campaign tool runs exactly as from a shell on the host and writes its
evidence to `runs/`. **Stop campaign** stops the runner and removes the
Compose projects `campaign-*` with their networks and volumes. The scenario in
progress stays unjudged and is shown as such.

## Security

The dashboard reaches the Docker socket, which gives it full control of
Docker on the machine. It therefore listens on `127.0.0.1` only. Requests that
change something (start, stop, clear) must carry the header
`X-Dashboard: 1`. A browser sends such a header to another origin only after a
CORS preflight, which the dashboard never allows. So another web page cannot
stop the stack or clear faults through the visitor's browser. There is no
login: anyone who can reach the port can operate the stack.

The campaign runner has the same access to Docker, host networking, and the
repository read-write. It only runs the campaign tool with the arguments
built from the scenario IDs of the catalog; unknown IDs are rejected.

## Limits

- **Container logs are read, not streamed.** The input and output logs that
  come from a container log are read again from its last 2000 lines on every
  refresh.
- **Taps start with the dashboard.** The input and output logs show only what
  happened since the dashboard started, at most the last 600 lines per tap.
- **Campaign state is inferred.** Whether a campaign is running is inferred
  from its files and containers, and from the campaign runner. A campaign
  started on the host and killed in the middle shows as "incomplete" once its
  files stop changing.
- **Without a rebuild, the images decide.** A campaign started without the
  rebuild option judges whatever `ota-outlaws/guardian:dev` holds. In a test
  run with an image from another branch, `counter_stuck` failed because that
  image did not report the fault type and severity yet; with the rebuild it
  passed.
- **One campaign at a time.** A second start is refused while the runner
  runs. The runner's output is kept until the next start; the evidence stays
  in `runs/`.
- **Unix only.** The Docker client uses the Unix socket, so the dashboard runs
  in its container (Docker Desktop provides the socket there too). Built on
  Windows directly, it reports that Docker is unavailable.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
