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

The dashboard is a web page for the safety evidence. It shows the result of
every HARA test with its evidence chain, runs campaigns, shows the live signal
chain and the DTCs stored in OpenSOVD, and starts and stops the components. It
only observes and operates the stack. It takes no part in the safety function, and the Guardian does not
depend on it. How to run it is in the [dashboard README](../../../components/dashboard/README.md).

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
| Components ([`components.rs`](../../../components/dashboard/src/components.rs)) | **Implemented** | Every 2 s: state, health, memory, settings of each container in the Compose project. Start, stop, restart |
| Docker client ([`docker.rs`](../../../components/dashboard/src/docker.rs)) | **Implemented** | The Docker Engine API over the Unix socket: list, inspect, stats, start/stop, logs, files |
| Taps ([`taps.rs`](../../../components/dashboard/src/taps.rs)) | **Implemented** | Passive observers of the data between the components, for the input and output logs |
| OpenSOVD ([`sovd.rs`](../../../components/dashboard/src/sovd.rs)) | **Implemented** | DTC list joined with the fault catalog, one DTC with its environment data, clearing |
| Campaign evidence ([`runs.rs`](../../../components/dashboard/src/runs.rs)) | **Implemented** | Campaigns in `runs/`, which one is running, the reports of its scenarios |
| Signal plot ([`signal.rs`](../../../components/dashboard/src/signal.rs)) | **Implemented** | Reduces a scenario's `recording.jsonl` to the series of its signal plot |
| Web page ([`static/`](../../../components/dashboard/static)) | **Implemented** | Campaign, live chain, and diagnostics view, designed after DASHBOARDS.md. Plain JavaScript, no build step. Fonts (Sora, Manrope) load from Google Fonts and fall back to system fonts offline |
| Launcher ([`launcher.rs`](../../../components/dashboard/src/launcher.rs)) | **Implemented** | Starts a campaign in a `campaign-runner` container, stops it, removes what it left behind |

## Design

