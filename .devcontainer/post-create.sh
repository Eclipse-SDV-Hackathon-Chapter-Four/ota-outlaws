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

# Runs once after the dev container is created.
set -euo pipefail

# The cargo registry volume is created as root; hand it to the dev user.
sudo chown -R "$(id -u):$(id -g)" /usr/local/cargo/registry

rustup component add clippy rustfmt

# kuksa-client: CLI for inspecting VSS signals in the KUKSA Data Broker
# (debugging only, the Guardian itself must go through uProtocol).
pipx install kuksa-client

echo "Toolchain:"
rustc --version
cargo --version
protoc --version
python --version
docker --version
