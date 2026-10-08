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

Hack Coach Interview Presentation: [Hack Coach Interview](OTA_Outlaws_HackCoachInterview_[EF_SDV_Hackathon_2026].pptx)

Pitch Slides : [Pitch Slides](OTA_Outlaws_Pitch_[EF_SDV_Hackathon_2026]_v2.pptx)

## Repository layout

| Folder | Content |
|--------|---------|
| [`components/`](components/) | Everything that is built: [`guardian`](components/guardian/) (core), [`guardian-service`](components/guardian-service/) (uProtocol service), [`watchdog`](components/watchdog/), [`vss-publisher`](components/vss-publisher/), [`contracts`](components/contracts/) (Protobuf), [`dashboard`](components/dashboard/), and the [`campaign`](components/campaign/) tool with its scenarios and CAN traces |
| [`config/`](config/) | [`guardian/`](config/guardian/) safety parameters and [`can/`](config/can/) DBC, VSS mapping, and the default CAN trace |
| [`deploy/`](deploy/) | [`docker-compose.yml`](deploy/docker-compose.yml) of the stack and [`diagnostics/`](deploy/diagnostics/) (OpenSOVD image, fault catalog), and [`autosd/`](deploy/autosd/) (AutoSD/Ankaios deployment and the openDuT bench); `docker compose up` works from the repository root through [`compose.yaml`](compose.yaml) |
| [`hardware/az3166/`](hardware/az3166/) | The MXChip AZ3166 board (ThreadX firmware) and the host bridge that starts campaigns from its button |
| [`third-party/`](third-party/) | `fault-lib`, vendored |
| [`docs/`](docs/) | Reference, how-to, and explanation |

## Run the tests

```sh
cargo test                                # all components, no Docker needed
cargo run -p campaign -- run --all        # fault campaigns through the real chain
```

The campaigns need Docker and the published diagnostics image described below. All test
levels, from unit tests to the hardware demo, are described in
[Run the Tests](docs/how-to/run-tests.md).

Compose runs KUKSA plus separate `opensovd-dfm` and `opensovd-gateway` containers.
Both services pull the versioned GHCR image configured in `deploy/diagnostics/image.env`.
The image supports Linux AMD64 (CI) and ARM64 (Apple Silicon and AutoSD).
The image also contains legacy example tools; the campaign suite uses Guardian.
This is a custom development image because the
published upstream gateway does not contain the example's DFM adapter.

## Pull the published diagnostics image

```sh
set -a
. deploy/diagnostics/image.env
set +a
docker compose pull opensovd-dfm opensovd-gateway
docker compose up -d --wait opensovd-dfm opensovd-gateway
```

Guardian CI pulls this image and checks its source-revision label. It still builds
our Guardian test runner, because that contains the code under test.
The separate **Diagnostics image** workflow is called before main-branch tests
and can also be manually dispatched. It checks for the pinned version first and
only builds when that tag is missing. Pull-request CI only pulls existing images;
publish a new pin before testing a PR that changes it. Existing tags are reused. Bump the `-v1` suffix in `deploy/diagnostics/image.env` and both Compose defaults
when changing the recipe; update the source SHA in the same places when upgrading.
The workflow builds on native AMD64 and ARM64 runners and caches build layers.
Publishing uses GitHub Actions' `GITHUB_TOKEN` with `packages: write`; routine CI
only needs `packages: read`. The package must be public for anonymous pulls and
fork PRs. An organization owner can set its visibility on the package settings page.

## Build from clean committed source (optional)

```sh
sh deploy/diagnostics/build-images.sh /path/to/Doctor-Whodunit
DIAGNOSTICS_IMAGE=local/opensovd-demo-fork:verified docker compose up -d --wait opensovd-dfm opensovd-gateway
curl -fsS http://localhost:7690/sovd/v1/apps/battery_guardian/faults
```

