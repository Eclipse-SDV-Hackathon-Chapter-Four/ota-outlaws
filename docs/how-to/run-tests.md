<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Run the Tests

Every command runs from the repository root. Unit tests need Rust; runtime
verification uses the shared campaign driver on AutoSD or Docker Compose.

| Level | Command | Proves | Needs |
|-------|---------|--------|-------|
| 0. Documentation checks | `python3 docs/check_architecture.py` | local Markdown links and selected architecture claims resolve against the implementation | Python 3 |
| 1. Unit and integration tests | `cargo test` | every component on its own, including the Guardian's requirements | Rust |
| 2. Format and lint | `cargo fmt --all --check`<br>`cargo clippy --all-targets -- -D warnings` | code quality, as in CI | Rust |
| 3. Fault campaigns | `make -C deploy/autosd campaigns` | the Guardian's reaction through the deployed chain, with diagnostic evidence and a verdict per scenario | AutoSD deployment |
| 4. Hardware demo | `cargo run -p campaign -- observe source_dropout` | the same verdict for a fault injected by hand | running stack |

## Prerequisites

- Rust (stable); `protoc` is vendored, no installation needed.
- For AutoSD campaigns, deploy with `make -C deploy/autosd up`. See the
  [AutoSD guide](../../deploy/autosd/README.md) for image and bench prerequisites.
- For the optional Compose runtime, use Docker with Compose and the pinned
  diagnostic image configured in `diagnostics/image.env`.

## 1. Unit and integration tests

```sh
cargo test                  # whole workspace
cargo test -p guardian      # one component: guardian, guardian-service,
                            # vss-publisher, thermal-contract, campaign
```

The Guardian's requirement tests are named after the requirement they verify,
for example `fsr_2_2_missing_samples_lead_to_degraded_within_budget`. The
campaign tool's tests check its verdict rules on synthetic recordings. No
Docker is needed.

## 2. Format and lint

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

CI runs both, and level 1, on every pull request.

## 3. Fault campaigns

```sh
make -C deploy/autosd campaigns
make -C deploy/autosd campaigns CAMPAIGNS="timeout counter_stuck invalid_quality"
```

The shared driver runs inside AutoSD and controls the deployed services through
Ankaios. Each scenario exercises the CAN provider, Data Broker, VSS Publisher,
uProtocol, Guardian, DFM and OpenSOVD, then restores the original replay.
Evidence is copied to `runs/autosd/<run>/`.

The same driver also supports Docker Compose:

```sh
cargo run -p campaign -- run --all
cargo run -p campaign -- run counter_stuck
cargo run -p campaign -- run --all --no-build
```

Compose creates an isolated project per scenario and stores evidence in
`runs/<campaign>/`. Both runtimes produce `campaign.md` plus per-scenario
`report.md`, `recording.jsonl`, `manifest.json` and `services.log`.

The exit code is 0 when every scenario with status `implemented` passed.
Scenarios for planned requirements run too and are reported as FAIL until the
requirement is implemented. To judge a recorded run again, for example after a
catalog change:

```sh
cargo run -p campaign -- evaluate runs/<campaign>/<scenario>
```

The scenarios and their expectations are in
[`campaign/scenarios.toml`](../../campaign/scenarios.toml); the tool is
described in [Campaign Tool](../reference/components/campaign.md).

## 4. Hardware demo

Start the stack, then record and judge while the fault is injected by hand,
for example by virtually unplugging the board:

```sh
docker compose up -d
cargo run -p campaign -- observe source_dropout --seconds 60
```

The verdict and evidence go to `runs/<time>-observe-source_dropout/`. Without
hardware, `docker stop kuksa-can-provider` and `docker start kuksa-can-provider`
inject the same fault.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).

The documentation validation command was added with the assistance of
**GitHub Copilot** using the model **GPT-6 Luna** (`GPT-6 Luna`).

The campaign-only runtime workflow was updated with assistance from **Codex**
using **GPT-6** (`gpt-6`).
