// Copyright (c) 2026 Contributors to the Eclipse Foundation
//
// See the NOTICE file(s) distributed with this work for additional
// information regarding copyright ownership.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// https://www.eclipse.org/legal/epl-2.0
//
// SPDX-License-Identifier: EPL-2.0

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5)

//! uProtocol payloads and topics shared by the VSS Publisher and the Battery
//! Thermal Guardian. The contract is described in `components/contracts/README.md`.

/// uProtocol transport over Zenoh, shared by the Guardian service and the
/// campaign tool. Enabled by the `transport` feature.
#[cfg(feature = "transport")]
pub mod transport;

/// Types generated from `components/contracts/battery_thermal.proto`.
pub mod v1 {
    include!(concat!(
        env!("OUT_DIR"),
        "/ota_outlaws.battery_thermal.v1.rs"
    ));
}

/// Address of a uProtocol topic: the parts of a uProtocol URI.
///
/// Kept as plain values so that this crate does not depend on a uProtocol
/// library version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    pub authority: &'static str,
    pub ue_id: u32,
    pub ue_version_major: u8,
    pub resource_id: u16,
}

/// `BatteryTemperature` messages, published by the VSS Publisher.
pub const BATTERY_TEMPERATURE: Topic = Topic {
    authority: "battery-vss",
    ue_id: 0x9001,
    ue_version_major: 1,
    resource_id: 0x9001,
};

/// `GuardianEvent` messages, published by the Battery Thermal Guardian.
pub const GUARDIAN_EVENTS: Topic = Topic {
    authority: "guardian",
    ue_id: 0x9002,
    ue_version_major: 1,
    resource_id: 0x8001,
};

/// `Heartbeat` messages, published by the Battery Thermal Guardian every
/// `T_hb_period` and watched by the watchdog (FSR-2.7).
pub const GUARDIAN_HEARTBEAT: Topic = Topic {
    authority: "guardian",
    ue_id: 0x9002,
    ue_version_major: 1,
    resource_id: 0x8002,
};

/// `SupervisorEvent` messages, published by the Guardian watchdog when the
/// Guardian's heartbeat is lost or returns (HARA DFR-5). A uEntity of its own,
/// so the warning does not depend on the Guardian.
pub const SUPERVISOR_EVENTS: Topic = Topic {
    authority: "guardian-watchdog",
    ue_id: 0x9003,
    ue_version_major: 1,
    resource_id: 0x8001,
};
