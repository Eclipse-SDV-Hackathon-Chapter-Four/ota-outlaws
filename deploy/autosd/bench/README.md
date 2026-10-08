<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Two-peer AutoSD campaign bench

Peer A is the simulated battery. The shared Rust campaign driver runs on peer B:

```text
A: ASC frame scheduler → SocketCAN vcan0
   → EDGAR CAN routes → patched Cannelloni TCP → NetBird
   → EDGAR CAN routes → SocketCAN vcan0
B: KUKSA CAN provider → Data Broker → VSS Publisher
   → uProtocol/Zenoh → Guardian → DFM / OpenSOVD
```

Ankaios owns B's application pod through Podman. openDuT owns peer registration,
the cluster deployment, routes and tunnel. The Rust driver owns scenario choice,
action offsets, event/diagnostic taps, correlation IDs, safety evaluation and
reports. `can_io.py` only schedules/suppresses raw ASC frames on A, captures raw
CAN on both guests and checks delivery integrity. It contains no scenario catalog,
Guardian test expectations, diagnostic client or safety evaluator.

## Dashboard

The same two-peer controller can be launched from the dashboard. Start
`make dashboard-controller` on the native host, then select **AutoSD / OpenDUT**
in the dashboard's Runtime selector. See the [dashboard README](../../../components/dashboard/README.md)
for configuration, evidence paths, cancellation/recovery and current observation
limits. Campaign scenarios and verdict logic remain in the shared Rust CLI.

## Local openDuT backend (Docker Compose)

The bench needs a separate openDuT 0.10.2 backend: CARL, Keycloak and NetBird
management/signal/relay behind Traefik. If you have no shared backend, run one
locally with Docker Compose from `deploy/autosd`:

```sh
make backend-up        # first start takes ~5-10 min (Keycloak provisioning)
echo '127.0.0.1 opendut.local auth.opendut.local netbird.opendut.local netbird-api.opendut.local netbird-relay.opendut.local signal.opendut.local' | sudo tee -a /etc/hosts   # once
make remote-doctor
```

`backend/backend.sh` clones openDuT at tag `v0.10.2` (commit verified) into the
ignored `backend/.state/` and runs openDuT's own
`.ci/deploy/localenv/docker-compose.yml` with
[`backend/compose.override.yaml`](../backend/compose.override.yaml). The override:

- publishes only HTTPS port 443 (ports 80/8080/8081 stay free for the Compose
  stack and dashboard),
- runs the amd64-only CARL image under emulation on ARM64 hosts,
- installs CLEO for the container's own architecture
  ([`backend/cleo-entrypoint.sh`](../backend/cleo-entrypoint.sh)); upstream always
  downloads x86_64,
- disables the telemetry stack, which the bench does not use,
- replaces upstream's `/provision` host bind mount, which Docker Desktop on
  macOS cannot share.

Once CARL is healthy and CLEO completes an authenticated `list peers`,
`backend-up` writes `backend`, `ca`, `edgar_sha256`, `backend_hosts` and
`cleo_command` into `bench/local.toml`. It creates that file from the example if
needed. The EDGAR digest is computed from CARL's own aarch64 distribution. CLEO
runs through `docker exec -i opendut-cleo`, so `OPENDUT_CLEO_SETUP` is not
needed. Guests reach the backend via the QEMU gateway `10.0.2.2:443`. The host
needs the `/etc/hosts` line above because the controller downloads EDGAR from
`https://opendut.local`.

| Target | Purpose |
|---|---|
| `backend-up` | Provision secrets (once), start, wait for readiness, write settings |
| `backend-config` | Rewrite the settings into `CONFIG` for a running backend |
| `backend-status` / `backend-logs BACKEND_SERVICES="carl keycloak"` | Inspect |
| `backend-down` | Stop containers; secrets, peers and NetBird state are kept |
| `backend-destroy` | Remove containers, volumes and generated secrets/CA |

`backend/backend.sh compose ...` runs any `docker compose` command on this
deployment. Generated secrets live in `backend/.state/secrets/` and must not be
committed. Base image, `air` launcher and their hashes still come from your
artifact bundle; the backend does not provide them.

## Run

