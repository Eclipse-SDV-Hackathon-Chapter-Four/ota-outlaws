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

//! Safety parameters of the Guardian.
//!
//! The values live in `config/guardian/safety-params.toml`. The meaning of each
//! parameter is explained in the "Parameters" section of
//! `docs/explanation/safety-concept.md`.

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardianConfig {
    pub thermal: ThermalConfig,
    pub freshness: FreshnessConfig,
    pub stuck: StuckConfig,
}

/// Thresholds for the maximum cell temperature (FSR-1.1, FSR-1.2).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThermalConfig {
    /// `θ_warn` in °C.
    pub warn_c: f32,
    /// `θ_crit` in °C.
    pub critical_c: f32,
}

/// Freshness monitoring (FSR-2.2).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessConfig {
    /// `T_stale` in milliseconds.
    pub stale_timeout_ms: u64,
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
        if self.freshness.stale_timeout_ms == 0 {
            return invalid("freshness.stale_timeout_ms must be greater than zero");
        }
        if self.stuck.timeout_ms == 0 {
            return invalid("stuck.timeout_ms must be greater than zero");
        }
        if !(self.stuck.min_reference_change_c.is_finite()
            && self.stuck.min_reference_change_c > 0.0)
        {
            return invalid("stuck.min_reference_change_c must be greater than zero");
        }
        Ok(())
    }
}
