#!/usr/bin/env python3
# Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0 which is available at
# https://www.eclipse.org/legal/epl-2.0
#
# SPDX-License-Identifier: EPL-2.0
"""Generate BMS_MSG1 CAN fault-injection ASC logs."""

import argparse
from pathlib import Path


TOTAL_FRAMES = 100
LEAD_IN_FRAMES = 40
FAULT_FRAMES = 20
RECOVERY_FRAMES = 40
TIMEOUT_LEAD_IN_FRAMES = 50
FRAME_PERIOD = 0.1
TIMEOUT_GAP = 1.8

# Values are (CellTempMax, CellTempMin, CellTempAvg, Quality) during injection.
SCENARIOS = {
    "normal": ("Nominal data with an incrementing alive counter.", None),
    "timeout": ("A 1.8-second missing-frame gap between normal sections.", None),
    "counter_error": (
        "AliveCounter skips from 39 to 45, then resumes incrementing.",
        None,
    ),
    "counter_stuck": (
        "AliveCounter remains fixed during the middle fault segment.",
        None,
    ),
    "invalid_quality": (
        "Quality alternates between invalid (00) and error/not available (FF).",
        None,
    ),
    "min_gt_avg": ("CellTempMin (60) is greater than CellTempAvg (40).", (70, 60, 40, 0x80)),
    "avg_gt_max": ("CellTempAvg (90) is greater than CellTempMax (70).", (70, 20, 90, 0x80)),
    "min_gt_max": ("CellTempMin (70) is greater than CellTempMax (40).", (40, 70, 50, 0x80)),
    "out_of_range": ("CellTempMax is 250, beyond the assumed operating range.", (250, 10, 80, 0x80)),
    "implausible_jump": (
        "Temperature jumps from nominal values to 160, then recovers.",
        (160, 160, 160, 0x80),
    ),
    "high_delta": ("CellTempMax=100 and CellTempMin=10, a 90-degree difference.", (100, 10, 55, 0x80)),
    "temp_stuck": (
        "Temperatures remain fixed at 39 during injection while the counter rolls.",
        (39, 39, 39, 0x80),
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
// Trace length: 100 CAN messages.
// Fault logs have a normal lead-in, a middle fault segment, and normal recovery.
// The timeout fault is a 1.8-second missing-frame gap in the middle.
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


def build_trace(name, fault_frames):
    if name == "normal":
        return [
            (index * FRAME_PERIOD, 40, 24, 32, 0x80, index % 256)
            for index in range(TOTAL_FRAMES)
        ]

    if name == "timeout":
        trace = []
        for index in range(TIMEOUT_LEAD_IN_FRAMES):
            trace.append(normal_frame(index, index))

        recovery_start = (
            trace[-1][0] + TIMEOUT_GAP
        )
        for index in range(TOTAL_FRAMES - TIMEOUT_LEAD_IN_FRAMES):
            counter = (TIMEOUT_LEAD_IN_FRAMES + index) % 256
            timestamp = recovery_start + index * FRAME_PERIOD
            trace.append((timestamp, 40, 24, 32, 0x80, counter))
        return trace

    trace = [
        normal_frame(index, index, variable=(name == "temp_stuck"))
        for index in range(LEAD_IN_FRAMES)
    ]

    for index in range(FAULT_FRAMES):
        timestamp = (LEAD_IN_FRAMES + index) * FRAME_PERIOD
        if name == "counter_error":
            counter = (45 + index) % 256
        elif name == "counter_stuck":
            counter = LEAD_IN_FRAMES
        else:
            counter = (LEAD_IN_FRAMES + index) % 256

        if name == "invalid_quality":
            max_temp, min_temp, avg_temp = 40, 24, 32
            quality = 0x00 if index % 2 == 0 else 0xFF
        elif fault_frames is None:
            max_temp, min_temp, avg_temp, quality = 40, 24, 32, 0x80
        else:
            max_temp, min_temp, avg_temp, quality = fault_frames
        trace.append(
            (timestamp, max_temp, min_temp, avg_temp, quality, counter)
        )

    if name == "counter_stuck":
        recovery_counter = LEAD_IN_FRAMES + 1
    elif name == "counter_error":
        recovery_counter = 45 + FAULT_FRAMES
    else:
        recovery_counter = LEAD_IN_FRAMES + FAULT_FRAMES

    for index in range(RECOVERY_FRAMES):
        absolute_index = LEAD_IN_FRAMES + FAULT_FRAMES + index
        trace.append(
            normal_frame(absolute_index, recovery_counter + index,
                         variable=(name == "temp_stuck"))
        )
    return trace


def normal_frame(index, counter, variable=False):
    if variable:
        max_temp = 38 + index % 3
        min_temp = 24 + index % 2
        avg_temp = 31 + index % 2
    else:
        max_temp, min_temp, avg_temp = 40, 24, 32
    timestamp = index * FRAME_PERIOD
    return (timestamp, max_temp, min_temp, avg_temp, 0x80, counter % 256)


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
            trace = build_trace(name, frames)
            log_file.write(render_log(filename, purpose, trace))
        print("Wrote {}".format(output))


if __name__ == "__main__":
    main()
