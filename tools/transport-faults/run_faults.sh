#!/usr/bin/env bash
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
#
# AI-assisted: Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5)
#
# Runs the real Guardian service against `fault_send` for each delivery fault
# and prints what the Guardian reported and how long it took.
#
#   cargo build -p guardian-service && (cd tools/transport-faults && cargo build --bins)
#   tools/transport-faults/run_faults.sh [fault ...]
set -euo pipefail

TARGET="${CARGO_TARGET_DIR:-$HOME/ota-outlaws/vss-publisher/target}/debug"
PORT="${PORT:-7457}"
START_S="${START_S:-6}"; DURATION_S="${DURATION_S:-4}"; TOTAL_S="${TOTAL_S:-16}"
DELAY_MS="${DELAY_MS:-1000}"
OUT="${OUT:-/tmp/tf-faults}"
FAULTS=("$@"); [[ ${#FAULTS[@]} -eq 0 ]] && FAULTS=(none duplicate reorder drop delay)
mkdir -p "$OUT"
cd "$(git rev-parse --show-toplevel)"

cleanup() { docker rm -f tf-zenoh >/dev/null 2>&1 || true; pkill -x guardian 2>/dev/null || true; }
trap cleanup EXIT
cleanup
docker run -d --rm --name tf-zenoh -p "127.0.0.1:$PORT:7447" docker.io/eclipse/zenoh:1.10.1 >/dev/null
sleep 3

for fault in "${FAULTS[@]}"; do
  glog="$OUT/guardian-$fault.log"; ilog="$OUT/inject-$fault.jsonl"; rm -f "$glog" "$ilog"
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" RUST_LOG=guardian_service=info "$TARGET/guardian" >"$glog" 2>&1 &
  gpid=$!
  sleep 3
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" "$TARGET/fault_send" --fault "$fault" \
    --start-s "$START_S" --duration-s "$DURATION_S" --total-s "$TOTAL_S" \
    --delay-ms "$DELAY_MS" --correlation-id "F-$fault" --log "$ilog" 2>/dev/null
  kill "$gpid" 2>/dev/null || true; wait "$gpid" 2>/dev/null || true
  python3 - "$fault" "$glog" "$ilog" "$DELAY_MS" <<'PY'
import sys, re, json
from datetime import datetime, timezone
fault, glog, ilog, delay = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
log = [json.loads(l) for l in open(ilog)]
start = next((e for e in log if e["event"] == "fault_start"), None)
end = next(e for e in log if e["event"] == "run_end")
stream_end = end["wall_ms"] - delay - 1500
inject = start["wall_ms"] if start else None
events = []
for line in open(glog):
    m = re.search(r"^(\S+)Z .*guardian event id=(\d+) cause=(\S+) at_ms=\d+ kind=(\w+) \{?(.*)$", re.sub(r"\x1b\[[0-9;]*m", "", line))
    if not m: continue
    t = int(datetime.fromisoformat(m.group(1) + "+00:00").timestamp() * 1000)
    if t > stream_end: continue          # the sender has stopped: ignore the freshness fault after the run
    detail = re.sub(r"\s+", " ", m.group(5))[:90]
    events.append((t, m.group(4), detail))
print(f"== {fault}" + (f" (injected at {inject})" if inject else " (no fault)"))
if not events: print("   Guardian reported no events")
for t, kind, detail in events:
    rel = f"{t - inject:+6d} ms" if inject else "        "
    print(f"   {rel}  {kind:22s} {detail}")
PY
done
