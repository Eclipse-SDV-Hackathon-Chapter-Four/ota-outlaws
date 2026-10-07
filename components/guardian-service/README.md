<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Guardian service

The Battery Thermal Guardian executable. It connects the
[Guardian core](../guardian) to uProtocol: it subscribes to `BatteryTemperature`
messages from the VSS Publisher, calls the core every 50 ms, and publishes every
core event as a `GuardianEvent`. The messages and topics are defined in
[`contracts/`](../contracts).

- Design: [Battery Thermal Guardian](../../docs/reference/components/battery-thermal-guardian.md)
- Contract: [Battery Thermal Contract](../contracts/README.md)

## Run

From the repository root, with a Zenoh peer or router to connect to:

```sh
ZENOH_CONNECT=tcp/127.0.0.1:7447 cargo run -p guardian-service
```

| Variable | Default | Meaning |
|----------|---------|---------|
| `GUARDIAN_CONFIG` | `config/guardian/safety-params.toml` | Safety parameters |
| `ZENOH_CONNECT` | — | Comma-separated Zenoh endpoints to connect to |
| `ZENOH_LISTEN` | — | Comma-separated Zenoh endpoints to listen on |
| `RUST_LOG` | `info` | Log filter. Every Guardian event is logged at `info`. |

## Test

```sh
cargo test -p guardian-service
```

[`tests/uprotocol.rs`](tests/uprotocol.rs) runs the service against a test
publisher over a real Zenoh connection on localhost, without a Data Broker. It
checks that a temperature above the warning threshold leads to WARNING, that
missing data leads to a freshness fault and the "monitoring unavailable"
warning, and that the cause links survive the transport.

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
