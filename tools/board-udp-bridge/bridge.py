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

"""Turns a button press on the board into one fault in the KUKSA Data Broker.

NORMAL OPERATION
  The KUKSA CAN Provider replays a fault-free trace in a loop (--baseline).
  The provider writes the signals into the Data Broker, the VSS Publisher
  forwards them, the Guardian stays calm. The bridge does not touch the data.

A FAULT
  The board sends a UDP datagram when its button is pressed:
      {"type":"can_fault","scenario":"counter_stuck"}
      {"type":"bad_sample","seq":32796}
  The bridge then
    1. pauses the baseline provider (docker pause, about 20 ms, no restart),
    2. writes the faulty frames itself, 10 per second, for the duration of the
       scenario, so the data stream has no gap at the switch,
    3. resumes the provider (docker unpause), which continues where it stopped.
  The fault is defined by the scenario name, not by a trace file.

INJECTION LOG
  Every step is appended to a JSON-lines file with wall-clock times. The first
  faulty write is `first_fault_frame_ms`, so a detection latency can be computed.

Scenarios (default duration): counter_stuck (2 s), invalid_quality (2 s),
timeout (1.8 s, no frames), signal_stuck (5 s), out_of_range (2 s),
avg_gt_max (2 s), bad_sample (one frame). An optional "duration_s" in the message
overrides the duration.

    python bridge.py --databroker 127.0.0.1:55556 --listen 0.0.0.0:30502
"""

import argparse
import atexit
import json
import os
import re
import socket
import subprocess
import threading
import time

from kuksa_client.grpc import Datapoint, VSSClient

BATTERY = "Vehicle.Powertrain.TractionBattery"
MAX = f"{BATTERY}.Temperature.Max"
AVG = f"{BATTERY}.Temperature.Average"
MIN = f"{BATTERY}.Temperature.Min"
QUALITY = f"{BATTERY}.BMS.SignalQuality"
COUNTER = f"{BATTERY}.BMS.AliveCounter"
VALID, INVALID = 0x80, 0x00
FRAME_PERIOD_S = 0.1

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PROVIDER_IMAGE = "ghcr.io/eclipse-kuksa/kuksa-can-provider/can-provider:main"
BASELINE_CONTAINER = "can-baseline"


def now_ms():
    return int(time.time() * 1000)


# ---------------------------------------------------------------- scenarios
# A scenario gives, for tick k, the frame to write, or None for "write nothing".
# `st` holds the signal values at the moment the baseline was paused.

def _frame(st, k, *, mx=None, av=None, mn=None, quality=VALID, counter=None):
    return {
        MAX: float(st["max"] if mx is None else mx),
        AVG: float(st["avg"] if av is None else av),
        MIN: float(st["min"] if mn is None else mn),
        QUALITY: quality,
        COUNTER: (st["counter"] + 1 + k) % 256 if counter is None else counter % 256,
    }


SCENARIOS = {
    # alive counter stays on one value while the frames keep coming (FSR-2.3)
    "counter_stuck": (2.0, lambda st, k: _frame(st, k, counter=st["counter"])),
    # the source declares its own values unusable (FSR-3.4)
    "invalid_quality": (2.0, lambda st, k: _frame(st, k, quality=INVALID)),
    # frames stop: the Guardian must notice the missing data (FSR-2.2)
    "timeout": (1.8, lambda st, k: None),
    # maximum frozen while average and minimum keep moving (FSR-2.4)
    "signal_stuck": (5.0, lambda st, k: _frame(
        st, k, mx=st["max"], av=st["avg"] + 0.1 * k, mn=st["min"] + 0.1 * k)),
    # implausible values (FSR-3.2, planned in the Guardian)
    "out_of_range": (2.0, lambda st, k: _frame(st, k, mx=200.0)),
    "avg_gt_max": (2.0, lambda st, k: _frame(st, k, av=st["max"] + 20.0)),
    # one single frame with INVALID quality
    "bad_sample": (FRAME_PERIOD_S, lambda st, k: _frame(st, k, quality=INVALID)),
}


# ------------------------------------------------------------------ docker
def sh(*command):
    return subprocess.run(command, capture_output=True, text=True)


