CAN fault injection ASC logs

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
