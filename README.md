<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# OTA Outlaws

Project plan: [Project Plan](docs/reference/project-plan.md)

Compose runs KUKSA plus separate `opensovd-dfm` and `opensovd-gateway` containers.
Both services use the single local image `local/opensovd-demo-fork:verified`.
The image also contains the example Guardian and fault injector used by the smoke check.
This is a custom development image because the
published upstream gateway does not contain the example's DFM adapter.

## Build from clean committed source

```sh
sh diagnostics/build-images.sh /path/to/Doctor-Whodunit
docker compose up -d --wait opensovd-dfm opensovd-gateway
curl -fsS http://localhost:7690/sovd/v1/apps/battery_guardian/faults
```

The source revision is pinned to `97dd4a503f25674e866a89829e2bd92d2cf2655d` in
`build-images.sh`. This local Doctor-Whodunit revision contains separate commits for restoring omitted upstream CLI
support crates and repairing the diagnostic demo. It retains the
adapter's original Git dependency: `bburda42dot/fault-lib` at
`2b638d84a38568a70d5acab4b46cbe17a84e8e7c`. The DFM daemon still builds from the
bundled fault library. It is not unmodified upstream `opensovd-core`.

The build refuses a dirty source checkout. It uses `git archive` to include only
tracked files at the pinned commit, builds with Cargo's `--locked` option, and
makes no source patches during the build. Base images are pinned by digest. It
copies compiled binaries into a runtime image rather than saving a running
container. Both services run different binaries from the same runtime image.

The image records its source commit in `org.opencontainers.image.revision` and a
Git blob manifest at `/usr/share/opensovd/source-files.txt`. `SOURCE_REF` can select
another committed integration revision. The Dockerfile and build helper are
tracked here, so there are no undocumented build steps.

Compose uses existing local images (`pull_policy: never`); starting services does
not compile anything. Override `DIAGNOSTICS_IMAGE` only with an image providing the required binaries at
`/usr/local/bin/`. To transfer the built image:

```sh
docker save local/opensovd-demo-fork:verified -o /tmp/diagnostics-image.tar
# Transfer the archive to the other machine, then:
docker load -i /tmp/diagnostics-image.tar
```

To start the existing KUKSA services as well, run `docker compose up -d`.
The databroker is published on host port `55556`; set `KUKSA_HOST_PORT` to override
it. Containers continue using `kuksa-databroker:55555`.

## Interfaces and IPC

DFM loads `diagnostics/catalog/battery_guardian.json`; its configured storage
directory is mounted at `/data` using the `dfm-storage` volume. Gateway exposes
`http://localhost:7690/sovd/v1/apps/battery_guardian/faults`.
Set `SOVD_PORT` to change the published port; access is bound to localhost.

The containers share a private IPC namespace. Each has a fresh `/tmp`; the
entrypoint links `/tmp/iceoryx2` to `/dev/shm/iceoryx2` so discovery files and shared
memory reset together when DFM is recreated.
Host IPC and privileged containers are not required. A future Guardian must join
`ipc: service:opensovd-dfm`, use the same entrypoint and fresh `/tmp`, and initialize
`fault_lib::FaultApi` with the same catalog. Gateway health checks query the real
DFM fault catalog and verify both services' communication. Checks run every two
seconds during the 30-second startup grace period until healthy, then every five
minutes to limit recurring request logs.

## Verification

```sh
python3 diagnostics/smoke_test.py
```

This creates a randomly named, isolated Compose project with separate storage and
port 17690 (`DIAGNOSTICS_TEST_PORT` overrides it). It does not touch the development
stack's fault records. It checks the five catalog faults, injects overtemperature
through uProtocol/Zenoh, verifies Guardian -> DFM -> OpenSOVD readback, clears the
active test fault, and verifies service availability after recreation. The isolated
test containers and volumes are removed afterward; failures print service logs.

The optional `diagnostics/compose.smoke.yml` overlay uses the image's **example Guardian and injector as test tools**, not the team's Guardian
implementation. This check does not validate CAN replay, mitigation, or a complete
correlated challenge evidence chain.

## Lifecycle

```sh
docker compose logs opensovd-dfm opensovd-gateway
docker compose down
```

The storage volume survives `down`; `docker compose down -v` removes it.
**Known limitation:** DFM fault-state durability across abrupt restart is not established.
The previous development image lost fault state in a restart test.
A volume mount alone does not establish durable fault persistence.
If DFM is recreated, recreate gateway and any reporters together, because they
join DFM's IPC namespace:

```sh
docker compose up -d --force-recreate --wait opensovd-dfm opensovd-gateway
```

The CAN services use the repository's BMS_MSG1 DBC, replay trace, and VSS mapping
in `can/`. This feeds the databroker. Start the VSS publisher in `vss-publisher/`
separately to connect the KUKSA path to Guardian through uProtocol.

Final reporting, clearing, and service-recreation checks passed on 6 October 2026
against the clean source image. Its embedded source manifest was verified against
the pinned Git commit; the source checkout remained clean.

## Validation history

On 6 October 2026, the initial reporting/readback check succeeded. The first
recreation check failed with `ServiceInCorruptedState` because discovery files
outlived shared memory; the entrypoint now keeps both in `/dev/shm` and resolves
that issue. A subsequent fault-persistence check failed: after DFM recreation,
OpenSOVD returned the catalog with reset fault statuses. This was observed in the previous development image and is not presented as passing. The smoke test checks
reporting, clearing and service recovery; it does not assert durable persistence.

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6.1 Sol** (`gpt-6.1-sol`).