Use an ARM64 Linux host with KVM or an Apple Silicon Mac with HVF. Install
Python 3.11.8+, QEMU with ARM64 UEFI firmware, Docker with Linux ARM64 image build
support, Make, Bash, curl, jq, tar and OpenSSH. On Linux, install libguestfs
`virt-customize` and allow your user to access `/dev/kvm`. Windows and software
emulation on x86 are not validated for measured campaigns. Start with 16 GB host
RAM and enough disk for images and both overlays. Defaults allocate 1 GB / two
vCPUs to A and 3 GB / four vCPUs to B; `local.toml` can override guest sizing.
Avoid other heavy VM/build jobs during measured replay. Source scheduling misses
remain failed stimulus evidence; increasing safety budgets is not a workaround.

The bench has no dependency on a GitHub runner, release URL, username, fixed
host port, existing VM, or sibling checkout. Supply a reusable ARM64 AutoSD
qcow2 image, an executable AutoSD `air` launcher, the backend CA and the matching
CLEO 0.10.2 tool. These files can live anywhere. Keep reviewed SHA-256 values
with the artifact bundle; `remote-doctor` checks the base and launcher before boot.

Run the following commands from `deploy/autosd`. Copy `bench/local.example.toml`
to the ignored `bench/local.toml` and edit the paths, hashes and backend settings.
Paths resolve relative to the TOML file, so moving the config and its `assets/` directory together does not change them. CLI options
override config values. Diagnostics defaults to `deploy/diagnostics/image.env`.

```sh
cp bench/local.example.toml bench/local.toml
# Edit bench/local.toml with your artifact bundle and openDuT backend settings.
# Export OPENDUT_CLEO_SETUP from your backend administrator if needed.
make remote-doctor
# If the system Python is older than 3.11.8, add PYTHON=/path/to/python3.
make remote-run
# Select cases from the shared catalog:
make remote-run SCENARIOS="normal source_shutdown can_link_interruption"
# Or run every supported case:
make remote-run SCENARIOS=""
```

Each invocation chooses a unique state directory under `runs/opendut-<id>`;
the controller prints it. To choose a location, invoke
`python3 bench/controller.py run --config /path/to/local.toml
--state /path/to/new-run normal timeout`. The reusable base is never modified.
Every measured run builds Guardian, publisher and campaign images from this
checkout; Rust and Cargo are needed only inside their Docker builds.

`virt-customize` injects a fresh key into each overlay before boot. If it is
unavailable, install Expect and explicitly export `AUTOSD_BOOTSTRAP_PASSWORD`
for a development base with password login enabled. The value goes through
the environment, never the config or argv. The fallback replaces guest host
keys and machine IDs after boot. It does not read your personal SSH keys.

With no scenario arguments the controller uses the shared `--all` selection.
It records unsupported cases in `run.log`. All evaluated FAIL or INCONCLUSIVE
verdicts fail the bench job, including cases catalogued as planned. The existing
standalone CLI's planned-case exit policy remains unchanged.

`OPENDUT_BACKEND_HOSTS` optionally supplies one guest `/etc/hosts` line. For the
reference local backend this is `10.0.2.2` followed by its `opendut.local`, auth,
NetBird management/API/relay and signal names. Remote backends should use resolvable
names. Supply all required backend services; CARL alone cannot carry CAN.
The NetBird policy must permit B to reach A on SSH port 22 for stimulus control,
UDP port 123 for B to synchronize its clock to A, as well as the EDGAR-managed
CAN tunnel ports.
The `cleo_command` TOML array selects the tool. Native CLEO uses per-run XDG
configuration with `OPENDUT_CLEO_SETUP` supplied through the environment.
An already authenticated container can be selected explicitly, for example
`["docker", "exec", "-i", "your-cleo-container", "/opt/cleo/opendut-cleo"]`.
The container needs the matching backend CA and authentication configured.
There is no default developer container name.

Local production diagnostics can be built with the existing
`sh ../../deploy/diagnostics/build-images.sh /path/to/Doctor-Whodunit` and selected with
`--diagnostics local/opensovd-demo-fork:verified`. Otherwise the controller reads
`deploy/diagnostics/image.env` and retrieves that production image. Log in to its
registry with Docker if it is private. Application Guardian, publisher and campaign
images are built from the checked-out source and use unique job tags.

In `deploy/autosd`, the existing `make up` and `campaigns` targets remain the
single-VM ASC path by default. `AUTOSD_CAN_MODE=socketcan` explicitly selects the
DUT receiver; it invokes `--use-socketcan --canport vcan0 --no-val2dbc`. No measured
ASC is mounted as the provider input. Docker Compose campaign support is retained.

