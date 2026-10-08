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
# Maintenance only: build/publish a new base outside PR jobs. Linux ARM64/KVM.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
: "${AIB_WRAPPER:?Path to the inspected AutoSD image-builder wrapper}"
: "${AIB_IMAGE:?Immutable automotive-image-builder image@sha256 digest}"
: "${AIR:?Path to the pinned AutoSD launcher}"
: "${BASE_VERSION:?Versioned release tag, e.g. autosd-can-base-v1}"
[[ "$AIB_IMAGE" == *@sha256:* && "$BASE_VERSION" =~ ^autosd-can-base-v[0-9]+$ ]] || exit 2
[[ -z "$(git -C "$repo" status --porcelain --untracked-files=all)" ]] || {
    echo 'Commit the base recipe before recording artifact provenance.' >&2; exit 2;
}
out=${BASE_OUT:?Absolute empty output directory}
[[ "$out" = /* && ! -e "$out" ]] || { echo 'BASE_OUT must be a new absolute directory' >&2; exit 2; }
mkdir -p "$out"
cp -R "$here/files" "$out/files"
cp "$here/peer.aib.yml" "$out/peer.aib.yml"
cp "$AIR" "$out/air"
cd "$out"
"$AIB_WRAPPER" -c "$AIB_IMAGE" -d build --build-dir /host/aib-build \
    --cache-max-size 16GB --target qemu --arch aarch64 peer.aib.yml peer-base.qcow2 2>&1 | tee build.log
# Keep the published base free of runtime identity/auth state. Each job injects
# its own key offline and boots an independent overlay.
virt-customize -a peer-base.qcow2 \
    --run-command 'rm -rf /etc/opendut /var/lib/netbird /root/.ssh; rm -f /etc/ssh/ssh_host_*; : > /etc/machine-id' \
    --run-command 'rpm -qa --qf "%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n" | sort > /tmp/ota-rpm-inventory.txt'
virt-cat -a peer-base.qcow2 /tmp/ota-rpm-inventory.txt > rpm-inventory.txt
export ARTIFACT_SOURCE_REV=$(git -C "$repo" rev-parse HEAD)
python3 - <<'PY'
import hashlib, json, os, time
from pathlib import Path
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
record=dict(version=os.environ['BASE_VERSION'],architecture='aarch64',opendut_version='0.10.2',
    source_revision=os.environ['ARTIFACT_SOURCE_REV'],builder_digest=os.environ['AIB_IMAGE'].split('@')[1],
    built_at_epoch=int(time.time()),base_sha256=sha('peer-base.qcow2'),air_sha256=sha('air'),
    manifest_sha256=sha('peer.aib.yml'),build_log_sha256=sha('build.log'),
    rpm_inventory=Path('rpm-inventory.txt').read_text().splitlines())
Path('provenance.json').write_text(json.dumps(record,indent=2)+'\n')
PY
sha256sum peer-base.qcow2 air provenance.json peer.aib.yml build.log rpm-inventory.txt > SHA256SUMS
printf 'Prepared %s in %s; review provenance before uploading this immutable release.\n' "$BASE_VERSION" "$out"
