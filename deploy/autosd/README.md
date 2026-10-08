<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Declarative OTA Outlaws deployment on AutoSD

The seven existing ARM64 services are declared in `pod.yaml`. `ankaios.yaml`
assigns that pod to the AutoSD agent using Ankaios's `podman-kube` runtime and
`ON_FAILURE` restart policy. Two systemd units run the local Ankaios server and
agent. Application code and uProtocol contracts are unchanged. The `campaigns`
target runs the shared scenario catalog inside AutoSD through Ankaios.

The default target is the existing automated `bench-beta` from sibling
`can-testbench/bench.json`, using that bench's SSH key and trusted known_hosts.
The original manual AutoSD VMs have separate disks.

For the portable local openDuT-connected two-peer campaign bench,
use the same Makefile from `deploy/autosd`:

```sh
make remote-doctor
make remote-run
make remote-run SCENARIOS=""
make remote-cleanup STATE=/absolute/path/to/run
```

Configure `bench/local.toml` first; see [bench/README.md](bench/README.md) for
prerequisites and evidence. `make remote-test` checks the bench helpers. The
`up`, `campaigns` and other existing targets retain the single-VM replay path.

## Deploy and run

Run Make **on your host computer**. It reaches the AutoSD guest over SSH.
Service configuration stays in `pod.yaml` and `ankaios.yaml`; the Makefile
uses native SSH, systemd and Ankaios commands for lifecycle and access.
`transfer.sh` prepares/transfers the deployment and installs missing binaries.

Run these commands from `deploy/autosd`:

```sh
make up
make status
make logs SERVICE=guardian
make connect
make down
```

| Target | Purpose |
|---|---|
| `up` | Prepare existing images/configuration, stop the old workload, transfer updates, install missing Ankaios, start and wait for readiness. |
| `up SKIP_IMAGES=1` | The same workflow, reusing application images already loaded on AutoSD. |
| `down` | Cancel active campaign jobs, wait for source restoration, then remove the Ankaios workload and stop its units. Retain images, storage and evidence. Safe to repeat. |
| `status` | Show active campaign jobs, Ankaios workload states and Podman container states. |
| `logs SERVICE=guardian` | Show the last 100 lines for the selected service; explain temporary absence during campaign restart/restoration. Defaults to Guardian. |
| `connect` | Keep OpenSOVD/Zenoh SSH forwards open until Ctrl-C. |
| `campaigns` | Run the shared Rust campaign CLI on the Ankaios-managed AutoSD workload and copy evidence to the host. |
| `help` | Show available targets; also the default when no target is given. |

Run the AutoSD campaigns after `make up`:

```sh
make campaigns
make campaigns CAMPAIGNS=counter_stuck
make campaigns CAMPAIGNS="timeout invalid_quality" CAMPAIGN_OUT=/tmp/autosd-evidence
```

`campaigns` builds the existing Rust campaign CLI for Linux ARM64, transfers it
and its catalogue inputs, and runs it **inside AutoSD**. The scenario driver,
recording, evaluation and reports are the same code used by `campaign run`.
Docker on the host compiles the tool; Ankaios manages the guest workloads.

The runner accepts a generic `--runtime-hook` command. AutoSD supplies
`deploy/autosd/runtime.sh`, which translates start/pause/stop/log/cleanup commands
to Ankaios and Podman. It contains no scenarios, recording logic or verdicts.
Replay startup gates live in `pod.yaml`; the adapter switches campaign mode on
for one-shot replay and restores the original source after each scenario. It
never rewrites the Ankaios manifest. Normal deployment still replays infinitely.
Update the guest configuration with `make up SKIP_IMAGES=1` after pulling this
change; the adapter checks for the startup gates before stopping the stack.

`--all` selects catalogue cases supported by the runtime. Planned verdicts remain
visible under the existing CLI's exit policy. The host-networked AutoSD pod has
no per-service network isolation, so `transport_dropout` is excluded and explicit
selection fails before changing the workload. External injection uses `observe`.
Source shutdown suspends the provider for the rest of that scenario, keeping
Ankaios from recreating the pod. Physical actuator effects are not verified.

A detached systemd job and inherited guest lock keep execution independent of
SSH. Repeated `make campaigns` calls reconnect to the same active job. SIGTERM
and Ctrl-C trigger the shared runner's cleanup; `make down` cancels the job and
waits before stopping the stack. Each scenario restarts the stack for its input
and again for restoration, interrupting the running demo. The CLI waits for
restoration before reporting that scenario's verdict.

