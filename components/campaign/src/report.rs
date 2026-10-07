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
use crate::evaluate::{Chain, Evaluation, LinkState, Outcome, TimelineEntry, Verdict};

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
    if !scenario.hara_tests.is_empty() {
        let _ = writeln!(out, "| HARA test | {} |", scenario.hara_tests.join(", "));
    }
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

    chain_markdown(&mut out, &evaluation.chain);

    let _ = writeln!(out, "\n## Checks\n");
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
    timeline_markdown(&mut out, &evaluation.timeline);
    let _ = writeln!(out, "\n## Inputs\n");
    if let Some(revision) = &manifest.git_revision {
        let _ = writeln!(out, "- git revision `{revision}`");
    }
    for (name, hash) in &manifest.inputs {
        let _ = writeln!(out, "- `{name}` sha256 `{hash}`");
    }
    out
}

/// A table cell: no line breaks, `|` escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn sample(sample: &Option<crate::recording::SampleRef>) -> String {
    match sample {
        Some(s) => format!("#{} (counter {})", s.sequence, s.alive_counter),
        None => "—".to_owned(),
    }
}

fn chain_markdown(out: &mut String, chain: &Chain) {
    let state = if chain.complete {
        "complete"
    } else {
        "incomplete"
    };
    let _ = writeln!(out, "\n## Evidence chain: {state}\n");
    let _ = writeln!(
        out,
        "Hazard → safety goal → fault → detection → mitigation → DTC → verdict. \
         Detections, mitigations, and DTCs are linked by the Guardian's session \
         and event IDs, not by timing.\n"
    );
    let _ = writeln!(out, "| Link | Evidence | |\n|---|---|---|");
    for link in &chain.links {
        let mark = match link.state {
            LinkState::Present => "✓",
            LinkState::Missing => "✗ missing",
            LinkState::NotExpected => "— not expected",
            LinkState::Unexpected => "! unexpected",
        };
        let _ = writeln!(out, "| {} | {} | {mark} |", link.link, cell(&link.evidence));
    }

    if !chain.detections.is_empty() {
        let _ = writeln!(out, "\n### Detection\n");
        let _ = writeln!(
            out,
            "| Event | Detection | After t0 | Guardian time | Sample | Recovered |\n|---|---|---|---|---|---|"
        );
        for d in &chain.detections {
            let recovered = match (d.recovered_event_id, d.recovered_ms) {
                (Some(id), Some(t)) => format!("#{id} at {}", seconds(t)),
                _ => "—".to_owned(),
            };
            let _ = writeln!(
                out,
                "| #{} | {} | {} | {} ms | {} | {recovered} |",
                d.event_id,
                cell(&d.event),
                d.latency_ms.map(seconds).unwrap_or_default(),
                d.guardian_time_ms,
                sample(&d.sample)
            );
        }
    }

    if !chain.mitigations.is_empty() {
        let _ = writeln!(out, "\n### Mitigation\n");
        let _ = writeln!(
            out,
            "| Event | Mitigation | After t0 | Cause chain |\n|---|---|---|---|"
        );
        for m in &chain.mitigations {
            let causes = if m.detection_event_id.is_some() {
                m.cause_chain.join(" → ")
            } else {
                format!("{} (not caused by a detection)", m.cause_chain.join(" → "))
            };
            let _ = writeln!(
                out,
                "| #{} | {} | {} | {} |",
                m.event_id,
                m.mitigation,
                m.latency_ms.map(seconds).unwrap_or_default(),
                cell(&causes)
            );
        }
    }

    if !chain.diagnostics.is_empty() {
        let _ = writeln!(out, "\n### DTCs in OpenSOVD\n");
        let _ = writeln!(
            out,
            "| DTC | Detection | Failed in OpenSOVD | Severity | Fault type | Status | Occurrences | Passed later |\n|---|---|---|---|---|---|---|---|"
        );
        for d in &chain.diagnostics {
            let status = &d.status;
            let flags: Vec<&str> = [
                "testFailed",
                "confirmedDtc",
                "pendingDtc",
                "testFailedSinceLastClear",
                "warningIndicatorRequested",
            ]
            .into_iter()
            .filter(|flag| status[*flag] == true)
            .collect();
            let status_text = if status.is_null() {
                "never shown".to_owned()
            } else {
                format!(
                    "{} (mask {})",
                    if flags.is_empty() {
                        "no flag set".to_owned()
                    } else {
                        flags.join(", ")
                    },
                    status["mask"].as_str().unwrap_or("?")
                )
            };
            let _ = writeln!(
                out,
                "| {} | #{} | {} | {} | {} | {} | {} | {} |",
                d.dtc,
                d.detection_event_id,
                d.latency_ms
                    .map(|l| format!("{} after the event", seconds(l)))
                    .unwrap_or_else(|| "never".to_owned()),
                d.severity.as_deref().unwrap_or("—"),
                d.fault_type.as_deref().unwrap_or("—"),
                cell(&status_text),
                d.occurrence_counter
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "—".to_owned()),
                if d.passed_later { "yes" } else { "no" }
            );
        }
        for d in &chain.diagnostics {
            if let Some(env) = d.environment_data.as_object() {
                let pairs: Vec<String> = env
                    .iter()
                    .map(|(k, v)| {
                        format!("{k}={}", v.as_str().map_or(v.to_string(), str::to_owned))
                    })
                    .collect();
                let _ = writeln!(
                    out,
                    "\n- `{}` (event #{}) environment data: {}",
                    d.dtc,
                    d.detection_event_id,
                    pairs.join(", ")
                );
            }
        }
    }
}

fn timeline_markdown(out: &mut String, timeline: &[TimelineEntry]) {
    if timeline.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## Guardian events\n");
    let _ = writeln!(
        out,
        "| # | Cause | At the tap | Guardian time | Event | Sample |\n|---|---|---|---|---|---|"
    );
    let first = &timeline[0].session_id;
    for e in timeline {
        let cause = match e.cause_event_id {
            0 => "—".to_owned(),
            id => format!("#{id}"),
        };
        let other = if &e.session_id == first {
            String::new()
        } else {
            format!(" (session {})", e.session_id)
        };
        let _ = writeln!(
            out,
            "| #{} | {cause} | {} | {} ms | {}{other} | {} |",
            e.event_id,
            seconds(e.delivered_ms),
            e.guardian_time_ms,
            cell(&e.event),
            sample(&e.sample)
        );
    }
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
        "| Scenario | HARA test | Requirements | Verdict | Reason | Evidence chain | Report |\n|---|---|---|---|---|---|---|"
    );
    for summary in summaries {
        let e = summary.evaluation;
        let verdict = match e.status {
            ScenarioStatus::Planned => format!("{} (planned)", symbol(e.verdict)),
            ScenarioStatus::Implemented => symbol(e.verdict).to_owned(),
        };
        let requirements: Vec<_> = e.requirements.keys().cloned().collect();
        let missing: Vec<&str> = e
            .chain
            .links
            .iter()
            .filter(|l| l.state == LinkState::Missing)
            .map(|l| l.link.as_str())
            .collect();
        let chain = if missing.is_empty() {
            "complete".to_owned()
        } else {
            format!("missing: {}", missing.join(", "))
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {chain} | [report]({}/report.md) |",
            e.scenario,
            e.hara_tests.join(", "),
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
