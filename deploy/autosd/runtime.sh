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

# Lifecycle adapter for the shared campaign runner; no scenarios or verdicts.
set -euo pipefail
root=${AUTOSD_GUEST_DIR:?AUTOSD_GUEST_DIR is required}
[[ "$root" =~ ^/[A-Za-z0-9_./-]+$ && "$root" != / ]] || exit 2
busybox=$root/busybox
ank=/opt/ota-ankaios-1.0.4/ank

stop_stack() {
    # A source-shutdown or interrupted diagnostics pause may leave frozen tasks.
    for name in guardian publisher dfm gateway can-provider databroker zenoh; do
        podman unpause "ota-autosd-$name" >/dev/null 2>&1 || true
    done
    if systemctl is-active --quiet ota-ankaios-server; then
        # Ankaios 1.0.4 evaluates its user config path even with CLI flags.
        [[ -n "${HOME:-}" ]] || { echo 'Ankaios needs a login environment; start the guest campaign with systemd-run --uid=root.' >&2; return 1; }
        "$ank" -k --no-wait delete workload thermal-stack >/dev/null || return 1
    fi
    # The CLI updates desired state; Podman removal is asynchronous. Keep
    # Ankaios alive until its agent has finished deleting the workload.
    for attempt in $(seq 1 90); do
        if ! podman pod exists ota-autosd; then
            systemctl stop ota-ankaios-agent ota-ankaios-server
            return 0
        fi
        sleep 1
    done
    echo 'Ankaios did not remove the workload within 90 seconds.' >&2
    return 1
}

start_stack() {
    echo 'Waiting for KUKSA port availability, then starting Ankaios...'
    for attempt in $(seq 1 75); do
        [[ -z "$(ss -tanH '( sport = :55555 )')" ]] && break
        sleep 1
    done
    [[ -z "$(ss -tanH '( sport = :55555 )')" ]] || { echo 'KUKSA port still occupied.' >&2; exit 1; }
    systemctl start ota-ankaios-server ota-ankaios-agent
    for attempt in $(seq 1 90); do
        count=$(podman ps --filter pod=ota-autosd --filter name=ota-autosd- --filter status=running --format '{{.Names}}' | wc -l)
        if [[ "$count" -eq 7 ]] && "$busybox" nc -z -w 1 127.0.0.1 17447 &&
            curl --max-time 2 -fs http://127.0.0.1:17690/sovd/v1/apps/battery_guardian/faults >/dev/null; then return 0; fi
        sleep 1
    done
    echo 'AutoSD workload did not become ready.' >&2; return 1
}

container() {
    case "$1" in
        opensovd-dfm) echo ota-autosd-dfm ;;
        opensovd-gateway) echo ota-autosd-gateway ;;
        kuksa-can-provider) echo ota-autosd-can-provider ;;
        kuksa-databroker) echo ota-autosd-databroker ;;
        vss-publisher) echo ota-autosd-publisher ;;
        guardian|zenoh) echo "ota-autosd-$1" ;;
        *) echo "Unknown service: $1" >&2; return 1 ;;
    esac
}

operation=$1
shift
case "$operation" in
    capabilities) exit 0 ;; # The pod uses host networking: no per-service isolation.
    prepare)
        evidence=$1
        trace=$2
        # Check deployment compatibility before interrupting a running stack.
        "$busybox" grep -q '.campaign-active' "$root/pod.yaml" || {
            echo 'Refresh the AutoSD deployment with make up SKIP_IMAGES=1 first.' >&2; exit 1;
        }
        "$busybox" cmp "$root/safety-params.toml" config/guardian/safety-params.toml
        "$busybox" cmp "$root/catalog/battery_guardian.json" diagnostics/catalog/battery_guardian.json
        cp "$root/can/BMS_MSG1_CAN.asc" "$evidence/original-source.asc"
        echo 'Restarting the Ankaios workload for the campaign trace...'
        stop_stack
        if [[ -n "$trace" ]]; then cp "$trace" "$root/can/BMS_MSG1_CAN.asc"; fi
        rm -f "$root/can/.campaign-source" "$root/can/.campaign-guardian"
        touch "$root/can/.campaign-active"
        chcon -R -t container_file_t -l s0 "$root/can"
        ;;
    start)
        case "$1" in
            kuksa-can-provider) touch "$root/can/.campaign-source" ;;
            guardian) touch "$root/can/.campaign-guardian" ;;
            *) start_stack ;; # Shared runner starts the prerequisite services together.
        esac
        ;;
    pause|resume|stop)
        verb=pause
        [[ "$operation" != resume ]] || verb=unpause
        # Stop means no further output for this scenario; pause avoids pod recreation.
        names=()
        for service in "$@"; do names+=("$(container "$service")"); done
        podman "$verb" "${names[@]}"
        ;;
    logs)
        follow=()
        if [[ "${1:-}" == --follow ]]; then follow=(--follow); shift; fi
        if [[ $# -gt 0 ]]; then exec podman logs "${follow[@]}" "$(container "$1")"; else
            for name in guardian publisher dfm gateway can-provider databroker zenoh; do
                podman logs "ota-autosd-$name" 2>&1 || true
            done
        fi
        ;;
    finish)
        evidence=$1
        [[ -f "$evidence/original-source.asc" ]] || exit 0
        echo 'Restoring the original AutoSD replay...'
        stopped=0
        stop_stack || stopped=$?
        cp "$evidence/original-source.asc" "$root/can/BMS_MSG1_CAN.asc"
        rm -f "$root/can/.campaign-active" "$root/can/.campaign-source" "$root/can/.campaign-guardian"
        chcon -R -t container_file_t -l s0 "$root/can"
        [[ "$stopped" -eq 0 ]] || exit "$stopped"
        start_stack
        echo 'Original AutoSD workload restored and ready.'
        ;;
    *) echo "Unsupported runtime operation: $operation" >&2; exit 2 ;;
esac