Reports and recordings stay in `AUTOSD_GUEST_DIR/campaign-reports/<run>/` and are
copied to `runs/autosd/<run>/` by default. Per-scenario files include the original
source backup, service logs, manifest and JSON/Markdown report. `input.tar.gz`
retains the transferred catalogue, traces, safety parameters and adapter.
`campaign.md` is the shared CLI's aggregate report; `exit-code` records its final
status. Failed cases and cleanup errors remain in the evidence. Guest
`systemd-run` and the deployment's BusyBox are required; Python is not required.
Application images are reused. Safety parameters and fault catalogue must match
the deployed inputs; a mismatch fails before stopping the stack.

For your already provisioned bench, update configuration with compatible guest images:

```sh
make up SKIP_IMAGES=1
```

Every `up` restarts the workload. It prepares the local bundle before stopping
services, then transfers it. A later transfer/install/start failure can leave the
stack stopped; fix the reported error and rerun `up`. Readiness means all seven
containers are running and OpenSOVD responds. It is not a campaign verdict.

Host prerequisites: Make, Bash, Docker, jq, SSH/scp, curl, tar/gzip and shasum.
Build the existing ARM64 application images using the repository instructions
before the first deployment. Make does not compile application source or pull
application images. The service/image mapping lives in `images.tsv`;
`DIAGNOSTICS_IMAGE` overrides the combined DFM/gateway image.

Before changing the guest, `up` compares the repository's DTC list with the
catalog embedded in the selected Guardian image. With `SKIP_IMAGES=1`, it checks
the image already loaded on AutoSD. If the lists differ, deployment stops with
rebuild instructions. For a catalog change, build Guardian from the same checkout
and transfer the image:

```sh
docker build --platform linux/arm64 -t ota-outlaws/guardian:dev -f ../../guardian-service/Containerfile ../..
make up
```

`SKIP_IMAGES=1` does not rebuild or transfer updated binaries.

`up` renders `pod.yaml` and embeds it as Ankaios's `otaPod` configuration object
in `ankaios-state.yaml`. It transfers CAN replay/mapping, diagnostic catalogue,
entrypoint and safety parameters. The guest's `images.json` records host image
IDs/digests and labels; `loaded-images.json` records actual loaded Podman image
IDs. Aliases refer to the images loaded under their original tags, because Docker
and Podman IDs may differ. `imagePullPolicy: Never` prevents guest pulls.
A static ARM64 BusyBox 1.37.0 tool is extracted from a pinned official image for
the shell-free CAN provider and guest bundle extraction; Docker may pull this
tool image. The AutoSD guest does not need a separate tar installation.

Missing Ankaios **1.0.4 ARM64** binaries are downloaded from the official release
and checked against its SHA-512 checksum before the stack is stopped. Systemd
units are refreshed on every `up`. The local server listens on `127.0.0.1:25551`;
the agent connects without TLS. A future remote server needs its own network and
authentication configuration.

Startup waits up to 75 seconds for sockets using KUKSA's local port 55555 to
expire (including TCP TIME_WAIT), then checks readiness for up to 90 attempts.
Ankaios internal pod recreation after arbitrary crashes remains unverified.

Units are installed without enabling automatic boot startup. To enable it, run
on the guest:

```sh
systemctl enable ota-ankaios-server.service ota-ankaios-agent.service
```

Guest reboot recovery has not been tested. `down` does not disable boot startup.

## Guest configuration

The pod contains DFM, OpenSOVD gateway, Zenoh, KUKSA databroker, publisher,
Guardian and CAN provider. It uses guest host networking with loopback TCP
listeners because this guest's bridge networking failed. Explicit IPv4 Zenoh
listeners accommodate the kernel's unavailable IPv6 socket family. Existing
Zenoh multicast discovery remains enabled. Publisher waits for KUKSA and Zenoh
TCP listeners before execution; the CAN provider uses the mounted BusyBox tool
to wait for KUKSA. These avoid transient connection failures being treated as
whole-workload failures during startup.

Diagnostics and Guardian use the pod's shared IPC namespace and default shared
`/dev/shm` (63 MiB usable on the tested guest). The existing entrypoint links
iceoryx discovery files into that shared memory. Each gets a separate memory
backed `/tmp`. Do not override `/dev/shm` with `emptyDir.medium: Memory`: Podman
6.1.0 makes those mounts container-local, breaking diagnostic discovery.

Deployment labels its dedicated configuration and DFM volume as shared
`container_file_t:s0`, including migration from the old private volume label.
SELinux stays enforcing. DFM durability across abrupt restarts is not established;
retaining a volume does not prove it. EDGAR and the CAN bench remain separate.

