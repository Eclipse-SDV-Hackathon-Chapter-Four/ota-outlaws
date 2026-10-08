#!/usr/bin/env python3
"""Run the OTA Outlaws campaign on Button A requests from the AZ3166."""

import argparse
import json
import logging
import re
import socket
import subprocess
import threading
import time
from pathlib import Path

CONSOLE_RESULT = re.compile(
    r"^(?:✓\s+PASS|✗\s+FAIL|\?\s+INCONCLUSIVE)\s+([A-Za-z0-9_-]+)\b"
)
LEGACY_RESULT = re.compile(
    r"^([A-Za-z0-9_-]+):\s+(Pass|Fail|Inconclusive)\s+[—-]"
)
SCENARIO_BLOCK = re.compile(
    r"(?ms)^\[\[scenario\]\]\s*(.*?)(?=^\[\[scenario\]\]|\Z)"
)
SCENARIO_ID = re.compile(r'(?m)^id\s*=\s*"([A-Za-z0-9_-]+)"\s*$')
STIMULUS_TYPE = re.compile(
    r'(?m)^stimulus\s*=\s*\{\s*type\s*=\s*"([A-Za-z0-9_-]+)"'
)
TRACE_PATH = re.compile(r'(?m)\btrace\s*=\s*"([^"]+\.asc)"')
ISOLATION_SERVICE = re.compile(r'\bisolate\s*=\s*"([^"]+)"')
MAX_REQUEST_ID = 0xFFFFFFFF
VERDICTS = {
    "Pass": "PASS",
    "Fail": "FAIL",
    "Inconclusive": "INCONCLUSIVE",
}


def campaign_scenario_total(campaign_dir):
    catalog_path = campaign_dir / "components" / "campaign" / "scenarios.toml"
    catalog = catalog_path.read_text(encoding="utf-8")
    blocks = SCENARIO_BLOCK.findall(catalog)
    if not blocks:
        raise ValueError("components/campaign/scenarios.toml contains no scenarios")

    total = 0
    for block in blocks:
        scenario_match = SCENARIO_ID.search(block)
        stimulus_match = STIMULUS_TYPE.search(block)
        if scenario_match is None or stimulus_match is None:
            raise ValueError("campaign catalog has a scenario without an id or stimulus")
        if stimulus_match.group(1) == "external":
            continue
        isolation_match = ISOLATION_SERVICE.search(block)
        if (
            isolation_match is not None
            and isolation_match.group(1) == "can-link"
        ):
            # The bridge invokes `campaign run --all` without a runtime hook;
            # the CLI's default runtime supports network isolation and watchdogs.
            continue

        total += 1
        for trace in TRACE_PATH.findall(block):
            trace_path = campaign_dir / trace
            if not trace_path.is_file():
                raise ValueError(
                    "campaign trace is missing: " + str(trace_path)
                )

    if total == 0:
        raise ValueError("campaign catalog has no runnable scenarios")
    return total


def decode_campaign_request(payload):
    try:
        message = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return None

    if not isinstance(message, dict) or message.get("type") != "campaign":
        return None
    request_id = message.get("id")
    if isinstance(request_id, bool) or not isinstance(request_id, int):
        raise ValueError("campaign request id must be an integer")
    if request_id < 0 or request_id > MAX_REQUEST_ID:
        raise ValueError("campaign request id is outside the uint32 range")
    return request_id


def parse_campaign_result(line):
    match = CONSOLE_RESULT.match(line)
    if match is not None:
        if line.startswith("✓"):
            return match.group(1), "PASS"
        if line.startswith("✗"):
            return match.group(1), "FAIL"
        return match.group(1), "INCONCLUSIVE"

    match = LEGACY_RESULT.match(line)
    if match is None:
        return None
    return match.group(1), VERDICTS[match.group(2)]


def send_message(sock, address, message):
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    sock.sendto(payload, address)


