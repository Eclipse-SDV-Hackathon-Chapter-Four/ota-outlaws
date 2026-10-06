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
# AI-assisted: Codex / GPT-6.1 Sol (gpt-6.1-sol)

"""Verify diagnostics in an isolated, disposable Compose project."""
import json
import os
from pathlib import Path
import subprocess
import time
from urllib.request import Request, urlopen
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
PORT = os.environ.get("DIAGNOSTICS_TEST_PORT", "17690")
ENV = dict(os.environ, SOVD_PORT=PORT)
PROJECT = "ota-diagnostics-test-" + uuid4().hex[:8]
URL = f"http://127.0.0.1:{PORT}/sovd/v1/apps/battery_guardian/faults"
CODE = "BatteryOverTempWarning"
COMPOSE = ["docker", "compose", "-p", PROJECT, "-f", "docker-compose.yml", "-f", "diagnostics/compose.smoke.yml"]

def compose(*args):
    subprocess.run(COMPOSE + list(args), cwd=ROOT, env=ENV, check=True)

def request(url, method="GET"):
    with urlopen(Request(url, method=method), timeout=5) as response:
        raw = response.read()
        return json.loads(raw) if raw else None

def wait_for(active):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = request(URL)
        fault = next(item for item in value["items"] if item["code"] == CODE)
        if fault["status"]["testFailed"] == active:
            return fault
        time.sleep(0.25)
    raise AssertionError(f"Expected testFailed={active}; last response={value}")

def main():
    print(f"Isolated verification project: {PROJECT}", flush=True)
    try:
        compose("up", "-d", "--wait", "opensovd-dfm", "opensovd-gateway", "diagnostics-test-guardian")
        snapshot = request(URL)
        assert len(snapshot["items"]) == 5, snapshot
        compose("run", "--rm", "diagnostics-test-injector")
        failed = wait_for(True)
        print("Overtemperature recorded:", json.dumps(failed, sort_keys=True), flush=True)
        compose("stop", "diagnostics-test-guardian")
        compose("rm", "-f", "diagnostics-test-guardian")
        request(URL, "DELETE")
        wait_for(False)
        print("PASS: active fault cleared through OpenSOVD", flush=True)
        compose("up", "-d", "--force-recreate", "--wait", "opensovd-dfm", "opensovd-gateway")
        assert len(request(URL)["items"]) == 5
        print("PASS: real catalog, uProtocol -> Guardian -> DFM -> OpenSOVD, clear, and service recreation", flush=True)
        print("KNOWN LIMITATION: durable persistence is not asserted; the previous image failed a restart check", flush=True)
    except Exception:
        compose("logs", "--no-color", "--tail", "80", "opensovd-dfm", "opensovd-gateway", "diagnostics-test-guardian")
        raise
    finally:
        # Only remove the randomly named test project's containers/volumes.
        compose("down", "-v", "--remove-orphans")

if __name__ == "__main__":
    main()