The provider replays its existing recorded BMS trace inside its container,
ramping from 40 to 140 degrees C. It crosses both thermal thresholds and the
125-degree plausibility limit, so out-of-range degradation is expected during
each replay cycle with the current Guardian. The default single-VM deployment
uses that in-container replay. The openDuT two-peer bench sets
`AUTOSD_CAN_MODE=socketcan`, which starts the same provider in SocketCAN mode so
its frames arrive through the managed CAN connection from peer A. Mitigation
events establish requests, not physical actuator effects.

## Connections for the generator and collector

Keep this running in a terminal:

```sh
make connect
```

- OpenSOVD: `http://127.0.0.1:17690/sovd/v1/apps/battery_guardian/faults`
- Zenoh for uProtocol: `tcp/127.0.0.1:17447`

Guest TCP listeners and SSH forwards bind to loopback. Host forwarding ports
can be changed with `SOVD_PORT=27690` and `ZENOH_PORT=27447` on the `connect` target.
The default replay feeds CAN → KUKSA → publisher → uProtocol → Guardian.
A generator that publishes directly over uProtocol must first arrange for the
built-in CAN provider to be stopped, otherwise both sources compete. The helper
no longer includes ad hoc source manipulation commands. Guardian never reads
the databroker directly.

```sh
make logs SERVICE=publisher
make logs SERVICE=gateway
```

## Target overrides

Use `AUTOSD_SSH_HOST`, `AUTOSD_SSH_PORT`, `AUTOSD_SSH_KEY`,
`AUTOSD_KNOWN_HOSTS`, `AUTOSD_BENCH_DIR`, `AUTOSD_GUEST_DIR` and
`AUTOSD_AGENT_NAME` for another existing AutoSD guest. Host, port, key, guest
directory and agent are Make variables as well as environment overrides. Example:

```sh
make status AUTOSD_SSH_PORT=2233
make connect SOVD_PORT=27690 ZENOH_PORT=27447
```

The guest directory must be an absolute path containing only letters, digits,
underscores, dots, slashes and hyphens. Agent names accept letters, digits,
underscores and hyphens. Guest requirements: root Podman, Bash, systemd,
`ss`, curl, synchronized clocks and sufficient image storage/memory.
The target host key must already be trusted. Deployment no longer uses Python.

## Executed validation — 7 October 2026

### Ten-code catalog and guest-local source dropout

After main added the overtemperature DTCs, a configuration update with the old
Guardian image failed with `catalog must contain exactly the core fault codes`.
The deployment preflight now rejects mismatched image/catalog DTC lists before
stopping services or transferring configuration. The old guest image was rejected
with `SKIP_IMAGES=1`; a fresh ARM64 Guardian build from `166e783` then deployed
successfully through `make up`, exposing ten codes with all seven services running.

The native ARM64 Rust campaign observer was rebuilt from the same source.
A detached guest-local systemd job paused the CAN provider for three seconds,
collected over guest loopback and reported PASS: fault 55 → degraded 56 → warning
57 → recovered 58 → OK 59. OpenSOVD confirmed the correlated failure and recovery.
Guardian kept the same container and process throughout; the original replay was
restored and hash-verified. Local evidence is retained under
`diagnostics/reports/autosd-guest-native/runs/20261007-132402-221522/`.
This historical check verifies source dropout,
not the full campaign suite or both new overtemperature DTCs.

### Rebased application build check

Rebuilt Guardian and publisher ARM64 images after rebasing onto `263da38`
and deployed them with `make up`. The then-current eight-code catalog was exposed,
all seven services ran under Ankaios, and the CAN replay reached the publisher.
Formatting, lint and standard Cargo tests passed. Repeated `make down` and a
configuration-only `make up SKIP_IMAGES=1` were also checked.

The team's existing Rust collector/verdict tool observed a source dropout over
`make connect` forwards on ports 27690/27447. For a healthy baseline, the team's
`campaign/traces/nominal.asc` temporarily replaced the guest replay; the actual
CAN provider was paused for three seconds and resumed. The result was PASS:
fault 84 → degraded 85 → mitigation request 86 → recovered 87 → OK 88, with a
matching Failed then Passed OpenSOVD record retaining fault history. Evidence is
retained under `runs/autosd-pr/20261007-123504-observe-source_dropout/`.

