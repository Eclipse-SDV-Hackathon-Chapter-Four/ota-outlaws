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

Every command runs from the repository root. The levels build on each other:
the first needs only Rust, the others need Docker.

| Level | Command | Proves | Needs |
|-------|---------|--------|-------|
| 0. Documentation checks | `python3 docs/check_architecture.py` | local Markdown links and selected architecture claims resolve against the implementation | Python 3 |
| 1. Unit and integration tests | `cargo test` | every component on its own, including the Guardian's requirements | Rust |
| 2. Format and lint | `cargo fmt --all --check`<br>`cargo clippy --all-targets -- -D warnings` | code quality, as in CI | Rust |
| 3. Fault campaigns | `cargo run -p campaign -- run --all` | the Guardian's reaction to real faults through the whole chain, with a verdict per scenario | Docker, diagnostics image |
| 4. Diagnostic campaigns | `python3 diagnostics/smoke_test.py` | the DFM and OpenSOVD path, including a diagnostics outage | Docker, diagnostics image |
| 5. Hardware demo | `cargo run -p campaign -- observe source_dropout` | the same verdict for a fault injected by hand | running stack |

## Prerequisites

- Rust (stable); `protoc` is vendored, no installation needed.
- Docker with Compose, for levels 3 to 5.
- The diagnostics image `local/opensovd-demo-fork:verified`, built once, for
  levels 3 to 5:

  ```sh
  sh diagnostics/build-images.sh /path/to/Doctor-Whodunit
  ```

  See [Build from clean committed source](../../README.md#build-from-clean-committed-source-optional).

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
cargo run -p campaign -- run --all              # every scenario
cargo run -p campaign -- run counter_stuck      # one scenario
cargo run -p campaign -- run --all --no-build   # reuse the built images
```

Each scenario replays a CAN trace once through the real chain (CAN provider,
Data Broker, VSS Publisher, uProtocol, Guardian, DFM, OpenSOVD) in its own
Compose project and judges the Guardian's reaction. The whole campaign takes
about ten minutes. It can run while the development stack is up.

The evidence goes to `runs/<campaign>/`:

- `campaign.md`: every scenario with its verdict;
- `<scenario>/report.md`: the evidence chain from hazard to verdict;
- `<scenario>/recording.jsonl`, `manifest.json`, `services.log`: the raw
  evidence.

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

## 4. Diagnostic campaigns

```sh
python3 diagnostics/smoke_test.py
```

Runs the Guardian service with a test publisher against the real DFM and
OpenSOVD, including a DFM and gateway outage. Reports go to
`diagnostics/reports/`. Details are in the
[README](../../README.md#verification).

## 5. Hardware demo

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
