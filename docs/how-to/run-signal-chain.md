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

Runs the chain from the recorded CAN trace to the Battery Thermal Guardian in
Docker:

```text
can/BMS_MSG1_CAN.asc → KUKSA CAN Provider → KUKSA Data Broker → VSS Publisher
  → uProtocol (Zenoh router) → Battery Thermal Guardian
```

## Start

From the repository root:

```sh
docker compose up --build -d
docker compose logs -f guardian
```

The first build takes a few minutes. If port 55555 is already in use on your
machine, remove the `ports` entry of `kuksa-databroker`; the services talk to
each other inside the Docker network and do not need it.

## What you should see

The trace has 16 frames, heating from 40 °C to 55 °C in 1.5 s, and repeats
forever. The Guardian logs its events:

```text
guardian event id=1 ... ThermalStateChanged { from: Clear, to: Monitoring, ... }
guardian event id=2 ... ThermalStateChanged { from: Monitoring, to: Warning, ... }
guardian event id=3 ... ThermalStateChanged { from: Warning, to: Critical, ... }
guardian event id=4 cause=Some(3) ... MitigationRequested { mitigation: DriverWarningOvertemp }
```

The Guardian stays CRITICAL when the trace starts over at 40 °C: it never lowers
its thermal state, because recovery is not implemented yet.

## Inject a source dropout

Stop the CAN provider:

```sh
docker stop kuksa-can-provider
docker compose logs --since 10s guardian
```

The Guardian reports the loss of fresh data (FSR-2.2), enters DEGRADED, and
requests the "monitoring unavailable" warning, linked by cause:

```text
guardian event id=5 ... FaultDetected { fault: FreshnessLost, ... }
guardian event id=6 cause=Some(5) ... MonitoringStatusChanged { from: Ok, to: Degraded }
guardian event id=7 cause=Some(6) ... MitigationRequested { mitigation: DriverWarningMonitoringUnavailable }
```

Restart it with `docker start kuksa-can-provider`. DEGRADED stays, because
recovery is not implemented yet; restart the Guardian for the next scenario.

## Measured on 2026-10-06

This is a manual run, not yet an automated campaign.

| Observation | Result |
|-------------|--------|
| Messages from the VSS Publisher | One per CAN frame, each with a consistent set of values |
| Frames that reach the Guardian | 93 % (a single frame is lost now and then) |
| False faults during 40 s of nominal replay | None |
| CAN provider stopped → `FreshnessLost` | 0.2 s (budget: `T_stale` + `T_react` = 0.8 s) |

## Stop

```sh
docker compose down
```

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
