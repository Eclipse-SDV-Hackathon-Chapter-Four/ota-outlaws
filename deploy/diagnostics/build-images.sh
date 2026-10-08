#!/bin/sh
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
# AI-assisted: Codex / GPT-6.1 Sol (gpt-6.1-sol)

set -eu
if [ "$#" -ne 1 ]; then
    echo "Usage: sh deploy/diagnostics/build-images.sh /path/to/Doctor-Whodunit" >&2
    exit 2
fi
source_repo=$(cd "$1" && pwd)
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$script_dir/image.env"
source_ref=${SOURCE_REF:-$PINNED_SOURCE_REV}
source_revision=$(git -C "$source_repo" rev-parse "$source_ref^{commit}")
if [ -n "$(git -C "$source_repo" status --porcelain --untracked-files=all)" ]; then
    echo "Source checkout is dirty; commit or resolve changes before building." >&2
    exit 1
fi
build_context=$(mktemp -d)
trap 'rm -rf "$build_context"' EXIT HUP INT TERM
# Only tracked bytes from the selected commit enter the build context.
# No working-tree files, existing binaries, or development-container snapshots.
git -C "$source_repo" archive "$source_revision" demo | tar -x -C "$build_context"
git -C "$source_repo" ls-tree -r "$source_revision" demo > "$build_context/source-files.txt"
cp "$script_dir/Dockerfile" "$build_context/Dockerfile"
echo "Building from clean committed source: $source_revision"
if [ "${PUSH_IMAGE:-0}" = 1 ]; then
    # Native Actions runners build each architecture with a persistent layer cache.
    : "${PUBLISH_TAG:?Set PUBLISH_TAG when pushing}"
    : "${IMAGE_ARCH:?Set IMAGE_ARCH when pushing}"
    docker buildx build --push --platform "linux/$IMAGE_ARCH" \
        --cache-from "type=gha,version=2,scope=diagnostics-$IMAGE_ARCH" \
        --cache-to "type=gha,version=2,mode=max,scope=diagnostics-$IMAGE_ARCH" \
        --label org.opencontainers.image.source=https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/ota-outlaws \
        --build-arg "SOURCE_REV=$source_revision" -t "$PUBLISH_TAG" "$build_context"
else
    docker build --build-arg "SOURCE_REV=$source_revision" \
        -t local/opensovd-demo-fork:verified "$build_context"
fi
