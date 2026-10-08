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
# AI-assisted: Codex / GPT-6 (gpt-6)
# Shared Rust runtime adapter, running on B. A owns stimulus only.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=${AUTOSD_GUEST_DIR:?}
state=$root/.bench-state
single=$here/runtime-single.sh
source_host=${AUTOSD_SOURCE_HOST:?}
source_dir=${AUTOSD_SOURCE_DIR:?}
[[ "$source_host" =~ ^root@[0-9.]+$ && "$source_dir" =~ ^/[A-Za-z0-9_./-]+$ ]] || exit 2
source_ssh=(ssh -i "$here/source-key" -o BatchMode=yes -o IdentitiesOnly=yes
    -o StrictHostKeyChecking=yes -o "UserKnownHostsFile=$here/source-known-hosts"
    -o ControlMaster=auto -o ControlPersist=60 -o "ControlPath=$here/source-ssh"
    -o ConnectTimeout=3 -o ServerAliveInterval=3 -o ServerAliveCountMax=2 "$source_host")
a() { "${source_ssh[@]}" "$@"; }
stop_source() {
    a "rm -f '$source_dir/muted' '$source_dir/epoch' '$source_dir/epoch.next'; if systemctl is-active --quiet ota-bench-source.service; then systemctl stop ota-bench-source.service; fi" >/dev/null
}
operation=$1; shift
case "$operation" in
    capabilities) printf 'distributed-source\ncan-link-isolation\n' ;;
    prepare)
        evidence=$1; trace=$2
        mkdir -p "$state"
        printf '%s\n' "$evidence" > "$state/evidence"
        printf '%s\n' "$trace" > "$state/trace"
        printf '%s/%s\n' "$(basename "$(dirname "$evidence")")" "$(basename "$evidence")" > "$state/run-id"
        a "rm -f '$source_dir/source-can.jsonl' '$source_dir/source-observed-can.jsonl' '$source_dir/muted' '$source_dir/epoch' '$source_dir/intended-stop'"
        rm -f "$state/link-down" "$state/link-interval.json" "$state/epoch"
        test -f "$root/can/.socketcan"
        # The single-guest hook still owns app lifecycle and restoration; leave
        # its original trace intact. No ASC becomes the DUT provider's input.
        "$single" prepare "$evidence" ''
        cp "$here/bench-identities.json" "$evidence/bench-identities.json"
        podman image inspect $(podman images --format '{{.Repository}}:{{.Tag}}' | grep '^localhost/ota-autosd/') > "$evidence/runtime-images.json"
        ip -details link show vcan0 > "$evidence/destination-link.txt"
        a "ip -details link show vcan0; systemctl is-active opendut-edgar" > "$evidence/source-link.txt"
        if [[ -n "$trace" ]]; then
            scp -q -i "$here/source-key" -o BatchMode=yes -o StrictHostKeyChecking=yes \
                -o ControlMaster=auto -o ControlPersist=60 -o "ControlPath=$here/source-ssh" \
                -o "UserKnownHostsFile=$here/source-known-hosts" "$trace" "$source_host:$source_dir/trace.asc"
        fi
        ;;
    start)
        if [[ "$1" == kuksa-can-provider ]]; then exit 0; fi
        "$single" start "$@"
        ;;
    ready)
        "$single" start kuksa-can-provider
        # Inspect the actual receiver process: its stdout may be buffered.
        # B never receives a path to the measured trace.
        evidence=$(cat "$state/evidence")
        # Resolve the fresh container before Guardian starts. Podman follow
        # polls journald and can consume most of the 300 ms startup budget;
        # Short journal reads flush the same real stdout promptly.
        # Scope to the container ID so logs from an earlier scenario cannot
        # satisfy the shared runner's readiness check.
        rm -f "$state/guardian-journal-id"
        if [[ "$(podman inspect ota-autosd-guardian --format '{{.HostConfig.LogConfig.Type}}')" == journald ]]; then
            podman inspect ota-autosd-guardian --format '{{.Id}}' > "$state/guardian-journal-id"
        fi
        python3 "$here/clock_io.py" --source "${source_host#root@}" > "$evidence/clock-synchronization.json" || {
            echo 'Peer clock quality failed; inspect clock-synchronization.json.' >&2; exit 1;
        }
        podman inspect ota-autosd-can-provider > "$evidence/provider.json"
        test -f "$root/can/.socketcan"
        for attempt in $(seq 1 100); do
            podman top ota-autosd-can-provider args > "$evidence/provider-processes.txt"
            if grep -E '^\./dbcfeeder .*--use-socketcan.*--canport vcan0' "$evidence/provider-processes.txt" >/dev/null &&
                podman logs ota-autosd-publisher 2>&1 | grep -F 'uProtocol transport ready' >/dev/null; then break; fi
            sleep .1
        done
        grep -E '^\./dbcfeeder .*--use-socketcan.*--canport vcan0' "$evidence/provider-processes.txt" >/dev/null || {
            echo 'DUT SocketCAN receiver did not become ready.' >&2; exit 1;
        }
        podman logs ota-autosd-publisher 2>&1 | grep -F 'uProtocol transport ready' >/dev/null || {
            echo 'Publisher transport did not become ready; inspect its service logs.' >&2; exit 1;
        }
        # Arm the receiver before starting Guardian; no battery frame is used
        # for readiness, so startup does not create false freshness/counter faults.
        systemd-run --quiet --collect --unit=ota-bench-destination --property=RuntimeMaxSec=900 \
            python3 "$here/can_io.py" capture --run-id "$(cat "$state/run-id")" --output "$evidence/destination-can.jsonl"
        for attempt in $(seq 1 100); do
            [[ -s "$evidence/destination-can.jsonl" ]] && break
            sleep .01
        done
        test -s "$evidence/destination-can.jsonl"
        a "systemd-run --quiet --collect --unit=ota-bench-source-capture --property=RuntimeMaxSec=900 python3 '$source_dir/can_io.py' capture --run-id '$(cat "$state/run-id")' --output '$source_dir/source-observed-can.jsonl'; for attempt in \$(seq 1 100); do test ! -s '$source_dir/source-observed-can.jsonl' || exit 0; sleep .01; done; exit 1"
        if [[ -n "$(cat "$state/trace")" ]]; then
            a "systemd-run --quiet --collect --unit=ota-bench-source --property=RuntimeMaxSec=900 --property=CPUSchedulingPolicy=fifo --property=CPUSchedulingPriority=10 python3 '$source_dir/can_io.py' replay --trace '$source_dir/trace.asc' --mute-file '$source_dir/muted' --stop-file '$source_dir/intended-stop' --epoch-file '$source_dir/epoch' --run-id '$(cat "$state/run-id")' --output '$source_dir/source-can.jsonl'; for attempt in \$(seq 1 100); do test ! -s '$source_dir/source-can.jsonl' || exit 0; sleep .01; done; exit 1"
            a "systemctl show ota-bench-source --property=CPUSchedulingPolicy --property=CPUSchedulingPriority" > "$evidence/source-scheduler.txt"
        fi
        ;;
    measure)
        has_trace=0
        [[ -z "$(cat "$state/trace")" ]] || has_trace=1
        # Choose the deadline on A after SSH command startup, so the outbound
        # trip cannot consume the arming lead before the source is released.
        before=$EPOCHREALTIME
        epoch=$(a "python3 - '$source_dir' '$has_trace' <<'PYARM'
import sys, time
from pathlib import Path
root = Path(sys.argv[1])
epoch = time.time_ns() // 1_000_000 + 150
if sys.argv[2] == '1':
    pending = root / 'epoch.next'
    pending.write_text(str(epoch) + '\\n')
    pending.replace(root / 'epoch')
else:
    (root / 'source-can.jsonl').write_text('{\"kind\":\"completed\"}\\n')
print(epoch, flush=True)
PYARM")
        printf '{"before_s":"%s","returned_s":"%s","epoch_ms":%s}\n' \
            "$before" "$EPOCHREALTIME" "$epoch" > "$(cat "$state/evidence")/source-arming.json"
        printf '%s\n' "$epoch" > "$state/epoch"
        printf '%s\n' "$epoch"
        ;;
    pause|resume|stop)
        # Provider shutdown in the shared catalog is a source shutdown on A,
        # preserving B's decoder for independent source/transport attribution.
        local_services=()
        for service in "$@"; do
            if [[ "$service" == kuksa-can-provider ]]; then
                case "$operation" in
                    stop) a "touch '$source_dir/intended-stop'"; stop_source ;;
                    pause) a "touch '$source_dir/muted'" ;;
                    resume) a "rm -f '$source_dir/muted'" ;;
                esac
            else local_services+=("$service"); fi
        done
        [[ ${#local_services[@]} == 0 ]] || "$single" "$operation" "${local_services[@]}"
        ;;
    disconnect|connect)
        [[ "$1" == can-link ]] || { echo 'Application network isolation unsupported' >&2; exit 2; }
        # Fault intervention on an openDuT-managed endpoint. Never create an
        # alternate tunnel, bridge or route. EDGAR keeps bench ownership.
        # Timestamp inside one process around the state change. Launching a
        # timestamp helper after `ip` would extend the recorded loss window by
        # that helper's startup time and misclassify recovered boundary frames.
        python3 - "$state" "$operation" <<'PYLINK'
import json, subprocess, sys, time
from pathlib import Path
s=Path(sys.argv[1]); operation=sys.argv[2]
before=time.time_ns()
subprocess.run(["ip", "link", "set", "vcan0", "down" if operation == "disconnect" else "up"], check=True)
after=time.time_ns()
if operation == "disconnect":
    (s/'link-down').write_text(str(after))
    (s/'link-transition.json').write_text(json.dumps(dict(down_before_ns=before, down_after_ns=after)))
else:
    transition=json.loads((s/'link-transition.json').read_text())
    transition.update(up_before_ns=before, up_after_ns=after)
    (s/'link-transition.json').write_text(json.dumps(transition))
    (s/'link-interval.json').write_text(json.dumps([int((s/'link-down').read_text()), before]))
PYLINK
        ;;
    logs)
        if [[ "${1:-}" == --follow && "${2:-}" == guardian && -s "$state/guardian-journal-id" ]]; then
            # A followed journal stream can buffer stdout to a pipe. Read the
            # actual fresh-container records in short, completed reads so the
            # readiness line is flushed promptly before scheduling the source.
            id=$(cat "$state/guardian-journal-id")
            while true; do
                journalctl --all --no-pager --output=cat --lines=all "CONTAINER_ID_FULL=$id"
                sleep .01
            done
        fi
        exec "$single" logs "$@"
        ;;
    finish)
        evidence=$1
        # All restoration steps run even if collection or another cleanup fails.
        result=0
        stop_source || result=1
        a "rm -f '$source_dir/intended-stop'" || result=1
        a "if systemctl is-active --quiet ota-bench-source-capture.service; then systemctl stop ota-bench-source-capture.service; fi" || result=1
        ip link set vcan0 up || result=1
        systemctl stop ota-bench-destination.service || result=1
        if [[ -f "$state/epoch" ]]; then
            scp -q -i "$here/source-key" -o BatchMode=yes -o StrictHostKeyChecking=yes \
                -o ControlMaster=auto -o ControlPersist=60 -o "ControlPath=$here/source-ssh" \
                -o "UserKnownHostsFile=$here/source-known-hosts" \
                "$source_host:$source_dir/source-can.jsonl" "$source_host:$source_dir/source-observed-can.jsonl" "$evidence/" || result=1
            if [[ -f "$state/link-interval.json" ]]; then cp "$state/link-interval.json" "$state/link-transition.json" "$evidence/"; fi
            python3 "$here/can_io.py" compare --run-id "$(cat "$state/run-id")" \
                --source "$evidence/source-can.jsonl" --source-capture "$evidence/source-observed-can.jsonl" --destination "$evidence/destination-can.jsonl" \
                --interruption "$evidence/link-interval.json" --output "$evidence/can-path.json" || result=1
        fi
        podman inspect $(podman ps -a --filter pod=ota-autosd --format '{{.Names}}') > "$evidence/runtime-containers.json" || result=1
        a "journalctl -u ota-bench-source -u ota-bench-source-capture -u opendut-edgar --no-pager -n 200" > "$evidence/source-runtime.log" || result=1
        journalctl -u opendut-edgar --no-pager -n 200 > "$evidence/edgar.log" || result=1
        journalctl -u ota-ankaios-agent -u ota-ankaios-server --no-pager -n 200 > "$evidence/ankaios.log" || result=1
        "$single" finish "$evidence" || result=1
        # Original .socketcan is deployment configuration; replay remains off.
        rm -rf "$state"
        exit "$result"
        ;;
    *) echo "Unknown bench runtime operation: $operation" >&2; exit 2 ;;
esac
