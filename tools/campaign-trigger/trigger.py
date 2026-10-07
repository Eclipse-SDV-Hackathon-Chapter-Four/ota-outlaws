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
#
# AI-assisted: Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5)

"""Starts a campaign when a button is pressed on the AZ3166 board.

The board sends JSON datagrams to UDP 30502:
    {"type":"sample","seq":N,"temperature_c":T}              every 100 ms (ignored)
    {"type":"campaign","id":N}                               button A (current firmware)
    {"type":"can_fault","scenario":"all"}                    button A (older firmware)
    {"type":"bad_sample","seq":N,"temperature_c":120}        button B

The campaign tool does not use the board's samples: it starts its own chain per
scenario and replays a CAN trace. The board is only the remote trigger. This
script listens on the UDP port and runs the campaign tool when a button event
arrives:

    button A  ->  cargo run -p campaign -- run --all        (--button-a all)
    button B  ->  cargo run -p campaign -- run spike        (--button-b spike)

One campaign at a time; button presses while one runs are ignored. Every press
is appended to a JSON-lines trigger log with the command, the exit code and the
evidence directory.

REPLIES TO THE BOARD (for "campaign" requests, to the address the request came
from), the same message types and fields as host/campaign_bridge.py, but each
result is sent the moment its scenario has finished, so the display can add one
line per test:
    {"type":"campaign_ack","id":N}
    {"type":"campaign_result","id":N,"scenario":"counter_stuck","verdict":"PASS"}
    {"type":"campaign_complete","id":N}
    {"type":"campaign_error","id":N}                       (busy, or the run failed)
verdict is PASS, FAIL or INCONCLUSIVE. With --extended every message also carries
n, total, hara, planned (a scenario for a requirement the Guardian does not
implement yet, expected to fail), pass and fail.

    python3 tools/campaign-trigger/trigger.py                 # real run
    python3 tools/campaign-trigger/trigger.py --button-a normal   # quick test
    python3 tools/campaign-trigger/trigger.py --dry-run           # print only
"""

import argparse
import json
import os
import re
import signal
import socket
import subprocess
import threading
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GCC_INCLUDE = "/usr/lib/gcc/x86_64-linux-gnu/11/include"


def now_ms():
    return int(time.time() * 1000)


# The fields of host/campaign_bridge.py. By default only these are sent, so that a
# firmware written for that script gets the same short messages. --extended adds
# n, total, hara, planned, pass and fail.
COMPAT_FIELDS = {
    "campaign_ack": {"type", "id"},
    "campaign_result": {"type", "id", "scenario", "verdict"},
    "campaign_complete": {"type", "id"},
    "campaign_error": {"type", "id"},
}


def load_catalog():
    """Scenarios of campaign/scenarios.toml: id -> {status, hara, external}."""
    text = (REPO / "campaign" / "scenarios.toml").read_text()
    catalog = {}
    for block in text.split("[[scenario]]")[1:]:
        ident = re.search(r'^id = "([^"]+)"', block, flags=re.MULTILINE)
        if not ident:
            continue
        status = re.search(r'^status = "([^"]+)"', block, flags=re.MULTILINE)
        hara = re.search(r'^hara_tests = \[([^\]]*)\]', block, flags=re.MULTILINE)
        stimulus = re.search(r'^stimulus = \{ type = "([^"]+)"', block, flags=re.MULTILINE)
        catalog[ident.group(1)] = {
            "status": status.group(1) if status else "implemented",
            "hara": ", ".join(re.findall(r'"([^"]+)"', hara.group(1))) if hara else "",
            "external": bool(stimulus and stimulus.group(1) == "external"),
        }
    return catalog


def leftover_projects():
    """Docker Compose projects of earlier campaign runs that were not removed."""
    out = subprocess.run(
        ["docker", "ps", "-a", "--filter", "name=^campaign-", "--format",
         '{{.Label "com.docker.compose.project"}}'],
        capture_output=True, text=True)
    return sorted({line.strip() for line in out.stdout.splitlines() if line.strip()})


