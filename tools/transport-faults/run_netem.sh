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
# Injects transport faults on the Zenoh router's network interface with
# `tc netem` (inside Docker, no sudo) and measures what the receiver sees.
#
#   ./run_netem.sh            # needs: docker, cargo-built send/recv
set -euo pipefail

TARGET="${CARGO_TARGET_DIR:-$HOME/ota-outlaws/vss-publisher/target}/debug"
PORT="${PORT:-7457}"
COUNT="${COUNT:-50}"          # messages per scenario
INTERVAL_MS="${INTERVAL_MS:-100}"
OUT="${OUT:-/tmp/tf-results}"
mkdir -p "$OUT"

cleanup() {
  docker rm -f tf-tc tf-zenoh >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

docker run -d --rm --name tf-zenoh -p "127.0.0.1:$PORT:7447" docker.io/eclipse/zenoh:1.10.1 >/dev/null
docker run -d --rm --name tf-tc --cap-add NET_ADMIN --network container:tf-zenoh \
  docker.io/library/alpine:3 sh -c "apk add -q iproute2 && sleep infinity" >/dev/null
for _ in $(seq 1 30); do docker exec tf-tc sh -c "command -v tc" >/dev/null 2>&1 && break; sleep 1; done
sleep 2

run_scenario() {   # run_scenario <name> <netem args or "none">
  local name="$1" rule="$2"
  docker exec tf-tc sh -c "tc qdisc del dev eth0 root 2>/dev/null || true"
  if [[ "$rule" != "none" ]]; then
    docker exec tf-tc sh -c "tc qdisc add dev eth0 root netem $rule"
  fi
  local csv="$OUT/$name.csv"
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" SECONDS=$((COUNT * INTERVAL_MS / 1000 + 9)) \
    "$TARGET/recv" > "$csv" 2>/dev/null &
  local rpid=$!
  sleep 2
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" "$TARGET/send" "$COUNT" "$INTERVAL_MS" 2>/dev/null
  wait "$rpid" || true
  python3 - "$name" "$rule" "$csv" "$COUNT" <<'PY'
import sys, statistics
name, rule, path, count = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
rows = [tuple(map(int, l.split(","))) for l in open(path) if l.strip()]
ns = [r[0] for r in rows]
uniq = set(ns)
lat = [r[2] - r[1] for r in rows]
ooo = sum(1 for a, b in zip(ns, ns[1:]) if b < a)
print(f"{name:10s} rule='{rule}'")
print(f"  sent={count} received={len(rows)} unique={len(uniq)} "
      f"missing={count - len(uniq)} duplicates={len(rows) - len(uniq)} out_of_order={ooo}")
if lat:
    print(f"  latency ms: min={min(lat)} median={int(statistics.median(lat))} max={max(lat)}")
PY
}

run_blackhole() {   # run_blackhole <start s> <duration s>: 100% loss for a window
  local start="$1" dur="$2" csv="$OUT/blackhole.csv"
  docker exec tf-tc sh -c "tc qdisc del dev eth0 root 2>/dev/null || true"
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" SECONDS=$((COUNT * INTERVAL_MS / 1000 + 12)) \
    "$TARGET/recv" > "$csv" 2>/dev/null &
  local rpid=$!
  sleep 2
  ZENOH_CONNECT="tcp/127.0.0.1:$PORT" "$TARGET/send" "$COUNT" "$INTERVAL_MS" 2>/dev/null &
  local spid=$!
  sleep $((start + 2))
  docker exec tf-tc sh -c "tc qdisc add dev eth0 root netem loss 100%"
  sleep "$dur"
  docker exec tf-tc sh -c "tc qdisc del dev eth0 root"
  wait "$spid" || true
  wait "$rpid" || true
  python3 - "$csv" "$start" "$dur" <<'PY'
import sys
path, start, dur = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
rows = [tuple(map(int, l.split(","))) for l in open(path) if l.strip()]
rows.sort(key=lambda r: r[2])
print(f"blackhole  loss 100% from t={start}s for {dur}s")
print(f"  received={len(rows)} unique={len({r[0] for r in rows})}")
prev = None
for r in rows:
    if prev is not None and r[2] - prev[2] > 500:
        print(f"  gap in arrivals: {r[2]-prev[2]} ms between n={prev[0]} and n={r[0]}")
    prev = r
late = [r[2]-r[1] for r in rows]
print(f"  latency ms: min={min(late)} max={max(late)} (late arrivals after the window)")
PY
}

case "${1:-all}" in
  blackhole) run_blackhole 2 3 ;;
  *)
    run_scenario baseline  none
    run_scenario delay     "delay 200ms"
    run_scenario loss      "loss 20%"
    run_scenario duplicate "duplicate 50%"
    run_scenario jitter    "delay 100ms 80ms distribution normal"
    run_blackhole 2 3 ;;
esac
