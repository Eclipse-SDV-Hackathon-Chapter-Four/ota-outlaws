<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Use the Dev Container

The repository ships a [Dev Container](https://containers.dev/) in
[`.devcontainer/`](../../.devcontainer/) so everyone builds with the same toolchain.

## What is inside

| Tool | Purpose |
|------|---------|
| Rust (stable), clippy, rustfmt | Battery Thermal Guardian |
| `protoc` | uProtocol / KUKSA gRPC code generation |
| Python 3.12, `kuksa-client` | Scripts and inspecting the KUKSA Data Broker |
| Docker (in Docker) + Compose | Running `docker-compose.yml` from inside the container |
| `can-utils`, `jq` | Inspecting CAN traces and evidence JSON |

Port `55555` (KUKSA Data Broker) is forwarded to the host.

## Open it

**VS Code:** install the *Dev Containers* extension, open the repository, and run
**Dev Containers: Reopen in Container**.

**CLI:**

```bash
npm install -g @devcontainers/cli
devcontainer up --workspace-folder .
devcontainer exec --workspace-folder . bash
```

## Run the KUKSA stack inside the container

```bash
docker compose up -d
kuksa-client grpc://127.0.0.1:55555
```

`kuksa-client` is for debugging only. The Guardian must receive VSS data through
the uProtocol service interface, never from the Data Broker directly.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
