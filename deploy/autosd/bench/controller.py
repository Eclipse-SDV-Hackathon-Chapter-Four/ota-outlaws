#!/usr/bin/env python3
# Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0 which is available at
# https://www.eclipse.org/legal/epl-2.0
#
# SPDX-License-Identifier: EPL-2.0
# AI-assisted: Codex / GPT-6 (gpt-6)
"""Own fresh VMs and openDuT resources; run the existing Rust campaign on B.

Shared backend credentials/CA are explicitly supplied. No scenario verdict is
implemented here. All job resources are recorded before they can be allocated.
"""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
import uuid
if sys.version_info < (3, 11, 8):
    raise SystemExit("Python 3.11.8+ required; select it with make PYTHON=/path/to/python3")
import tomllib

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def local_config(path):
    """Resolve portable TOML paths relative to the config, never the caller's cwd."""
    if not path:
        return {}
    path = Path(path).expanduser().resolve()
    settings = tomllib.loads(path.read_text())
    allowed = {"base", "air", "ca", "backend", "backend_hosts", "edgar_sha256",
               "base_sha256", "air_sha256", "cleo_command", "diagnostics",
               "source_memory", "dut_memory", "source_cpus", "dut_cpus"}
    unknown = settings.keys() - allowed
    if unknown:
        raise ValueError("Unknown bench settings: " + ", ".join(sorted(unknown)))
    for key, value in settings.items():
        if key in ("source_cpus", "dut_cpus"):
            if type(value) is not int or not 1 <= value <= 8:
                raise ValueError(f"{key} must be an integer between 1 and 8")
        elif key != "cleo_command" and not isinstance(value, str):
            raise ValueError(f"{key} must be a string")
    for key in ("base", "air", "ca"):
        if key in settings:
            value = Path(settings[key]).expanduser()
            settings[key] = str((path.parent / value).resolve())
    if "cleo_command" in settings:
        command = settings["cleo_command"]
        if not isinstance(command, list) or not command or not all(isinstance(s, str) for s in command):
            raise ValueError("cleo_command must be a non-empty array of strings")
        if "/" in command[0]:
            command[0] = str((path.parent / Path(command[0]).expanduser()).resolve())
        settings["cleo_command"] = json.dumps(command)
    return settings


def production_diagnostics():
    return next(line.split("=", 1)[1].strip()
                for line in (ROOT / "diagnostics/image.env").read_text().splitlines()
                if line.startswith("DIAGNOSTICS_IMAGE="))


def preflight(args):
    """Fail before allocating resources; no dependency on GitHub or sibling repos."""
    if sys.version_info < (3, 11, 8):
        raise RuntimeError("Python 3.11.8+ is required")
    if platform.machine().lower() not in ("arm64", "aarch64"):
        raise RuntimeError("Measured ARM64 campaigns require an ARM64 host with KVM or HVF")
    if sys.platform not in ("linux", "darwin"):
        raise RuntimeError("Supported hosts are ARM64 Linux and Apple Silicon macOS")
    tools = ["docker", "qemu-img", "qemu-system-aarch64", "ssh", "scp", "ssh-keygen", "make", "bash", "curl", "jq", "tar"]
    if shutil.which("virt-customize") is None:
        if not os.environ.get("AUTOSD_BOOTSTRAP_PASSWORD"):
            raise RuntimeError("Install virt-customize or explicitly set AUTOSD_BOOTSTRAP_PASSWORD for a development base")
        tools.append("expect")
    missing = [tool for tool in tools if not shutil.which(tool)]
    if missing:
        raise RuntimeError("Missing host tools: " + ", ".join(missing))
    if sys.platform == "linux" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
        raise RuntimeError("ARM64 KVM must be accessible at /dev/kvm")
    for name in ("base", "air", "ca"):
        value = getattr(args, name)
        if not value or not Path(value).is_file():
            raise RuntimeError(f"Configure an existing {name} file in --config or --{name}")
    if not os.access(args.air, os.X_OK):
        raise RuntimeError("The configured air launcher must be executable")
    for key, path in (("base_sha256", args.base), ("air_sha256", args.air)):
        expected = getattr(args, key)
        if not expected or len(expected) != 64:
            raise RuntimeError(f"Configure {key} from the reviewed artifact bundle")
        with open(path, "rb") as file:
            actual = hashlib.file_digest(file, "sha256").hexdigest()
        if actual != expected:
            raise RuntimeError(f"{key} mismatch")
    if not args.backend or not args.backend.startswith("https://") or not args.edgar_sha256:
        raise RuntimeError("Configure an HTTPS openDuT backend and its EDGAR SHA-256")
    command = json.loads(args.cleo_command)
    if not command or not shutil.which(command[0]):
        raise RuntimeError("Configured CLEO command is unavailable")
    run(["docker", "info"], timeout=20)
    run(["qemu-system-aarch64", "-machine", "virt", "-accel",
         "hvf" if sys.platform == "darwin" else "kvm", "-cpu", "host",
         "-nodefaults", "-display", "none", "-S", "-monitor", "stdio"],
        data="quit\n", timeout=20)
    info = json.loads(run(["qemu-img", "info", "--output=json", args.base]).stdout)
    if info.get("format") != "qcow2" or info.get("backing-filename"):
        raise RuntimeError("Base must be a standalone qcow2 image without a backing disk")
    version = run([*command, "--version"], timeout=20).stdout
    if "Version:       0.10.2" not in version:
        raise RuntimeError("Only verified openDuT 0.10.2 supported")
    print("Local prerequisites and artifact hashes verified.", flush=True)


