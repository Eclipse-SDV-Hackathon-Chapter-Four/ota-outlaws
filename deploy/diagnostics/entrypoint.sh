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
# Discovery metadata must share the lifetime of DFM's shared-memory namespace.
# /tmp is a fresh per-container tmpfs, hiding baked-in development IPC artifacts.
mkdir -p /dev/shm/iceoryx2
ln -sfn /dev/shm/iceoryx2 /tmp/iceoryx2
exec "$@"
