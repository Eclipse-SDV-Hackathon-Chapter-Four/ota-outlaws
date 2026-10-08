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
"""Evidence-integrity tests; these do not substitute for real openDuT delivery."""
import importlib.util
import tempfile
import json
import subprocess
import sys
import unittest
from unittest.mock import patch, MagicMock
from types import SimpleNamespace
import threading
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "can_io", Path(__file__).parents[1] / "can_io.py"
)
io = importlib.util.module_from_spec(spec)
spec.loader.exec_module(io)


def frame(n, time_ns=None):
    return dict(
        kind="frame",
        can_id=0x500,
        data=f"{n:016x}",
        wall_ns=n * 100_000_000 if time_ns is None else time_ns,
        scheduled_ns=n * 100_000_000,
        elapsed_ns=n * 100_000_000,
    )


class Integrity(unittest.TestCase):
    def test_prearmed_replay_sends_nothing_until_released(self):
        for cancel in (False, True):
            with self.subTest(cancel=cancel), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                trace = root / "trace.asc"
                trace.write_text("0.000000 1 500 Rx d 8 28 00 18 00 20 00 80 00\n")
                output = root / "source.jsonl"
                epoch = root / "epoch"
                bus = MagicMock()
                bus.__enter__.return_value = bus
                args = SimpleNamespace(trace=trace, interface="vcan0", output=output,
                                       epoch_ms=None, epoch_file=epoch, run_id="test",
                                       mute_file=None)
                io.STOP = False
                with patch.object(io, "opened", return_value=bus):
                    worker = threading.Thread(target=io.replay, args=(args,))
                    worker.start()
                    try:
                        deadline = time.monotonic() + 2
                        while not output.exists() or not output.stat().st_size:
                            self.assertLess(time.monotonic(), deadline)
                            time.sleep(.005)
                        self.assertEqual(io.read(output)[0]["kind"], "ready")
                        bus.send.assert_not_called()
                        if cancel:
                            io.STOP = True
                        else:
                            epoch.write_text(str(time.time_ns() // 1_000_000 + 30))
                        worker.join(2)
                        self.assertFalse(worker.is_alive())
                        if cancel:
                            bus.send.assert_not_called()
                            self.assertEqual(io.read(output)[-1]["kind"], "stopped")
                        else:
                            bus.send.assert_called_once()
                            self.assertEqual(io.read(output)[-1]["kind"], "completed")
                    finally:
                        io.STOP = True
                        worker.join(2)
                        io.STOP = False

    def test_inflight_frame_can_arrive_after_link_recovery(self):
        source = [frame(1), frame(2), frame(3, 230_000_000), frame(4)]
        destination = [frame(1), frame(3, 260_000_000), frame(4)]
        result = io.compare(source + [dict(kind="completed")], destination,
                            [150_000_000, 250_000_000], source)
        self.assertEqual(result["classification"], "verified")
        self.assertEqual(result["interruption_boundary_deliveries"], 1)
        # Delivery while the endpoint is down cannot count as a successful fault.
        destination[1]["wall_ns"] = 240_000_000
        self.assertEqual(io.compare(source + [dict(kind="completed")], destination,
                                   [150_000_000, 250_000_000], source)["classification"],
                         "transport_failure")
        # A missing frame outside the interruption must still fail transport.
        self.assertEqual(io.compare(source + [dict(kind="completed")], destination[1:],
                                   [150_000_000, 250_000_000], source)["classification"],
                         "transport_failure")

    def test_unexpected_termination_is_a_source_failure(self):
        source = [dict(kind="armed", count=4), frame(1), frame(2), dict(kind="stopped")]
        self.assertEqual(io.compare(source, source[1:3])["classification"], "source_failure")
        source[-1]["intentional_stop"] = True
        self.assertEqual(io.compare(source, source[1:3])["classification"], "verified")

    def test_actual_frames_required(self):
        source = [dict(kind="armed", count=1), frame(1), dict(kind="completed")]
        self.assertEqual(io.compare(source, [])["classification"], "transport_failure")

    def test_duplicate_and_reordering_rejected(self):
        source = [frame(1), frame(2), dict(kind="completed")]
        for dest in [[frame(1), frame(1), frame(2)], [frame(2), frame(1)]]:
            self.assertEqual(
                io.compare(source, dest)["classification"], "transport_failure"
            )

    def test_complete_delivery(self):
        frames = [frame(1), frame(2)]
        self.assertEqual(
            io.compare(frames + [dict(kind="completed")], frames)["classification"],
            "verified",
        )

    def test_dead_source_is_not_link_fault(self):
        self.assertEqual(io.compare([frame(1)], [])["classification"], "source_failure")

    def test_late_source_is_not_guardian_fault(self):
        f = frame(1)
        f["elapsed_ns"] += 21_000_000
        self.assertEqual(
            io.compare([f, dict(kind="completed")], [f])["classification"],
            "source_failure",
        )

    def test_independent_link_loss(self):
        source = [frame(n) for n in range(6)]
        dest = [source[0], source[1], source[4], source[5]]
        self.assertEqual(
            io.compare(
                source + [dict(kind="completed")], dest, (200_000_000, 400_000_000)
            )["classification"],
            "verified",
        )
        self.assertEqual(
            io.compare(
                source + [dict(kind="completed")], [], (200_000_000, 400_000_000)
            )["classification"],
            "transport_failure",
        )

    def test_source_capture_must_confirm_send(self):
        self.assertEqual(
            io.compare(
                [frame(1), dict(kind="completed")], [frame(1)], observed_source=[]
            )["classification"],
            "source_failure",
        )

    def test_missing_capture_is_retained_as_incomplete(self):
        with tempfile.TemporaryDirectory() as d:
            output = Path(d) / "can-path.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).parents[1] / "can_io.py"),
                    "compare",
                    "--run-id",
                    "campaign/scenario",
                    "--source",
                    d + "/absent",
                    "--destination",
                    d + "/absent",
                    "--output",
                    str(output),
                ],
                capture_output=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertEqual(
                json.loads(output.read_text())["classification"], "evidence_incomplete"
            )

    def test_trace_strictness(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "bad.asc"
            p.write_text("0.1 1 500 Rx d 8 00 01\n")
            with self.assertRaises(ValueError):
                list(io.frames(p))

    def test_all_shared_traces_parse(self):
        for path in (Path(__file__).parents[4] / "campaign/traces").glob("*.asc"):
            self.assertTrue(list(io.frames(path)), path.name)


if __name__ == "__main__":
    unittest.main()
