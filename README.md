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

## Run the tests

```sh
cargo test                                # all components, no Docker needed
cargo run -p campaign -- run --all        # fault campaigns through the real chain
```

The campaigns need Docker and the diagnostics image built below. All test
levels, from unit tests to the hardware demo, are described in
[Run the Tests](docs/how-to/run-tests.md).

Compose runs KUKSA plus separate `opensovd-dfm` and `opensovd-gateway` containers.
Both services use the single local image `local/opensovd-demo-fork:verified`.
The image also contains legacy example tools; the campaign suite uses Guardian.
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
Host IPC and privileged containers are not required. Guardian joins
`ipc: service:opensovd-dfm`, uses the same entrypoint and fresh `/tmp`, and initializes
`fault_lib::FaultApi` with the same catalog. Gateway health checks query the real
DFM fault catalog and verify both services' communication. Checks run every two
seconds during the 30-second startup grace period until healthy, then every five
minutes to limit recurring request logs.

## Guardian reporting

The Guardian service (`guardian-service`) writes the four diagnostic codes emitted by
`guardian::FaultCode` to DFM: `BTG_TempFreshnessLost`, `BTG_TempCounterStuck`,
`BTG_TempSignalStuck`, and `BTG_TempQualityInvalid`. The catalogue deliberately
contains only these four codes. Thermal state changes and mitigation requests
remain uProtocol events; they are not invented diagnostic faults.

Guardian joins DFM's private IPC namespace and loads the same mounted catalogue.
A dedicated worker initializes `fault_lib::FaultApi`, creates reporters, and
reports Failed and Passed records. Startup records are NotTested, never a fabricated healthy
baseline. The core stays independent of diagnostics and continues operating while
DFM or OpenSOVD is unavailable. The reporting queue is bounded and nonblocking;
rejected events are counted and logged. The worker retries explicit enqueue
errors and preserves fault lifecycle ordering. Successful enqueue is logged as
such; Guardian does not poll OpenSOVD or claim delivery confirmation. Readback,
correlation and visibility checks run in the integration tests today and belong
in a future evidence collector for deployed operation.

Each service start generates a fresh UUID session ID. The uProtocol event envelope
and DFM environment data contain that session ID and event ID. Environment data
also includes the detecting requirement, Guardian monotonic time, and last sample
sequence, source timestamp and alive counter when available. A record from an old
session cannot confirm a new failure event. `FaultRecovered` links to its
original detection; DFM preserves that detection’s metadata on Passed. A
`FaultTestPassed` event confirms initial healthy observation and can clear a
previous session’s current failure after testing, while preserving history. Campaign tests own scenario/hazard metadata;
the Guardian does not know which campaign is running.

Configuration: `FAULT_CATALOG` (container default `/etc/guardian/catalog/battery_guardian.json`)
and `SOVD_ENTITY` (default `battery_guardian`). `SOVD_URL` is used only by the
integration tests. Their OpenSOVD visibility budget is two seconds. uProtocol
event sends have a 100-ms timeout. Enqueue success does not guarantee diagnostic
visibility or durable delivery across DFM restarts; a future collector must
observe and report those failures.

Recovery uses `config/guardian/safety-params.toml`: at least ten consecutive
fresh, valid, healthy samples over at least one second. Every active input fault
must recover before monitoring returns to OK. A stuck maximum must move again.
Thermal recovery requires monitoring OK and 2 °C hysteresis: Critical → Warning
below 53 °C, then Warning → Monitoring below 43 °C, each with a separate recovery
period. Interruptions reset the period; escalation remains immediate. Recovery
updates diagnostic state and publishes transitions, but does not verify or withdraw
warnings on a physical actuator.

## Verification

```sh
python3 diagnostics/smoke_test.py
```

This builds the Guardian integration test image and runs five isolated
campaigns: freshness loss, signal stuck, counter stuck, invalid quality, and a
DFM/gateway outage. Inputs use the `BatteryTemperature` protobuf over real
uProtocol/Zenoh. Tests assert baseline diagnostics, the fault → degraded →
mitigation-request cause chain, session IDs, all available diagnostic metadata,
and visibility timing. The outage pauses both diagnostics services, verifies the
safety request while they are paused, resumes them, and checks retained evidence
readback. Every scenario gets a fresh process, IPC namespace and storage volume,
ensuring independent campaign evidence. Each campaign then restores healthy input
and verifies fault recovery, monitoring OK, and matching DFM Passed readback
without deleting fault history.

Reports, failed verdicts and logs are preserved in a unique timestamped directory
under `diagnostics/reports/`; test containers and isolated volumes are removed.
`DIAGNOSTICS_TEST_PORT` changes the default port 17690. `SKIP_TEST_BUILD=1` reuses
an already built test image. The development stack is never cleared by these tests.

These tests prove the team's diagnostics path and mitigation **requests**. They do
not prove an actuator effect, CAN/KUKSA replay, AutoSD, Ankaios or remote reruns.
The image's starter Guardian/injector binaries are no longer used by this suite.

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
in `can/`. This feeds the databroker. Compose starts the VSS publisher and Guardian to connect the KUKSA path
through uProtocol.

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

## Guardian integration validation

On 7 October 2026, all five final Guardian campaigns passed: freshness loss,
signal stuck, counter stuck, invalid quality, and paused DFM/gateway with retained
fault readback after resume. Each asserted exact session/event metadata and the
fault → degraded → mitigation-request cause chain. Normal diagnostic visibility
was within the 2-second budget. The workspace's 49 tests, five VSS Publisher tests,
formatting and warning-free lint checks passed. The updated Guardian runtime image
was built and the live Guardian/DFM/gateway services were recreated successfully;
the live gateway exposes exactly the four core diagnostic codes.

Evidence is preserved under `diagnostics/reports/20261007-002330-a5a9a93c/`.
An earlier packaging-transition run is also retained under
`diagnostics/reports/20261007-002232-cebb1a09/`: its counter campaign failed before
startup while the test image/entrypoint was being updated. It is not hidden or
counted as a passing campaign. The subsequent complete final suite passed.

Signal-stuck testing and recovery require the full three-second detector observation
period plus the one-second healthy confirmation period. A maximum that jumps once
and freezes again while reference temperatures move does not recover. The local
`third-party/fault-lib` patch blocks newer IPC records behind older retries;
Guardian still performs no OpenSOVD polling. Delivery remains best effort, with
bounded queues and the upstream retry limit. The outage campaign restores healthy
input while diagnostics are paused, then verifies the final Passed state after resume.

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6.1 Sol** (`gpt-6.1-sol`). The section "Run the tests" was added with the
assistance of **Claude Code** using the model **Claude Opus 5.5**
(`claude-opus-5-5`).