The design follows the look of
[eclipsesdv.org](https://eclipsesdv.org/): Sora for headings and Manrope for
text and numbers, the Eclipse SDV purple, magenta, and orange, and its logo.
Every view opens with one sentence that answers its question, at most four key
metrics each shown against a reference, one chart, and the details on request.
The page is light; **Dark** in the header switches, and the choice is
remembered. Purple marks what to look at, never "good". The attention colour,
the Eclipse orange, appears only on something that needs action, always with a
sign and words.

**Runtime** in the header selects what the dashboard watches and runs
campaigns on: the Docker Compose stack on this host, or AutoSD peers through
OpenDUT, if configured (see the
[dashboard README](../../../components/dashboard/README.md)). On AutoSD,
Ankaios manages the services, so start, stop, restart, and clearing DTCs are
not offered; campaigns build the images from the checkout and show the bench
phase, evidence retrieval errors, and cleanup failures under "Runner output".

## Campaign view

The start page answers whether the Guardian passes its HARA tests, and why.

- **Headline sentence**, generated from the data: for example "All 23 HARA tests
  passed, but 1 evidence chain is incomplete.", "2 HARA tests failed: TS-12,
  TS-21.", or, when nothing is wrong, "A normal run. … nothing needs your
  attention." While a campaign runs it names the running scenario. Under it, a
  line lists what changed since this browser last saw a finished campaign.
- **Four key metrics**, each with its reference: tests passing (target: all,
  and the previous campaign), evidence chains complete, the tightest reaction
  as a percentage of its budget, and open requirements (planned checks that do
  not pass yet).
- **Needs your attention**, only if a test failed or could not be judged.
- **Reaction time against budget**: one bar per timed test, sorted by how much
  of its budget it used. The light band is the budget; a bar beyond it is
  marked. The tightest test is highlighted. Eight bars show; the rest follow.
- **All HARA tests**, collapsed: one row per HARA test with its title,
  scenarios, verdict, and reaction, then the scenarios that belong to no HARA
  test ("Further checks"). A click on a row opens the report of its scenarios:
  the reason, the injected fault and onset, the [signal plot](#signal-plot)
  (also while the scenario runs), the
  [evidence chain](campaign.md#evidence-chain), and, one click further, the
  evidence table, detections, mitigations (with their cause chain), DTCs, the
  checks with latency and budget, the Guardian events, and the run details.
  **Run again →** starts that scenario alone.

A planned scenario that fails shows as Planned: it checks a requirement that is
not implemented yet. Tests without a scenario in the catalog, and scenarios of
type `external`, which someone else has to inject, do not appear.

Filters and the selection live in the address: the campaign and the opened
tests are in the URL, and **Copy link to this view** copies it. **Print this
report** opens the browser's print dialog, with the signal plot of every judged
scenario; "Save as PDF" writes the file.

The dashboard reads what the campaign tool writes to `runs/`. To show what is
still to come, the campaign tool writes `plan.json` with the campaign's
scenarios when it starts. A scenario counts as running while its Compose
project (`campaign-<scenario>`) has running containers, or while its recording
keeps growing.

### Signal plot

Each scenario report plots what the campaign tool recorded, on one time axis
from the start of the recording:

| Row | Shows | From the recording |
|-----|-------|--------------------|
| Plot | Maximum, average, and minimum cell temperature as the Guardian received them over uProtocol. Samples whose quality is not `VALID` are shaded and break the lines; so are gaps in the stream (more than four sample periods, at least 400 ms) | `battery_temperature` |
| Vertical lines | The tool's injections (dashed; setup steps faint, the fault labeled), and the onset t0 from the report (solid) | `injection`, `report.json` |
| thermal | The Guardian's thermal state: CLEAR, MONITORING, WARNING, CRITICAL, MITIGATING | `guardian_event` `ThermalStateChanged` |
| monitoring | The Guardian's monitoring status | `guardian_event` `MonitoringStatusChanged` |
| events | Detected faults ▲, recoveries ▼, mitigation requests ◆, watchdog events ■. Passed fault tests are left out | `guardian_event`, `supervisor_event` |
| DTC | While a DTC's `testFailed` flag is set in OpenSOVD | `sovd_fault` |

What follows the evaluation window (`window_end_ms`) is shaded: it was recorded
during teardown and is not judged. Hovering shows the nearest sample and the
states at that time. The page fetches the series from
`GET /api/signal/<run_id>`, where `run_id` is the manifest's
(`<campaign>/<scenario>`, or `<campaign>` for an `observe` run); judged runs
once, a running scenario on every poll.

## Live chain view

The headline says whether the chain runs ("The chain is running. All 8
components are up.", or which component is down). A row of the eight
components follows, each with a dot for its Docker state (green running, red
exited, amber paused); a click shows its container log with **Start**, **Stop**,
and **Restart**.

While a campaign runs, the view shows the chain of the running scenario instead:
"Scenario counter_stuck is running. 7 of 7 containers are up.", with the
containers of its Compose project `campaign-<scenario>` as they are built and
torn down. Between two scenarios the headline says so.

**Container events** lists, with time, chain (the scenario or the stack),
container, and what happened, every start, exit (with its exit code), stop,
kill, pause, resume, and restart: who was switched on or off, and when. The
dashboard asks Docker for these once a second, through the events API with a
window in the past (`GET /events?since=…&until=…`), and keeps the last 500.
They show what happened since the dashboard started. **Start all** starts
the services in dependency order: the Zenoh router, the Data Broker, DFM, the
gateway, then the chain from the CAN provider to the Guardian. **Stop all**
goes the other way.

Three streams show how a fault travels through the stack's chain (not during
a campaign):

| Stream | Source |
|--------|--------|
| Sample at the Guardian input | uProtocol tap on `BatteryTemperature` (`//battery-vss/9001/1/9001`) |
| Guardian events | uProtocol tap on `GuardianEvent` (`//guardian/9002/1/8001`) |
| Open DTCs in OpenSOVD | The fault list of the `battery_guardian` entity: DTCs that are active or failed since the last clear |

The Guardian, the gateway, and the watchdog share DFM's IPC namespace. When DFM
starts, the dashboard therefore restarts the running services that share its
namespace, as the [README](../../../README.md#lifecycle) asks for. Otherwise
they would stay in the namespace of the stopped DFM and lose their connection
to it.

The taps are listeners of their own, like the campaign's evidence collector.
They keep the last 600 lines each, in memory, and only listen; they never
publish. The dashboard is not the Guardian, so the rule that the Guardian
reads VSS data only through uProtocol is not affected.

## Diagnostics view

Two parts answer which DTCs the Guardian reports.

- **Raised in the last campaign**: every DTC that the evidence chains of the
  last finished campaign show, with its severity, the HARA tests and scenarios
  that raised it, how soon OpenSOVD reported it, and how often it was cleared
  again. It comes from the scenario reports, so it is there after the campaign
  has torn down its chains. The headline sums it up, for example "The campaign
  of 2026-10-07 23:12 raised 10 different DTCs in 20 scenarios."
- **Live chain**: the DTCs of the stack's OpenSOVD, if you started the stack
  under Live chain. A campaign builds its own OpenSOVD for every scenario, so
  this part is empty during and after a campaign: the note says so. Failed or
  active DTCs are listed with severity, summary, status, and the occurrence
  counter; all DTCs of the `battery_guardian` entity follow in a collapsed
  list. A click on a row shows the status bits and the environment data
  (Guardian session, event ID, requirement, triggering sample). **Clear** clears
  one DTC, and **Clear all DTCs** clears the entity. Both use OpenSOVD's
  `DELETE …/faults[/{code}]`, after a confirmation.

OpenSOVD reports severity as a number. The dashboard takes the name from the
DFM fault catalog, and from the number only for a code that is not in the
catalog.

### Running a campaign

The campaign view starts campaigns: all scenarios, or the ones ticked in the
list (all except `external` ones, which need someone else to inject). The
option **rebuild the Guardian and VSS Publisher images first** runs the tool
without `--no-build`, so the campaign judges the current code; without it,
the campaign uses the images that exist. The output of the campaign tool
appears under "Runner output" below the results, and its exit code when it ends (0: every
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

The runner gets the diagnostics image of the running stack as
`DIAGNOSTICS_IMAGE`: it has no registry login, and the image the stack runs is
there locally. Without a running DFM it uses the Compose default from GHCR,
which needs a login (see [Build and Run the Whole Stack](../../how-to/run-the-stack.md)).

The campaign tool runs exactly as from a shell on the host and writes its
evidence to `runs/`. **Stop** stops the runner and removes the
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