The source revision is pinned to `97dd4a503f25674e866a89829e2bd92d2cf2655d` in
`deploy/diagnostics/image.env`. This committed Doctor-Whodunit revision contains separate commits for restoring omitted upstream CLI
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

Compose pulls the published image when missing; starting diagnostics does not
compile anything. Set `DIAGNOSTICS_IMAGE=local/opensovd-demo-fork:verified` to use
a local source build. Override `DIAGNOSTICS_IMAGE` only with an image providing the required binaries at
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

DFM loads `deploy/diagnostics/catalog/battery_guardian.json`; its configured storage
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

The Guardian service (`guardian-service`) writes ten diagnostic codes to DFM.

- the input faults of `guardian::FaultCode`: `BTG_TempFreshnessLost`,
  `BTG_TempCounterStuck`, `BTG_TempSignalStuck`, `BTG_TempQualityInvalid`,
  `BTG_TempOrderImplausible`, `BTG_TempOutOfRange`, `BTG_TempRateImplausible`,
  and `BTG_TempNoDataAtStartup`;
- the overtemperature codes `BTG_TempOverTempWarning` and
  `BTG_TempOverTempCritical`, derived from thermal state changes
  (`components/guardian-service/src/dtc.rs`). They fail when the thermal state reaches
  WARNING or CRITICAL from valid data and pass when it is lowered again. A
  WARNING raised by an implausible sample is caused by its sensor fault and is
  not an overtemperature.

Mitigation requests remain uProtocol events only.

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
also includes the detecting requirement, Guardian monotonic time, the fault type
(`fault_type`) and severity from the catalogue, and the triggering sample
(`sample`: sequence, source timestamp, and alive counter) when available. The
DFM keeps the catalogue's severity in its records, but not the fault type, so
both are written into the environment data. A record from an old
session cannot confirm a new failure event. `FaultRecovered` links to its
original detection; DFM preserves that detection’s metadata on Passed. A
`FaultTestPassed` event confirms initial healthy observation and can clear a
previous session’s current failure after testing, while preserving history. Campaign tests own scenario/hazard metadata;
the Guardian does not know which campaign is running.

Configuration: `FAULT_CATALOG` (container default `/etc/guardian/catalog/battery_guardian.json`)
and `SOVD_ENTITY` (default `battery_guardian`). Campaigns collect OpenSOVD
evidence against the visibility budget in the scenario catalog. uProtocol
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

From `deploy/autosd`, deploy with `make up` and run the shared campaign driver
inside AutoSD through Ankaios:

```sh
make campaigns
make campaigns CAMPAIGNS="timeout counter_stuck invalid_quality"
```

The scenarios in `components/campaign/scenarios.toml` exercise the deployed CAN → KUKSA →
uProtocol → Guardian → DFM → OpenSOVD chain. Reports and recordings are copied
into `runs/autosd/<run>/`; the original replay is restored after each campaign.
See [AutoSD deployment](deploy/autosd/README.md) for prerequisites and
[Run the tests](docs/how-to/run-tests.md) for unit tests and the Compose runtime.

Diagnostics outage is not covered by the shipped scenario catalog. Campaigns
verify mitigation requests; physical actuator effects and durable fault
persistence remain outside their assertions.

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
OpenSOVD returned the catalog with reset fault statuses. This was observed in the previous development image and is not presented as passing.
Campaigns do not assert durable persistence.

## Historical diagnostic validation

The retired diagnostic harness passed five isolated checks on 7 October 2026.
Its passing evidence remains under `deploy/diagnostics/reports/20261007-002330-a5a9a93c/`;
the earlier failed packaging-transition run remains under
`deploy/diagnostics/reports/20261007-002232-cebb1a09/`. These historical results do not
establish coverage in the current campaign catalog.

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6.1 Sol** (`gpt-6.1-sol`). The section "Run the tests" was added with the
assistance of **Claude Code** using the model **Claude Opus 5.5**
(`claude-opus-5-5`).

The AutoSD campaign validation note was added with assistance from **Codex**
using **GPT-6** (`gpt-6`).
