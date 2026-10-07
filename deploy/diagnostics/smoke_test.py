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

"""Replay Guardian campaigns against isolated DFM/OpenSOVD services.
Reports and failure logs are retained. The development stack is never cleared.
"""
import json
import os
from pathlib import Path
import subprocess
import time
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[2]
RUN = ROOT / "deploy" / "diagnostics" / "reports" / (time.strftime("%Y%m%d-%H%M%S") + "-" + uuid4().hex[:8])
RUN.mkdir(parents=True)
BASE_ENV = dict(os.environ, SOVD_PORT=os.environ.get("DIAGNOSTICS_TEST_PORT", "17690"))

def run_scenario(scenario, build=False):
    evidence = RUN / scenario
    evidence.mkdir()
    env = dict(BASE_ENV, SCENARIO=scenario, EVIDENCE_DIR=str(evidence))
    project = "ota-diagnostics-test-" + uuid4().hex[:8]
    command = ["docker", "compose", "-p", project, "-f", "deploy/docker-compose.yml", "-f", "deploy/diagnostics/compose.smoke.yml"]
    def compose(*args, check=True, **kw):
        return subprocess.run(command + list(args), cwd=ROOT, env=env, check=check, **kw)
    def wait_marker(name, process):
        deadline = time.monotonic() + 30
        while not (evidence / name).exists():
            if process.poll() is not None:
                raise AssertionError(f"Campaign exited before {name}; see {evidence}/campaign.log")
            if time.monotonic() > deadline:
                raise AssertionError(f"Campaign timed out waiting for {name}")
            time.sleep(0.05)
    process = None
    paused = False
    try:
        if build:
            compose("build", "diagnostics-campaign")
        compose("up", "-d", "--wait", "opensovd-dfm", "opensovd-gateway")
        with (evidence / "campaign.log").open("w") as log:
            process = subprocess.Popen(command + ["run", "--rm", "diagnostics-campaign"], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
            wait_marker("ready", process)
            if scenario == "outage":
                compose("pause", "opensovd-dfm", "opensovd-gateway")
                paused = True
                (evidence / "inject").touch()
                wait_marker("mitigated", process)
                # Keep diagnostics paused until detection AND recovery are queued.
                # The final diagnostic state must reflect Passed, not a late Failed.
                wait_marker("recovery-queued", process)
                compose("unpause", "opensovd-dfm", "opensovd-gateway")
                paused = False
                (evidence / "recovered").touch()
            code = process.wait(timeout=40)
            assert code == 0, f"Campaign exited {code}; see {evidence}/campaign.log"
        report = json.loads((evidence / "report.json").read_text())
        assert report["verdict"] == "PASS"
        print(f"PASS: {scenario}; evidence: {evidence}", flush=True)
    except Exception as error:
        # Never overwrite a failed campaign with a success from a later rerun.
        (evidence / "failure.json").write_text(json.dumps({"scenario": scenario, "verdict": "FAIL", "error": str(error)}, indent=2))
        raise
    finally:
        if paused:
            compose("unpause", "opensovd-dfm", "opensovd-gateway", check=False)
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        with (evidence / "services.log").open("w") as log:
            compose("logs", "--no-color", check=False, stdout=log, stderr=subprocess.STDOUT)
        compose("down", "-v", "--remove-orphans", check=False)

if __name__ == "__main__":
    failures = []
    for index, scenario in enumerate(["freshness", "stuck", "counter", "quality", "outage"]):
        try:
            run_scenario(scenario, build=index == 0 and not os.environ.get("SKIP_TEST_BUILD"))
        except Exception as error:
            failures.append(f"{scenario}: {error}")
            print(f"FAIL: {failures[-1]}", flush=True)
    print(f"Evidence retained at {RUN}", flush=True)
    if failures:
        raise SystemExit("\n".join(failures))
