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
"""SocketCAN frame IO only. Scenario selection, actions and verdicts stay in Rust.

ASC timestamps are absolute offsets from a scheduled epoch. Capture uses kernel
receive timestamps. No data is sent on B; no trace is decoded here.
"""
import argparse
import errno
import json
import signal
import socket
import struct
import time
from pathlib import Path

FRAME = struct.Struct("=IB3x8s")
# Bench scheduling tolerance: half the nominal 100 ms CAN cycle. This is
# independent of the Guardian's HARA-derived safety/reaction parameters.
SOURCE_LATENESS_LIMIT_NS = 50_000_000
STOP = False


def emit(file, **entry):
    file.write(json.dumps(entry) + "\n")
    file.flush()


def frames(path):
    previous = -1
    for line in Path(path).read_text().splitlines():
        fields = line.split()
        if len(fields) < 7 or fields[3:5] != ["Rx", "d"]:
            continue
        offset = round(float(fields[0]) * 1_000_000_000)
        can_id = int(fields[2], 16)
        dlc = int(fields[5])
        payload = bytes.fromhex("".join(fields[6:]))
        if (
            offset < previous
            or offset < 0
            or not 0 <= can_id <= 0x7FF
            or not 0 <= dlc <= 8
            or len(payload) != dlc
        ):
            raise ValueError(
                "Expected ordered standard classic CAN frames with exact DLC"
            )
        previous = offset
        yield offset, can_id, payload


def opened(interface):
    bus = socket.socket(socket.PF_CAN, socket.SOCK_RAW, socket.CAN_RAW)
    bus.bind((interface,))
    return bus


def replay(args):
    trace = list(frames(args.trace))
    if not trace:
        raise ValueError("ASC has no CAN frames")
    with opened(args.interface) as bus, open(args.output, "w") as log:
        epoch = args.epoch_ms
        if args.epoch_file:
            # Parse the trace and open CAN before acknowledging readiness. The
            # shared driver releases this process only after Guardian is ready.
            emit(log, kind="ready", run_id=args.run_id, wall_ns=time.time_ns())
            deadline = time.monotonic() + 90
            while not Path(args.epoch_file).exists():
                if STOP:
                    emit(log, kind="stopped", wall_ns=time.time_ns())
                    return
                if time.monotonic() >= deadline:
                    raise TimeoutError("Driver did not release the armed source")
                time.sleep(0.005)
            epoch = int(Path(args.epoch_file).read_text())
        if epoch is None:
            raise ValueError("Replay requires --epoch-ms or --epoch-file")
        origin = time.monotonic_ns() + epoch * 1_000_000 - time.time_ns()
        emit(
            log,
            kind="armed",
            epoch_ms=epoch,
            run_id=args.run_id,
            count=len(trace),
        )
        for sequence, (offset, can_id, payload) in enumerate(trace):
            if STOP:
                break
            remaining = origin + offset - time.monotonic_ns()
            # Keep the vCPU runnable for most of a nominal CAN cycle before sending.
            # HVF can wake a sleeping vCPU almost a cycle late, so a 20 ms spin
            # leaves insufficient headroom. Long trace gaps still sleep first.
            # Leave 10% idle time so the FIFO task stays below RT CPU throttling.
            if remaining > 90_000_000:
                time.sleep((remaining - 90_000_000) / 1e9)
            while not STOP and time.monotonic_ns() < origin + offset:
                pass
            if STOP:
                break
            if args.mute_file and Path(args.mute_file).exists():
                emit(
                    log,
                    kind="suppressed",
                    sequence=sequence,
                    scheduled_ns=offset,
                    wall_ns=time.time_ns(),
                    run_id=args.run_id,
                )
                continue
            before = time.monotonic_ns()
            bus.send(FRAME.pack(can_id, len(payload), payload))
            emit(
                log,
                kind="frame",
                sequence=sequence,
                run_id=args.run_id,
                can_id=can_id,
                data=payload.hex(),
                scheduled_ns=offset,
                elapsed_ns=before - origin,
                wall_ns=time.time_ns(),
            )
        stop_file = getattr(args, "stop_file", None)
        emit(log, kind="stopped" if STOP else "completed", wall_ns=time.time_ns(),
             intentional_stop=bool(STOP and stop_file and Path(stop_file).exists()))


def capture(args):
    with opened(args.interface) as bus, open(args.output, "w") as log:
        # SO_TIMESTAMPNS = 35 on Linux aarch64; SCM_TIMESTAMPNS uses two longs.
        bus.setsockopt(socket.SOL_SOCKET, 35, 1)
        bus.settimeout(0.2)
        emit(log, kind="ready", run_id=args.run_id, wall_ns=time.time_ns())
        while not STOP:
            try:
                data, ancillary, _, _ = bus.recvmsg(FRAME.size, 128)
            except socket.timeout:
                continue
            except OSError as error:
                if error.errno != errno.ENETDOWN:
                    raise
                # The destination endpoint is deliberately lowered by the
                # driver. Keep this capture alive across restoration.
                time.sleep(0.01)
                continue
            can_id, dlc, payload = FRAME.unpack(data)
            if can_id != 0x500:
                continue
            wall_ns = time.time_ns()
            for level, kind, value in ancillary:
                if level == socket.SOL_SOCKET and kind == 35:
                    seconds, nanos = struct.unpack("=qq", value[:16])
                    wall_ns = seconds * 1_000_000_000 + nanos
            emit(
                log,
                kind="frame",
                run_id=args.run_id,
                can_id=can_id,
                data=payload[:dlc].hex(),
                wall_ns=wall_ns,
            )


