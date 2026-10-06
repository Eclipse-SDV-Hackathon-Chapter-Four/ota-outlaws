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

- Requirements: [Safety Concept](../../docs/explanation/safety-concept.md)
- Design: [Battery Thermal Guardian](../../docs/reference/components/battery-thermal-guardian.md)
- Parameters: [`config/guardian/safety-params.toml`](../../config/guardian/safety-params.toml)

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

| Part | Status |
|------|--------|
| Core: FSR-1.1, FSR-1.2, FSR-2.2, FSR-2.4, FSR-2.5, fault codes for FSR-D.1 | Implemented, unit-tested |
| uProtocol input and output adapters | Not implemented |
| DFM adapter | Not implemented |
| Executable | Not implemented |

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
