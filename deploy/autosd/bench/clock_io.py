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
"""Validate relative guest time using the source peer as the NTP reference.

No scenario or safety verdict is implemented here.
"""
import argparse
import datetime
import ipaddress
import json
import math
import subprocess
import time


def fields(text):
    return {
        line.split(":", 1)[0].strip(): line.split(":", 1)[1].strip()
        for line in text.splitlines() if ":" in line
    }


def relative_bound(tracking, ntp_data, source, now=None):
    """Bound B's offset from A, excluding A's unrelated upstream UTC distance."""
    tracking = fields(tracking)
    peer = fields(ntp_data)
    if tracking["Leap status"] != "Normal" or peer["Leap status"] != "Normal":
        raise ValueError("Chrony is not synchronized")
    if "(" + source + ")" not in tracking["Reference ID"] or peer["Remote address"].split()[0] != source:
        raise ValueError("Chrony is not using this bench's source peer")
    if peer["NTP tests"].split() != ["111", "111", "1111"]:
        raise ValueError("Latest peer NTP packet was rejected by Chrony")
    if int(peer["Total good RX"]) < 1:
        raise ValueError("No valid peer clock samples")
    reference = datetime.datetime.strptime(tracking["Ref time (UTC)"], "%a %b %d %H:%M:%S %Y").replace(tzinfo=datetime.timezone.utc).timestamp()
    if now is None:
        now = datetime.datetime.now(datetime.timezone.utc).timestamp()
    age = now - reference
    if not 0 <= age <= 10:
        raise ValueError("Peer clock sample is stale or from the future")
    values = [float(tracking["System time"].split()[0]),
              float(peer["Offset"].split()[0]),
              float(peer["Peer delay"].split()[0]),
              float(peer["Peer dispersion"].split()[0]),
              float(tracking["Skew"].split()[0])]
    if not all(math.isfinite(value) for value in values):
        raise ValueError("Invalid clock sample")
    correction, offset, delay, dispersion, skew = values
    if delay < 0 or dispersion < 0 or skew < 0:
        raise ValueError("Invalid clock uncertainty")
    return math.ceil((abs(correction) + abs(offset) + delay / 2 + dispersion + age * skew / 1e6) * 1e9)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    args = parser.parse_args()
    ipaddress.IPv4Address(args.source)
    deadline = time.monotonic() + 10
    rejected = []
    while True:
        result = dict(source=args.source, synchronized=False)
        try:
            for key, command in (("tracking", ["tracking"]), ("ntp_data", ["ntpdata", args.source])):
                result[key] = subprocess.check_output(["chronyc", "-n", *command], text=True, timeout=5)
            result["relative_bound_ns"] = relative_bound(result["tracking"], result["ntp_data"], args.source)
            result["synchronized"] = result["relative_bound_ns"] <= 10_000_000
            if not result["synchronized"]:
                result["error"] = "Peer clock bound exceeds 10 ms"
        except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
            result["error"] = str(error)
        if result["synchronized"] or time.monotonic() >= deadline:
            break
        rejected.append(dict(error=result["error"], relative_bound_ns=result.get("relative_bound_ns")))
        time.sleep(.25)
    result["rejected_samples"] = rejected
    print(json.dumps(result, indent=2))
    return 0 if result["synchronized"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
