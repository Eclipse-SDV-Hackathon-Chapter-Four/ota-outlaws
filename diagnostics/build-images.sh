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
    echo "Usage: sh diagnostics/build-images.sh /path/to/Doctor-Whodunit" >&2
    exit 2
fi
source_repo=$(cd "$1" && pwd)
source_ref=${SOURCE_REF:-97dd4a503f25674e866a89829e2bd92d2cf2655d}
source_revision=$(git -C "$source_repo" rev-parse "$source_ref^{commit}")
if [ -n "$(git -C "$source_repo" status --porcelain --untracked-files=all)" ]; then
    echo "Source checkout is dirty; commit or resolve changes before building." >&2
    exit 1
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
build_context=$(mktemp -d)
trap 'rm -rf "$build_context"' EXIT HUP INT TERM
# Only tracked bytes from the selected commit enter the build context.
# No working-tree files, existing binaries, or development-container snapshots.
git -C "$source_repo" archive "$source_revision" demo | tar -x -C "$build_context"
git -C "$source_repo" ls-tree -r "$source_revision" demo > "$build_context/source-files.txt"
cp "$script_dir/Dockerfile" "$build_context/Dockerfile"
echo "Building from clean committed source: $source_revision"
docker build --build-arg "SOURCE_REV=$source_revision" \
    -t local/opensovd-demo-fork:verified "$build_context"
