import json
import socket
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from campaign_bridge import (
    campaign_scenario_total,
    decode_campaign_request,
    parse_campaign_result,
    run_campaign,
)


class CampaignBridgeTests(unittest.TestCase):
    def test_decodes_campaign_request(self):
        self.assertEqual(
            decode_campaign_request(b'{"type":"campaign","id":42}'),
            42,
        )

    def test_ignores_non_campaign_messages(self):
        self.assertIsNone(decode_campaign_request(b'{"type":"sample","seq":42}'))

    def test_rejects_invalid_campaign_request_id(self):
        with self.assertRaises(ValueError):
            decode_campaign_request(b'{"type":"campaign","id":-1}')

    def test_parses_campaign_stdout_verdicts(self):
        self.assertEqual(
            parse_campaign_result(
                "✓ PASS          counter_stuck [TS-07] 4/4 checks met"
            ),
            ("counter_stuck", "PASS"),
        )
        self.assertEqual(
            parse_campaign_result(
                "✗ FAIL          timeout [TS-05, TS-15] 2/4 checks met"
            ),
            ("timeout", "FAIL"),
        )
        self.assertEqual(
            parse_campaign_result(
                "? INCONCLUSIVE  quality_single_invalid [TS-16] 1/4 checks met"
            ),
            ("quality_single_invalid", "INCONCLUSIVE"),
        )
        self.assertEqual(
            parse_campaign_result("counter_stuck: Pass — legacy output"),
            ("counter_stuck", "PASS"),
        )
        self.assertIsNone(parse_campaign_result("running counter_stuck …"))

    def test_counts_runnable_catalog_scenarios_and_checks_traces(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            campaign_dir = Path(temp_dir)
            catalog_dir = campaign_dir / "campaign"
            trace_dir = catalog_dir / "traces"
            trace_dir.mkdir(parents=True)
            (trace_dir / "latest.asc").write_text("trace", encoding="utf-8")
            (catalog_dir / "scenarios.toml").write_text(
                '[[scenario]]\n'
                'id = "first"\n'
                'stimulus = { type = "can_trace", trace = "campaign/traces/latest.asc" }\n'
                '\n'
                '[[scenario]]\n'
                'id = "external_only"\n'
                'stimulus = { type = "external" }\n'
                '\n'
                '[[scenario]]\n'
                'id = "second"\n'
                'stimulus = { type = "no_source", duration_ms = 1000 }\n'
                '\n'
                '[[scenario]]\n'
                'id = "network_isolation"\n'
                'stimulus = { type = "can_trace", trace = "campaign/traces/latest.asc", isolate = "guardian" }\n'
                '\n'
                '[[scenario]]\n'
                'id = "can_link_isolation"\n'
                'stimulus = { type = "can_trace", trace = "campaign/traces/latest.asc", isolate = "can-link" }\n',
                encoding="utf-8",
            )

            self.assertEqual(campaign_scenario_total(campaign_dir), 3)

            (trace_dir / "latest.asc").unlink()
            with self.assertRaisesRegex(ValueError, "campaign trace is missing"):
                campaign_scenario_total(campaign_dir)

    def test_sends_campaign_result_before_process_finishes(self):
        class FakeProcess:
            returncode = 1

            def __init__(self, receiver):
                self.stdout = self.lines(receiver)

            @staticmethod
            def lines(receiver):
                yield "quality_single_invalid: Inconclusive — evidence missing\n"
                receiver.settimeout(1)
                payload, _ = receiver.recvfrom(1024)
                result = json.loads(payload.decode("utf-8"))
                if result != {
                    "type": "campaign_result",
                    "id": 42,
                    "n": 1,
                    "total": 1,
                    "scenario": "quality_single_invalid",
                    "verdict": "INCONCLUSIVE",
                }:
                    raise AssertionError("campaign result was not sent immediately")

            def wait(self):
                return self.returncode

        def start_fake_campaign(command, cwd, **kwargs):
            self.assertIn("--all", command)
            return FakeProcess(receiver)

        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
            receiver.bind(("127.0.0.1", 0))
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                lock = threading.Lock()
                lock.acquire()
                with patch(
                    "campaign_bridge.subprocess.Popen",
                    side_effect=start_fake_campaign,
                ):
                    run_campaign(
                        sender,
                        receiver.getsockname(),
                        42,
                        Path("."),
                        1,
                        0.5,
                        lock,
                    )

            payload, _ = receiver.recvfrom(1024)
            self.assertEqual(
                json.loads(payload.decode("utf-8")),
                {"type": "campaign_complete", "id": 42},
            )


if __name__ == "__main__":
    unittest.main()
