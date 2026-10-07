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

//! Safety parameters of the Guardian.
//!
//! The values live in `config/guardian/safety-params.toml`, which explains
//! each parameter. The requirements they parameterize are traced in
//! `docs/reference/hara.md`.

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardianConfig {
    pub thermal: ThermalConfig,
    pub freshness: FreshnessConfig,
    pub stuck: StuckConfig,
    pub plausibility: PlausibilityConfig,
    #[serde(default)]
    pub recovery: RecoveryConfig,
}

/// Plausibility of a sample (FSR-3.2, FSR-3.3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlausibilityConfig {
    /// `θ_min` in °C.
    pub min_c: f32,
    /// `θ_max` in °C.
    pub max_c: f32,
    /// `r_max`: fastest plausible rise of the maximum, in °C per second.
    pub max_rise_c_per_s: f32,
    /// Resolution of the temperature signal in °C. A rise of one step is
    /// always plausible: it can appear at any moment, however close the
    /// source timestamps are.
    pub resolution_c: f32,
    /// `N_suspect` for spikes: rate-implausible samples within
    /// `suspect_window_ms` that lead to DEGRADED. Fewer only set SUSPECT
    /// (FSR-3.5, HARA DFR-4, TS-22, TS-23).
    pub suspect_spikes: u32,
    /// `T_suspect` in milliseconds.
    pub suspect_window_ms: u64,
}

/// Sustained recovery, with hysteresis and a minimum healthy observation period.
/// `valid_samples` also sets how many consecutive fresh, valid samples bring
/// SUSPECT back to OK (HARA DFR-8).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryConfig {
    pub hysteresis_c: f32,
    pub valid_samples: u32,
    pub min_duration_ms: u64,
}
impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            hysteresis_c: 2.0,
            valid_samples: 10,
            min_duration_ms: 1000,
        }
    }
}

/// Thresholds for the maximum cell temperature (FSR-1.1, FSR-1.2) and the
/// rising-trend criterion (FSR-1.3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalConfig {
    /// `θ_warn` in °C.
    pub warn_c: f32,
    /// `θ_crit` in °C.
    pub critical_c: f32,
    /// `r_trend`: sustained rise of the maximum, in °C per second, that raises
    /// WARNING below `θ_warn` (FSR-1.3, HARA F-7, TS-27).
    pub trend_rise_c_per_s: f32,
    /// `T_trend` in milliseconds: how long the rise must be sustained.
    pub trend_duration_ms: u64,
}

/// Freshness monitoring (FSR-2.2) and repeated frames (FSR-2.3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessConfig {
    /// `T_stale` in milliseconds.
    pub stale_timeout_ms: u64,
    /// `N_suspect`: repeated frames that set SUSPECT.
    pub suspect_repeated_frames: u32,
    /// `N_stuck`: repeated frames that lead to DEGRADED (counter stuck).
    pub stuck_repeated_frames: u32,
}

/// Stuck value detection (FSR-2.4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StuckConfig {
    /// `T_stuck` in milliseconds.
    pub timeout_ms: u64,
    /// `Δ_stuck` in °C.
    pub min_reference_change_c: f32,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot parse Guardian configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid Guardian configuration: {0}")]
    Invalid(String),
}

impl GuardianConfig {
    /// Parses and validates a configuration in TOML format.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let config: GuardianConfig = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// Rejects values that would silently disable a safety mechanism.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |message: &str| Err(ConfigError::Invalid(message.to_owned()));

        if !self.thermal.warn_c.is_finite() || !self.thermal.critical_c.is_finite() {
            return invalid("thermal thresholds must be finite");
        }
        if self.thermal.warn_c >= self.thermal.critical_c {
            return invalid("thermal.warn_c must be below thermal.critical_c");
        }
        if !(self.thermal.trend_rise_c_per_s.is_finite() && self.thermal.trend_rise_c_per_s > 0.0) {
            return invalid("thermal.trend_rise_c_per_s must be greater than zero");
        }
        if self.thermal.trend_duration_ms == 0 {
            return invalid("thermal.trend_duration_ms must be greater than zero");
        }
        if self.freshness.stale_timeout_ms == 0 {
            return invalid("freshness.stale_timeout_ms must be greater than zero");
        }
        let freshness = &self.freshness;
        if freshness.suspect_repeated_frames == 0 {
            return invalid("freshness.suspect_repeated_frames must be greater than zero");
        }
        if freshness.stuck_repeated_frames < 2 {
            // A single repeated frame, such as a duplicate, is not a frozen source.
            return invalid("freshness.stuck_repeated_frames must be at least 2");
        }
        if freshness.suspect_repeated_frames > freshness.stuck_repeated_frames {
            return invalid(
                "freshness.suspect_repeated_frames must not exceed freshness.stuck_repeated_frames",
            );
        }
        if self.stuck.timeout_ms == 0 {
            return invalid("stuck.timeout_ms must be greater than zero");
        }
        if !(self.stuck.min_reference_change_c.is_finite()
            && self.stuck.min_reference_change_c > 0.0)
        {
            return invalid("stuck.min_reference_change_c must be greater than zero");
        }
        let plausibility = &self.plausibility;
        if !(plausibility.min_c.is_finite() && plausibility.max_c.is_finite())
            || plausibility.min_c >= plausibility.max_c
        {
            return invalid("plausibility.min_c must be below plausibility.max_c");
        }
        if plausibility.max_c < self.thermal.critical_c {
            // Otherwise a critical temperature could never be valid.
            return invalid("plausibility.max_c must not be below thermal.critical_c");
        }
        if !(plausibility.max_rise_c_per_s.is_finite() && plausibility.max_rise_c_per_s > 0.0) {
            return invalid("plausibility.max_rise_c_per_s must be greater than zero");
        }
        if !(plausibility.resolution_c.is_finite() && plausibility.resolution_c >= 0.0) {
            return invalid("plausibility.resolution_c must not be negative");
        }
        if plausibility.suspect_spikes < 2 {
            // A single spike must only set SUSPECT (FSR-3.5).
            return invalid("plausibility.suspect_spikes must be at least 2");
        }
        if plausibility.suspect_window_ms == 0 {
            return invalid("plausibility.suspect_window_ms must be greater than zero");
        }
        if !self.recovery.hysteresis_c.is_finite()
            || self.recovery.hysteresis_c <= 0.0
            || self.recovery.hysteresis_c >= self.thermal.critical_c - self.thermal.warn_c
        {
            return invalid(
                "recovery.hysteresis_c must be positive and below the thermal threshold gap",
            );
        }
        if self.recovery.valid_samples < 2 || self.recovery.min_duration_ms == 0 {
            return invalid("recovery requires at least two samples and a positive duration");
        }
        Ok(())
    }
}
