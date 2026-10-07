<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Campaign Traces

The CAN traces the [campaign tool](../../docs/reference/components/campaign.md)
replays. The scenarios that use them are in
[`campaign/scenarios.toml`](../scenarios.toml). One script generates all of
them: `python3 campaign/generate_traces.py`.

| Group | Traces |
|-------|--------|
| Fault logs (100 frames each) | `normal`, `timeout`, `counter_error`, `counter_stuck`, `invalid_quality`, `min_gt_avg`, `avg_gt_max`, `min_gt_max`, `out_of_range`, `implausible_jump`, `high_delta`, `temp_stuck` |
| Campaign traces | `heating`, `max_stuck`, `spike`, `isolated_spike`, `drift`, `nominal`, `invalid_during_warning` (TS-11), `invalid_during_critical` (TS-11), `saturation_255` (TS-26), `quality_single` (TS-16), `duplicate_message` (TS-07), `hot_spot` (FSR-1.4) |

## Fault logs

```text
All ASC files use CAN ID 0x500, standard 11-bit CAN, Rx, DLC 8.
Every ASC file starts with a BMS_MSG1 signal-layout header for readability.

Payload layout (UINT16 values, little endian):
  bytes 0-1: CellTempMax
  bytes 2-3: CellTempMin
  bytes 4-5: CellTempAvg
  byte 6:    Quality (00 invalid, 80 valid, FF error/not available)
  byte 7:    AliveCounter (0-255)

Temperature values in these traces are raw unsigned integer degrees for
simulation only. A negative temperature cannot be represented as a negative
UINT16 value without a DBC-defined signed encoding or offset. The out-of-range
scenario therefore uses high positive values. Confirm scaling, offset,
signedness, units, ranges, and invalid-value encoding against the project DBC.

Each ASC contains exactly 100 CAN messages. Fault scenarios begin with normal
frames, inject the fault in the middle, and finish with normal recovery frames.
Most scenarios use 40 normal lead-in frames, 20 fault frames, and 40 recovery
frames. The timeout scenario uses 50 normal frames before and after a 1.8-second
missing-frame gap, keeping the total transmitted message count at 100.
The normal.asc file contains 100 nominal frames.

Scenario files:
  normal.asc             100 nominal temperatures and incrementing counter
  timeout.asc            50 normal frames, missing-frame gap, 50 normal frames
  counter_error.asc      counter skips from 39 to 45, then resumes incrementing
  counter_stuck.asc      counter sticks in the middle, then resumes incrementing
  invalid_quality.asc    quality is 00 or FF in the fault segment
  min_gt_avg.asc         minimum temperature exceeds average in the fault segment
  avg_gt_max.asc         average temperature exceeds maximum in the fault segment
  min_gt_max.asc         minimum temperature exceeds maximum in the fault segment
  out_of_range.asc       temperature exceeds the assumed range in the fault segment
  implausible_jump.asc   temperature jumps sharply in the fault segment
  high_delta.asc         maximum and minimum differ by 90 degrees in the fault segment
  temp_stuck.asc         temperatures stick while the counter continues incrementing

ASC frame timestamps indicate when frames appear in the trace. With the given
8-byte layout there is no application timestamp signal; outdated-data faults
must be tested by delaying or stopping frames (as in timeout.asc), or by adding
a timestamp signal to the CAN message definition.
```

## AI Assistance

This document was revised with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`) and **Claude Sonnet 5.5** (`claude-sonnet-5-5`): the fault-log section above is the former
`Fault_Injection_CAN_Logs/README.txt`, unchanged.
