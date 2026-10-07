import json
import socket
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from campaign_bridge import (
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
            parse_campaign_result("counter_stuck: Pass — control recovered"),
            ("counter_stuck", "PASS"),
        )
        self.assertEqual(
            parse_campaign_result("timeout: Fail — response exceeded budget"),
            ("timeout", "FAIL"),
        )
        self.assertEqual(
            parse_campaign_result("building campaign v0.1.0"),
            None,
        )

    def test_sends_campaign_result_before_process_finishes(self):
        class FakeProcess:
            returncode = 1

            def __init__(self, receiver):
                self.stdout = self.lines(receiver)

            @staticmethod
            def lines(receiver):
                yield "counter_stuck: Pass — control recovered\n"
                receiver.settimeout(1)
                payload, _ = receiver.recvfrom(1024)
                result = json.loads(payload.decode("utf-8"))
                if result.get("type") != "campaign_result":
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