## Readiness, timing and evidence

The controller creates two independent qcow2 overlays, unique machine/SSH
identities, UUID peer/interface/device/cluster IDs and job-local ports/state.
Before applications are deployed it proves that fresh random CAN probe payloads
are absent with no deployment, arrive in both directions after deployment,
disappear after deleting that deployment and arrive again after redeployment.
A CARL online status or successful `cansend` is insufficient.

Before every measured replay, the adapter checks SocketCAN provider readiness,
publisher transport readiness, OpenSOVD availability and Guardian subscription.
Both independent CAN captures and the source replay process acknowledge readiness
before Guardian starts. The waiting source is released for a future epoch and every ASC timestamp is an absolute offset
from that epoch on A's monotonic clock. Existing action offsets are anchored to
that same epoch on B. A missed arming deadline is an execution failure, not a
request to stretch the trace. Before TLS provisioning, the controller initializes
each fresh guest's clock from the host and verifies the difference is within 1 s,
including SSH uncertainty;
the probes are retained as `a-boot-clock.json` and `b-boot-clock.json`. TLS certificate
verification remains enabled. A then uses Chrony's
[local reference mode](https://chrony-project.org/doc/4.6/chrony.conf.html#local)
at stratum 10, and B synchronizes to A before application deployment. This bounds
relative peer time without depending on public NTP availability or allowing an
upstream correction to step A's clock during a trace; it is not proof of absolute UTC accuracy. Before
any replay, the adapter bounds relative offset using current peer NTP delay,
dispersion, offset and remaining clock correction; a bound above 10 ms rejects
execution. Host SSH midpoint estimates are retained with their uncertainty for
diagnostics, and are not used as proof of synchronization. Original guest Chrony
configuration is backed up and restored during teardown; only job-owned overlays
are modified.

The source chooses its future epoch with a 150 ms arming lead after the SSH
command arrives on A, keeping outbound command latency out of that lead and
startup inside the existing 300 ms freshness requirement. B still rejects an
epoch that has elapsed on return; `source-arming.json` retains the command's
send and return times against that epoch. The source keeps its schedule when muted: frames are explicitly
marked suppressed, never sent in a catch-up burst. The source runs at guest FIFO priority 10 to reduce competing guest work; no host
scheduler privileges are needed. It sleeps until 90 ms before each deadline, then
uses a bounded active wait to reduce vCPU wake-up jitter. The AutoSD bench accepts
source scheduling lateness below 50 ms (half the nominal 100 ms CAN cycle), and
records both actual lateness and the limit in `can-path.json`. This is a bench
validation tolerance, not a HARA safety parameter; lateness at or above 50 ms
still rejects stimulus integrity. These integrity checks do not change the
100 ms reaction, freshness, diagnostic or recovery budgets in the shared catalog
and safety parameter file. The existing evaluator still measures safety onset at
the uProtocol input and correlated Guardian events on its monotonic recording
clock. Raw CAN timestamps additionally expose transport delay; they do not replace
that safety reference.

Each scenario retains the existing `manifest.json`, `recording.jsonl`, reports,
service logs and correlation IDs, plus:

- `source-can.jsonl`: scheduled sends, actual send times, intentional suppression,
  completion or explicitly requested termination, and campaign/scenario identity.
  Unexpected termination is a source failure.
- `source-observed-can.jsonl` and `destination-can.jsonl`: independent SocketCAN
  captures with kernel receive timestamps. Counts, payloads and ordering must
  agree outside an explicitly recorded CAN-link interruption. In-flight boundary
  frames may arrive after recovery; the checker still requires destination silence
  during the outage, actual loss, ordered delivery and no missing outside frames.
  `interruption_boundary_deliveries` records these accepted frames explicitly.
- `can-path.json` and `failure-layer.json`: source integrity, transport integrity
  and downstream/Guardian attribution. Verified CAN with a failed safety check
  remains a failed safety check. Missing source completion, missing bus evidence
  or bad stimulus timing cannot earn a Guardian pass.
- Runtime image/container identities, actual provider process arguments, peer
  descriptors, clock checks, CAN link state, transition timestamps and EDGAR logs
  from both peers. `source-runtime.log` also retains replay/capture service errors.

`services.log`, `error.txt` and `restoration-error.txt` retain errors. Reports are
judged even after execution failure. The host retrieves evidence before destroying
the overlays; evidence stays in the local run directory even when the command
returns a failure status.

## Scenario applicability

| Scenario | Two-peer behavior |
|---|---|
| Existing ASC scenarios | Same trace and evaluator, replayed on A and decoded on B |
| `startup_without_source` | Decoder and Guardian ready; A emits no BMS frames |
| `source_shutdown` | Stop A's replay; B's decoder and the openDuT link stay up |
| `source_dropout_replay` | Mute A for two seconds and resume its absolute schedule; shared expectations from the external source-dropout case |
| `can_link_interruption` | Lower B's openDuT-managed `vcan0` temporarily; A continues emitting and its independent capture must prove it |
| `transport_dropout` | Unsupported here: isolates Guardian after the uProtocol tap; B's host-network pod has no independent Guardian network |
| `source_dropout` | Existing external hardware case remains observe-only |
| Planned catalog scenarios | Run and retain their verdicts; a planned requirement is not silently declared implemented |

Guardian watchdog scenarios (`guardian_crash`, `guardian_hang`) are unsupported
by the current seven-service AutoSD pod: it contains no watchdog workload. Use
the focused default cases until that workload is integrated. Explicit unsupported
selections fail before scenario stimulus or application changes; the shared catalog is preserved.

The endpoint interruption is an explicit fault intervention on an existing
openDuT connection, not a second transport implementation. Its interface state is
restored even on interruption. The openDuT cluster stays deployed while the fault
is active. Source muting and destination link state are independently controlled.
No openDuT executor is declared: cluster deployment establishes the network only;
the shared campaign is launched through guest systemd after readiness. This avoids
starting measured tests as a side effect of deploying a cluster in 0.10.2.

## Version and compatibility provenance

Infrastructure was reused from `../can-testbench`: the base-image manifest,
vcan/CAN-gateway modules and `max_hops=2`, guest provisioning, Cannelloni TCP wrapper
and the exact 1.1.0 TCP receive/reconnect patch. The upstream GPL notice is preserved
beside the patch. The reference smoke container and diagnostic smoke harness were
not imported.

`assets.lock.json` pins upstream Cannelloni 1.1.0 with SHA-256. EDGAR/CLEO are
retrieved from the explicitly configured CARL 0.10.2 backend, and their distribution
digests are supplied by that backend's administrator. The reference EDGAR hash in
the lock is backend-specific; it is not assumed to match another server's packaged
configuration. Provisioning checks the chosen distribution hash before extraction.
CLEO must report version 0.10.2.

The implemented commands were checked against the local pinned CLI's help and
the `v0.10.2` source: `apply`, `generate-setup-string`, `await peer-online`,
`create/delete cluster-deployment`, `await cluster-peers-online`,
`delete cluster-descriptor`, `delete peer`, `setup --persistent user`, and
EDGAR `setup managed --no-confirm`. The local CLI reports release 0.10.2,
commit `eb8d15df6a65719db4b77c4ef660695ce238cf03`.

## Reusable base and backend

Use an explicitly configured openDuT 0.10.2 CARL/auth/NetBird backend reachable
from the host and both guests. It can run locally or on another machine; a sibling
checkout or a developer's backend is not required. For native CLEO, supply `OPENDUT_CLEO_SETUP` through the shell environment;
the bench stores its configuration only inside the run directory. An explicitly
configured container transport may instead use its own existing configuration. Its administrator supplies the CA and reviewed
EDGAR archive digest. Peer and cluster UUIDs are isolated between concurrent runs;
cleanup deletes only resources saved in the run's `owned.json`.

The base is a reusable **ARM64 standalone qcow2** image. Distribute an immutable,
versioned bundle containing `peer-base.qcow2`, `air`, `provenance.json`,
`SHA256SUMS`, the manifest, RPM inventory and build log. A filesystem directory or
HTTPS artifact store is sufficient. Put the files anywhere and configure their
paths and reviewed SHA-256 digests in `local.toml`. The bench verifies those hashes
before creating independent writable overlays; it does not rebuild the OS per run.
No hosted bundle or download URL has been published by this change.

`base-artifact.sh` is the separate maintainer recipe. Supply `AIB_WRAPPER`, pinned
`AIB_IMAGE=image@sha256:...`, pinned `AIR`, `BASE_VERSION=autosd-can-base-v1` and a
new absolute `BASE_OUT`. It uses the inspected AutoSD wrapper and this manifest,
records builder/repository/manifest/launcher/base/RPM provenance, and removes
runtime credentials and identities before distribution. Review the resulting
bundle and share its trusted hashes separately. OS repositories are moving inputs;
recording exact installed RPMs documents the image without claiming byte-for-byte
OS reproducibility.

The production diagnostic image from `deploy/diagnostics/image.env` must be available to
Docker. A locally built production image may be selected explicitly, provided its
revision label matches the pinned production revision. Application images are built
from the current checkout. No diagnostic smoke harness or smoke job is introduced.

## Cleanup and recovery

The Rust runner restores each scenario after success, failure, SIGINT or SIGTERM.
The bench adapter stops stimulus/captures, restores B's endpoint, collects raw CAN
and delegates application restoration to the existing Ankaios adapter. Host
cleanup cancels and waits for the guest campaign, restores stimulus/link state,
collects evidence, undeploys/deletes only this cluster and its peers, stops both
owned VM process groups and removes overlays and private credentials. Timeouts
bound guest campaigns and host operations. The controller runs teardown in its
finalizer after success, failure, SIGINT or SIGTERM.

If backend access fails during teardown, `cleanup.json` records it and the job
fails. Use the retained `owned.json` UUIDs to retry removal on that same backend;
do not delete unrelated resources. A host power loss cannot execute cleanup:
the shared-backend operator must sweep orphaned `ota-<job>-*` resources against
recorded job identities. Credential input and private keys are excluded from the
retained evidence.

## Verification

Executed on an Apple Silicon Mac with HVF, the configured local openDuT 0.10.2
backend and the reused AutoSD base:

- `make remote-doctor`: artifact hashes, native architecture, Docker access, standalone
  qcow2, actual QEMU/HVF initialization and CLEO 0.10.2 passed.
- 70 shared campaign evaluator tests, 22 bench tests, campaign Clippy, Rust format
  and shell syntax checks passed. The ARM64 shared campaign image built locally.
- Multiple fresh overlay pairs proved bidirectional CAN probes cross only while
  the openDuT cluster is deployed; undeployment blocks them and redeployment
  restores them. Process inspection confirmed B's provider receives SocketCAN.
- In `runs/opendut-b3d5746cf9f3`, `normal`, `timeout`, `invalid_quality` and
  `source_dropout_replay` passed the shared evaluator and CAN integrity checks.
  Link interruption failed an overly strict boundary-delivery check; its raw
  evidence exposed an in-flight frame arriving just after recovery. The checker
  now accepts that frame while still requiring real loss and outage silence.
- In `runs/opendut-77faaedd7bea`, normal, timeout, source dropout and CAN-link
  interruption passed. The link case emitted 200 frames on A and received the
  expected 179 on B, with all four shared safety checks passing and maximum source
  lateness about 1.2 ms. Invalid quality remained INCONCLUSIVE because stimulus
  lateness exceeded the then-configured 20 ms source-integrity limit.
- A focused `source_shutdown` recheck on the latter pair passed all three shared
  checks and proved deliberate source termination independently of link state.
  The final invalid-quality recheck delivered all 100 frames in order but recorded
  a 111 ms source scheduling miss; it remains INCONCLUSIVE. Two other VMs were
  running on this 16 GB host. No complete five-case PASS run with the final helper
  revisions is claimed.

Real retries exposed and fixed missing login environment in the systemd campaign,
stale journal identity after development-image cloning, buffered CAN-provider
logs and a readiness pipeline failure. Source prearming and guest pacing reduce
startup/wake-up jitter; hypervisor contention can still invalidate stimulus.
Failed and inconclusive reports, including a production DFM receiver error during
an unsuccessful readiness attempt, remain alongside successful reports. Completed
bench runs retained evidence and reported successful owned-resource cleanup.

Linux/KVM has not been executed here. These checks do not claim every supported
catalog scenario passes, that a base bundle is hosted, or that measured timing is
reliable on a host concurrently running unrelated VM/build workloads. Use a quiet
host and inspect source/clock integrity rather than relaxing safety budgets.

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6** (`gpt-6`), and extended with **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
