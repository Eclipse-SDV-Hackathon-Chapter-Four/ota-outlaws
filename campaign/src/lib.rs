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

//! Fault campaigns for the Battery Thermal Guardian: the campaign runner and
//! the evidence collector of the challenge, as two parts of one tool. The
//! design is described in `docs/reference/components/campaign.md`.

pub mod catalog;
pub mod evaluate;
pub mod onset;
pub mod record;
pub mod recording;
pub mod report;
pub mod runner;

use std::path::Path;

use anyhow::Context as _;
use guardian::GuardianConfig;

use crate::catalog::Catalog;
use crate::evaluate::Budgets;
use crate::onset::OnsetParams;

pub const CATALOG: &str = "campaign/scenarios.toml";
pub const SAFETY_PARAMS: &str = "config/guardian/safety-params.toml";
pub const FAULT_CATALOG: &str = "diagnostics/catalog/battery_guardian.json";

/// Everything an evaluation needs besides the recording.
pub struct Context {
    pub catalog: Catalog,
    pub budgets: Budgets,
    pub onset: OnsetParams,
    /// Diagnostic trouble codes the Guardian can report, from the DFM catalog.
    pub fault_codes: Vec<String>,
    /// Fault type and severity of each code, from the DFM catalog.
    pub classes: evaluate::Classes,
}

impl Context {
    pub fn load(repo: &Path) -> anyhow::Result<Self> {
        let catalog =
            Catalog::from_toml_str(&std::fs::read_to_string(repo.join(CATALOG)).context(CATALOG)?)?;
        let config = GuardianConfig::from_toml_str(
            &std::fs::read_to_string(repo.join(SAFETY_PARAMS)).context(SAFETY_PARAMS)?,
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut budgets = catalog.budgets.clone();
        budgets.insert("T_stale".into(), config.freshness.stale_timeout_ms);
        budgets.insert("T_stuck".into(), config.stuck.timeout_ms);
        budgets.insert("T_recover".into(), config.recovery.min_duration_ms);
        let cycle_ms = *budgets
            .get("cycle")
            .context("the catalog's [budgets] needs 'cycle'")?;
        // N_stuck repeated frames at the signal cycle (FSR-2.3).
        budgets.insert(
            "T_counter_stuck".into(),
            u64::from(config.freshness.stuck_repeated_frames) * cycle_ms,
        );
        let onset = OnsetParams {
            cycle_ms,
            stale_ms: config.freshness.stale_timeout_ms,
            stuck_reference_change_c: config.stuck.min_reference_change_c,
        };
        let faults: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(FAULT_CATALOG)).context(FAULT_CATALOG)?,
        )?;
        let classes = faults["faults"]
            .as_array()
            .context("DFM catalog without faults")?
            .iter()
            .filter_map(|fault| {
                Some((
                    fault["id"]["Text"].as_str()?.to_owned(),
                    (
                        fault["category"].as_str()?.to_owned(),
                        fault["severity"].as_str()?.to_owned(),
                    ),
                ))
            })
            .collect();
        let fault_codes = faults["faults"]
            .as_array()
            .context("DFM catalog without faults")?
            .iter()
            .filter_map(|fault| fault["id"]["Text"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        catalog.check_dtcs(&fault_codes)?;
        Ok(Context {
            catalog,
            budgets,
            onset,
            fault_codes,
            classes,
        })
    }
}
