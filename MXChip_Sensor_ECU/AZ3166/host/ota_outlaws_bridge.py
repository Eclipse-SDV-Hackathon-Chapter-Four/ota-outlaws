#!/usr/bin/env python3
#
# Contributors:
# Uttarkar Sopan - Feature enhancements and maintenance
# Microsoft Copilot - AI-assisted modifications
#

"""Bridge AZ3166 UDP samples and button events into the ota-outlaws KUKSA stack."""

import argparse
import json
import logging
import math
import re
import socketserver
import subprocess
import threading
import time
from pathlib import Path

LOGGER = logging.getLogger("az3166-ota-bridge")
FAULT_SCENARIOS = {
    "heating": "heating.asc",
    "invalid_during_warning": "invalid_during_warning.asc",
    "max_stuck": "max_stuck.asc",
    "spike": "spike.asc",
}
ALL_FAULTS_SCENARIO = "all"
FAULT_REPLAY_GAP_SECONDS = 5
TRACE_FRAME_PERIOD_SECONDS = 0.1
TRACE_FRAME_PATTERN = re.compile(
    r"^\s*(\d+(?:\.\d+)?)\s+1\s+[0-9A-Fa-f]+\s+Rx\s+d\s+\d+"
)
TEMPERATURE_PATHS = (
    "Vehicle.Powertrain.TractionBattery.Temperature.Max",
    "Vehicle.Powertrain.TractionBattery.Temperature.Average",
    "Vehicle.Powertrain.TractionBattery.Temperature.Min",
)
QUALITY_PATH = "Vehicle.Powertrain.TractionBattery.BMS.SignalQuality"
COUNTER_PATH = "Vehicle.Powertrain.TractionBattery.BMS.AliveCounter"
MAX_DATAGRAM_LENGTH = 512


def decode_message(data):
    if len(data) > MAX_DATAGRAM_LENGTH:
        raise ValueError("datagram exceeds the maximum length")
    try:
        message = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("invalid UTF-8 JSON datagram: {}".format(error))
    if not isinstance(message, dict):
        raise ValueError("message must be a JSON object")

    message_type = message.get("type")
    if message_type == "sample":
        sequence = message.get("seq")
        temperature = message.get("temperature_c")
        if type(sequence) is not int or not 0 <= sequence <= 0xFFFFFFFF:
            raise ValueError("sample seq must be an unsigned 32-bit integer")
        if (type(temperature) not in (int, float) or not math.isfinite(temperature)
                or not -40.0 <= temperature <= 125.0):
            raise ValueError("sample temperature_c must be finite and in [-40, 125]")
        return {"type": "sample", "seq": sequence, "temperature_c": float(temperature)}

    if message_type == "bad_sample":
        sequence = message.get("seq")
        temperature = message.get("temperature_c")
        if type(sequence) is not int or not 0 <= sequence <= 0xFFFFFFFF:
            raise ValueError("bad_sample seq must be an unsigned 32-bit integer")
        if (type(temperature) not in (int, float) or not math.isfinite(temperature)
                or not -40.0 <= temperature <= 125.0):
            raise ValueError("bad_sample temperature_c must be finite and in [-40, 125]")
        return {
            "type": "bad_sample",
            "seq": sequence,
            "temperature_c": float(temperature),
        }

    if message_type == "can_fault":
        scenario = message.get("scenario")
        if (not isinstance(scenario, str) or
                (scenario != ALL_FAULTS_SCENARIO and scenario not in FAULT_SCENARIOS)):
            raise ValueError("unsupported CAN fault scenario: {!r}".format(scenario))
        return {"type": "can_fault", "scenario": scenario}

    raise ValueError("unsupported message type: {!r}".format(message_type))


def temperature_updates(temperature_c, sequence):
    updates = {path: temperature_c for path in TEMPERATURE_PATHS}
    updates[QUALITY_PATH] = 128
    updates[COUNTER_PATH] = sequence & 0xFF
    return updates


def trace_duration_seconds(trace):
    timestamps = []
    with trace.open("r", encoding="ascii") as trace_file:
        for line in trace_file:
            match = TRACE_FRAME_PATTERN.match(line)
            if match:
                timestamps.append(float(match.group(1)))

    if len(timestamps) < 2:
        raise ValueError("CAN trace has fewer than two timestamped frames: {}".format(trace))
    return timestamps[-1] - timestamps[0] + TRACE_FRAME_PERIOD_SECONDS


