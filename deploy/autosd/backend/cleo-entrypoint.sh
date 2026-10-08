#!/bin/bash
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

# Entrypoint of the openDuT localenv CLEO container (see compose.override.yaml).
# Downloads CLEO for this container's architecture from CARL, then idles so
# the bench can run it with `docker exec`.
set -euo pipefail
carl="https://${OPENDUT_CLEO_NETWORK_CARL_HOST:?}"
until curl --silent --output /dev/null "$carl"; do
    echo "Waiting for $carl ..."
    sleep 5
done
cd /usr/local/bin
curl --fail --silent --show-error --output cleo.tar.gz \
    "$carl/api/cleo/$(uname -m)-unknown-linux-gnu/download"
tar --strip-components=1 -xf cleo.tar.gz
rm cleo.tar.gz
./opendut-cleo --version
exec sleep infinity