```sh
# While make connect SOVD_PORT=27690 ZENOH_PORT=27447 is running:
cargo run -p campaign -- observe source_dropout --seconds 25 \
  --zenoh tcp/127.0.0.1:27447 --sovd http://127.0.0.1:27690/sovd/v1
```

`observe` records and judges a manually injected fault; it does not inject it.
The original guest trace was restored afterward. This fresh check covers source
dropout on the rebased build. The five-case results below are retained from the
earlier application build; that full suite was not repeated after rebasing.

### Earlier deployment and campaign checks

The Make workflow was exercised on the existing bench: `make up` exported,
transferred and loaded all application images, recorded seven ARM64 Podman image
entries and reached readiness. Two consecutive `make down` calls succeeded,
followed by `make up SKIP_IMAGES=1`. `make status` reported `Running(Ok)` with
all seven services; publisher logs showed the CAN replay flowing. `make connect`
forwarded both OpenSOVD and Zenoh on alternate local ports, and Ctrl-C closed it.
Shell syntax checks passed. Existing Ankaios binaries were reused; fresh binary
installation was not repeated. The guest was left running with the default replay.
These deployment checks do not rerun the fault campaigns recorded below.

Ankaios 1.0.4 reported `thermal-stack / bench-beta / podman-kube / Running(Ok)`
on AutoSD with Podman 6.1.0 and SELinux enforcing. All seven services ran and the
replay fed KUKSA → publisher → uProtocol → Guardian. OpenSOVD returned four codes.
The helper's supervised stop/deploy/start lifecycle was exercised.

Stopping the provider produced event 72 (FreshnessLost), 73 (degraded, cause 72)
and 74 (monitoring-unavailable mitigation request, cause 73). OpenSOVD reported
`BTG_TempFreshnessLost.testFailed=true`. Restarting it produced event 75
(recovered, cause 72) and 76 (monitoring OK, cause 75). OpenSOVD then reported
`testFailed=false` while retaining `testFailedSinceLastClear=true`.
This is manual deployment validation, not a generated campaign verdict.
The existing CAN bench also passed all 22 bidirectional checks with this stack
running; the systemd units passed `systemd-analyze verify`.

### Historical isolated diagnostic checks

The retired diagnostic harness ran five isolated checks on AutoSD. Both runs
remain under `diagnostics/reports/autosd/`: `20261007-093427-fb4566f0` failed
before fault injection; `20261007-093633-bc228709` passed. These checks ran a
separate Guardian process rather than exercising the deployed Ankaios workload,
and do not establish coverage in the current scenario catalog.

### Shared driver validation

The generic runtime-hook implementation passed `timeout` inside AutoSD, with
uProtocol recordings and correlated diagnostics, and restored the original
replay afterward. Evidence: `runs/autosd/20261007-165018-11932/`.
The existing host `campaign run normal --no-build` path also passed through the
same driver. All 37 evaluator tests and campaign lint checks passed.

An earlier hook run failed when OpenSOVD became ready before Zenoh; its
INCONCLUSIVE evidence remains in `runs/autosd/20261007-164711-11560/`.
The adapter now waits for all seven containers and Zenoh before returning from
start. The successful rerun includes this readiness check. The inherited guest
lock was verified to reject overlapping execution. A second invocation reconnected
to a focused `normal` run; SIGTERM retained its INCONCLUSIVE verdict and returned
failure after restoring the original source byte-for-byte and the healthy stack.
Evidence: `runs/autosd/20261007-165532-12190/`.

Before the driver was unified, the former Rust AutoSD backend passed `timeout`,
`invalid_quality`, `startup_without_source`, and `source_shutdown`; evidence is
retained in `runs/autosd/20261007-161308-8934/` and
`runs/autosd/20261007-161722-9338/`. Its cancelled run also remains in
`runs/autosd/20261007-162115-9647/`. Those results are historical checks of the
replaced backend. Current command-hook catalogue results are documented below.

### Both runtime catalogue checks (2026-10-07)

The shared command-hook runner passed all 16 AutoSD-supported implemented
scenarios in one `make campaigns` run:
`runs/autosd/20261007-174438-14014/`. Per-container network isolation remains
unsupported on the host-networked pod; explicit selection fails before touching
services. The external `source_dropout` case requires `campaign observe`.

Docker Compose ran all 17 implemented automatic cases twice. Every case passed
in the suites or focused rechecks, but the full runs were **not consistently
green**: real transient freshness faults appeared during nominal input. Those
FAIL recordings remain in `runs/compose-verification/20261007-174450/` and
`runs/compose-final-verification/20261007-175736-297504000-16246/`.
Focused passing rechecks are under `runs/compose-recheck/` and
`runs/compose-focused-final/`. No safety budget was loosened.