def baseline_start(args):
    sh("docker", "rm", "-f", BASELINE_CONTAINER)
    folder, file = os.path.dirname(args.baseline), os.path.basename(args.baseline)
    return sh(
        "docker", "run", "-d", "--rm", "--name", BASELINE_CONTAINER, "--network", args.network,
        "-e", "KUKSA_ADDRESS=kuksa-databroker", "-e", "KUKSA_PORT=55555",
        "-e", "DBC_FILE=/can/BMS_MSG1_CAN.dbc", "-e", "MAPPING_FILE=/can/vss_dbc.json",
        "-e", f"CANDUMP_FILE=/trace/{file}",
        "-v", f"{args.can_dir}:/can:ro", "-v", f"{folder}:/trace:ro",
        PROVIDER_IMAGE, "--dumpfile", f"/trace/{file}", "--infinite",
    )


# ---------------------------------------------------------------- injector
class Injector:
    def __init__(self, args, client, lock):
        self.args, self.client, self.lock = args, client, lock
        self.active = False

    def log(self, event, **fields):
        line = {"event": event, "wall_ms": now_ms(), **fields}
        print(f"[bridge] INJECTION {json.dumps(line)}")
        with open(self.args.log, "a") as handle:
            handle.write(json.dumps(line) + "\n")

    def start(self, scenario, duration=None):
        if self.active:
            print(f"[bridge] fault {scenario!r} ignored, a fault is already running")
            return
        if scenario not in SCENARIOS:
            print(f"[bridge] unknown scenario {scenario!r}; known: {', '.join(SCENARIOS)}")
            return
        self.active = True
        threading.Thread(target=self.run, args=(scenario, duration), daemon=True).start()

    def read_state(self):
        values = self.client.get_current_values([MAX, AVG, MIN, QUALITY, COUNTER])
        def number(path, default):
            datapoint = values.get(path)
            return default if datapoint is None or datapoint.value is None else datapoint.value
        return {"max": number(MAX, 40.0), "avg": number(AVG, 37.0), "min": number(MIN, 34.0),
                "quality": number(QUALITY, VALID), "counter": int(number(COUNTER, 0))}

    def write(self, frame):
        with self.lock:
            self.client.set_current_values({path: Datapoint(value) for path, value in frame.items()})

    def run(self, scenario, duration):
        default_duration, frame_of = SCENARIOS[scenario]
        duration = float(duration) if duration else default_duration
        correlation_id = f"{scenario}-{now_ms()}"
        self.log("fault_requested", scenario=scenario, correlation_id=correlation_id, duration_s=duration)
        paused = False
        try:
            if self.args.source == "provider" and not self.args.dry_run:
                result = sh("docker", "pause", BASELINE_CONTAINER)
                if result.returncode != 0:
                    print(f"[bridge] cannot pause the baseline: {result.stderr.strip()[:160]}")
                    self.log("fault_failed", correlation_id=correlation_id, reason="pause failed")
                    return
                paused = True
            paused_ms = now_ms()
            if paused:
                self.log("baseline_paused", correlation_id=correlation_id)
            state = self.read_state()
            ticks = max(1, round(duration / FRAME_PERIOD_S))
            began = time.monotonic()
            first_fault_ms = None
            for k in range(ticks):
                frame = frame_of(state, k)
                if frame is not None:
                    if first_fault_ms is None:
                        first_fault_ms = now_ms()
                        self.log("fault_started", scenario=scenario, correlation_id=correlation_id,
                                 first_fault_frame_ms=first_fault_ms, start_values=state)
                    self.write(frame)
                time.sleep(max(0.0, began + (k + 1) * FRAME_PERIOD_S - time.monotonic()))
            if first_fault_ms is None:     # a scenario without frames (timeout): data stops at the pause
                self.log("fault_started", scenario=scenario, correlation_id=correlation_id,
                         first_fault_frame_ms=paused_ms, note="no frames written", start_values=state)
        finally:
            if paused:
                sh("docker", "unpause", BASELINE_CONTAINER)
                self.log("baseline_resumed", correlation_id=correlation_id)
            self.log("fault_ended", scenario=scenario, correlation_id=correlation_id)
            self.active = False