def run(args, *, data=None, timeout=120, check=True, env=None):
    result = subprocess.run(
        [str(a) for a in args],
        input=data,
        text=True,
        capture_output=True,
        timeout=timeout,
        env=env,
    )
    if check and result.returncode:
        # Never include stdin: it can contain EDGAR setup credentials.
        raise RuntimeError(f"{args[0]} failed ({result.returncode}): {result.stderr}")
    return result


class Bench:
    def __init__(self, args):
        self.args = args
        self.state = Path(args.state).resolve()
        self.state.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(self.state, 0o700)
        self.owned = self.state / "owned.json"
        if self.owned.exists():
            self.config = json.loads(self.owned.read_text())
        else:
            job = uuid.uuid4().hex[:12]
            self.config = dict(job=job, cluster_id=str(uuid.uuid4()), peers=[], vms=[],
                               inputs=dict(base_sha256=args.base_sha256,
                                           air_sha256=args.air_sha256,
                                           edgar_sha256=args.edgar_sha256,
                                           backend=args.backend,
                                           source_memory=args.source_memory,
                                           dut_memory=args.dut_memory,
                                           source_cpus=args.source_cpus,
                                           dut_cpus=args.dut_cpus))
            for role in ["a", "b"]:
                with socket.socket() as port:
                    port.bind(("127.0.0.1", 0))
                    number = port.getsockname()[1]
                self.config["peers"].append(
                    dict(
                        role=role,
                        name=f"ota-{job}-{role}",
                        ssh_port=number,
                        id=str(uuid.uuid4()),
                        interface_id=str(uuid.uuid4()),
                        device_id=str(uuid.uuid4()),
                    )
                )
            self.save()
        self.cleo_cmd = json.loads(args.cleo_command)
        self.env = dict(os.environ)
        # CLEO reads configuration beneath XDG_CONFIG_HOME; never change HOME.
        self.env["XDG_CONFIG_HOME"] = str(self.state / "cleo-config")
        self.env["XDG_DATA_HOME"] = str(self.state / "cleo-data")
        self.env["SSL_CERT_FILE"] = str(Path(args.ca).resolve())

    def save(self):
        self.owned.write_text(json.dumps(self.config, indent=2) + "\n")

    def cleo(self, *args, **kwargs):
        return run([*self.cleo_cmd, *args], env=self.env, **kwargs).stdout

    def ssh_args(self, peer):
        return [
            "ssh",
            "-p",
            str(peer["ssh_port"]),
            "-i",
            self.state / "key",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            f"UserKnownHostsFile={self.state}/known_hosts",
            "root@127.0.0.1",
        ]

    def ssh(self, peer, command, **kwargs):
        return run([*self.ssh_args(peer), command], **kwargs)

    def copy(self, peer, paths, destination):
        run(
            [
                "scp",
                "-P",
                str(peer["ssh_port"]),
                *self.ssh_args(peer)[3:-1],
                *paths,
                f"root@127.0.0.1:{destination}",
            ],
            timeout=600,
        )

    def apply(self, descriptor):
        # Local container transport is explicit; native CLEO reads host files.
        if self.cleo_cmd[:2] == ["docker", "exec"]:
            container = (
                self.cleo_cmd[3] if self.cleo_cmd[2] == "-i" else self.cleo_cmd[2]
            )
            destination = f'/tmp/ota-{self.config["job"]}-{descriptor.name}'
            run(["docker", "cp", descriptor, f"{container}:{destination}"])
            try:
                self.cleo("apply", destination)
            finally:
                run(["docker", "exec", container, "rm", "-f", destination], check=False)
        else:
            self.cleo("apply", str(descriptor))

    def descriptors(self):
        for peer in self.config["peers"]:
            # JSON is valid YAML and avoids requiring a host YAML library.
            descriptor = dict(
                kind="PeerDescriptor",
                version="v1",
                metadata=dict(id=peer["id"], name=peer["name"]),
                spec=dict(
                    location="AutoSD-QEMU",
                    network=dict(
                        interfaces=[
                            dict(id=peer["interface_id"], name="vcan0", kind="vcan")
                        ]
                    ),
                    topology=dict(
                        devices=[
                            {
                                "id": peer["device_id"],
                                "name": peer["name"] + "-can",
                                "description": "Job-owned AutoSD CAN endpoint",
                                "interface-id": peer["interface_id"],
                                "tags": ["ota-campaign"],
                            }
                        ]
                    ),
                ),
            )
            path = self.state / f'{peer["role"]}.yaml'
            path.write_text(json.dumps(descriptor))
            self.apply(path)
        descriptor = dict(
            kind="ClusterDescriptor",
            version="v1",
            metadata=dict(
                id=self.config["cluster_id"], name="ota-" + self.config["job"]
            ),
            spec={
                "leader-id": self.config["peers"][0]["id"],
                "devices": [p["device_id"] for p in self.config["peers"]],
            },
        )
        path = self.state / "cluster.yaml"
        path.write_text(json.dumps(descriptor))
        self.apply(path)

    def assets(self):
        assets = self.state / "assets"
        assets.mkdir(exist_ok=True)
        lock = json.loads((HERE / "assets.lock.json").read_text())
        release = assets / "cannelloni-1.1.0-arm64.tar.gz"
        run(["curl", "-fL", "--retry", "3", lock["cannelloni"]["url"], "-o", release])
        if (
            hashlib.sha256(release.read_bytes()).hexdigest()
            != lock["cannelloni"]["sha256"]
        ):
            raise RuntimeError("Cannelloni checksum mismatch")
        edgar = assets / "edgar-0.10.2-arm64.tar.gz"
        run(
            [
                "curl",
                "-fL",
                "--cacert",
                self.args.ca,
                self.args.backend.rstrip("/")
                + "/api/edgar/aarch64-unknown-linux-gnu/download",
                "-o",
                edgar,
            ]
        )
        if hashlib.sha256(edgar.read_bytes()).hexdigest() != self.args.edgar_sha256:
            raise RuntimeError(
                "EDGAR checksum differs from configured backend distribution"
            )
        shutil.copyfile(self.args.ca, assets / "opendut-ca.pem")
        # Reuse precisely the reference bench compatibility patch/build recipe.
        context = self.state / "cannelloni-build"
        (context / "source").mkdir(parents=True, exist_ok=True)
        import tarfile

        with tarfile.open(release) as outer:
            nested = outer.extractfile("cannelloni/sources/cannelloni-1.1.0.tar.gz")
            with tarfile.open(fileobj=nested, mode="r:gz") as source:
                for member in source.getmembers():
                    parts = member.name.split("/", 1)
                    if len(parts) == 2 and parts[1]:
                        member.name = parts[1]
                        source.extract(member, context / "source", filter="data")
        shutil.copyfile(
            HERE / "patches/cannelloni-1.1.0-tcp-read.patch", context / "tcp-read.patch"
        )
        tag = "ota-cannelloni:" + self.config["job"]
        run(
            [
                "docker",
                "build",
                "--platform",
                "linux/arm64",
                "-t",
                tag,
                "-f",
                HERE / "build/Dockerfile.cannelloni",
                context,
            ],
            timeout=900,
        )
        self.export(tag, "/build/cannelloni", assets / "cannelloni-tcp-fixed")
        return assets

    def export(self, tag, path, destination):
        container = run(
            ["docker", "create", "--platform", "linux/arm64", tag, "/campaign"]
        ).stdout.strip()
        try:
            run(["docker", "cp", f"{container}:{path}", destination])
        finally:
            run(["docker", "rm", "-f", container], check=False)

    def boot(self):
        # Serialize only host port assignment/boot; campaigns remain concurrent.
        import fcntl
        import tempfile

        lock_path = Path(tempfile.gettempdir()) / f"ota-autosd-ports-{os.getuid()}.lock"
        with open(lock_path, "a") as lock:
            os.chmod(lock_path, 0o600)
            fcntl.flock(lock, fcntl.LOCK_EX)
            self._boot_locked()

    def _boot_locked(self):
        if self.config["vms"]:
            raise RuntimeError("State already owns VMs; clean it before a new run")
        run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", self.state / "key"])
        for peer in self.config["peers"]:
            with socket.socket() as port:
                port.bind(("127.0.0.1", 0))
                peer["ssh_port"] = port.getsockname()[1]
            self.save()
            disk = self.state / f'{peer["role"]}.qcow2'
            run(
                [
                    "qemu-img",
                    "create",
                    "-f",
                    "qcow2",
                    "-F",
                    "qcow2",
                    "-b",
                    Path(self.args.base).resolve(),
                    disk,
                ]
            )
            # Inject this job's public key and unique identity before first boot.
            # virt-customize is offline; no developer SSH key/password is needed.
            commands = [
                "--run-command",
                "rm -f /etc/ssh/ssh_host_*; : > /etc/machine-id",
                "--ssh-inject",
                f"root:file:{self.state}/key.pub",
            ]
            offline = shutil.which("virt-customize") is not None
            if offline:
                run(["virt-customize", "-a", disk, *commands], timeout=180)

            log = open(self.state / f'{peer["role"]}-console.log', "w")
            process = subprocess.Popen(
                [
                    self.args.air,
                    "--arch",
                    "aarch64",
                    "--nographics",
                    "--memory",
                    self.args.source_memory if peer["role"] == "a" else self.args.dut_memory,
                    "--ssh-port",
                    str(peer["ssh_port"]),
                    disk,
                    "-smp",
                    str(self.args.source_cpus if peer["role"] == "a" else self.args.dut_cpus),
                ],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            log.close()
            self.config["vms"].append(dict(pid=process.pid, disk=str(disk)))
            self.save()
            # Trust first keys only for our newly created VM on an allocated port.
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline:
                args = self.ssh_args(peer)
                args[args.index("StrictHostKeyChecking=yes")] = (
                    "StrictHostKeyChecking=accept-new"
                )
                if not offline:
                    run(
                        [
                            "expect",
                            HERE / "scripts/bootstrap-ssh.exp",
                            str(peer["ssh_port"]),
                            str(self.state / "known_hosts"),
                            (self.state / "key.pub").read_text().strip(),
                        ],
                        check=False,
                        timeout=20,
                    )
                if run([*args, "true"], check=False, timeout=5).returncode == 0:
                    break
                if process.poll() is not None:
                    raise RuntimeError("QEMU exited before SSH")
                time.sleep(2)
            else:
                raise RuntimeError("Fresh guest SSH did not become ready")
            if not offline:
                # Explicit password bootstrap for a development base.
                # Fresh overlays only: regenerate base-image identities/keys.
                self.ssh(
                    peer,
                    "rm -f /etc/machine-id; systemd-machine-id-setup; "
                    "systemctl restart systemd-journald; "
                    "rm -f /etc/ssh/ssh_host_*; ssh-keygen -A; systemctl restart sshd",
                )
                run(
                    [
                        "ssh-keygen",
                        "-R",
                        f'[127.0.0.1]:{peer["ssh_port"]}',
                        "-f",
                        self.state / "known_hosts",
                    ]
                )
                run([*args, "true"])

    def provision(self, assets):
        for peer in self.config["peers"]:
            self.ssh(peer, "mkdir -p /tmp/can-testbench-provision")
            self.copy(
                peer,
                [
                    *(
                        assets / p
                        for p in [
                            "opendut-ca.pem",
                            "cannelloni-1.1.0-arm64.tar.gz",
                            "edgar-0.10.2-arm64.tar.gz",
                            "cannelloni-tcp-fixed",
                        ]
                    ),
                    HERE / "files/cannelloni-tcp",
                    HERE / "scripts/provision-guest.sh",
                ],
                "/tmp/can-testbench-provision/",
            )
            command = (
                "OPENDUT_BACKEND_URL="
                + shlex.quote(self.args.backend)
                + " bash /tmp/can-testbench-provision/provision-guest.sh "
                + shlex.quote(peer["name"])
                + " "
                + shlex.quote(self.args.backend_hosts)
            )
            self.ssh(peer, command, timeout=180)
            setup = self.cleo("generate-setup-string", peer["id"]).strip()
            self.ssh(
                peer,
                "cd /opt/can-testbench/edgar && ./opendut-edgar setup managed --no-confirm --log-file=-",
                data=setup + "\n",
                timeout=180,
            )
            self.ssh(peer, "systemctl daemon-reload; systemctl start opendut-edgar")
        self.cleo(
            "await",
            "peer-online",
            "--timeout",
            "120",
            *[p["id"] for p in self.config["peers"]],
            timeout=130,
        )

    def probe(self, sender, receiver, expected):
        payload = os.urandom(8).hex().upper()
        listener = subprocess.Popen(
            [*self.ssh_args(receiver), "candump -n 1 -T 1500 vcan0,6AA:7FF"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            time.sleep(0.2)
            self.ssh(sender, f"cansend vcan0 6AA#{payload}")
            output, err = listener.communicate(timeout=5)
        finally:
            if listener.poll() is None:
                listener.kill()
                listener.wait()
        observed = payload in output.replace(" ", "").upper()
        with open(self.state / "connectivity.jsonl", "a") as log:
            log.write(
                json.dumps(
                    dict(
                        sender=sender["id"],
                        receiver=receiver["id"],
                        payload=payload,
                        expected=expected,
                        observed=observed,
                        wall_ns=time.time_ns(),
                        capture=output,
                    )
                )
                + "\n"
            )
        if observed != expected:
            raise RuntimeError("CAN dependency/traversal probe failed")

    def connectivity(self):
        a, b = self.config["peers"]
        # Prove absence without deployment, then fresh delivery in both directions.
        self.probe(a, b, False)
        self.cleo("create", "cluster-deployment", self.config["cluster_id"])
        self.cleo(
            "await",
            "cluster-peers-online",
            "--timeout",
            "60",
            self.config["cluster_id"],
            timeout=70,
        )
        for peer in (a, b):
            self.ssh(peer, "chronyc waitsync 20 0.01 0 1", timeout=30)
        # Retry readiness probes; measured campaigns have no retry/relaxed budget.
        for attempt in range(10):
            try:
                self.probe(a, b, True)
                self.probe(b, a, True)
                break
            except RuntimeError:
                if attempt == 9:
                    raise
                time.sleep(1)
        for peer in (a, b):
            count = int(
                self.ssh(
                    peer, "pgrep -fc '^/usr/local/bin/cannelloni.real ' || true"
                ).stdout.strip()
            )
            if count != 1:
                raise RuntimeError(f"Expected one managed tunnel, found {count}")
        # Removing the actual openDuT deployment must remove connectivity.
        self.cleo("delete", "cluster-deployment", self.config["cluster_id"])
        time.sleep(2)
        self.probe(a, b, False)
        self.cleo("create", "cluster-deployment", self.config["cluster_id"])
        self.cleo(
            "await",
            "cluster-peers-online",
            "--timeout",
            "60",
            self.config["cluster_id"],
            timeout=70,
        )
        for attempt in range(10):
            try:
                self.probe(a, b, True)
                break
            except RuntimeError:
                if attempt == 9:
                    raise
                time.sleep(1)
        identities = []
        for peer in (a, b):
            identity = json.loads(
                self.ssh(
                    peer,
                    "python3 - <<'PY'\n"
                    "import json, subprocess\nfrom pathlib import Path\n"
                    "print(json.dumps(dict(machine_id=Path('/etc/machine-id').read_text().strip(), "
                    "hostname=subprocess.check_output(['hostname'],text=True).strip(), "
                    "kernel=subprocess.check_output(['uname','-r'],text=True).strip(), "
                    "architecture=subprocess.check_output(['uname','-m'],text=True).strip())))\nPY",
                ).stdout
            )
            identity["peer_id"] = peer["id"]
            identities.append(identity)
        if identities[0]["machine_id"] == identities[1]["machine_id"]:
            raise RuntimeError("Cloned guest machine identities were not regenerated")
        self.config["runtime_identities"] = identities
        self.config["backend_version"] = self.cleo("--version")
        self.save()

    def synchronize(self):
        import ipaddress
        a, b = self.config["peers"]
        addresses = [self.ssh(peer, "ip -4 -o addr show wt0 | awk '{print $4}' | cut -d/ -f1").stdout.strip() for peer in (a, b)]
        for address in addresses:
            ipaddress.IPv4Address(address)
        self.config["clock_configured"] = True
        self.save()
        for peer in (a, b):
            self.ssh(peer, "cp /etc/chrony.conf /opt/ota-bench-clock-original.conf")
        self.ssh(a, "cat >> /etc/chrony.conf; systemctl restart chronyd", data="\nallow " + addresses[1] + "/32\n")
        self.ssh(b, "cat > /etc/chrony.conf; systemctl restart chronyd", data=
                 "server " + addresses[0] + " iburst minpoll 0 maxpoll 0\n"
                 "driftfile /var/lib/chrony/drift\nmakestep 0.001 3\nrtcsync\n")
        self.ssh(b, "mkdir -p /opt/ota-bench-clock")
        self.copy(b, [HERE / "clock_io.py"], "/opt/ota-bench-clock/")
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            result = self.ssh(b, "python3 /opt/ota-bench-clock/clock_io.py --source " + addresses[0], check=False)
            (self.state / "clock-synchronization.json").write_text(result.stdout + result.stderr)
            if result.returncode == 0:
                self.config["clock_synchronization"] = json.loads(result.stdout)
                self.save()
                return
            time.sleep(1)
        raise RuntimeError("Guest peer-clock synchronization did not meet the 10 ms bound")

    def applications(self):
        job = self.config["job"]
        a, b = self.config["peers"]
        if not self.args.skip_build:
            for service, path in [
                ("guardian", "guardian-service"),
                ("vss-publisher", "vss-publisher"),
                ("campaign", "campaign"),
            ]:
                run(
                    [
                        "docker",
                        "build",
                        "--platform",
                        "linux/arm64",
                        "-t",
                        f"ota-outlaws/{service}:{job}",
                        "-f",
                        ROOT / path / "Containerfile",
                        ROOT,
                    ],
                    timeout=1200,
                )
        else:
            # Local verification may use known built images when explicitly requested.
            for service in ["guardian", "vss-publisher", "campaign"]:
                run(
                    [
                        "docker",
                        "tag",
                        (
                            f"ota-outlaws/{service}:autosd"
                            if service == "campaign"
                            else f"ota-outlaws/{service}:dev"
                        ),
                        f"ota-outlaws/{service}:{job}",
                    ]
                )
        for image in [
            "ghcr.io/eclipse-kuksa/kuksa-can-provider/can-provider:main",
            "ghcr.io/eclipse-kuksa/kuksa-databroker:main",
            "eclipse/zenoh:1.10.1",
            self.args.diagnostics,
        ]:
            if run(["docker", "image", "inspect", image], check=False).returncode:
                run(["docker", "pull", "--platform", "linux/arm64", image], timeout=600)
        production_pin = next(
            line.split("=", 1)[1].strip()
            for line in (ROOT / "diagnostics/image.env").read_text().splitlines()
            if line.startswith("PINNED_SOURCE_REV=")
        )
        diagnostics_info = json.loads(
            run(["docker", "image", "inspect", self.args.diagnostics]).stdout
        )[0]
        if (
            diagnostics_info["Config"]
            .get("Labels", {})
            .get("org.opencontainers.image.revision")
            != production_pin
        ):
            raise RuntimeError(
                "Diagnostics image does not match the production source pin"
            )
        env = {
            **os.environ,
            "AUTOSD_SSH_PORT": str(b["ssh_port"]),
            "AUTOSD_SSH_KEY": str(self.state / "key"),
            "AUTOSD_KNOWN_HOSTS": str(self.state / "known_hosts"),
            "AUTOSD_AGENT_NAME": b["name"],
            "AUTOSD_CAN_MODE": "socketcan",
            "AUTOSD_IMAGE_TAG": job,
            "DIAGNOSTICS_IMAGE": self.args.diagnostics,
        }
        result = run(
            ["make", "-C", ROOT / "deploy/autosd", "up"], env=env, timeout=1200
        )
        (self.state / "deployment.log").write_text(result.stdout + result.stderr)
        tools = self.state / "input"
        (tools / "campaign").mkdir(parents=True)
        shutil.copytree(ROOT / "campaign/traces", tools / "campaign/traces")
        shutil.copyfile(
            ROOT / "campaign/scenarios.toml", tools / "campaign/scenarios.toml"
        )
        shutil.copytree(ROOT / "config/guardian", tools / "config/guardian")
        shutil.copytree(ROOT / "diagnostics/catalog", tools / "diagnostics/catalog")
        revision = run(["git", "-C", ROOT, "rev-parse", "HEAD"]).stdout.strip()
        if run(
            ["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=all"]
        ).stdout.strip():
            revision += "-dirty"
        (tools / "git-revision.txt").write_text(revision + "\n")
        shutil.copyfile(ROOT / "deploy/autosd/runtime.sh", tools / "runtime-single.sh")
        for file in ["runtime.sh", "can_io.py", "clock_io.py"]:
            shutil.copyfile(HERE / file, tools / file)
        self.export(f"ota-outlaws/campaign:{job}", "/campaign", tools / "campaign-cli")
        # Dedicated B->A key restricted to this pair. No backend credentials on B.
        run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", tools / "source-key"])
        self.ssh(
            a,
            "umask 077; cat >> /root/.ssh/authorized_keys",
            data=(tools / "source-key.pub").read_text(),
        )
        # A's NetBird address is the only inter-guest SSH path. There is no second
        # CAN route. Use the host-observed key under that IP in B's known_hosts.
        ip = self.ssh(
            a, "ip -4 -o addr show wt0 | awk '{print $4}' | cut -d/ -f1"
        ).stdout.strip()
        import ipaddress

        ipaddress.ip_address(ip)
        hostkey = self.ssh(a, "cat /etc/ssh/ssh_host_ed25519_key.pub").stdout.strip()
        (tools / "source-known-hosts").write_text(ip + " " + hostkey + "\n")
        source_dir = "/opt/ota-bench-source"
        self.ssh(a, f"mkdir -p {source_dir}")
        self.copy(a, [HERE / "can_io.py"], source_dir + "/")
        (tools / "bench-identities.json").write_text(json.dumps(self.config, indent=2))
        archive = self.state / "input.tar.gz"
        run(["tar", "-czf", archive, "-C", tools, "."])
        guest = "/opt/ota-outlaws-autosd"
        self.ssh(b, f"mkdir -p {guest}/bench-tools")
        self.copy(b, [archive], guest + "/bench-tools/")
        self.ssh(
            b,
            f"{guest}/busybox tar -xzf {guest}/bench-tools/input.tar.gz -C {guest}/bench-tools; chmod 700 {guest}/bench-tools/runtime*.sh {guest}/bench-tools/campaign-cli {guest}/bench-tools/source-key",
        )
        # Retain host SSH clock estimates for diagnostics only; NTP gates readiness.
        before = time.time_ns()
        remote = int(self.ssh(a, "date +%s%N").stdout)
        after = time.time_ns()
        self.config["source_clock_probe"] = dict(
            offset_ns=remote - (before + after) // 2,
            uncertainty_ns=(after - before) // 2,
        )
        before = time.time_ns()
        remote_b = int(self.ssh(b, "date +%s%N").stdout)
        after = time.time_ns()
        self.config["destination_clock_probe"] = dict(
            offset_ns=remote_b - (before + after) // 2,
            uncertainty_ns=(after - before) // 2,
        )
        self.save()
        (tools / "bench-identities.json").write_text(json.dumps(self.config, indent=2))
        self.copy(b, [tools / "bench-identities.json"], guest + "/bench-tools/")
        # Guest systemd bounds execution and survives temporary SSH disconnects.
        # Explicit User supplies the login environment required by Ankaios 1.0.4.
        command = (
            f"cd {guest}/bench-tools; export AUTOSD_GUEST_DIR={guest} AUTOSD_SOURCE_HOST=root@{ip} AUTOSD_SOURCE_DIR={source_dir}; exec ./campaign-cli run --no-build --runtime-hook ./runtime.sh --run-id {job} --out {guest}/campaign-reports --zenoh tcp/127.0.0.1:17447 --sovd http://127.0.0.1:17690/sovd/v1 "
            + " ".join(shlex.quote(s) for s in self.args.scenarios)
        )
        unit = "ota-campaign-" + job
        self.config["campaign_unit"] = unit
        self.save()
        self.ssh(
            b,
            f"systemd-run --quiet --collect --uid=root --unit={unit} --property=RuntimeMaxSec=3600 --property=TimeoutStopSec=330 /bin/bash -c "
            + shlex.quote(command + f" > {guest}/bench-tools/run.log 2>&1"),
        )
        deadline = time.monotonic() + 4000
        while time.monotonic() < deadline:
            if self.ssh(
                b, f"systemctl is-active {unit}", check=False
            ).stdout.strip() not in ("active", "activating", "deactivating"):
                break
            time.sleep(2)
        else:
            raise RuntimeError("Campaign deadline exceeded")
        self.ssh(
            b,
            "python3 - " + shlex.quote(f"{guest}/campaign-reports/{job}") + " <<'PY'\n"
            "import json, sys\nfrom pathlib import Path\n"
            'reports=[json.loads(p.read_text()) for p in Path(sys.argv[1]).glob("*/report.json")]\n'
            'assert reports and all(r["verdict"] == "PASS" for r in reports), "Non-PASS campaign verdict; see retained reports"\nPY',
        )
        result = self.ssh(
            b, f"cat {guest}/campaign-reports/{job}/exit-code"
        ).stdout.strip()
        if result != "0":
            raise RuntimeError(
                "Shared campaign returned " + result + "; evidence retained"
            )

    def cleanup(self):
        if self.config.get("cleanup_complete"):
            return True
        failures = []

        def attempt(label, function):
            try:
                function()
            except Exception as error:
                failures.append(label + ": " + str(error))

        if self.config["vms"]:
            b = self.config["peers"][1]
            unit = self.config.get("campaign_unit")
            if unit:
                attempt(
                    "stop campaign",
                    lambda: self.ssh(
                        b,
                        f"systemctl kill --kill-whom=main --signal=SIGTERM {unit}",
                        check=False,
                    ),
                )
                # Wait for Rust restoration before removing either peer.
                deadline = time.monotonic() + 330
                while time.monotonic() < deadline:
                    try:
                        state = self.ssh(
                            b, f"systemctl is-active {unit}", check=False, timeout=5
                        ).stdout.strip()
                    except Exception:
                        break
                    if state not in ("active", "activating", "deactivating"):
                        break
                    time.sleep(1)
            # Emergency restoration if systemd killed the Rust process hard.
            attempt(
                "source restore",
                lambda: self.ssh(
                    self.config["peers"][0],
                    "rm -f /opt/ota-bench-source/muted /opt/ota-bench-source/epoch /opt/ota-bench-source/epoch.next /opt/ota-bench-source/intended-stop; systemctl stop ota-bench-source ota-bench-source-capture 2>/dev/null || true",
                ),
            )
            attempt(
                "destination restore",
                lambda: self.ssh(
                    b,
                    "ip link set vcan0 up; systemctl stop ota-bench-destination 2>/dev/null || true",
                ),
            )

            def collect_source():
                a = self.config["peers"][0]
                for filename in ["source-can.jsonl", "source-observed-can.jsonl"]:
                    remote = "/opt/ota-bench-source/" + filename
                    if self.ssh(a, "test -f " + remote, check=False).returncode == 0:
                        run(
                            [
                                "scp",
                                "-P",
                                str(a["ssh_port"]),
                                *self.ssh_args(a)[3:-1],
                                "root@127.0.0.1:" + remote,
                                self.state / ("emergency-" + filename),
                            ]
                        )

            attempt("source evidence", collect_source)
            # Collect before destroying the fresh disks, including preflight failures.
            archive = self.state / "guest-evidence.tar.gz"

            def collect():
                command = "/opt/ota-outlaws-autosd/busybox tar -czf - -C /opt/ota-outlaws-autosd campaign-reports bench-tools/run.log loaded-images.json"
                with open(archive, "wb") as output:
                    result = subprocess.run(
                        [*self.ssh_args(b), command],
                        stdout=output,
                        stderr=subprocess.PIPE,
                        timeout=120,
                    )
                if result.returncode:
                    raise RuntimeError("Evidence collection incomplete")

            if unit:
                attempt("evidence", collect)
        if self.config.get("clock_configured"):
            for peer in self.config["peers"]:
                attempt("restore guest clock config", lambda peer=peer: self.ssh(peer,
                        "if test -f /opt/ota-bench-clock-original.conf; then cp /opt/ota-bench-clock-original.conf /etc/chrony.conf; rm -f /opt/ota-bench-clock-original.conf; systemctl restart chronyd; fi"))
        # Delete only UUIDs saved before allocation. Never touch other clusters.
        attempt(
            "undeploy",
            lambda: self.cleo(
                "delete", "cluster-deployment", self.config["cluster_id"]
            ),
        )
        attempt(
            "cluster",
            lambda: self.cleo(
                "delete", "cluster-descriptor", self.config["cluster_id"]
            ),
        )
        for peer in self.config["peers"]:
            if self.config["vms"]:
                attempt(
                    "stop EDGAR",
                    lambda peer=peer: self.ssh(
                        peer, "systemctl stop opendut-edgar", check=False, timeout=15
                    ),
                )
            attempt("peer", lambda peer=peer: self.cleo("delete", "peer", peer["id"]))
        for vm in self.config["vms"]:

            def kill(vm=vm):
                cmd = run(
                    ["ps", "-p", str(vm["pid"]), "-o", "command="], check=False
                ).stdout
                if vm["disk"] in cmd and "air" in cmd:
                    os.killpg(vm["pid"], signal.SIGTERM)
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        if not run(
                            ["ps", "-p", str(vm["pid"]), "-o", "command="], check=False
                        ).stdout.strip():
                            break
                        time.sleep(0.2)
                    else:
                        os.killpg(vm["pid"], signal.SIGKILL)
                Path(vm["disk"]).unlink(missing_ok=True)

            attempt("VM", kill)
        # Private keys and CLEO credentials never enter uploaded evidence.
        if not failures:
            for key in [self.state / "key", self.state / "input/source-key"]:
                key.unlink(missing_ok=True)
            shutil.rmtree(self.state / "cleo-config", ignore_errors=True)
            shutil.rmtree(self.state / "cleo-data", ignore_errors=True)
            (self.state / "input.tar.gz").unlink(missing_ok=True)
            self.config["cleanup_complete"] = True
            self.save()
        (self.state / "cleanup.json").write_text(
            json.dumps(dict(failures=failures), indent=2) + "\n"
        )
        return not failures


def interrupted(sig, frame):
    raise KeyboardInterrupt()


def main():
    parser = argparse.ArgumentParser()
    config_parser = argparse.ArgumentParser(add_help=False)
    config_parser.add_argument("--config")
    preliminary, _ = config_parser.parse_known_args()
    parser.add_argument("--config")
    parser.add_argument("operation", choices=["doctor", "run", "cleanup"])
    parser.add_argument("--state", default=str(ROOT / "runs" / ("opendut-" + uuid.uuid4().hex[:12])))
    parser.add_argument("--base", default=os.environ.get("AUTOSD_BASE_IMAGE"))
    parser.add_argument("--air", default=os.environ.get("AUTOSD_AIR"))
    parser.add_argument("--base-sha256", default=os.environ.get("AUTOSD_BASE_SHA256"))
    parser.add_argument("--air-sha256", default=os.environ.get("AUTOSD_AIR_SHA256"))
    parser.add_argument("--backend", default=os.environ.get("OPENDUT_BACKEND_URL"))
    parser.add_argument(
        "--backend-hosts", default=os.environ.get("OPENDUT_BACKEND_HOSTS", "")
    )
    parser.add_argument("--ca", default=os.environ.get("OPENDUT_CA"))
    parser.add_argument(
        "--edgar-sha256", default=os.environ.get("OPENDUT_EDGAR_SHA256")
    )
    parser.add_argument(
        "--cleo-command",
        default=os.environ.get("OPENDUT_CLEO_COMMAND", '["opendut-cleo"]'),
    )
    parser.add_argument("--diagnostics", default=os.environ.get("DIAGNOSTICS_IMAGE", production_diagnostics()))
    parser.add_argument("--source-memory", default="1G")
    parser.add_argument("--dut-memory", default="3G")
    parser.add_argument("--source-cpus", type=int, choices=range(1, 9), default=2)
    parser.add_argument("--dut-cpus", type=int, choices=range(1, 9), default=4)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("scenarios", nargs="*", default=["--all"])
    try:
        parser.set_defaults(**local_config(preliminary.config))
    except (OSError, ValueError, TypeError) as error:
        parser.error(str(error))
    args = parser.parse_intermixed_args()
    if not args.scenarios:
        args.scenarios = ["--all"]
    if args.operation != "cleanup":
        try:
            preflight(args)
        except (RuntimeError, OSError, ValueError) as error:
            parser.error(str(error))
        if args.operation == "doctor":
            return 0
    if not args.ca:
        parser.error("Explicit shared-backend CA required")
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, interrupted)
    if args.operation == "cleanup" and not (Path(args.state) / "owned.json").exists():
        return 0
    bench = Bench(args)
    print("Bench state and retained evidence: " + str(bench.state), flush=True)
    if args.operation == "cleanup":
        if bench.config.get("cleanup_complete"):
            return 0
        setup = os.environ.get("OPENDUT_CLEO_SETUP")
        if setup:
            bench.cleo("setup", "--persistent", "user", data=setup + "\n")
        return 0 if bench.cleanup() else 1
    failed = False
    try:
        if not all(
            [args.base, args.air, args.backend, args.edgar_sha256, args.diagnostics]
        ):
            raise RuntimeError(
                "Explicit base image, air, backend, EDGAR digest and production diagnostics image required"
            )
        version = bench.cleo("--version")
        if "Version:       0.10.2" not in version:
            raise RuntimeError("Only verified openDuT 0.10.2 supported")
        # Optional native CLEO setup uses stdin; config stays in this job's XDG path.
        setup = os.environ.get("OPENDUT_CLEO_SETUP")
        if setup:
            bench.cleo("setup", "--persistent", "user", data=setup + "\n")
        print("Creating fresh AutoSD peers...", flush=True)
        bench.boot()
        bench.descriptors()
        print("Provisioning openDuT 0.10.2 and CAN compatibility...", flush=True)
        bench.provision(bench.assets())
        print("Verifying actual CAN deployment dependency...", flush=True)
        bench.connectivity()
        print("Synchronizing guest clocks across the bench...", flush=True)
        bench.synchronize()
        print(
            "Building/deploying applications and running shared campaigns on B...",
            flush=True,
        )
        bench.applications()
    except BaseException as error:
        failed = True
        (bench.state / "error.txt").write_text(str(error) + "\n")
        print(str(error), file=sys.stderr)
    finally:
        # Second signal cannot skip cleanup; cleanup can also be retried explicitly.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        if not bench.cleanup():
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
