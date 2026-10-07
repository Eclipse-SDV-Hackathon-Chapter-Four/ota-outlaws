#
# Contributors:
# Uttarkar Sopan - Feature enhancements and maintenance
# Microsoft Copilot - AI-assisted modifications
#
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from ota_outlaws_bridge import (
    ALL_FAULTS_SCENARIO,
    COUNTER_PATH,
    FAULT_REPLAY_GAP_SECONDS,
    FAULT_SCENARIOS,
    QUALITY_PATH,
    TEMPERATURE_PATHS,
    Bridge,
    ComposeReplay,
    decode_message,
    temperature_updates,
    trace_duration_seconds,
)


class DecodeMessageTests(unittest.TestCase):
    def test_fault_scenarios_match_campaign_traces_only(self):
        self.assertEqual(list(FAULT_SCENARIOS.values()), [
            "heating.asc",
            "invalid_during_warning.asc",
            "max_stuck.asc",
            "spike.asc",
        ])

    def test_decodes_normal_sample(self):
        message = decode_message(
            b'{"type":"sample","seq":9,"temperature_c":23.75,"timestamp_ms":1234}'
        )
        self.assertEqual(message, {
            "type": "sample",
            "seq": 9,
            "temperature_c": 23.75,
        })

    def test_decodes_one_shot_bad_sample(self):
        message = decode_message(
            b'{"type":"bad_sample","seq":10,"temperature_c":120}'
        )
        self.assertEqual(message, {
            "type": "bad_sample",
            "seq": 10,
            "temperature_c": 120.0,
        })

    def test_rejects_bad_sample_without_valid_temperature(self):
        for temperature in (None, float("nan"), 126, -41):
            with self.subTest(temperature=temperature):
                with self.assertRaises(ValueError):
                    decode_message(json.dumps({
                        "type": "bad_sample",
                        "seq": 10,
                        "temperature_c": temperature,
                    }).encode())

    def test_accepts_only_supported_fault_scenarios(self):
        for scenario in tuple(FAULT_SCENARIOS) + (ALL_FAULTS_SCENARIO,):
            with self.subTest(scenario=scenario):
                message = decode_message(
                    json.dumps({"type": "can_fault", "scenario": scenario}).encode()
                )
                self.assertEqual(message["scenario"], scenario)
        with self.assertRaises(ValueError):
            decode_message(b'{"type":"can_fault","scenario":"../../unknown.asc"}')

    def test_rejects_non_finite_or_out_of_range_temperature(self):
        for temperature in (float("nan"), 126, -41):
            with self.subTest(temperature=temperature):
                with self.assertRaises(ValueError):
                    decode_message(json.dumps({
                        "type": "sample",
                        "seq": 1,
                        "temperature_c": temperature,
                    }).encode())

    def test_rejects_invalid_sequence(self):
        with self.assertRaises(ValueError):
            decode_message(b'{"type":"sample","seq":-1,"temperature_c":20}')


class TemperatureUpdateTests(unittest.TestCase):
    def test_maps_sample_to_guardian_bms_paths(self):
        updates = temperature_updates(120.0, 258)
        self.assertEqual([updates[path] for path in TEMPERATURE_PATHS], [120.0] * 3)
        self.assertEqual(updates[QUALITY_PATH], 128)
        self.assertEqual(updates[COUNTER_PATH], 2)


