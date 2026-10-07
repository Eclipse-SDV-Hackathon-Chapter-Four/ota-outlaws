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

//! Reports: `report.json` per scenario for comparing reruns, `report.md` with
//! the evidence chain, and `campaign.md` over all scenarios of a campaign.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::catalog::{Scenario, ScenarioStatus};
use crate::evaluate::{Evaluation, Outcome, Verdict};

/// Facts about a run, written when it starts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub run_id: String,
    pub scenario: String,
    /// `run`: the tool injected the fault. `observe`: someone else did.
    pub mode: String,
    pub started_at: String,
    pub git_revision: Option<String>,
    pub stimulus: serde_json::Value,
    /// SHA-256 of the files the result depends on.
    pub inputs: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct Report<'a> {
    pub manifest: &'a Manifest,
    pub hazard: Option<&'a str>,
    pub safety_goal: Option<&'a str>,
    pub description: &'a str,
    pub note: Option<&'a str>,
    #[serde(flatten)]
    pub evaluation: &'a Evaluation,
}

fn symbol(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "PASS",
        Verdict::Fail => "FAIL",
        Verdict::Inconclusive => "INCONCLUSIVE",
    }
}

fn seconds(ms: u64) -> String {
    format!("{:.2} s", ms as f64 / 1000.0)
}

pub fn markdown(manifest: &Manifest, scenario: &Scenario, evaluation: &Evaluation) -> String {
    let mut out = String::new();
    let planned = if evaluation.status == ScenarioStatus::Planned {
        " (checks a planned requirement)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "# {} — {}{planned}\n",
        scenario.id,
        symbol(evaluation.verdict)
    );
    let _ = writeln!(out, "{}\n", evaluation.reason);
    let _ = writeln!(out, "| | |\n|---|---|");
    let _ = writeln!(out, "| Run | `{}` ({}) |", manifest.run_id, manifest.mode);
    let _ = writeln!(out, "| Started | {} |", manifest.started_at);
    let _ = writeln!(
        out,
        "| Hazard → safety goal | {} → {} |",
        scenario.hazard.as_deref().unwrap_or("—"),
        scenario.safety_goal.as_deref().unwrap_or("—")
    );
    let _ = writeln!(
        out,
        "| Injected fault | {} ({}) |",
        scenario.description, scenario.fault_class
    );
    if let Some(session) = &evaluation.session_id {
        let _ = writeln!(out, "| Guardian session | `{session}` |");
    }
    let _ = writeln!(
        out,
        "| Observed | {} samples, {} Guardian events |",
        evaluation.samples, evaluation.guardian_events
    );
    match &evaluation.onset {
        Some(onset) => {
            let _ = writeln!(
                out,
                "| Onset t0 | {} — {} |",
                seconds(onset.t_ms),
                onset.description
            );
        }
        None => {
            let _ = writeln!(out, "| Onset t0 | not observed |");
        }
    }
    if let Some(note) = &scenario.note {
        let _ = writeln!(out, "| Note | {note} |");
    }

    let _ = writeln!(out, "\n## Evidence chain\n");
    let _ = writeln!(
        out,
        "| Requirement | Expectation | Observed | Latency | Budget | Result |\n|---|---|---|---|---|---|"
    );
    for check in &evaluation.checks {
        let expectation = serde_json::to_value(&check.expectation)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
            .unwrap_or_default();
        let (result, detail) = match &check.outcome {
            Outcome::Met { detail } => ("✓", detail),
            Outcome::Failed { detail } => ("✗ FAIL", detail),
            Outcome::Unobservable { detail } => ("? INCONCLUSIVE", detail),
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            check.expectation.requirement(),
            expectation,
            detail,
            check.latency_ms.map(seconds).unwrap_or_default(),
            check.budget_ms.map(seconds).unwrap_or_default(),
            result
        );
    }
    if !evaluation.violations.is_empty() {
        let _ = writeln!(out, "\n## Forbidden reactions\n");
        for violation in &evaluation.violations {
            let _ = writeln!(
                out,
                "- **{}** ({}): {}",
                violation.rule, violation.requirement, violation.detail
            );
        }
    }
    let _ = writeln!(out, "\n## Result per requirement\n");
    for (requirement, verdict) in &evaluation.requirements {
        let _ = writeln!(out, "- {requirement}: {}", symbol(*verdict));
    }
    let _ = writeln!(out, "\n## Inputs\n");
    if let Some(revision) = &manifest.git_revision {
        let _ = writeln!(out, "- git revision `{revision}`");
    }
    for (name, hash) in &manifest.inputs {
        let _ = writeln!(out, "- `{name}` sha256 `{hash}`");
    }
    out
}

pub struct Summary<'a> {
    pub run_dir: String,
    pub evaluation: &'a Evaluation,
}

pub fn campaign_markdown(campaign_id: &str, summaries: &[Summary<'_>]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Campaign {campaign_id}\n");
    let _ = writeln!(
        out,
        "| Scenario | Requirements | Verdict | Reason | Report |\n|---|---|---|---|---|"
    );
    for summary in summaries {
        let e = summary.evaluation;
        let verdict = match e.status {
            ScenarioStatus::Planned => format!("{} (planned)", symbol(e.verdict)),
            ScenarioStatus::Implemented => symbol(e.verdict).to_owned(),
        };
        let requirements: Vec<_> = e.requirements.keys().cloned().collect();
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | [report]({}/report.md) |",
            e.scenario,
            requirements.join(", "),
            verdict,
            e.reason,
            summary.run_dir
        );
    }
    let _ = writeln!(
        out,
        "\nScenarios marked *planned* check requirements that are not implemented yet; they are expected to fail and are reported anyway."
    );
    out
}