# -------------------------------------------------------------------- main
def parse_endpoint(text):
    host, port = text.rsplit(":", 1)
    return host, int(port)


class DryClient:
    """Stands in for the Data Broker with --dry-run."""
    def __enter__(self): return self
    def __exit__(self, *exc): return False
    def get_current_values(self, paths): return {}
    def set_current_values(self, values): pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--listen", default="0.0.0.0:30502")
    parser.add_argument("--databroker", default="127.0.0.1:55556")
    parser.add_argument("--source", choices=["provider", "board"], default="provider",
                        help="provider: the CAN Provider writes the normal data. "
                             "board: the board's own samples are the normal data")
    parser.add_argument("--baseline", default=os.path.join(REPO, "tools", "board-udp-bridge", "traces", "baseline_calm.asc"),
                        help="fault-free trace for --source provider. can/BMS_MSG1_CAN.asc heats the pack "
                             "to 55 C and makes the Guardian CRITICAL in every loop")
    parser.add_argument("--can-dir", default=os.path.join(REPO, "can"))
    parser.add_argument("--network", default="ota-outlaws_default", help="Docker network of the compose stack")
    parser.add_argument("--log", default="injection-log.jsonl")
    parser.add_argument("--seconds", type=float, default=0, help="stop after this long (0 = Ctrl+C)")
    parser.add_argument("--dry-run", action="store_true", help="no Data Broker, no containers")
    args = parser.parse_args()
    args.baseline = os.path.abspath(args.baseline)
    args.can_dir = os.path.abspath(args.can_dir)

    listen_host, listen_port = parse_endpoint(args.listen)
    db_host, db_port = parse_endpoint(args.databroker)
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((listen_host, listen_port))
    sock.settimeout(0.2)
    print(f"[bridge] listening on udp/{listen_port}, source={args.source}, databroker {db_host}:{db_port}")

    lock = threading.Lock()
    suppressed = ignored = written = faults = 0
    last_seq = None
    started = time.monotonic()

    with (DryClient() if args.dry_run else VSSClient(db_host, db_port)) as client:
        injector = Injector(args, client, lock)

        if args.source == "provider" and not args.dry_run:
            sh("docker", "stop", "kuksa-can-provider")        # one writer only
            result = baseline_start(args)
            if result.returncode != 0:
                raise SystemExit(f"[bridge] cannot start the baseline: {result.stderr.strip()[:200]}")
            atexit.register(lambda: sh("docker", "kill", BASELINE_CONTAINER))
            injector.log("baseline_started", baseline=os.path.basename(args.baseline))

        while True:
            if args.seconds and time.monotonic() - started > args.seconds:
                break
            try:
                data, addr = sock.recvfrom(2048)
            except socket.timeout:
                continue
            try:
                msg = json.loads(data)
            except ValueError:
                print(f"[bridge] not JSON from {addr[0]}: {data[:80]!r}")
                ignored += 1
                continue

            kind = msg.get("type")
            if kind == "can_fault":
                faults += 1
                injector.start(msg.get("scenario"), msg.get("duration_s"))
            elif kind == "bad_sample":
                faults += 1
                injector.start("bad_sample")
            elif kind == "sample":
                if args.source == "provider" or injector.active:
                    suppressed += 1          # the board's samples are not the data source
                    continue
                value, seq = msg.get("temperature_c"), msg.get("seq")
                if isinstance(value, (int, float)) and isinstance(seq, int):
                    if last_seq is not None and seq != last_seq + 1:
                        print(f"[bridge] board seq jump {last_seq} -> {seq}")
                    last_seq = seq
                    injector.write({MAX: float(value), AVG: float(value), MIN: float(value),
                                    QUALITY: VALID, COUNTER: seq % 256})
                    written += 1
            else:
                print(f"[bridge] message type {kind!r} from {addr[0]}: {msg}")
                ignored += 1

    print(f"[bridge] done: faults={faults} board_samples_ignored={suppressed} written={written} other={ignored}")


if __name__ == "__main__":
    main()
