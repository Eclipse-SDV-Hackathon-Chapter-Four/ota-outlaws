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
# AI-assisted: Codex / GPT-6 (gpt-6); Claude Code / Claude Opus 5.5 (claude-opus-5-5)

# Run on the host from deploy/autosd: make up SKIP_IMAGES=1
# Service definitions stay in pod.yaml; Ankaios is the only lifecycle owner.
.DEFAULT_GOAL := help
SHELL := /bin/bash

REPO_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/../..)
CAMPAIGNS ?= --all
CAMPAIGN_OUT ?= $(REPO_DIR)/runs/autosd

PYTHON ?= $(shell command -v python3.14 || command -v python3.13 || command -v python3.12 || command -v python3.11 || command -v python3)
CONFIG ?= bench/local.toml
CONTROLLER_PORT ?= 18100
SCENARIOS ?= normal timeout invalid_quality source_dropout_replay can_link_interruption

AUTOSD_BENCH_DIR ?= $(abspath ../../../can-testbench)
AUTOSD_SSH_HOST ?= root@127.0.0.1
AUTOSD_SSH_PORT ?= $(shell jq -er '.peers[] | select(.name == "bench-beta") | .ssh_port' "$(AUTOSD_BENCH_DIR)/bench.json")
AUTOSD_SSH_KEY ?= $(AUTOSD_BENCH_DIR)/.local/id_ed25519
AUTOSD_KNOWN_HOSTS ?= $(AUTOSD_BENCH_DIR)/.local/known_hosts
AUTOSD_GUEST_DIR ?= /opt/ota-outlaws-autosd
AUTOSD_AGENT_NAME ?= bench-beta
DIAGNOSTICS_IMAGE ?= local/opensovd-demo-fork:verified
SKIP_IMAGES ?= 0
AUTOSD_CAN_MODE ?= replay
SERVICE ?= guardian
SOVD_PORT ?= 17690
ZENOH_PORT ?= 17447

# Pass legacy guest settings only to single-VM scripts. Bench commands use
# their own TOML/backend and production image defaults.
DEPLOY_SETTINGS = AUTOSD_BENCH_DIR AUTOSD_SSH_HOST AUTOSD_SSH_PORT AUTOSD_SSH_KEY \
    AUTOSD_KNOWN_HOSTS AUTOSD_GUEST_DIR AUTOSD_AGENT_NAME DIAGNOSTICS_IMAGE SKIP_IMAGES \
    CAMPAIGNS CAMPAIGN_OUT AUTOSD_CAN_MODE
DEPLOY_ENV = $(foreach setting,$(DEPLOY_SETTINGS),$(setting)="$($(setting))")
SSH = ssh -p "$(AUTOSD_SSH_PORT)" -i "$(AUTOSD_SSH_KEY)" \
    -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5 \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$(AUTOSD_KNOWN_HOSTS)"

.PHONY: help up down status logs connect campaigns remote-doctor remote-run remote-cleanup remote-test dashboard-controller \
    backend-up backend-config backend-status backend-logs backend-down backend-destroy
help:
	@echo 'Local openDuT 0.10.2 backend (Docker Compose) for the two-peer bench:'
	@echo 'backend-up      Start CARL/Keycloak/NetBird and write backend settings to CONFIG'
	@echo 'backend-config  Rewrite backend settings (CA, EDGAR digest, CLEO) into CONFIG'
	@echo 'backend-status  Show backend containers; backend-logs [BACKEND_SERVICES=carl]'
	@echo 'backend-down    Stop the backend, keeping secrets and data'
	@echo 'backend-destroy Remove the backend, its volumes and local secrets'
	@echo ''
	@echo 'Two-peer openDuT bench (configure bench/local.toml):'
	@echo 'remote-doctor  Verify host prerequisites and reusable artifact hashes'
	@echo 'remote-run     Create fresh peers and run SCENARIOS; empty selects all supported cases'
	@echo 'remote-cleanup Retry interrupted cleanup: STATE=/absolute/path/to/run'
	@echo 'remote-test    Check CAN evidence, clocks and local configuration'
	@echo 'dashboard-controller Serve the local AutoSD/openDuT dashboard adapter'
	@echo ''
	@echo 'Single-VM AutoSD deployment:'
	@echo 'up       Transfer updates, install missing Ankaios, start and wait for readiness'
	@echo '         Use SKIP_IMAGES=1 to reuse guest application images'
	@echo 'down     Cancel active campaigns, then stop the workload and Ankaios'
	@echo 'status   Show Ankaios workload and container states'
	@echo 'logs     Show the last 100 lines; select SERVICE=guardian (default)'
	@echo 'connect  Forward OpenSOVD and Zenoh until Ctrl-C'
	@echo 'campaigns Run the shared Rust campaign CLI inside the Ankaios-managed AutoSD guest'
	@echo '          Select catalogue IDs, e.g. CAMPAIGNS="timeout invalid_quality"; default --all'

# ss includes TIME_WAIT sockets; probing a listener alone misses KUKSA rebind failures.
up:
	@$(DEPLOY_ENV) ./transfer.sh
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

