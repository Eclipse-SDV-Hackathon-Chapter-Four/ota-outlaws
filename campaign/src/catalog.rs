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

//! The scenario catalog (`campaign/scenarios.toml`): one test case per
//! scenario, with its stimulus, its fault onset, and its expectations.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::onset::Onset;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Budget parameters that are not in the Guardian's parameter file, in
    /// milliseconds, for example `T_react` and `T_diag`.
    pub budgets: BTreeMap<String, u64>,
    #[serde(rename = "scenario")]
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub description: String,
    pub fault_class: String,
    pub hazard: Option<String>,
    pub safety_goal: Option<String>,
    /// Test scenarios of the HARA this scenario implements, for example TS-04.
    #[serde(default)]
    pub hara_tests: Vec<String>,
    /// `implemented`: every requirement the scenario checks is implemented,
    /// so a failure is a defect. `planned`: the scenario checks a planned
    /// requirement and is expected to fail until it is implemented.
    pub status: ScenarioStatus,
    /// Why the scenario expects what it expects, shown in the report.
    pub note: Option<String>,
    pub stimulus: Stimulus,
    pub onset: Onset,
    #[serde(rename = "expect")]
    pub expectations: Vec<Expectation>,
    /// Read by the trace generator, not by this tool.
    pub generate: Option<toml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioStatus {
    Implemented,
    Planned,
}

/// How the fault gets into the system.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Stimulus {
    /// Replay a CAN trace once through the KUKSA CAN Provider.
    CanTrace {
        trace: String,
        /// Services to pause after the trace has started, and for how long,
        /// for example the DFM and OpenSOVD for a diagnostics outage.
        #[serde(default)]
        pause: Vec<String>,
        pause_after_ms: Option<u64>,
        pause_for_ms: Option<u64>,
        /// Services to stop for good after the trace has started, for example
        /// the CAN provider to shut the source down.
        #[serde(default)]
        stop: Vec<String>,
        stop_after_ms: Option<u64>,
        /// A service to cut off from the network for a while, for example the
        /// Guardian for a dropout between the publisher and the Guardian.
        isolate: Option<String>,
        isolate_after_ms: Option<u64>,
        isolate_for_ms: Option<u64>,
    },
    /// Start the chain without any temperature source and record for
    /// `duration_ms`.
    NoSource { duration_ms: u64 },
    /// The tool does not inject; someone else does, for example by unplugging
    /// the hardware source. Only usable with `campaign observe`.
    External,
}

/// One expected (or forbidden) reaction.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expectation {
    /// The Guardian reports this fault within the budget after the onset.
    Fault {
        dtc: String,
        budget: String,
        requirement: String,
    },
    /// The fault sets monitoring to DEGRADED and requests the "monitoring
    /// unavailable" warning, linked by cause.
    Degraded { dtc: String, requirement: String },
    /// The fault is visible in OpenSOVD with this run's session and event ID,
    /// within the budget after the Guardian reported it.
    Sovd {
        dtc: String,
        budget: String,
        requirement: String,
    },
    /// The fault recovers, monitoring returns to OK, and OpenSOVD shows the
    /// fault as passed with its history kept.
    Recovery { dtc: String, requirement: String },
    /// The thermal state reaches `state` (or a more severe one) within the
    /// budget after `after`, or after the scenario onset.
    Thermal {
        state: String,
        budget: String,
        after: Option<Onset>,
        requirement: String,
    },
    /// The change to CRITICAL causes the driver warning
    /// `DRIVER_WARNING_OVERTEMP`, published within the budget after `after`,
    /// or after the scenario onset.
    DriverWarningOvertemp {
        budget: String,
        after: Option<Onset>,
        requirement: String,
    },
    /// The thermal state never reaches `state` after the onset.
    NotThermal { state: String, requirement: String },
    /// No fault is reported during the scenario.
    NoFault { requirement: String },
    /// With no sample at the Guardian's input, it reports this fault within
    /// the budget after its own start, measured on its own clock.
    StartupFault {
        dtc: String,
        budget: String,
        requirement: String,
    },
    /// Samples keep reaching the tap after the onset, so the source and the
    /// publisher are alive and a loss at the Guardian lies behind the tap.
    SamplesContinue { requirement: String },
    /// The thermal state is never lowered after the onset.
    NotLowered { requirement: String },
    /// The thermal state reaches `state` from valid data, and OpenSOVD shows
    /// `dtc` failed for that change within the budget, with the catalog's
    /// fault type and severity. If the state is lowered again, OpenSOVD
    /// shows the DTC passed with its history kept.
    OvertempDtc {
        dtc: String,
        state: String,
        budget: String,
        requirement: String,
    },
}

impl Expectation {
    pub fn requirement(&self) -> &str {
        match self {
            Expectation::Fault { requirement, .. }
            | Expectation::Degraded { requirement, .. }
            | Expectation::Sovd { requirement, .. }
            | Expectation::Recovery { requirement, .. }
            | Expectation::Thermal { requirement, .. }
            | Expectation::DriverWarningOvertemp { requirement, .. }
            | Expectation::NotThermal { requirement, .. }
            | Expectation::NoFault { requirement }
            | Expectation::StartupFault { requirement, .. }
            | Expectation::SamplesContinue { requirement }
            | Expectation::NotLowered { requirement }
            | Expectation::OvertempDtc { requirement, .. } => requirement,
        }
    }
}

#[derive(Debug)]
pub enum CatalogError {
    Parse(toml::de::Error),
    Invalid(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Parse(error) => write!(f, "invalid scenario catalog: {error}"),
            CatalogError::Invalid(message) => write!(f, "invalid scenario catalog: {message}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl Catalog {
    pub fn from_toml_str(text: &str) -> Result<Self, CatalogError> {
        let catalog: Catalog = toml::from_str(text).map_err(CatalogError::Parse)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn scenario(&self, id: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|scenario| scenario.id == id)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        let mut seen = std::collections::BTreeSet::new();
        for scenario in &self.scenarios {
            if !seen.insert(&scenario.id) {
                return Err(CatalogError::Invalid(format!(
                    "scenario {} appears twice",
                    scenario.id
                )));
            }
            if scenario.expectations.is_empty() {
                return Err(CatalogError::Invalid(format!(
                    "scenario {} has no expectations",
                    scenario.id
                )));
            }
        }
        Ok(())
    }
}
