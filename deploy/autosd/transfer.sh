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

# Called by make up on the host. Prepare everything before stopping the guest.
# Lifecycle/status/access commands are in the Makefile; service settings are YAML.
set -euo pipefail
cd "$(dirname "$0")"
repo=$(cd ../.. && pwd)

# These values are substituted into YAML, systemd units and remote shell paths.
# Restrict their syntax rather than introducing a template/parser dependency.
[[ "$AUTOSD_GUEST_DIR" =~ ^/[A-Za-z0-9_./-]+$ && "$AUTOSD_GUEST_DIR" != / ]] || {
    echo 'AUTOSD_GUEST_DIR must be an absolute path without spaces or shell characters.' >&2; exit 1;
}
[[ "$AUTOSD_AGENT_NAME" =~ ^[A-Za-z0-9_-]+$ ]] || { echo 'Invalid Ankaios agent name.' >&2; exit 1; }
[[ "$SKIP_IMAGES" == 0 || "$SKIP_IMAGES" == 1 ]] || { echo 'SKIP_IMAGES must be 0 or 1.' >&2; exit 1; }
options=(-i "$AUTOSD_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5
    -o StrictHostKeyChecking=yes -o "UserKnownHostsFile=$AUTOSD_KNOWN_HOSTS")
ssh_guest=(ssh -p "$AUTOSD_SSH_PORT" "${options[@]}" "$AUTOSD_SSH_HOST")
work=$(mktemp -d)
container=''
cleanup() {
    if [[ -n "$container" ]]; then docker rm "$container" >/dev/null; fi
    rm -rf "$work"
}
trap cleanup EXIT

# Read the small declarative image list and resolve the diagnostics override.
while read -r name image; do
    case "$name" in ''|'#'*) continue ;; esac
    [[ "$image" != '${DIAGNOSTICS_IMAGE}' ]] || image=$DIAGNOSTICS_IMAGE
    if [[ -n "${AUTOSD_IMAGE_TAG:-}" ]]; then
        case "$name" in
            guardian) image="ota-outlaws/guardian:$AUTOSD_IMAGE_TAG" ;;
            publisher) image="ota-outlaws/vss-publisher:$AUTOSD_IMAGE_TAG" ;;
        esac
    fi
    printf '%s %s\n' "$name" "$image"
done < images.tsv > "$work/images.tsv"
if [[ "$SKIP_IMAGES" == 0 ]]; then
    images=()
    while read -r name image; do images+=("$image"); done < "$work/images.tsv"
    docker image inspect "${images[@]}" | jq -e '
        if all(.[]; .Os == "linux" and .Architecture == "arm64") then
            [.[] | {Id, RepoTags, RepoDigests, Os, Architecture, labels: .Config.Labels}]
        else error("All application images must be linux/arm64") end
    ' > "$work/images.json"
else
    # Fail before stopping a working stack if its required guest images are absent.
    while read -r name image; do
        "${ssh_guest[@]}" "podman image exists localhost/ota-autosd/$name:deployed"
    done < "$work/images.tsv"
fi

# A catalog update also needs a compatible Guardian build. Check before stopping
# services or transferring configuration, including when guest images are reused.
if [[ "$SKIP_IMAGES" == 0 ]]; then
    guardian_image=$(awk '$1 == "guardian" {print $2}' "$work/images.tsv")
    container=$(docker create --platform linux/arm64 "$guardian_image")
    docker cp "$container:/etc/guardian/catalog/battery_guardian.json" "$work/guardian-catalog.json"
    docker rm "$container" >/dev/null
    container=''
else
    "${ssh_guest[@]}" 'podman run --rm --network=none --entrypoint /bin/cat localhost/ota-autosd/guardian:deployed /etc/guardian/catalog/battery_guardian.json' > "$work/guardian-catalog.json"
fi
expected_codes=$(jq -ce '[.faults[].id.Text] | sort' "$repo/diagnostics/catalog/battery_guardian.json")
image_codes=$(jq -ce '[.faults[].id.Text] | sort' "$work/guardian-catalog.json")
if [[ "$image_codes" != "$expected_codes" ]]; then
    echo 'Guardian image and repository fault catalog have different DTCs.' >&2
    echo 'From the repository root, rebuild and transfer the Guardian image:' >&2
    echo '  docker build --platform linux/arm64 -t ota-outlaws/guardian:dev -f guardian-service/Containerfile .' >&2
    echo '  make -C deploy/autosd up' >&2
    exit 1
fi
if [[ "$SKIP_IMAGES" == 0 ]]; then
    echo 'Exporting existing ARM64 application images...'
    docker save "${images[@]}" | gzip -1 > "$work/images.tar.gz"
fi

# The upstream CAN provider has no shell. Supply its existing pinned static tool.
container=$(docker create --platform linux/arm64 \
    busybox@sha256:5cec3fc171c87218698e85a52af7087de727372aae264a787b8112901a5b0092)
docker cp "$container:/bin/busybox" "$work/busybox"
docker rm "$container" >/dev/null
container=''