# Wait for campaign source restoration before removing the Ankaios workload.
down:
	@echo 'Cancelling active campaigns and waiting for source restoration...'
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'set -eu; \
	    while read -r campaign rest; do \
	        [ -n "$$campaign" ] || continue; \
	        echo "Cancelling $$campaign; waiting for cleanup..."; \
	        systemctl kill --kill-whom=main --signal=SIGTERM "$$campaign" 2>/dev/null || true; \
	        finished=0; \
	        for attempt in $$(seq 1 330); do \
	            state=$$(systemctl show "$$campaign" --property=ActiveState --value 2>/dev/null || true); \
	            case "$$state" in active|activating|deactivating) sleep 1 ;; \
	                *) finished=1; break ;; esac; \
	        done; \
	        if [ "$$finished" -ne 1 ]; then \
	            echo "Campaign cleanup is still running. Inspect $$campaign before retrying down." >&2; exit 1; \
	        fi; \
	    done < <(systemctl list-units "ota-campaign-*" --state=active,activating,deactivating \
	        --no-legend --plain --no-pager); \
	    echo "Stopping the Ankaios workload, if present..."; \
	    if systemctl is-active --quiet ota-ankaios-server.service; then \
	        /opt/ota-ankaios-1.0.4/ank -k delete workload thermal-stack >/dev/null; \
	    fi; \
	    for attempt in $$(seq 1 90); do \
	        podman pod exists ota-autosd || break; sleep 1; \
	    done; \
	    if podman pod exists ota-autosd; then \
	        echo "Ankaios has not finished removing ota-autosd; its agent remains running." >&2; exit 1; \
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
	    campaigns=$$(systemctl list-units "ota-campaign-*" --state=active,activating,deactivating --no-legend --plain --no-pager); \
	    if [ -n "$$campaigns" ]; then \
	        echo "Active AutoSD campaign; the stack may be restarting or restoring:"; \
	        printf "%s\n" "$$campaigns"; \
	        echo "Use make campaigns to follow progress and wait for restoration."; \
	    fi; \
	    if systemctl is-active --quiet ota-ankaios-server.service; then \
	        /opt/ota-ankaios-1.0.4/ank -k get workloads; \
	    else echo "Ankaios server stopped"; fi; \
	    podman ps -a --filter pod=ota-autosd --filter name=ota-autosd- \
	        --format "table {{.Names}}\t{{.Status}}"'

logs:
	@case "$(SERVICE)" in guardian|publisher|dfm|gateway|can-provider|databroker|zenoh) ;; \
	    *) echo 'Unknown SERVICE; see images.tsv for service names.' >&2; exit 1 ;; esac
	@$(SSH) "$(AUTOSD_SSH_HOST)" 'set -eu; \
	    if ! podman container exists ota-autosd-$(SERVICE); then \
	        campaigns=$$(systemctl list-units "ota-campaign-*" --state=active,activating,deactivating --no-legend --plain --no-pager); \
	        if [ -n "$$campaigns" ]; then \
	            echo "$(SERVICE) container is temporarily absent during the active campaign restart/restoration."; \
	            echo "Use make campaigns to follow progress; service logs resume when the stack is ready."; exit 0; \
	        fi; \
	        echo "No $(SERVICE) container is present. Use make status to inspect the stack." >&2; exit 1; \
	    fi; \
	    podman logs --tail 100 ota-autosd-$(SERVICE)'

# Forwards bind to host loopback. Existing forwards need different local ports.
connect:
	@echo 'OpenSOVD: http://127.0.0.1:$(SOVD_PORT)/sovd/v1/apps/battery_guardian/faults'
	@echo 'Zenoh: tcp/127.0.0.1:$(ZENOH_PORT) (Ctrl-C closes both forwards)'
	@$(SSH) -o ExitOnForwardFailure=yes -N \
	    -L '127.0.0.1:$(SOVD_PORT):127.0.0.1:17690' \
	    -L '127.0.0.1:$(ZENOH_PORT):127.0.0.1:17447' \
	    "$(AUTOSD_SSH_HOST)"

# Fault replay and verification run on the deployed AutoSD guest.
campaigns:
	@$(DEPLOY_ENV) ./campaigns.sh

# The shared controller owns fresh peers, bench resources and final teardown.
remote-doctor:
	$(PYTHON) bench/controller.py doctor --config "$(CONFIG)"

remote-run:
	$(PYTHON) bench/controller.py run --config "$(CONFIG)" $(SCENARIOS)

remote-cleanup:
	@test -n "$(STATE)" || { echo 'STATE must name the existing run directory'; exit 2; }
	$(PYTHON) bench/controller.py cleanup --config "$(CONFIG)" --state "$(STATE)"

# openDuT's own localenv Compose deployment, pinned to 0.10.2.
backend-up backend-config backend-status backend-down backend-destroy:
	CONFIG="$(abspath $(CONFIG))" backend/backend.sh $(@:backend-%=%)

backend-logs:
	backend/backend.sh logs $(BACKEND_SERVICES)

remote-test:
	$(PYTHON) -m unittest discover -s bench/tests -v

# Runs on the native host, where QEMU can use HVF or KVM.
dashboard-controller:
	$(PYTHON) -u bench/dashboard.py --config "$(CONFIG)" --port "$(CONTROLLER_PORT)"