class Trigger:
    def __init__(self, args):
        self.args = args
        self.catalog = load_catalog()
        self.known = set(self.catalog)
        self.busy = threading.Lock()
        self.child = None
        self.log_path = Path(args.log)
        self.log_path.parent.mkdir(parents=True, exist_ok=True)

    def log(self, event, **fields):
        line = {"event": event, "wall_ms": now_ms(), **fields}
        print(f"[trigger] {json.dumps(line)}", flush=True)
        with open(self.log_path, "a") as handle:
            handle.write(json.dumps(line) + "\n")

    def send(self, reply, message):
        """Sends a reply to the board, if the request came with a reply address."""
        if reply is None:
            return
        sock, address = reply
        if self.args.reply_port:
            address = (address[0], self.args.reply_port)
        if not self.args.extended:
            keep = COMPAT_FIELDS.get(message.get("type"))
            if keep:
                message = {key: value for key, value in message.items() if key in keep}
        payload = json.dumps(message, separators=(",", ":")).encode()
        try:
            sock.sendto(payload, address)
            # UDP gives no delivery confirmation: this proves the datagram left the PC.
            self.log("reply_sent", to=f"{address[0]}:{address[1]}", bytes=len(payload),
                     message_type=message.get("type"), payload=payload.decode())
        except OSError as error:
            self.log("reply_failed", error=str(error), message_type=message.get("type"))

    def scenarios_for(self, target):
        if target == "all":
            if self.args.scenarios:
                return [x for x in self.args.scenarios.split(",") if x in self.known]
            return [i for i, c in self.catalog.items() if not c["external"]]
        return [target]

    def cleanup(self, reason):
        """Removes the Docker projects an interrupted campaign run left behind.

        The campaign tool names its project campaign-<scenario>. A project left over
        from an interrupted run would be reused by the next run and mix old and new
        containers, which gives wrong verdicts.
        """
        if self.args.dry_run:
            return
        for project in leftover_projects():
            result = subprocess.run(
                ["docker", "compose", "-p", project, "down", "-v", "--remove-orphans"],
                capture_output=True, text=True, timeout=180)
            self.log("cleanup", reason=reason, project=project, exit_code=result.returncode)

    def command_for(self, target):
        command = ["cargo", "run", "-q", "-p", "campaign", "--", "run"]
        if target == "all" and not self.args.scenarios:
            command.append("--all")
        else:
            command.extend(self.scenarios_for(target))
        if not self.args.build:
            command.append("--no-build")
        return command

    def start(self, source, target, request_id=None, reply=None):
        if target != "all" and target not in self.known:
            self.log("rejected", source=source, target=target,
                     reason="not a scenario of campaign/scenarios.toml")
            self.send(reply, {"type": "campaign_error", "id": request_id})
            return
        if not self.busy.acquire(blocking=False):
            self.log("ignored", source=source, target=target,
                     reason="a campaign is already running")
            self.send(reply, {"type": "campaign_error", "id": request_id})
            return
        total = len(self.scenarios_for(target))
        self.send(reply, {"type": "campaign_ack", "id": request_id, "total": total})
        threading.Thread(target=self.run, args=(source, target, request_id, reply),
                         daemon=True).start()

    def run(self, source, target, request_id, reply):
        command = self.command_for(target)
        total = len(self.scenarios_for(target))
        self.log("campaign_requested", source=source, target=target, request_id=request_id,
                 total=total, command=" ".join(command))
        passed = failed = 0
        try:
            if self.args.dry_run:
                time.sleep(1)
                self.log("campaign_finished", target=target, exit_code=None, note="dry run")
                self.send(reply, {"type": "campaign_complete", "id": request_id,
                                  "total": 0, "pass": 0, "fail": 0})
                return
            env = dict(os.environ)
            if "BINDGEN_EXTRA_CLANG_ARGS" not in env and os.path.isdir(GCC_INCLUDE):
                env["BINDGEN_EXTRA_CLANG_ARGS"] = f"-I{GCC_INCLUDE}"
            process = subprocess.Popen(command, cwd=REPO, env=env, text=True,
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            self.child = process
            evidence, verdicts = None, []
            for line in process.stdout:
                print(f"[campaign] {line.rstrip()}", flush=True)
                match = re.match(r"^evidence: (.+)$", line.strip())
                if match:
                    evidence = match.group(1)
                match = re.match(r"^([a-z0-9_]+): (Pass|Fail|Inconclusive)\b", line.strip())
                if match:
                    name, verdict = match.group(1), match.group(2).upper()
                    verdicts.append({"scenario": name, "verdict": verdict})
                    passed += verdict == "PASS"
                    failed += verdict != "PASS"
                    info = self.catalog.get(name, {})
                    self.log("result", scenario=name, verdict=verdict, n=len(verdicts), total=total)
                    # Sent now, not at the end: the display adds one line per test.
                    self.send(reply, {"type": "campaign_result", "id": request_id,
                                      "n": len(verdicts), "total": total, "scenario": name,
                                      "hara": info.get("hara", ""), "verdict": verdict,
                                      "planned": info.get("status") == "planned"})
            code = process.wait()
            self.child = None
            self.log("campaign_finished", target=target, exit_code=code,
                     evidence=evidence, verdicts=verdicts)
            if code in (0, 1) and verdicts:
                self.send(reply, {"type": "campaign_complete", "id": request_id,
                                  "total": total, "pass": passed, "fail": failed})
            else:
                self.send(reply, {"type": "campaign_error", "id": request_id})
        except OSError:
            self.log("campaign_failed_to_start", target=target)
            self.send(reply, {"type": "campaign_error", "id": request_id})
        finally:
            self.busy.release()


    def stop(self):
        """Stops a running campaign and removes what it left behind."""
        process = self.child
        if process is not None and process.poll() is None:
            self.log("stopping_campaign", pid=process.pid)
            try:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=15)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        self.cleanup("shutdown")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--listen", default="0.0.0.0:30502")
    parser.add_argument("--button-a", default="all", help="'all' (default) or one scenario id")
    parser.add_argument("--button-b", default="spike", help="scenario id (default spike)")
    parser.add_argument("--scenarios", default=None,
                        help="comma-separated ids that button A runs instead of all (for tests)")
    parser.add_argument("--extended", action="store_true",
                        help="also send n, total, hara, planned, pass, fail in the replies "
                             "(default: only the fields of host/campaign_bridge.py)")
    parser.add_argument("--reply-port", type=int, default=None,
                        help="send replies to this UDP port of the board instead of the port "
                             "the request came from")
    parser.add_argument("--allow", default=None, help="only accept datagrams from this IP")
    parser.add_argument("--build", action="store_true",
                        help="let the campaign tool build the images (default: --no-build)")
    parser.add_argument("--log", default=str(REPO / "runs" / "trigger-log.jsonl"))
    parser.add_argument("--dry-run", action="store_true", help="do not run the campaign tool")
    args = parser.parse_args()

    host, port = args.listen.rsplit(":", 1)
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((host, int(port)))
    sock.settimeout(1.0)
    trigger = Trigger(args)
    trigger.cleanup("startup")
    trigger.log("listening", listen=args.listen, button_a=args.button_a, button_b=args.button_b,
                dry_run=args.dry_run)

    samples, last_report, last_source = 0, time.monotonic(), None
    try:
        while True:
            try:
                data, addr = sock.recvfrom(2048)
            except socket.timeout:
                data = None
            if time.monotonic() - last_report >= 10:
                note = f"board alive, {samples} samples in 10 s" if samples else "no sample from the board in 10 s"
                trigger.log("heartbeat", note=note, last_source=last_source)
                samples, last_report = 0, time.monotonic()
            if data is None:
                continue
            if args.allow and addr[0] != args.allow:
                continue
            last_source = addr[0]
            try:
                message = json.loads(data)
            except ValueError:
                trigger.log("not_json", source=addr[0], payload=data[:60].decode("utf-8", "replace"))
                continue
            kind = message.get("type")
            if kind == "sample":
                samples += 1
            elif kind == "campaign":
                request_id = message.get("id")
                if isinstance(request_id, bool) or not isinstance(request_id, int):
                    trigger.log("rejected", source=addr[0], reason="campaign id is not an integer")
                    continue
                trigger.log("button", source=addr[0], button="A", message=message)
                trigger.start(addr[0], "all" if args.button_a == "all" else args.button_a,
                              request_id, (sock, addr))
            elif kind == "can_fault":
                scenario = message.get("scenario")
                target = args.button_a if scenario == "all" else scenario
                trigger.log("button", source=addr[0], button="A", message=message)
                trigger.start(addr[0], target)
            elif kind == "bad_sample":
                trigger.log("button", source=addr[0], button="B", message=message)
                trigger.start(addr[0], args.button_b)
            else:
                trigger.log("unknown_message", source=addr[0], message=message)
    except KeyboardInterrupt:
        pass
    finally:
        trigger.stop()
        trigger.log("stopped")


if __name__ == "__main__":
    main()
