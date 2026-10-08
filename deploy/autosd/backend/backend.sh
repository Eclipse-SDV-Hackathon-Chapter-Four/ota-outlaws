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
# AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5)

# Local openDuT 0.10.2 backend for the two-peer AutoSD bench.
# Runs openDuT's own localenv Docker Compose deployment at the pinned release,
# with compose.override.yaml, and writes the bench's backend settings.
#
#   backend.sh up       provision secrets, start, wait, write backend settings
#   backend.sh config   rewrite backend settings into the bench config
#   backend.sh status   show containers
#   backend.sh logs [service...]
#   backend.sh down     stop containers, keep secrets and volumes
#   backend.sh destroy  remove containers, volumes and local secrets
#   backend.sh compose ...  any docker compose command on this deployment
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OPENDUT_VERSION=v0.10.2
OPENDUT_COMMIT=eb8d15df6a65719db4b77c4ef660695ce238cf03
OPENDUT_REPO=${OPENDUT_REPO:-https://github.com/eclipse-opendut/opendut.git}
STATE=${OPENDUT_BACKEND_STATE:-$HERE/.state}
SRC=${OPENDUT_SRC:-$STATE/opendut-$OPENDUT_VERSION}
LOCALENV=$SRC/.ci/deploy/localenv
SECRETS=$STATE/secrets
CONFIG=${CONFIG:-$HERE/../bench/local.toml}
EXAMPLE=$HERE/../bench/local.example.toml
# Domain names come from upstream .env.development.
DOMAIN=opendut.local
NAMES="$DOMAIN auth.$DOMAIN netbird.$DOMAIN netbird-api.$DOMAIN netbird-relay.$DOMAIN signal.$DOMAIN"
CLEO_CONTAINER=opendut-cleo
CLEO=/usr/local/bin/opendut-cleo
WAIT_SECONDS=${OPENDUT_BACKEND_WAIT:-1200}

die() { echo "backend: $*" >&2; exit 1; }

fetch_source() {
    if [[ ! -d $SRC/.git ]]; then
        mkdir -p "$STATE"
        git -c advice.detachedHead=false clone --quiet --depth 1 \
            --branch "$OPENDUT_VERSION" "$OPENDUT_REPO" "$SRC"
    fi
    local actual
    actual=$(git -C "$SRC" rev-parse HEAD)
    [[ $actual == "$OPENDUT_COMMIT" ]] \
        || die "$SRC is at $actual, expected openDuT $OPENDUT_VERSION ($OPENDUT_COMMIT)"
}

compose() {
    local env_files=(--env-file "$LOCALENV/.env.development")
    [[ -f $SECRETS/.env ]] && env_files+=(--env-file "$SECRETS/.env")
    # Shell values take precedence over the upstream env file. Upstream binds
    # the host path /provision, which Docker Desktop on macOS cannot share.
    mkdir -p "$STATE/shared-certs"
    OTA_BACKEND_DIR=$HERE \
    SHARED_CERTS_HOST_DIR=$STATE/shared-certs \
    OPENDUT_LOCALENV_TELEMETRY_ENABLED=0 \
        docker compose --project-name opendut \
            --file "$LOCALENV/docker-compose.yml" \
            --file "$HERE/compose.override.yaml" \
            "${env_files[@]}" "$@"
}

health() {
    docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$1" 2>/dev/null || echo missing
}

wait_ready() {
    local deadline=$((SECONDS + WAIT_SECONDS)) state
    # Keycloak's first start and realm provisioning take several minutes.
    while state=$(health opendut-carl); [[ $state != healthy ]]; do
        ((SECONDS < deadline)) || die "CARL not healthy after ${WAIT_SECONDS}s (state: $state); see: $0 logs carl keycloak init_keycloak netbird-management"
        echo "Waiting for CARL (keycloak: $(health opendut-keycloak), netbird: $(health opendut-netbird-management), carl: $state) ..."
        sleep 15
    done
    until docker exec "$CLEO_CONTAINER" "$CLEO" --version >/dev/null 2>&1; do
        ((SECONDS < deadline)) || die "CLEO not installed in $CLEO_CONTAINER; see: $0 logs cleo"
        sleep 5
    done
    # Authenticated round trip through Keycloak and CARL.
    docker exec "$CLEO_CONTAINER" "$CLEO" list peers >/dev/null \
        || die "CLEO cannot query CARL"
}

write_config() {
    local ca=$STATE/opendut-ca.pem edgar
    [[ -f $SECRETS/pki/opendut-ca.pem ]] || die "no provisioned CA; run: $0 up"
    cp "$SECRETS/pki/opendut-ca.pem" "$ca"
    # The bench verifies the EDGAR archive it downloads against this digest.
    edgar=$(curl --fail --silent --show-error --cacert "$ca" \
        --resolve "$DOMAIN:443:127.0.0.1" \
        "https://$DOMAIN/api/edgar/aarch64-unknown-linux-gnu/download" \
        | shasum -a 256 | cut -d' ' -f1)
    [[ ${#edgar} == 64 ]] || die "could not download EDGAR from CARL"
    [[ -f $CONFIG ]] || cp "$EXAMPLE" "$CONFIG"
    python3 - "$CONFIG" "https://$DOMAIN" "$ca" "$edgar" "$CLEO_CONTAINER" "$CLEO" "10.0.2.2 $NAMES" <<'PY'
import json, sys, tomllib
path, backend, ca, edgar, container, cleo, hosts = sys.argv[1:]
values = {
    "backend": json.dumps(backend),
    "ca": json.dumps(ca),
    "edgar_sha256": json.dumps(edgar),
    "cleo_command": json.dumps(["docker", "exec", "-i", container, cleo]),
    "backend_hosts": json.dumps(hosts),
}
lines = [line for line in open(path).read().splitlines()
         if line.split("=", 1)[0].strip().lstrip("# ") not in values]
lines += ["", "# Local openDuT backend, written by backend/backend.sh"]
lines += [f"{key} = {value}" for key, value in values.items()]
text = "\n".join(lines).strip() + "\n"
tomllib.loads(text)
open(path, "w").write(text)
PY
    echo "Backend settings written to $CONFIG (EDGAR sha256 $edgar)."
}

check_hosts() {
    local name ip missing=()
    for name in $NAMES; do
        ip=$(python3 -c 'import socket, sys; print(socket.gethostbyname(sys.argv[1]))' "$name" 2>/dev/null || true)
        [[ $ip == 127.0.0.1 ]] || missing+=("$name")
    done
    if ((${#missing[@]})); then
        echo "The bench host must resolve the backend names. Add them once with:"
        echo "  echo '127.0.0.1 $NAMES' | sudo tee -a /etc/hosts"
    fi
}

case ${1:-} in
    up)
        command -v docker >/dev/null || die "docker is required"
        fetch_source
        if [[ ! -f $SECRETS/.env ]]; then
            compose up --build provision-secrets
            rm -rf "$SECRETS"
            docker cp opendut-provision-secrets:/provision/ "$SECRETS"
            chmod -R go= "$SECRETS"
        fi
        compose up --detach --build
        wait_ready
        write_config
        check_hosts
        ;;
    config) write_config; check_hosts ;;
    status) fetch_source; compose ps --all ;;
    logs) shift; fetch_source; compose logs --tail 200 "$@" ;;
    compose) shift; fetch_source; compose "$@" ;;
    down) fetch_source; compose down ;;
    destroy)
        fetch_source
        compose down --volumes --remove-orphans
        rm -rf "$SECRETS" "$STATE/opendut-ca.pem" "$STATE/shared-certs"
        ;;
    *) sed -n '14,24p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
