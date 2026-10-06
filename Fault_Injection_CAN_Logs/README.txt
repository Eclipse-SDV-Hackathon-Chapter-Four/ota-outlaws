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

Scenario files:
  normal.asc             nominal temperatures and incrementing counter
  timeout.asc            intentional gap between groups of frames
  counter_error.asc      alive counter skips forward
  counter_stuck.asc      alive counter remains constant
  invalid_quality.asc    quality is 00 or FF
  min_gt_avg.asc         minimum temperature exceeds average
  avg_gt_max.asc         average temperature exceeds maximum
  min_gt_max.asc         minimum temperature exceeds maximum
  out_of_range.asc       temperature exceeds the assumed operating range
  implausible_jump.asc   temperature jumps sharply between frames
  high_delta.asc         maximum and minimum differ by 90 degrees
  temp_stuck.asc         all temperatures remain unchanged

ASC frame timestamps indicate when frames appear in the trace. With the given
8-byte layout there is no application timestamp signal; outdated-data faults
must be tested by delaying or stopping frames, or by adding a timestamp signal
to the CAN message definition.
