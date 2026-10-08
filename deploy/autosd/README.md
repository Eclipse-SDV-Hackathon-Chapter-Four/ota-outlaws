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
agent. Application code, uProtocol contracts, scenario generation and evidence
collection are unchanged.

The default target is the existing automated `bench-beta` from sibling
`can-testbench/bench.json`, using that bench's SSH key and trusted known_hosts.
The original manual AutoSD VMs have separate disks.

## Deploy and run

Run Make **on your host computer**. It reaches the AutoSD guest over SSH.
Service configuration stays in `pod.yaml` and `ankaios.yaml`; the Makefile
uses native SSH, systemd and Ankaios commands for lifecycle and access.
`transfer.sh` prepares/transfers the deployment and installs missing binaries.

From the OTA Outlaws repository root:

```sh
make -C deploy/autosd up
make -C deploy/autosd status
make -C deploy/autosd logs SERVICE=guardian
make -C deploy/autosd connect
make -C deploy/autosd down
```

| Target | Purpose |
|---|---|
| `up` | Prepare existing images/configuration, stop the old workload, transfer updates, install missing Ankaios, start and wait for readiness. |
| `up SKIP_IMAGES=1` | The same workflow, reusing application images already loaded on AutoSD. |
| `down` | Remove the Ankaios workload and stop its units; retain images, storage and evidence. Safe to repeat. |
| `status` | Show Ankaios workload states and Podman container states. |
| `logs SERVICE=guardian` | Show the last 100 lines for the selected service; defaults to Guardian. |
| `connect` | Keep OpenSOVD/Zenoh SSH forwards open until Ctrl-C. |
| `help` | Show available targets; also the default when no target is given. |

For your already provisioned bench, update configuration with compatible guest images:

```sh
make -C deploy/autosd up SKIP_IMAGES=1
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
docker build --platform linux/arm64 -t ota-outlaws/guardian:dev -f guardian-service/Containerfile .
make -C deploy/autosd up
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
including heating to 55 degrees C. It does not yet consume the openDuT SocketCAN
tunnel. Mitigation events establish requests, not physical actuator effects.

## Connections for the generator and collector

Keep this running in a terminal:

```sh
make -C deploy/autosd connect
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
make -C deploy/autosd logs SERVICE=publisher
make -C deploy/autosd logs SERVICE=gateway
```

## Target overrides

Use `AUTOSD_SSH_HOST`, `AUTOSD_SSH_PORT`, `AUTOSD_SSH_KEY`,
`AUTOSD_KNOWN_HOSTS`, `AUTOSD_BENCH_DIR`, `AUTOSD_GUEST_DIR` and
`AUTOSD_AGENT_NAME` for another existing AutoSD guest. Host, port, key and guest
directory and agent are Make variables as well as environment overrides. Example:

```sh
make -C deploy/autosd status AUTOSD_SSH_PORT=2233
make -C deploy/autosd connect SOVD_PORT=27690 ZENOH_PORT=27447
```

The guest directory must be an absolute path containing only letters, digits,
underscores, dots, slashes and hyphens. Agent names accept letters, digits,
underscores and hyphens. Guest requirements: root Podman, Bash, systemd,
`ss`, curl, synchronized clocks and sufficient image storage/memory.
The target host key must already be trusted. Deployment no longer uses Python.

## Executed validation — 7 October 2026

### Ten-code catalog deployment

After main added the overtemperature DTCs, a configuration update with the old
Guardian image failed with `catalog must contain exactly the core fault codes`.
The deployment preflight now rejects mismatched image/catalog DTC lists before
stopping services or transferring configuration. The old guest image was rejected
with `SKIP_IMAGES=1`; a fresh ARM64 Guardian build from `166e783` then deployed
successfully through `make up`, exposing ten codes with all seven services running.

### Rebased application build check

Rebuilt Guardian and publisher ARM64 images after rebasing onto `263da38`
and deployed them with `make up`. The then-current eight-code catalog was exposed,
all seven services ran under Ankaios, and the CAN replay reached the publisher.
Formatting, lint and standard Cargo tests passed. Repeated `make down` and a
configuration-only `make up SKIP_IMAGES=1` were also checked.

### Earlier deployment checks

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

References: [Podman Kube YAML support](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html),
[Ankaios configuration objects](https://eclipse-ankaios.github.io/ankaios/main/usage/manifest/config-objects/),
[Ankaios installation](https://eclipse-ankaios.github.io/ankaios/main/usage/installation/).

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6** (`gpt-6`).
