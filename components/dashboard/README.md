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

## Configuration

| Variable | Compose value | Meaning |
|----------|---------------|---------|
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
```

The tests cover the Docker API parsing (HTTP responses, chunked bodies, log
frames, tar archives), the HARA titles, the start
order, the taps' ring buffers and line formats, the DTC catalog join, the
campaign evidence reader and run lookup, the signal plot series (DTC flag
changes, half-written recordings), the campaign runner's container settings and path
translation, and the header check for changes.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
