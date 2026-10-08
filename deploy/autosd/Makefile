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

# Run on the host: make -C deploy/autosd up SKIP_IMAGES=1
# Service definitions stay in pod.yaml; Ankaios is the only lifecycle owner.
.DEFAULT_GOAL := help
SHELL := /bin/bash

AUTOSD_BENCH_DIR ?= $(abspath ../../../can-testbench)
AUTOSD_SSH_HOST ?= root@127.0.0.1
AUTOSD_SSH_PORT ?= $(shell jq -er '.peers[] | select(.name == "bench-beta") | .ssh_port' "$(AUTOSD_BENCH_DIR)/bench.json")
AUTOSD_SSH_KEY ?= $(AUTOSD_BENCH_DIR)/.local/id_ed25519
AUTOSD_KNOWN_HOSTS ?= $(AUTOSD_BENCH_DIR)/.local/known_hosts
AUTOSD_GUEST_DIR ?= /opt/ota-outlaws-autosd
AUTOSD_AGENT_NAME ?= bench-beta
DIAGNOSTICS_IMAGE ?= local/opensovd-demo-fork:verified
SKIP_IMAGES ?= 0
SERVICE ?= guardian
SOVD_PORT ?= 17690
ZENOH_PORT ?= 17447

# Export target settings so transfer.sh and recursive make use the same guest.
export AUTOSD_BENCH_DIR AUTOSD_SSH_HOST AUTOSD_SSH_PORT AUTOSD_SSH_KEY
export AUTOSD_KNOWN_HOSTS AUTOSD_GUEST_DIR AUTOSD_AGENT_NAME DIAGNOSTICS_IMAGE SKIP_IMAGES
SSH = ssh -p "$(AUTOSD_SSH_PORT)" -i "$(AUTOSD_SSH_KEY)" \
    -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5 \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$(AUTOSD_KNOWN_HOSTS)"

.PHONY: help up down status logs connect
help:
	@echo 'up       Transfer updates, install missing Ankaios, start and wait for readiness'
	@echo '         Use SKIP_IMAGES=1 to reuse guest application images'
	@echo 'down     Stop the workload and Ankaios; retain images, storage and evidence'
	@echo 'status   Show Ankaios workload and container states'
	@echo 'logs     Show the last 100 lines; select SERVICE=guardian (default)'
	@echo 'connect  Forward OpenSOVD and Zenoh until Ctrl-C'

# ss includes TIME_WAIT sockets; probing a listener alone misses KUKSA rebind failures.
up:
	@./transfer.sh
	@echo 'Waiting for KUKSA port availability, then starting Ankaios...'
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'set -eu; \
	    for attempt in $$(seq 1 75); do \
	        [ -z "$$(ss -tanH "( sport = :55555 )")" ] && break; \
	        sleep 1; \
	    done; \
	    if [ -n "$$(ss -tanH "( sport = :55555 )")" ]; then \
	        echo "KUKSA port still has sockets; inspect guest port 55555." >&2; exit 1; \
	    fi; \
	    systemctl start ota-ankaios-server.service ota-ankaios-agent.service; \
	    for attempt in $$(seq 1 90); do \
	        count=$$(podman ps --filter pod=ota-autosd --filter name=ota-autosd- \
	            --filter status=running --format "{{.Names}}" | wc -l); \
	        if [ "$$count" -eq 7 ] && curl --max-time 2 -fs \
	            http://127.0.0.1:17690/sovd/v1/apps/battery_guardian/faults >/dev/null; then \
	            exit 0; \
	        fi; \
	        sleep 1; \
	    done; \
	    echo "Stack did not become ready. Use make status and make logs." >&2; exit 1'
	@echo 'AutoSD stack ready. Use make connect to access diagnostics and Zenoh.'

# Ask Ankaios to remove its workload before stopping its units. Repeated down is safe.
down:
	@echo 'Stopping the Ankaios workload, if present...'
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'set -eu; \
	    if systemctl is-active --quiet ota-ankaios-server.service; then \
	        /opt/ota-ankaios-1.0.4/ank -k delete workload thermal-stack >/dev/null; \
	    fi; \
	    for unit in ota-ankaios-agent.service ota-ankaios-server.service; do \
	        if systemctl cat "$$unit" >/dev/null 2>&1; then systemctl stop "$$unit"; fi; \
	    done; \
	    if podman pod exists ota-autosd; then \
	        echo "ota-autosd still exists. Inspect it before proceeding." >&2; exit 1; \
	    fi'
	@echo 'AutoSD stack stopped; images, storage and evidence retained.'

status:
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'set -eu; \
	    if systemctl is-active --quiet ota-ankaios-server.service; then \
	        /opt/ota-ankaios-1.0.4/ank -k get workloads; \
	    else echo "Ankaios server stopped"; fi; \
	    podman ps -a --filter pod=ota-autosd --filter name=ota-autosd- \
	        --format "table {{.Names}}\t{{.Status}}"'

logs:
	@case "$(SERVICE)" in guardian|publisher|dfm|gateway|can-provider|databroker|zenoh) ;; \
	    *) echo 'Unknown SERVICE; see images.tsv for service names.' >&2; exit 1 ;; esac
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'podman logs --tail 100 ota-autosd-$(SERVICE)'

# Forwards bind to host loopback. Existing forwards need different local ports.
connect:
	@echo 'OpenSOVD: http://127.0.0.1:$(SOVD_PORT)/sovd/v1/apps/battery_guardian/faults'
	@echo 'Zenoh: tcp/127.0.0.1:$(ZENOH_PORT) (Ctrl-C closes both forwards)'
	@$(SSH) -o ExitOnForwardFailure=yes -N \
	    -L '127.0.0.1:$(SOVD_PORT):127.0.0.1:17690' \
	    -L '127.0.0.1:$(ZENOH_PORT):127.0.0.1:17447' \
	    "$(AUTOSD_SSH_HOST)"