# Render the one pod manifest and embed it as Ankaios's otaPod config object.
sed "s|\${AUTOSD_DIR}|$AUTOSD_GUEST_DIR|g" pod.yaml > "$work/pod.yaml"
sed "s|agent: bench-beta|agent: \"$AUTOSD_AGENT_NAME\"|" ankaios.yaml > "$work/ankaios-state.yaml"
printf '\nconfigs:\n  otaPod: |\n' >> "$work/ankaios-state.yaml"
sed 's/^/    /' "$work/pod.yaml" >> "$work/ankaios-state.yaml"
for unit in ota-ankaios-*.service; do
    sed -e "s|/opt/ota-outlaws-autosd|$AUTOSD_GUEST_DIR|g" \
        -e "s|--name bench-beta|--name $AUTOSD_AGENT_NAME|" "$unit" > "$work/$unit"
done
cp -R "$repo/can" "$work/can"
case "${AUTOSD_CAN_MODE:-replay}" in
    replay) ;;
    socketcan) touch "$work/can/.socketcan" ;;
    *) echo 'AUTOSD_CAN_MODE must be replay or socketcan' >&2; exit 2 ;;
esac
cp -R "$repo/diagnostics/catalog" "$work/catalog"
cp "$repo/diagnostics/entrypoint.sh" "$work/entrypoint.sh"
cp "$repo/config/guardian/safety-params.toml" "$work/safety-params.toml"

# Download missing Ankaios before interrupting services. Always refresh units.
if "${ssh_guest[@]}" 'test -x /opt/ota-ankaios-1.0.4/ank && test -x /opt/ota-ankaios-1.0.4/ank-server && test -x /opt/ota-ankaios-1.0.4/ank-agent'; then
    :
else
    probe_status=$?
    [[ "$probe_status" == 1 ]] || exit "$probe_status"
    base=https://github.com/eclipse-ankaios/ankaios/releases/download/v1.0.4
    archive=ankaios-linux-arm64.tar.gz
    curl --fail --location "$base/$archive" -o "$work/$archive"
    curl --fail --location "$base/$archive.sha512sum.txt" -o "$work/$archive.sha512sum.txt"
    expected=$(awk 'NR == 1 {print $1}' "$work/$archive.sha512sum.txt")
    actual=$(shasum -a 512 "$work/$archive" | awk '{print $1}')
    [[ "$actual" == "$expected" ]] || { echo 'Ankaios checksum mismatch.' >&2; exit 1; }
fi
# macOS tar's Apple metadata is unnecessary on the Linux guest.
COPYFILE_DISABLE=1 tar --exclude=./bundle.tar.gz -czf "$work/bundle.tar.gz" -C "$work" .
make --no-print-directory down
"${ssh_guest[@]}" "mkdir -p '$AUTOSD_GUEST_DIR/.transfer'"
# AutoSD has no tar binary. Keep the unpacker separate from the file it extracts.
scp -P "$AUTOSD_SSH_PORT" "${options[@]}" "$work/busybox" "$AUTOSD_SSH_HOST:$AUTOSD_GUEST_DIR/.transfer/busybox"
scp -P "$AUTOSD_SSH_PORT" "${options[@]}" "$work/bundle.tar.gz" "$AUTOSD_SSH_HOST:$AUTOSD_GUEST_DIR/bundle.tar.gz"

# Apply the trusted local bundle, label shared mounts, and retain loaded image IDs.
"${ssh_guest[@]}" "bash -s -- '$AUTOSD_GUEST_DIR'" <<'GUEST'
set -euo pipefail
cd "$1"
chmod 755 .transfer/busybox
.transfer/busybox tar -xzf bundle.tar.gz
rm bundle.tar.gz
rm -f can/.campaign-active can/.campaign-source can/.campaign-guardian
chcon -R -t container_file_t -l s0 can catalog entrypoint.sh safety-params.toml busybox
podman volume exists ota-autosd-dfm || podman volume create ota-autosd-dfm
volume_path=$(podman volume inspect ota-autosd-dfm --format '{{.Mountpoint}}')
chcon -R -t container_file_t -l s0 "$volume_path"
if [[ -f images.tar.gz ]]; then
    podman load -i images.tar.gz
    aliases=()
    while read -r name image; do
        alias="localhost/ota-autosd/$name:deployed"
        # Tag the image just loaded under its original tag; Docker/Podman IDs may differ.
        podman tag "$image" "$alias"
        aliases+=("$alias")
    done < images.tsv
    podman image inspect "${aliases[@]}" > loaded-images.json
    rm images.tar.gz
fi
if [[ -f ankaios-linux-arm64.tar.gz ]]; then
    mkdir -p /opt/ota-ankaios-1.0.4
    .transfer/busybox tar -xzf ankaios-linux-arm64.tar.gz -C /opt/ota-ankaios-1.0.4
    rm ankaios-linux-arm64.tar.gz ankaios-linux-arm64.tar.gz.sha512sum.txt
fi
cp ota-ankaios-*.service /etc/systemd/system/
systemctl daemon-reload
rm -rf .transfer
GUEST
echo 'Images, configuration and Ankaios units ready.'
