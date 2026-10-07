<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Run the Signal Chain

Runs the chain from the recorded CAN trace to the Battery Thermal Guardian and
its diagnostics in Docker:

```text
config/can/BMS_MSG1_CAN.asc → KUKSA CAN Provider → KUKSA Data Broker → VSS Publisher
  → uProtocol (Zenoh router) → Battery Thermal Guardian → DFM → OpenSOVD
```

## Start

The DFM and OpenSOVD image is built once, as described in
[Build from clean committed source](../../README.md#build-from-clean-committed-source).
Then, from the repository root:

```sh
docker compose up --build -d
docker compose logs -f guardian
```

The first build takes a few minutes. The Data Broker is published on host port
55556 (`KUKSA_HOST_PORT`), OpenSOVD on 7690 (`SOVD_PORT`).

## What you should see

The trace has 16 frames, heating from 40 °C to 55 °C in 1.5 s, and repeats
forever. The Guardian logs its events:

```text
guardian event id=1 ... ThermalStateChanged { from: Clear, to: Monitoring, ... }
guardian event id=2 ... ThermalStateChanged { from: Monitoring, to: Warning, ... }
guardian event id=3 ... FaultTestPassed { fault: FreshnessLost, ... }
...
guardian event id=6 ... ThermalStateChanged { from: Warning, to: Critical, ... }
guardian event id=7 cause=Some(6) ... MitigationRequested { mitigation: DriverWarningOvertemp }
guardian event id=8 ... ThermalStateChanged { from: Critical, to: Warning, ... }
guardian event id=10 ... ThermalStateChanged { from: Warning, to: Critical, ... }
```

`FaultTestPassed` reports each input monitor's first healthy test. Each time the
trace starts over at 40 °C, the temperature stays below 53 °C (`θ_crit` minus
the 2 °C hysteresis) for 1.1 s, so the Guardian lowers CRITICAL to WARNING
(FSR-1.5) and raises it again at 55 °C, with a new warning. It does not reach
MONITORING, which needs 1 s below 43 °C.

## Inject a source dropout

Stop the CAN provider, then start it again:

```sh
docker stop kuksa-can-provider
docker start kuksa-can-provider
docker compose logs --since 30s guardian
```

The Guardian reports the loss of fresh data (FSR-2.2), enters DEGRADED, and
requests the "monitoring unavailable" warning. When data flows again, the fault
recovers (FSR-2.6) and monitoring returns to OK. The events are linked by cause:

```text
guardian event id=33 ... FaultDetected { fault: FreshnessLost, ... }
guardian event id=34 cause=Some(33) ... MonitoringStatusChanged { from: Ok, to: Degraded }
guardian event id=35 cause=Some(34) ... MitigationRequested { mitigation: DriverWarningMonitoringUnavailable }
guardian event id=36 cause=Some(33) ... FaultRecovered { fault: FreshnessLost, ... }
guardian event id=37 cause=Some(36) ... MonitoringStatusChanged { from: Degraded, to: Ok }
```

OpenSOVD keeps the fault in its history:

```sh
curl -s http://127.0.0.1:7690/sovd/v1/apps/battery_guardian/faults
```

`BTG_TempFreshnessLost` shows `testFailed: false` (recovered) and
`testFailedSinceLastClear: true` (it occurred).

## Measured

These are manual runs, not yet automated campaigns.

| Observation | Result | Date |
|-------------|--------|------|
| Messages from the VSS Publisher | One per CAN frame, each with a consistent set of values | 2026-10-06 |
| Frames that reach the Guardian | 93 % (a single frame is lost now and then) | 2026-10-06 |
| False faults during 40 s of nominal replay | None | 2026-10-06 |
| CAN provider stopped → `FreshnessLost` | 0.2 s (budget: `T_stale` + `T_react` = 0.8 s) | 2026-10-06 |
| CAN provider stopped → DEGRADED → recovered → OK, OpenSOVD history kept | Yes | 2026-10-07 |

## Stop

```sh
docker compose down
```

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