def ensure_success(response, operation):
    try:
        result = json.loads(response)
    except ValueError:
        if response == "OK":
            return
        raise RuntimeError(
            "KUKSA {} returned an unexpected response: {}".format(operation, response)
        )
    if isinstance(result, dict) and "error" in result:
        raise RuntimeError("KUKSA {} failed: {}".format(operation, result["error"]))


class KuksaWriter:
    def __init__(self, host, port):
        try:
            from kuksa_client import KuksaClientThread
        except ImportError as error:
            raise RuntimeError(
                "Install the host adapter dependency with "
                "`python -m pip install -r requirements.txt`"
            ) from error

        self.client = KuksaClientThread({
            "ip": host,
            "port": port,
            "protocol": "grpc",
            "insecure": True,
        })
        self.client.start()
        try:
            deadline = time.time() + 15
            while time.time() < deadline:
                if self.client.checkConnection():
                    return
                if not self.client.is_alive():
                    raise RuntimeError("KUKSA client stopped before connecting")
                time.sleep(0.1)
            raise RuntimeError("Timed out connecting to KUKSA")
        except Exception:
            self.client.stop()
            self.client.join(timeout=5)
            raise

    def write_temperature(self, temperature_c, sequence):
        response = self.client.setValues(
            temperature_updates(temperature_c, sequence),
            timeout=10,
        )
        ensure_success(response, "temperature update")

    def close(self):
        try:
            self.client.stop()
        finally:
            self.client.join(timeout=5)


