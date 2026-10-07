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

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5); Codex / GPT-6.1 Sol (gpt-6.1-sol)

//! Battery Thermal Guardian core.
//!
//! A deterministic, I/O-free implementation of the Guardian's safety behavior.
//! The requirements it implements are defined in
//! `docs/reference/hara.md`; the design is described in
//! `docs/reference/components/battery-thermal-guardian.md`.

mod config;
mod detectors;
mod guardian;
mod model;

pub use config::{
    ConfigError, FreshnessConfig, GuardianConfig, PlausibilityConfig, RecoveryConfig, StuckConfig,
    ThermalConfig,
};
pub use guardian::Guardian;
pub use model::{
    Event, EventId, EventKind, FaultCode, Millis, Mitigation, MonitoringStatus, Quality, Sample,
    SampleRef, ThermalState,
};
