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
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("clock_io", Path(__file__).parents[1] / "clock_io.py")
clock = importlib.util.module_from_spec(spec)
spec.loader.exec_module(clock)

SOURCE = "100.64.0.2"
TRACKING = "Reference ID : 64400002 (100.64.0.2)\nLeap status : Normal\nSystem time : 0.0002 seconds fast of NTP time\nRef time (UTC) : Thu Jan 01 00:00:00 2026\nSkew : 0.2 ppm\n"
PEER = "Remote address : 100.64.0.2 (64400002)\nLeap status : Normal\nTotal good RX : 8\nOffset : -0.0003 seconds\nPeer delay : 0.002 seconds\nPeer dispersion : 0.00001 seconds\nNTP tests : 111 111 1111\n"


class PeerClock(unittest.TestCase):
    def test_local_peer_bound(self):
        self.assertLess(clock.relative_bound(TRACKING, PEER, SOURCE, now=1767225601), 2_000_000)

    def test_network_uncertainty_is_not_hidden(self):
        peer = PEER.replace("0.002 seconds", "0.040 seconds")
        self.assertGreater(clock.relative_bound(TRACKING, peer, SOURCE, now=1767225601), 10_000_000)

    def test_stale_clock_sample_is_rejected(self):
        with self.assertRaises(ValueError):
            clock.relative_bound(TRACKING, PEER, SOURCE, now=1767225620)

    def test_frequency_uncertainty_is_counted(self):
        tracking = TRACKING.replace("0.2 ppm", "10000 ppm")
        self.assertGreater(clock.relative_bound(tracking, PEER, SOURCE, now=1767225601), 10_000_000)

    def test_unsynchronized_wrong_or_invalid_source_is_rejected(self):
        for tracking, peer in ((TRACKING.replace("Normal", "Not synchronised"), PEER),
                               (TRACKING, PEER.replace("100.64.0.2", "100.64.0.3")),
                               (TRACKING, PEER.replace("RX : 8", "RX : 0")),
                               (TRACKING, PEER.replace("1111", "1101")),
                               (TRACKING, PEER.replace("0.002 seconds", "nan seconds"))):
            with self.subTest(tracking=tracking, peer=peer), self.assertRaises(ValueError):
                clock.relative_bound(tracking, peer, SOURCE, now=1767225601)


if __name__ == "__main__":
    unittest.main()
