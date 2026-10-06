#!/usr/bin/env python3
"""Generate BMS_MSG1 CAN fault-injection ASC logs."""

import argparse
from pathlib import Path


# Each frame is: (timestamp, max_temp, min_temp, avg_temp, quality, counter).
Frame = tuple

SCENARIOS = {
    "normal": (
        "Nominal temperatures with an incrementing alive counter.",
        [
            (i / 10, 40 + i, 24 + i, 32 + i, 0x80, i)
            for i in range(16)
        ],
    ),
    "timeout": (
        "The 1.8-second gap between frames simulates missing data.",
        [
            (0.0, 40, 24, 32, 0x80, 0),
            (0.1, 41, 25, 33, 0x80, 1),
            (0.2, 42, 26, 34, 0x80, 2),
            (2.0, 43, 27, 35, 0x80, 3),
            (2.1, 44, 28, 36, 0x80, 4),
        ],
    ),
    "counter_error": (
        "AliveCounter skips from 1 to 6 instead of incrementing by one.",
        [
            (0.0, 40, 24, 32, 0x80, 0),
            (0.1, 40, 24, 32, 0x80, 1),
            (0.2, 40, 24, 32, 0x80, 6),
            (0.3, 40, 24, 32, 0x80, 7),
            (0.4, 40, 24, 32, 0x80, 8),
        ],
    ),
    "counter_stuck": (
        "AliveCounter remains at 10 in every transmitted frame.",
        [
            (i / 10, 40, 24, 32, 0x80, 10)
            for i in range(5)
        ],
    ),
    "invalid_quality": (
        "Quality alternates between invalid (00) and error/not available (FF).",
        [
            (0.0, 40, 24, 32, 0x00, 0),
            (0.1, 40, 24, 32, 0xFF, 1),
            (0.2, 40, 24, 32, 0x00, 2),
            (0.3, 40, 24, 32, 0xFF, 3),
        ],
    ),
    "min_gt_avg": (
        "CellTempMin (60) is greater than CellTempAvg (40).",
        [
            (i / 10, 70, 60, 40, 0x80, i)
            for i in range(3)
        ],
    ),
    "avg_gt_max": (
        "CellTempAvg (90) is greater than CellTempMax (70).",
        [
            (i / 10, 70, 20, 90, 0x80, i)
            for i in range(3)
        ],
    ),
    "min_gt_max": (
        "CellTempMin (70) is greater than CellTempMax (40).",
        [
            (i / 10, 40, 70, 50, 0x80, i)
            for i in range(3)
        ],
    ),
    "out_of_range": (
        "CellTempMax is 250, beyond the assumed operating range.",
        [
            (i / 10, 250, 10, 80, 0x80, i)
            for i in range(3)
        ],
    ),
    "implausible_jump": (
        "Temperature jumps from 30 to 160 between frames 100 ms apart.",
        [
            (0.0, 30, 30, 30, 0x80, 0),
            (0.1, 160, 160, 160, 0x80, 1),
            (0.2, 160, 160, 160, 0x80, 2),
            (0.3, 160, 160, 160, 0x80, 3),
        ],
    ),
    "high_delta": (
        "CellTempMax=100 and CellTempMin=10, a 90-degree difference.",
        [
            (i / 10, 100, 10, 55, 0x80, i)
            for i in range(3)
        ],
    ),
    "temp_stuck": (
        "All three temperature signals remain fixed at 39 while counter rolls.",
        [
            (i / 10, 39, 39, 39, 0x80, i)
            for i in range(6)
        ],
    ),
}


def encode_frame(frame):
    timestamp, max_temp, min_temp, avg_temp, quality, counter = frame
    values = (max_temp, min_temp, avg_temp)
    if any(not 0 <= value <= 0xFFFF for value in values):
        raise ValueError(
            "Temperature value outside UINT16 range: {}".format(values)
        )
    if not 0 <= quality <= 0xFF or not 0 <= counter <= 0xFF:
        raise ValueError("Quality and alive counter must be byte values")

    payload = [
        max_temp & 0xFF, max_temp >> 8,
        min_temp & 0xFF, min_temp >> 8,
        avg_temp & 0xFF, avg_temp >> 8,
        quality, counter,
    ]
    data = " ".join("{:02X}".format(byte) for byte in payload)
    return "{:.6f} 1 500 Rx d 8 {}".format(timestamp, data)


def make_header(filename, purpose):
    return """// -----------------------------------------------------------------------------
// File Name : {filename}
// Message : BMS_MSG1 (Battery Management System)
// CAN ID : 0x500
// DLC : 8 Bytes
//
// Purpose:
// This ASC file is a sample CAN trace generated with the assistance of
// Microsoft Copilot for simulation and testing purposes.
// Scenario: {purpose}
//
// Signal Layout:
//
// Byte 0-1 : CellTempMax (UINT16, Little Endian)
// Maximum cell temperature in battery pack
//
// Byte 2-3 : CellTempMin (UINT16, Little Endian)
// Minimum cell temperature in battery pack
//
// Byte 4-5 : CellTempAvg (UINT16, Little Endian)
// Average cell temperature in battery pack
//
// Byte 6 : Quality
// Signal validity/status indicator
// Example:
// 0x00 = Invalid
// 0x80 = Valid
// 0xFF = Error/Not Available
//
// Byte 7 : AliveCounter
// Rolling counter (0-255)
// Incremented every transmitted frame (except intentional counter faults)
//
// Example Frame:
// 28 00 18 00 20 00 80 00
//
// Decoded:
// CellTempMax = 0x0028 = 40
// CellTempMin = 0x0018 = 24
// CellTempAvg = 0x0020 = 32
// Quality = 0x80 (Valid)
// AliveCounter = 0
//
// Note:
// Signal scaling, offset, units and value ranges are assumed for simulation.
// Actual interpretation shall follow the project DBC specification.
// -----------------------------------------------------------------------------
// Generated for CANoe simulation with Microsoft Copilot assistance.
// Verify signal encoding, scaling, endianness and invalid values
// against the latest DBC before integration testing.
""".format(filename=filename, purpose=purpose)


def render_log(filename, purpose, frames):
    frame_lines = [encode_frame(frame) for frame in frames]
    return (
        make_header(filename, purpose)
        + "\ndate Tue Oct 06 09:00:00 2026\n"
        + "base hex timestamps absolute\n"
        + "internal events logged\n"
        + "Begin Triggerblock Tue Oct 06 09:00:00.000 2026\n"
        + "\n".join(frame_lines)
        + "\nEnd TriggerBlock\n"
    )


def main():
    parser = argparse.ArgumentParser(
        description="Generate BMS_MSG1 CAN fault-injection ASC logs."
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parent,
        help="Directory for generated logs (default: the script's directory).",
    )
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    for name, (purpose, frames) in SCENARIOS.items():
        filename = "{}.asc".format(name)
        output = args.output_dir / filename
        with output.open("w", encoding="ascii", newline="\n") as log_file:
            log_file.write(render_log(filename, purpose, frames))
        print("Wrote {}".format(output))


if __name__ == "__main__":
    main()