class ComposeReplay:
    def __init__(self, compose_dir, fault_dir, manage_provider):
        self.compose_dir = compose_dir.resolve()
        self.fault_dir = fault_dir.resolve()
        self.manage_provider = manage_provider
        self._lock = threading.Lock()
        self.active = threading.Event()

        if not self.compose_dir.is_dir():
            raise ValueError("Compose directory does not exist: {}".format(self.compose_dir))
        if not self.fault_dir.is_dir():
            raise ValueError("Fault log directory does not exist: {}".format(self.fault_dir))

        if self.manage_provider:
            self._compose("stop", "kuksa-can-provider")
            LOGGER.info("Stopped the default CAN provider; the AZ3166 supplies normal samples")

    def _compose(self, *arguments, **kwargs):
        check = kwargs.pop("check", True)
        if kwargs:
            raise TypeError("unexpected Compose options: {}".format(sorted(kwargs)))
        return subprocess.run(
            ["docker", "compose"] + list(arguments),
            cwd=str(self.compose_dir),
            check=check,
            universal_newlines=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def replay(self, scenario):
        if scenario != ALL_FAULTS_SCENARIO and scenario not in FAULT_SCENARIOS:
            raise ValueError("unsupported CAN fault scenario: {!r}".format(scenario))
        if not self._lock.acquire(False):
            LOGGER.warning("Ignoring CAN replay request; a replay is already active")
            return

        self.active.set()
        try:
            scenarios = (tuple(FAULT_SCENARIOS) if scenario == ALL_FAULTS_SCENARIO
                         else (scenario,))
            traces = []
            for fault_scenario in scenarios:
                trace = self.fault_dir / FAULT_SCENARIOS[fault_scenario]
                if not trace.is_file():
                    raise IOError("fault trace does not exist: {}".format(trace))
                traces.append((fault_scenario, trace, trace_duration_seconds(trace)))

            for index, (fault_scenario, trace, duration) in enumerate(traces, 1):
                self._replay_trace(fault_scenario, trace, duration)
                if index < len(traces):
                    self.active.clear()
                    LOGGER.info(
                        "Waiting %d seconds before the next fault; live AZ3166 samples resume",
                        FAULT_REPLAY_GAP_SECONDS,
                    )
                    time.sleep(FAULT_REPLAY_GAP_SECONDS)
                    self.active.set()
        finally:
            self.active.clear()
            self._lock.release()

    def _replay_trace(self, scenario, trace, duration):
        filename = trace.name
        volume = "{}:/faults:ro".format(self.fault_dir.as_posix())
        LOGGER.info(
            "Replaying CAN fault scenario %s from %s for %.1f seconds",
            scenario, trace, duration,
        )
        replay_started = time.monotonic()
        result = self._compose(
            "run",
            "--rm",
            "--detach",
            "--no-deps",
            "--volume",
            volume,
            "kuksa-can-provider",
            "--dumpfile",
            "/faults/{}".format(filename),
        )
        container_id = result.stdout.strip()
        if not container_id:
            raise RuntimeError("Docker Compose did not return the replay container ID")

        replay_started = time.monotonic()
        try:
            try:
                wait_result = subprocess.run(
                    ["docker", "wait", container_id],
                    check=True,
                    universal_newlines=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=duration,
                )
                if wait_result.stdout.strip() != "0":
                    raise RuntimeError(
                        "CAN replay container exited with status {}".format(
                            wait_result.stdout.strip()
                        )
                    )
            except subprocess.TimeoutExpired:
                LOGGER.info(
                    "CAN fault %s reached its %.1f-second trace duration",
                    scenario, duration,
                )

            remaining = duration - (time.monotonic() - replay_started)
            if remaining > 0:
                time.sleep(remaining)
            LOGGER.info("Finished CAN fault scenario %s", scenario)
        finally:
            inspect_result = subprocess.run(
                ["docker", "inspect", container_id],
                check=False,
                universal_newlines=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            if inspect_result.returncode == 0:
                stop_result = subprocess.run(
                    ["docker", "stop", "--time", "2", container_id],
                    check=False,
                    universal_newlines=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                if stop_result.returncode != 0:
                    LOGGER.error(
                        "Could not stop CAN replay container %s: %s",
                        container_id,
                        stop_result.stderr.strip(),
                    )

    def close(self):
        if self.manage_provider:
            result = self._compose("start", "kuksa-can-provider", check=False)
            if result.returncode:
                LOGGER.error(
                    "Could not restart the normal CAN provider: %s",
                    result.stderr.strip(),
                )
            else:
                LOGGER.info("Restarted the normal CAN provider")


class Bridge:
    def __init__(self, writer, replay):
        self.writer = writer
        self.replay = replay

    def handle(self, data, address):
        try:
            message = decode_message(data)
        except ValueError as error:
            LOGGER.warning(
                "Rejected datagram from %s:%d: %s; payload=%r",
                address[0], address[1], error, data,
            )
            return

        message_type = message["type"]
        if message_type == "can_fault":
            try:
                self.replay.replay(message["scenario"])
            except (OSError, RuntimeError, subprocess.CalledProcessError, ValueError):
                LOGGER.exception("CAN replay failed")
            return

        if self.replay.active.is_set():
            LOGGER.info("Ignoring AZ3166 sample during CAN replay")
            return

        if message_type == "bad_sample":
            temperature = message["temperature_c"]
            sequence = message["seq"]
            LOGGER.warning("Injecting one-shot AZ3166 sample at %.1f C", temperature)
        else:
            temperature = message["temperature_c"]
            sequence = message["seq"]
            LOGGER.info("AZ3166 HTS221 sample %.2f C (sequence %u)", temperature, sequence)

        try:
            self.writer.write_temperature(temperature, sequence)
        except RuntimeError:
            LOGGER.exception("Could not update KUKSA with the AZ3166 temperature")


class UdpHandler(socketserver.BaseRequestHandler):
    def handle(self):
        data = self.request[0]
        self.server.bridge.handle(data, self.client_address)


class UdpServer(socketserver.ThreadingUDPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, address, bridge):
        self.bridge = bridge
        super().__init__(address, UdpHandler)


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", default="0.0.0.0", help="UDP bind address")
    parser.add_argument("--udp-port", type=int, default=30502)
    parser.add_argument("--kuksa-host", default="127.0.0.1")
    parser.add_argument("--kuksa-port", type=int, default=55556)
    parser.add_argument("--compose-dir", type=Path, required=True,
                        help="local ota-outlaws repository containing docker-compose.yml")
    parser.add_argument("--fault-dir", type=Path,
                        help="defaults to <compose-dir>/campaign/traces")
    parser.add_argument("--no-compose-control", action="store_true",
                        help="do not stop/start kuksa-can-provider; manage source exclusivity manually")
    return parser.parse_args()


def main():
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    args = parse_args()
    fault_dir = args.fault_dir or args.compose_dir / "campaign" / "traces"
    writer = KuksaWriter(args.kuksa_host, args.kuksa_port)
    try:
        replay = ComposeReplay(args.compose_dir, fault_dir, not args.no_compose_control)
    except Exception:
        writer.close()
        raise

    bridge = Bridge(writer, replay)
    server = None
    try:
        server = UdpServer((args.listen, args.udp_port), bridge)
        LOGGER.info(
            "Listening on UDP %s:%d; KUKSA gRPC at %s:%d",
            args.listen, args.udp_port, args.kuksa_host, args.kuksa_port,
        )
        with server:
            server.serve_forever()
    except KeyboardInterrupt:
        LOGGER.info("Stopping AZ3166 OTA Outlaws bridge")
    finally:
        if server is not None:
            server.server_close()
        try:
            replay.close()
        finally:
            writer.close()


if __name__ == "__main__":
    main()