def read(path):
    return [
        json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()
    ]


def compare(source, destination, interruption=None, observed_source=None):
    """Check ordered measured delivery, keeping readiness outside the window.

    interruption: pair of actual link-down/up wall timestamps. Frames sent in
    this interval may be lost or arrive after recovery; every other frame must
    arrive once. Destination silence and actual loss prove the interruption.
    """
    sent = [r for r in source if r["kind"] == "frame"]
    received = [r for r in destination if r["kind"] == "frame"]
    expected = [
        r
        for r in sent
        if not interruption or not interruption[0] <= r["wall_ns"] < interruption[1]
    ]
    key = lambda r: (r["can_id"], r["data"])
    boundary_deliveries = 0
    missing = []
    # Received frames must remain an ordered subsequence of emitted frames.
    # In-flight frames sent during the outage may arrive after recovery;
    # requiring them to be lost mistakes real transport delay for failure.
    cursor = 0
    for frame in sent:
        if cursor < len(received) and key(frame) == key(received[cursor]):
            if interruption and interruption[0] <= frame["wall_ns"] < interruption[1]:
                boundary_deliveries += 1
            cursor += 1
        else:
            missing.append(frame)
    matches = cursor == len(received) and (
        all(interruption[0] <= frame["wall_ns"] < interruption[1] for frame in missing)
        if interruption else not missing
    )
    # Prove a live source, actual frame loss and destination silence while the
    # endpoint is down. Optional boundary delivery cannot hide loss elsewhere.
    loss_proven = not interruption or (
        bool(missing)
        and bool(sent) and bool(received)
        and sent[0]["wall_ns"] < interruption[0] < interruption[1] < sent[-1]["wall_ns"]
        and received[0]["wall_ns"] < interruption[0] < interruption[1] < received[-1]["wall_ns"]
        and not any(interruption[0] <= r["wall_ns"] < interruption[1] for r in received)
    )
    armed = next((r for r in source if r["kind"] == "armed"), None)
    source_ok = bool(source) and (
        source[-1]["kind"] == "completed"
        or (source[-1]["kind"] == "stopped" and source[-1].get("intentional_stop") is True)
    )
    if armed and source[-1]["kind"] == "completed":
        source_ok = (
            source_ok
            and len(sent) + sum(r["kind"] == "suppressed" for r in source)
            == armed["count"]
        )
    observed_source_ok = observed_source is None or (
        [(r["can_id"], r["data"]) for r in observed_source if r["kind"] == "frame"]
        == [(r["can_id"], r["data"]) for r in sent]
    )
    source_ok = source_ok and observed_source_ok
    lateness = max((r["elapsed_ns"] - r["scheduled_ns"] for r in sent), default=0)
    # This is stimulus integrity, not a relaxed Guardian safety budget.
    timing_ok = lateness < SOURCE_LATENESS_LIMIT_NS
    classification = (
        "source_failure"
        if not source_ok or not timing_ok
        else "transport_failure" if not matches or not loss_proven else "verified"
    )
    return dict(
        classification=classification,
        source_frames=len(sent),
        destination_frames=len(received),
        expected_frames=len(expected),
        interruption_boundary_deliveries=boundary_deliveries,
        missing_frames=len(missing),
        source_bus_confirmed=observed_source_ok,
        max_source_lateness_ns=lateness,
        source_lateness_limit_ns=SOURCE_LATENESS_LIMIT_NS,
        source_ok=source_ok,
        ordered_delivery=matches,
        interruption_proven=loss_proven,
    )


def stop(_signal, _frame):
    global STOP
    STOP = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=["replay", "capture", "compare"])
    parser.add_argument("--interface", default="vcan0")
    parser.add_argument("--trace")
    parser.add_argument("--mute-file")
    parser.add_argument("--stop-file")
    epoch = parser.add_mutually_exclusive_group()
    epoch.add_argument("--epoch-ms", type=int)
    epoch.add_argument("--epoch-file")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--source")
    parser.add_argument("--destination")
    parser.add_argument("--source-capture")
    parser.add_argument("--interruption")
    args = parser.parse_args()
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, stop)
    if args.operation == "compare":
        try:
            interval = None
            if args.interruption and Path(args.interruption).exists():
                interval = json.loads(Path(args.interruption).read_text())
            source = read(args.source)
            destination = read(args.destination)
            # Captures are armed before Guardian and no readiness BMS frame is
            # emitted. Keep every observed frame; filtering by another peer's
            # wall clock could discard a valid first frame under small clock skew.
            observed_source = read(args.source_capture) if args.source_capture else None
            result = compare(source, destination, interval, observed_source)
        except (OSError, ValueError, KeyError) as error:
            result = {"classification": "evidence_incomplete", "error": str(error)}
        result["run_id"] = args.run_id
        Path(args.output).write_text(json.dumps(result, indent=2) + "\n")
        return 0 if result["classification"] == "verified" else 1
    {"replay": replay, "capture": capture}[args.operation](args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
