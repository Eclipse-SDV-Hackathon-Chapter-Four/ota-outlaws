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

# Development image for the Battery Thermal Guardian (Rust) and its tooling.
FROM mcr.microsoft.com/devcontainers/rust:1-1-bookworm

# protoc:    uProtocol and KUKSA gRPC code generation (prost / tonic build scripts)
# can-utils: candump / canplayer / cansend for inspecting CAN traces
# jq:        reading evidence and report JSON in the shell
RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        protobuf-compiler \
        libprotobuf-dev \
        pkg-config \
        libssl-dev \
        can-utils \
        jq \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*