class BridgeTests(unittest.TestCase):
    class FakeWriter:
        def __init__(self):
            self.updates = []

        def write_temperature(self, temperature_c, sequence):
            self.updates.append((temperature_c, sequence))

    class FakeReplay:
        def __init__(self):
            import threading
            self.active = threading.Event()
            self.scenarios = []

        def replay(self, scenario):
            self.scenarios.append(scenario)

    def setUp(self):
        self.writer = self.FakeWriter()
        self.replay = self.FakeReplay()
        self.bridge = Bridge(self.writer, self.replay)

    def test_forwards_normal_sample(self):
        self.bridge.handle(
            b'{"type":"sample","seq":3,"temperature_c":24.5}',
            ("127.0.0.1", 1234),
        )
        self.assertEqual(self.writer.updates, [(24.5, 3)])

    def test_injects_one_shot_high_sample(self):
        self.bridge.handle(
            b'{"type":"bad_sample","seq":4,"temperature_c":119.5}',
            ("127.0.0.1", 1234),
        )
        self.assertEqual(self.writer.updates, [(119.5, 4)])

    def test_starts_only_validated_can_scenario(self):
        self.bridge.handle(
            b'{"type":"can_fault","scenario":"heating"}',
            ("127.0.0.1", 1234),
        )
        self.assertEqual(self.replay.scenarios, ["heating"])

    def test_starts_all_campaign_fault_scenarios(self):
        self.bridge.handle(
            b'{"type":"can_fault","scenario":"all"}',
            ("127.0.0.1", 1234),
        )
        self.assertEqual(self.replay.scenarios, ["all"])

    def test_suppresses_board_sample_during_replay(self):
        self.replay.active.set()
        self.bridge.handle(
            b'{"type":"sample","seq":5,"temperature_c":24.5}',
            ("127.0.0.1", 1234),
        )
        self.assertEqual(self.writer.updates, [])


class ComposeReplayTests(unittest.TestCase):
    def test_replays_all_fault_traces_fully_with_five_second_gaps(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            compose_dir = os.path.join(temp_dir, "compose")
            fault_dir = os.path.join(temp_dir, "faults")
            os.makedirs(compose_dir)
            os.makedirs(fault_dir)
            for filename in FAULT_SCENARIOS.values():
                with open(os.path.join(fault_dir, filename), "w", encoding="ascii") as trace:
                    trace.write(
                        "date Wed Oct 07 09:00:00 2026\n"
                        "0.000000 1 500 Rx d 8 28 00 18 00 20 00 80 00\n"
                        "1.000000 1 500 Rx d 8 28 00 18 00 20 00 80 01\n"
                        "End TriggerBlock\n"
                    )

            replay = ComposeReplay(
                Path(compose_dir), Path(fault_dir), manage_provider=False
            )
            replay._compose = mock.Mock(
                side_effect=[
                    SimpleNamespace(stdout="container-{}".format(index))
                    for index in range(len(FAULT_SCENARIOS))
                ]
            )

            with mock.patch(
                "ota_outlaws_bridge.subprocess.run",
                return_value=SimpleNamespace(stdout="0", returncode=1, stderr=""),
            ) as run, mock.patch(
                "ota_outlaws_bridge.time.monotonic", return_value=0.0
            ), mock.patch("ota_outlaws_bridge.time.sleep") as sleep:
                replay.replay(ALL_FAULTS_SCENARIO)

            self.assertEqual(replay._compose.call_count, len(FAULT_SCENARIOS))
            self.assertEqual(
                [call.args[-1] for call in replay._compose.call_args_list],
                ["/faults/{}".format(name) for name in FAULT_SCENARIOS.values()],
            )
            self.assertEqual(run.call_count, len(FAULT_SCENARIOS) * 2)
            self.assertEqual(
                [call.kwargs.get("timeout") for call in run.call_args_list[::2]],
                [1.1] * len(FAULT_SCENARIOS),
            )
            self.assertEqual(sleep.call_args_list, [
                mock.call(1.1),
                mock.call(FAULT_REPLAY_GAP_SECONDS),
                mock.call(1.1),
                mock.call(FAULT_REPLAY_GAP_SECONDS),
                mock.call(1.1),
                mock.call(FAULT_REPLAY_GAP_SECONDS),
                mock.call(1.1),
            ])

    def test_trace_duration_includes_last_frame_period(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            trace = Path(temp_dir) / "trace.asc"
            trace.write_text(
                "0.000000 1 500 Rx d 8 00 00 00 00 00 00 80 00\n"
                "2.500000 1 500 Rx d 8 00 00 00 00 00 00 80 01\n",
                encoding="ascii",
            )
            self.assertAlmostEqual(trace_duration_seconds(trace), 2.6)


if __name__ == "__main__":
    unittest.main()
