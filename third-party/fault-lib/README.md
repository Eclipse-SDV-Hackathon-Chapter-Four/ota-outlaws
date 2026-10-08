<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Patched fault_lib dependency

Source: Doctor-Whodunit `demo/fault-lib/src/fault_lib` at commit
`97dd4a503f25674e866a89829e2bd92d2cf2655d`. Original Apache-2.0 notices,
LICENSE and NOTICE are preserved. `common` and iceoryx2 retain the upstream pins.

The IPC worker sends records in FIFO order across transient failures: newer
messages wait behind pending retries, and a deferred head blocks retry processing
of the tail. Capacity is enforced by backpressure rather than evicting accepted
records. Existing retry limits still apply; this is ordered best-effort delivery,
not a persistence acknowledgment or durable outbox. The upstream subscriber's
small overwrite buffer can still lose records during a large backlog. The outage
regression establishes a healthy baseline before pausing DFM to isolate the two
failure/recovery records. No OpenSOVD polling is added.

Before it creates its iceoryx2 node, the IPC worker removes the stale resources
of dead nodes (`Node::cleanup_dead_nodes`). A reporter that is killed never
releases its publisher slot on the DFM event service, and that service only has
two (iceoryx2 default, set by DFM, which creates it). With the Guardian and the
[watchdog](../../components/watchdog/README.md) both reporting, a restarted Guardian
otherwise got `ExceedsMaxSupportedPublishers` on every retry and never reached
DFM again. iceoryx2 treats every node with the caller's own PID as alive, so this
only works when reporters do not all run as PID 1 in separate PID namespaces;
`deploy/docker-compose.yml` puts them into DFM's PID namespace.

Cargo.toml expands the upstream workspace settings for this repository. The IPC behavior patch is in ipc_worker.rs. Three assertions in reporter.rs tests
also use assert_ne! to satisfy newer Clippy versions. Other Rust files are
unchanged from the pinned source.
The whole crate is included because its private IPC worker cannot be overridden
through the upstream public API. Remove this local patch after an upstream release
provides the same ordering guarantee.

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6.1 Sol** (`gpt-6.1-sol`), and of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
