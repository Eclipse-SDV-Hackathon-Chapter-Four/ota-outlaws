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

Web page for the safety evidence: the result of every HARA test with its
evidence chain and a signal plot per scenario, the live signal chain, and the
DTCs in OpenSOVD. How it is built is described in the
[component documentation](../../docs/reference/components/dashboard.md).

## Run

Part of the Compose stack. From the repository root:

```sh
docker compose up --build -d
```

Then open <http://localhost:8080>. To run only the dashboard against a stack
that is already up:

```sh
docker compose up --build -d --no-deps dashboard
```

Campaigns start from the campaign view (**Run all**), or on the host
while the dashboard is open:

```sh
cargo run -p campaign -- run --all
```

## AutoSD / OpenDUT backend

Start the local bench adapter on the native ARM64 host, using the same
configuration as `make remote-run`:

```sh
make -C deploy/autosd dashboard-controller CONFIG=bench/local.toml
```

The existing bench prerequisites, artifact hashes, CLEO/backend configuration
and optional `AUTOSD_BOOTSTRAP_PASSWORD` apply; see the
[two-peer bench README](../../deploy/autosd/bench/README.md). Supply any bootstrap
credential through the environment before starting the adapter. It is never
sent to the browser or dashboard container.

Recreate the dashboard with its updated configuration:

```sh
DASHBOARD_PORT=18080 docker compose up --build -d --no-deps dashboard
```

Choose **AutoSD / OpenDUT** in the **Runtime** selector, then start a campaign.
The dashboard sends selected scenario IDs to the host adapter. The existing
controller creates fresh peers, deploys the openDuT CAN bench, builds application
images and invokes the shared Rust campaign tool on B with the AutoSD runtime
hook. All-scenarios selection includes only cases supported by that runtime;
per-service network isolation and watchdog scenarios are excluded. Compose
remains available from the same selector. Switching is blocked while a campaign
or its cleanup is active.

The outer guest campaign deadline scales with the selected scenario count so
full suites can include repeated workload preparation and restoration. The
individual runtime operations and evaluation timing budgets retain their limits.

The adapter binds to `127.0.0.1:18100` and requires the dashboard header; it
provides no CORS access. On Docker Desktop the container reaches it through
`http://host.docker.internal:18100`. Set `OPENDUT_CONTROLLER_URL` to override
that address. For a native Linux host, run the dashboard on the host with
`OPENDUT_CONTROLLER_URL=http://127.0.0.1:18100`, `DASHBOARD_BACKEND=opendut`,
`REPO_DIR` set to the checkout and `LISTEN=127.0.0.1:18080`; the loopback service
is not exposed to a bridged Linux container. Native mode supports the OpenDUT
backend; Compose launching still requires the dashboard container.

The host adapter and dashboard must use the same checkout. Public reports are
published atomically under `runs/opendut-dashboard/<run-id>`. Controller state,
keys and original guest archives stay under `runs/.dashboard-opendut/<run-id>`.
Only report/recording files are published; archive paths and links cannot expose
keys. Live recordings, reports and read-only Podman workload/log/diagnostic
snapshots are retrieved while the guest campaign runs. Completed reports remain
available after the fresh VMs are removed. The runner log shows preparation and
cleanup phases, and reports sync/cleanup errors without hiding failed scenarios.

**Stop campaign** sends SIGTERM to the owning controller and returns promptly.
The UI keeps the job active until guest restoration, evidence collection and
owned-resource cleanup finish. It does not remove Compose projects or shared
openDuT resources. If cleanup fails or the adapter restarts during a job,
**Stop campaign** retries recovery before another job may start. Keep the host
adapter running for live control; a normal adapter shutdown requests cancellation
and waits for its controller to finish cleanup.

Ankaios owns the ephemeral measured workload, so component start/stop and DTC
clearing are disabled in OpenDUT mode. Component state and container logs come
from peer B; input/output evidence comes from the campaign recordings and plots.
The dashboard does not pretend that local Compose containers are the remote
workload. Remote memory counters and detailed running-container configuration
files are not collected. Diagnostic details are retrieved on demand while B is
active; after teardown use the retained campaign's DTC evidence.

## Configuration

| Variable | Compose value | Meaning |
|----------|---------------|---------|
| `DASHBOARD_BACKEND` | `compose` | Initial runtime (`compose` or `opendut`); can be switched in the page |
| `OPENDUT_CONTROLLER_URL` | `http://host.docker.internal:18100` | Native host adapter URL |
| `DASHBOARD_PORT` | `8080` | Host port, bound to `127.0.0.1` (set when starting Compose) |
| `SOVD_URL` | `http://opensovd-gateway:7690/sovd/v1` | OpenSOVD |
| `SOVD_ENTITY` | `battery_guardian` | SOVD entity of the DTCs |
| `FAULT_CATALOG` | `/repo/deploy/diagnostics/catalog/battery_guardian.json` | Names, summaries, categories, and severities of the DTCs |
| `DATABROKER_ADDR` | `http://kuksa-databroker:55555` | KUKSA Data Broker, for the KUKSA tap |
| `REPO_DIR` | `/repo` | The repository inside the container |
| `RUNS_DIR` | `/repo/runs` | The campaign tool's evidence directory |
| `HOST_REPO_DIR` | — | The repository's path on the Docker host, only if it cannot be read from the dashboard's own mount |
| `ZENOH_CONNECT` | `tcp/zenoh:7447` | Zenoh router, for the uProtocol taps |
| `DOCKER_SOCKET` | `/var/run/docker.sock` | Docker Engine API |
| `DASHBOARD_PROJECT` | — | Compose project, only if the dashboard cannot read it from its own container |
| `LISTEN` | `0.0.0.0:8080` | Address inside the container |

The repository is mounted read-only at `/repo`. A campaign started from the
dashboard runs in a separate `campaign-runner` container with the repository
read-write and host networking; see the
[component documentation](../../docs/reference/components/dashboard.md#running-a-campaign).

## Security

The dashboard controls Docker through its socket, which amounts to root access
on the Docker host. It listens on `127.0.0.1` only and has no login. Do not
publish its port on a shared network.

## Test

```sh
cargo test -p dashboard
make -C deploy/autosd remote-test
```

The tests cover the Docker API parsing (HTTP responses, chunked bodies, log
frames, tar archives), the HARA titles, the start
order, the taps' ring buffers and line formats, the DTC catalog join, the
campaign evidence reader and run lookup, the signal plot series (DTC flag
changes, half-written recordings), the campaign runner's container settings and path
translation, and the header check for changes.

The bench tests also cover adapter cancellation and cleanup recovery across
restarts, runtime capability filtering, and safe report publication. Subprocess
lifecycle tests use an explicit mock; they do not replace running campaigns on
real AutoSD peers. For an integration check, launch a Compose campaign, switch
to AutoSD after completion, launch a remote campaign, cancel it, wait for
restoration/cleanup, then switch back. Check that the runtime selector stays
disabled while either job or its cleanup is active and that each runtime's
reports remain accessible.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`), and **Codex** using **GPT-6** (`gpt-6`).