def run_campaign(
    sock, address, request_id, campaign_dir, scenario_total, display_interval, lock
):
    # No --out of its own: the campaign tool writes to runs/<timestamp>/, where the
    # dashboard looks for it. An own directory would nest the run one level deeper.
    command = [
        "cargo",
        "run",
        "--manifest-path",
        str(campaign_dir / "Cargo.toml"),
        "-p",
        "campaign",
        "--",
        "run",
        "--all",
    ]

    try:
        logging.info("Starting full campaign for request %d", request_id)
        process = subprocess.Popen(
            command,
            cwd=str(campaign_dir),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            bufsize=1,
        )
        result_count = 0
        for line in process.stdout:
            line = line.rstrip("\r\n")
            if line:
                logging.info("campaign: %s", line)
            result = parse_campaign_result(line)
            if result is None:
                continue

            scenario, verdict = result
            result_count += 1
            if result_count > scenario_total:
                raise RuntimeError(
                    "campaign printed more results than catalog scenarios"
                )
            send_message(
                sock,
                address,
                {
                    "type": "campaign_result",
                    "id": request_id,
                    "n": result_count,
                    "total": scenario_total,
                    "scenario": scenario,
                    "verdict": verdict,
                },
            )
            logging.info("Request %d: %s %s", request_id, scenario, verdict)
            time.sleep(display_interval)

        return_code = process.wait()
        if return_code not in (0, 1):
            raise RuntimeError(
                "campaign command exited with status " + str(return_code)
            )
        if result_count != scenario_total:
            raise RuntimeError(
                "campaign produced {} of {} scenario results".format(
                    result_count, scenario_total
                )
            )

        send_message(sock, address, {"type": "campaign_complete", "id": request_id})
        logging.info("Campaign request %d complete", request_id)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        logging.exception("Campaign request %d failed", request_id)
        try:
            send_message(
                sock,
                address,
                {"type": "campaign_error", "id": request_id},
            )
        except OSError:
            logging.exception("Could not return campaign error to the board")
    finally:
        lock.release()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--campaign-dir",
        required=True,
        type=Path,
        help="path to the ota-outlaws repository root",
    )
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=30502)
    parser.add_argument(
        "--display-interval",
        type=float,
        default=2.0,
        help="seconds to leave each scenario verdict on the OLED",
    )
    args = parser.parse_args()

    campaign_dir = args.campaign_dir.expanduser().resolve()
    if not (campaign_dir / "Cargo.toml").is_file():
        parser.error("--campaign-dir must contain Cargo.toml")
    try:
        scenario_total = campaign_scenario_total(campaign_dir)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    if args.port < 1 or args.port > 65535:
        parser.error("--port must be between 1 and 65535")
    if args.display_interval < 0.5:
        parser.error("--display-interval must be at least 0.5 seconds")

    lock = threading.Lock()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((args.host, args.port))
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s %(levelname)s %(message)s",
        )
        logging.info(
            "Listening on %s:%d for AZ3166 campaign requests (%d scenarios)",
            args.host,
            args.port,
            scenario_total,
        )
        while True:
            payload, address = sock.recvfrom(2048)
            logging.info("Received %d bytes from %s:%d: %s", len(payload), address[0], address[1],
                         payload[:100].decode("utf-8", "replace"))
            try:
                request_id = decode_campaign_request(payload)
            except ValueError:
                logging.warning("Rejected malformed campaign request from %s", address)
                continue
            if request_id is None:
                continue

            if not lock.acquire(False):
                send_message(
                    sock,
                    address,
                    {"type": "campaign_error", "id": request_id},
                )
                logging.warning("Rejected overlapping campaign request %d", request_id)
                continue

            worker = threading.Thread(
                target=run_campaign,
                args=(
                    sock,
                    address,
                    request_id,
                    campaign_dir,
                    scenario_total,
                    args.display_interval,
                    lock,
                ),
                daemon=True,
            )
            try:
                send_message(
                    sock,
                    address,
                    {"type": "campaign_ack", "id": request_id},
                )
                worker.start()
            except (OSError, RuntimeError):
                lock.release()
                logging.exception("Could not start campaign worker")
                try:
                    send_message(
                        sock,
                        address,
                        {"type": "campaign_error", "id": request_id},
                    )
                except OSError:
                    logging.exception("Could not return campaign startup error to the board")


if __name__ == "__main__":
    main()
