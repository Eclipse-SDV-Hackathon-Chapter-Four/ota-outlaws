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

# Reconnect to an existing guest job, or start one with persistent guest logs.
set -euo pipefail
cd "$(dirname "$0")"
[[ "$AUTOSD_GUEST_DIR" =~ ^/[A-Za-z0-9_./-]+$ && "$AUTOSD_GUEST_DIR" != / ]] || {
    echo 'Invalid AUTOSD_GUEST_DIR.' >&2; exit 1;
}
[[ -n "$CAMPAIGNS" ]] || { echo 'Select a campaign or use --all.' >&2; exit 1; }
read -r -a scenarios <<< "$CAMPAIGNS"
# Scenario IDs come from the shared catalog, never a second AutoSD case list.
for scenario in "${scenarios[@]}"; do
    [[ "$scenario" == --all || "$scenario" =~ ^[a-z][a-z0-9_]*$ ]] || {
        echo "Invalid scenario: $scenario" >&2; exit 1;
    }
done
repo=$(cd ../.. && pwd)
options=(-i "$AUTOSD_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5
    -o StrictHostKeyChecking=yes -o "UserKnownHostsFile=$AUTOSD_KNOWN_HOSTS")
ssh_guest=(ssh -p "$AUTOSD_SSH_PORT" "${options[@]}" "$AUTOSD_SSH_HOST")
# Check for an existing job before checking workload readiness: its cleanup may
# currently have Ankaios stopped. Its lock remains owned until restoration ends.
active=$("${ssh_guest[@]}" "systemctl list-units 'ota-campaign-*' --state=active,activating,deactivating --no-legend --plain --no-pager")
unit=$(printf '%s\n' "$active" | awk '$1 ~ /^ota-campaign-[0-9]+-[0-9]+-[0-9]+\.service$/ { print $1; exit }')
if [[ -n "$unit" ]]; then
    run_id=${unit#ota-campaign-}
    run_id=${run_id%.service}
    echo "Reconnecting to AutoSD campaign $run_id; waiting for its results and restoration."
    launching=false
else
    run_id="$(date -u +%Y%m%d-%H%M%S)-$$"
    unit="ota-campaign-$run_id.service"
    launching=true
fi
remote="$AUTOSD_GUEST_DIR/campaign-reports/$run_id"
local_run="$CAMPAIGN_OUT/$run_id"
mkdir -p "$local_run"
if [[ "$launching" == true ]]; then
    "${ssh_guest[@]}" "command -v systemd-run >/dev/null && test -x '$AUTOSD_GUEST_DIR/busybox' && test -f '$AUTOSD_GUEST_DIR/can/BMS_MSG1_CAN.asc' && systemctl is-active --quiet ota-ankaios-server.service ota-ankaios-agent.service"
    # Build the same Rust campaign CLI for Linux ARM64. Docker is only the
    # host compiler; scenario execution and service lifecycle stay on AutoSD.
    work=$(mktemp -d)
    container=''
    cleanup() {
        if [[ -n "$container" ]]; then docker rm "$container" >/dev/null; fi
        rm -rf "$work"
    }
    trap cleanup EXIT
    docker build --platform linux/arm64 -f "$repo/components/campaign/Containerfile" \
        -t ota-outlaws/campaign:autosd "$repo"
    container=$(docker create --platform linux/arm64 ota-outlaws/campaign:autosd /campaign)
    docker cp "$container:/campaign" "$work/campaign-cli"
    docker rm "$container" >/dev/null
    container=''
    mkdir -p "$work/campaign-input/components/campaign" "$work/campaign-input/config/guardian" "$work/campaign-input/deploy/diagnostics/catalog"
    revision=$(git -C "$repo" rev-parse HEAD)
    if [[ -n "$(git -C "$repo" status --porcelain --untracked-files=no)" ]]; then revision="$revision-dirty"; fi
    printf '%s\n' "$revision" > "$work/campaign-input/git-revision.txt"
    cp runtime.sh "$work/campaign-input/runtime.sh"
    cp "$repo/components/campaign/scenarios.toml" "$work/campaign-input/components/campaign/"
    cp -R "$repo/components/campaign/traces" "$work/campaign-input/components/campaign/"
    cp "$repo/config/guardian/safety-params.toml" "$work/campaign-input/config/guardian/"
    cp "$repo/deploy/diagnostics/catalog/battery_guardian.json" "$work/campaign-input/deploy/diagnostics/catalog/"
    COPYFILE_DISABLE=1 tar -czf "$work/input.tar.gz" -C "$work/campaign-input" .
    "${ssh_guest[@]}" "mkdir -p '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id' '$remote'"
    scp -q -P "$AUTOSD_SSH_PORT" "${options[@]}" "$work/campaign-cli" "$work/input.tar.gz" \
        "$AUTOSD_SSH_HOST:$AUTOSD_GUEST_DIR/.campaign-tools/$run_id/"
    "${ssh_guest[@]}" "cp '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id/input.tar.gz' '$remote/input.tar.gz'; chmod +x '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id/campaign-cli'; '$AUTOSD_GUEST_DIR/busybox' tar -xzf '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id/input.tar.gz' -C '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id'"
    echo "Starting ${scenarios[*]} with the shared Rust runner on AutoSD through Ankaios."
    echo "Evidence: $local_run"
    # The process opens its own log; systemd cannot open these SELinux paths
    # using StandardOutput=append. Detached execution survives SSH disconnects.
    "${ssh_guest[@]}" "systemd-run --quiet --collect --uid=root --unit='$unit' --property=RuntimeMaxSec=3600 --property=TimeoutStopSec=330 /bin/bash -c \
        \"cd '$AUTOSD_GUEST_DIR/.campaign-tools/$run_id'; exec 9>'$AUTOSD_GUEST_DIR/.campaign.lock'; '$AUTOSD_GUEST_DIR/busybox' flock -n 9 || exit 1; export AUTOSD_GUEST_DIR='$AUTOSD_GUEST_DIR'; exec ./campaign-cli run --runtime-hook './runtime.sh' --zenoh tcp/127.0.0.1:17447 --sovd http://127.0.0.1:17690/sovd/v1 --no-build --run-id '$run_id' --out '$AUTOSD_GUEST_DIR/campaign-reports' ${scenarios[*]} > '$remote/run.log' 2>&1\""

fi
set +e
"${ssh_guest[@]}" "bash -s -- '$remote' '$unit' '$AUTOSD_GUEST_DIR/busybox'" <<'GUEST' | tee "$local_run/connection.log"
set -eu
report_dir=$1
unit=$2
busybox=$3
next=1
while :; do
    if [[ -f "$report_dir/run.log" ]]; then
        lines=$("$busybox" wc -l < "$report_dir/run.log")
        if (( lines >= next )); then
            "$busybox" sed -n "${next},${lines}p" "$report_dir/run.log"
            next=$((lines + 1))
        fi
    fi
    state=$(systemctl show "$unit" --property=ActiveState --value 2>/dev/null || true)
    case "$state" in active|activating|deactivating) sleep 1 ;;
        *) break ;;
    esac
done
if [[ ! -f "$report_dir/exit-code" ]]; then
    echo "Campaign ended before recording its exit status. Evidence: $report_dir" >&2
    journalctl -u "$unit" --no-pager -n 10 >&2
    exit 1
fi
exit "$(cat "$report_dir/exit-code")"
GUEST
result=${PIPESTATUS[0]}
set -e
# Preflight/lock failures may not produce evidence. Never attempt to archive a
# nonexistent directory or claim that remote evidence exists when it does not.
if ! "${ssh_guest[@]}" "test -d '$remote'"; then
    echo "No remote evidence directory was created. Connection log: $local_run/connection.log" >&2
    exit 1
fi
if ! "${ssh_guest[@]}" "'$AUTOSD_GUEST_DIR/busybox' tar -czf - -C '$AUTOSD_GUEST_DIR/campaign-reports' '$run_id'" | tar -xzf - -C "$CAMPAIGN_OUT"; then
    echo "Evidence copy failed; retry retrieval from $AUTOSD_SSH_HOST:$remote" >&2
    exit 1
fi
echo "Evidence: $local_run"
exit "$result"