Testing also exposed two evaluator boundaries: samples recorded before the
Guardian process existed, and a WARNING event arriving just before its input
sample. Regression tests cover both. Interrupted executions now retain
INCONCLUSIVE reports even when the partial recording meets its expectations.
Compose runs use separate project/evidence names, and reusing an evidence
folder fails without overwriting its recording or exit status. Two concurrent
nominal runs passed under `runs/compose-concurrency/`.

A real two-second DFM/OpenSOVD pause resumed successfully on Compose, with failed
HTTP polls retained, under `runs/compose-outage-verification/`. Cancelling while
diagnostics were paused removed all campaign containers, retained the recording,
and returned exit status 1 under `runs/compose-cancellation/`. AutoSD lock
contention, reconnection, unsupported isolation, configuration mismatch, and
missing-image preflight were also checked. The latter failures occurred before
service teardown. All 40 campaign tests, lint, formatting, and shell syntax checks
passed.

The updated ARM64 runner also passed a real two-second DFM/OpenSOVD pause
(`runs/autosd/20261007-182041-18774/`). Cancelling a second run while both
containers were paused through `make down` restored the source and returned
INCONCLUSIVE with exit status 1 before stopping the deployment
(`runs/autosd/20261007-182401-19073/`). All 18 full-run source backups matched the
original replay byte-for-byte; the pod and Ankaios manifests were unchanged.
Updated evaluator reports are under `runs/autosd-reevaluated/`, with the original
reports retained.

### Full deployed CAN chain validation

The verification run used one deterministic CAN ASC recording containing all
five cases, keeping the same deployed Guardian running across them: CAN replay
gap, maximum temperature stuck, alive counter stuck, invalid CAN quality, and a
replay gap while DFM/OpenSOVD were paused through input recovery. Each case also
exercised thermal escalation and Critical → Warning → Monitoring recovery.
Checks covered cause IDs, diagnostic event/sample metadata, recovery, retained
fault history and Guardian container identity/start time.

The original verification harness is retained with the historical run evidence
as `execution.py`. Those five deployed-stack checks used the former Python
harness. The current `campaigns` target instead runs the shared Rust catalogue,
recordings and evaluator through its Ankaios command adapter. The historical run restored
the original source and restarted the workload afterward.

On 7 October 2026, all five cases passed on the Ankaios-managed AutoSD stack in
one Guardian session (`42f2109d-5262-45fe-9df4-5bf6df237ff7`). Event chains were:

| Case | Fault → degraded → mitigation request → recovered → OK |
|---|---|
| Freshness | 6 → 7 → 8 → 9 → 10 |
| Stuck signal | 15 → 16 → 17 → 18 → 19 |
| Stuck counter | 24 → 25 → 26 → 27 → 28 |
| Invalid quality | 33 → 34 → 35 → 36 → 37 |
| Diagnostics outage | 42 → 43 → 44 → 45 → 46 |

Local evidence: `diagnostics/reports/autosd-full-chain/20261007-100903-b400ae0e/`.
It includes the actual combined ASC, raw deployed service logs, OpenSOVD records,
image/container identities, executed harness snapshot and restored-source hash.
Earlier failed attempts are retained alongside it. The initial verification trace
used insufficient stuck-signal recovery movement; subsequent attempts exposed
startup dependency races and KUKSA's TIME_WAIT bind limitation. The passing run
used adequate recovery movement, dependency waits and a clean bindable port.
After restoration, Ankaios reported `Running(Ok)` again.

This historical single-VM run proves functional CAN-provider → KUKSA → publisher
→ uProtocol/Zenoh → deployed Guardian → DFM → OpenSOVD campaigns on AutoSD. Its
input uses the provider's virtual CAN replay bus. The separate two-peer bench
validates the openDuT-managed SocketCAN path with independent
source and destination captures. Guardian event
correlation comes from deployed logs and OpenSOVD metadata; this run did not
independently subscribe to the output event topic. It checked the five implemented
fault cases, not every ASC file or every challenge fault class, and did not
verify an actuator effect, reboot recovery or remote rerun consistency.

References: [Podman Kube YAML support](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html),
[Ankaios configuration objects](https://eclipse-ankaios.github.io/ankaios/main/usage/manifest/config-objects/),
[Ankaios installation](https://eclipse-ankaios.github.io/ankaios/main/usage/installation/).

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6** (`gpt-6`).
