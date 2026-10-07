<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Guardian core

The deterministic safety core of the Battery Thermal Guardian, written in Rust.
It has no I/O: adapters feed it samples and publish its events.

- Requirements: [Safety Concept](../docs/reference/hara.md)
- Design: [Battery Thermal Guardian](../docs/reference/components/battery-thermal-guardian.md)
- Parameters: [`config/guardian/safety-params.toml`](../config/guardian/safety-params.toml)

## Build and test

From the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

The tests in [`tests/requirements.rs`](tests/requirements.rs) are named after the
requirement they verify, for example `fsr_2_2_missing_samples_lead_to_degraded_within_budget`.

## Status

The core, uProtocol adapter, executable, and DFM/OpenSOVD diagnostics are implemented and tested.
Recovery requires at least ten consecutive fresh, valid, healthy samples over at
least one second. Each input fault emits `FaultRecovered`, and monitoring returns
to OK only after all active faults recover. A stuck maximum must move again.
Thermal recovery uses 2 °C hysteresis: Critical → Warning below 53 °C, then
Warning → Monitoring below 43 °C, each with its own recovery period and only
while monitoring is OK. Invalid, repeated, missing or stuck input interrupts
recovery. Escalation remains immediate. Details:

- Parts: [Battery Thermal Guardian](../docs/reference/components/battery-thermal-guardian.md#core-and-adapters)
- Requirements: status column of the [Safety Concept](../docs/reference/hara.md#functional-safety-requirements)

After startup, monitors remain NotTested until sustained healthy input produces
`FaultTestPassed`. This also clears a previous session's current DFM failure
without deleting its history or inventing a detection cause in the new session.
The first stuck-signal monitor test observes more than the configured three-second
stuck interval. Subsequent detected faults recover through `FaultRecovered`.
Only `testFailed` clearing is required; historical confirmation and warning bits
follow DFM's lifecycle/reset policy.

Signal-stuck testing and recovery require the full three-second detector observation
period plus the one-second healthy confirmation period. A maximum that jumps once
and freezes again while reference temperatures move does not recover. The local
`third-party/fault-lib` patch blocks newer IPC records behind older retries;
Guardian still performs no OpenSOVD polling. Delivery remains best effort, with
bounded queues and the upstream retry limit. The outage campaign restores healthy
input while diagnostics are paused, then verifies the final Passed state after resume.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

Recovery behavior was added with assistance from **Codex** using **GPT-6.1 Sol** (`gpt-6.1-sol`).
