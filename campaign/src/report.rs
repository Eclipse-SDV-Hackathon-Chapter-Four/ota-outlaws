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
//! the checks and the evidence chain, `campaign.md` over all scenarios of a
//! campaign, and the console lines printed while a campaign runs.
//!
//! Every report leads with the verdict and the checks that decided it, failed
//! checks first; the evidence chain and the raw event list follow.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::catalog::{Expectation, Scenario, ScenarioStatus};
use crate::evaluate::{Chain, Check, Evaluation, LinkState, Outcome, TimelineEntry, Verdict};

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

/// The verdict with a mark that stands out in a list: ✓ PASS, ✗ FAIL,
/// ? INCONCLUSIVE.
pub fn marked(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "✓ PASS",
        Verdict::Fail => "✗ FAIL",
        Verdict::Inconclusive => "? INCONCLUSIVE",
    }
}

fn outcome_mark(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Met { .. } => "✓",
        Outcome::Failed { .. } => "✗",
        Outcome::Unobservable { .. } => "?",
    }
}

fn outcome_detail(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Met { detail } | Outcome::Failed { detail } | Outcome::Unobservable { detail } => {
            detail
        }
    }
}

/// What a check expects, in words, with its budget expression.
pub fn describe(expectation: &Expectation) -> String {
    let after = |after: &Option<crate::onset::Onset>| match after {
        Some(onset) => format!(" after '{onset}'"),
        None => String::new(),
    };
    match expectation {
        Expectation::Fault { dtc, budget, .. } => format!("{dtc} reported within {budget}"),
        Expectation::Degraded { dtc, .. } => {
            format!("{dtc} sets DEGRADED and requests the monitoring-unavailable warning")
        }
        Expectation::Sovd { dtc, budget, .. } => {
            format!("{dtc} visible in OpenSOVD with this run's IDs within {budget}")
        }
        Expectation::Recovery { dtc, .. } => {
            format!("{dtc} recovers, monitoring returns to OK, OpenSOVD shows it passed with history")
        }
        Expectation::Thermal {
            state,
            budget,
            after: a,
            ..
        } => format!("thermal state reaches {state} within {budget}{}", after(a)),
        Expectation::DriverWarningOvertemp {
            budget, after: a, ..
        } => format!(
            "DRIVER_WARNING_OVERTEMP requested within {budget}{}",
            after(a)
        ),
        Expectation::NotThermal { state, .. } => format!("thermal state never reaches {state}"),
        Expectation::NoFault { .. } => "no fault reported".to_owned(),
        Expectation::StartupFault { dtc, budget, .. } => {
            format!("{dtc} reported within {budget} of the Guardian's start")
        }
        Expectation::SamplesContinue { .. } => {
            "samples keep reaching the tap, so the loss is behind it".to_owned()
        }
        Expectation::NotLowered { .. } => "thermal state never lowered".to_owned(),
        Expectation::SovdFault { dtc, budget, .. } => {
            format!("{dtc} failed in OpenSOVD within {budget}")
        }
        Expectation::SovdRecovery { dtc, .. } => {
            format!("{dtc} passed again in OpenSOVD, history kept")
        }
        Expectation::InputQuality { quality, .. } => {
            format!("a {quality} sample reaches the Guardian's input")
        }
        Expectation::OvertempDtc {
            dtc, state, budget, ..
        } => format!("{state} raises {dtc} in OpenSOVD within {budget}"),
        Expectation::DiagnosticsOutage { dtc, budget, .. } => format!(
            "{dtc} detected and recovered while diagnostics are paused; lifecycle in OpenSOVD within {budget} after the resume"
        ),
        Expectation::SupervisorWarning { budget, .. } => format!(
            "watchdog requests DRIVER_WARNING_MONITORING_UNAVAILABLE within {budget}"
        ),
        Expectation::SupervisorRestored { .. } => {
            "watchdog reports the Guardian restored".to_owned()
        }
    }
}

/// Failed checks first, then inconclusive ones, then met ones; otherwise in
/// catalog order.
fn ordered(checks: &[Check]) -> Vec<&Check> {
    let rank = |c: &Check| match c.outcome {
        Outcome::Failed { .. } => 0,
        Outcome::Unobservable { .. } => 1,
        Outcome::Met { .. } => 2,
    };
    let mut sorted: Vec<&Check> = checks.iter().collect();
    sorted.sort_by_key(|c| rank(c));
    sorted
}

fn met(evaluation: &Evaluation) -> usize {
    evaluation
        .checks
        .iter()
        .filter(|c| matches!(c.outcome, Outcome::Met { .. }))
        .count()
}

fn timing(check: &Check) -> String {
    match (check.latency_ms, check.budget_ms) {
        (Some(latency), Some(budget)) => format!("{} of {}", seconds(latency), seconds(budget)),
        (Some(latency), None) => seconds(latency),
        (None, Some(budget)) => format!("budget {}", seconds(budget)),
        (None, None) => String::new(),
    }
}

/// The console lines for one judged scenario: a headline, and for a scenario
/// that did not pass, the reason and every check that did not hold.
pub fn console(evaluation: &Evaluation) -> String {
    let mut out = String::new();
    let planned = if evaluation.status == ScenarioStatus::Planned {
        " (planned)"
    } else {
        ""
    };
    let tests = if evaluation.hara_tests.is_empty() {
        String::new()
    } else {
        format!("  [{}]", evaluation.hara_tests.join(", "))
    };
    let _ = write!(
        out,
        "{:<15} {}{tests}{planned}  {}/{} checks met",
        marked(evaluation.verdict),
        evaluation.scenario,
        met(evaluation),
        evaluation.checks.len()
    );
    if evaluation.verdict == Verdict::Pass {
        return out;
    }
    let _ = write!(out, "\n{:<15} {}", "", evaluation.reason);
    for check in ordered(&evaluation.checks) {
        if matches!(check.outcome, Outcome::Met { .. }) {
            continue;
        }
        let timing = timing(check);
        let timing = if timing.is_empty() {
            String::new()
        } else {
            format!(" ({timing})")
        };
        let _ = write!(
            out,
            "\n{:<15} {} {}: {} — {}{timing}",
            "",
            outcome_mark(&check.outcome),
            check.expectation.requirement(),
            describe(&check.expectation),
            outcome_detail(&check.outcome)
        );
    }
    for violation in &evaluation.violations {
        let _ = write!(
            out,
            "\n{:<15} ! forbidden ({}): {}",
            "", violation.requirement, violation.detail
        );
    }
    out
}

/// Counts per verdict, for the console and `campaign.md`.
fn tally(evaluations: &[&Evaluation]) -> String {
    let count = |v: Verdict| evaluations.iter().filter(|e| e.verdict == v).count();
    let planned = evaluations
        .iter()
        .filter(|e| e.status == ScenarioStatus::Planned)
        .count();
    let mut text = format!(
        "{} scenario(s): {} PASS, {} FAIL, {} INCONCLUSIVE",
        evaluations.len(),
        count(Verdict::Pass),
        count(Verdict::Fail),
        count(Verdict::Inconclusive)
    );
    if planned > 0 {
        let _ = write!(text, " ({planned} planned, expected to fail)");
    }
    text
}

/// The console summary after a campaign: counts, then one line per scenario
/// that did not pass.
pub fn console_summary(campaign_id: &str, evaluations: &[&Evaluation]) -> String {
    let mut out = format!("\n── Campaign {campaign_id}: {} ──", tally(evaluations));
    for e in evaluations.iter().filter(|e| e.verdict != Verdict::Pass) {
        let planned = if e.status == ScenarioStatus::Planned {
            " (planned)"
        } else {
            ""
        };
        let _ = write!(
            out,
            "\n{:<15} {}{planned} — {}",
            marked(e.verdict),
            e.scenario,
            e.reason
        );
    }
    out
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
        marked(evaluation.verdict),
        scenario.id,
    );
    let chain = if evaluation.chain.complete {
        "evidence chain complete".to_owned()
    } else {
        let missing: Vec<&str> = evaluation
            .chain
            .links
            .iter()
            .filter(|l| l.state == LinkState::Missing)
            .map(|l| l.link.as_str())
            .collect();
        format!(
            "evidence chain incomplete (missing: {})",
            missing.join(", ")
        )
    };
    let _ = writeln!(
        out,
        "> **{}:** {}. {} of {} checks met; {chain}.\n",
        symbol(evaluation.verdict),
        evaluation.reason,
        met(evaluation),
        evaluation.checks.len()
    );
    let _ = writeln!(out, "| | |\n|---|---|");
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
        cell(&scenario.description),
        scenario.fault_class
    );
    match &evaluation.onset {
        Some(onset) => {
            let _ = writeln!(
                out,
                "| Onset t0 | {} — {} |",
                seconds(onset.t_ms),
                cell(&onset.description)
            );
        }
        None => {
            let _ = writeln!(out, "| Onset t0 | not observed |");
        }
    }
    let requirements: Vec<String> = evaluation
        .requirements
        .iter()
        .map(|(requirement, verdict)| {
            let mark = match verdict {
                Verdict::Pass => "✓",
                Verdict::Fail => "✗",
                Verdict::Inconclusive => "?",
            };
            format!("{requirement} {mark}")
        })
        .collect();
    if !requirements.is_empty() {
        let _ = writeln!(out, "| Requirements | {} |", requirements.join(", "));
    }
    if let Some(note) = &scenario.note {
        let _ = writeln!(out, "| Note | {} |", cell(note));
    }
    let _ = writeln!(
        out,
        "| Run | `{}` ({}), started {} |",
        manifest.run_id, manifest.mode, manifest.started_at
    );
    if let Some(session) = &evaluation.session_id {
        let _ = writeln!(out, "| Guardian session | `{session}` |");
    }
    let _ = writeln!(
        out,
        "| Observed | {} samples, {} Guardian events |",
        evaluation.samples, evaluation.guardian_events
    );

    let _ = writeln!(
        out,
        "\n## Checks: {} of {} met\n",
        met(evaluation),
        evaluation.checks.len()
    );
    if evaluation.checks.is_empty() {
        let _ = writeln!(out, "No check was judged: the onset was not observed.");
    } else {
        let _ = writeln!(
            out,
            "| | Requirement | Check | Observed | Time (of budget) |\n|---|---|---|---|---|"
        );
        for check in ordered(&evaluation.checks) {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                outcome_mark(&check.outcome),
                check.expectation.requirement(),
                cell(&describe(&check.expectation)),
                cell(outcome_detail(&check.outcome)),
                timing(check)
            );
        }
        let _ = writeln!(
            out,
            "\n✓ met, ✗ failed (FAIL), ? evidence missing (INCONCLUSIVE). Times count from t0 or from the event the check names."
        );
    }
    if !evaluation.violations.is_empty() {
        let _ = writeln!(out, "\n## Forbidden reactions\n");
        for violation in &evaluation.violations {
            let _ = writeln!(
                out,
                "- ✗ **{}** ({}): {}",
                violation.rule, violation.requirement, violation.detail
            );
        }
    }

    chain_markdown(&mut out, &evaluation.chain);
    timeline_markdown(&mut out, &evaluation.timeline, evaluation.window_end_ms);
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
                match (d.latency_ms, d.history_ms) {
                    (Some(l), _) => format!("{} after the event", seconds(l)),
                    (None, Some(t)) => format!(
                        "only in its history, first shown at {} (no poll caught it failed)",
                        seconds(t)
                    ),
                    (None, None) => "never".to_owned(),
                },
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

fn timeline_markdown(out: &mut String, timeline: &[TimelineEntry], window_end: Option<u64>) {
    if timeline.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## Guardian events\n");
    let mut window_marked = false;
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
        if let Some(end) = window_end.filter(|end| !window_marked && e.delivered_ms > *end) {
            window_marked = true;
            let _ = writeln!(
                out,
                "| | | {} | | *The trace ended here; the events below are not judged.* | |",
                seconds(end)
            );
        }
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
    let evaluations: Vec<&Evaluation> = summaries.iter().map(|s| s.evaluation).collect();
    let _ = writeln!(out, "# Campaign {campaign_id}\n");
    let _ = writeln!(out, "**{}.**\n", tally(&evaluations));
    let _ = writeln!(
        out,
        "| Verdict | Scenario | HARA test | Checks met | Reason | Evidence chain | Report |\n|---|---|---|---|---|---|---|"
    );
    for summary in summaries {
        let e = summary.evaluation;
        let verdict = match e.status {
            ScenarioStatus::Planned => format!("{} (planned)", marked(e.verdict)),
            ScenarioStatus::Implemented => marked(e.verdict).to_owned(),
        };
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
            "| {verdict} | {} | {} | {}/{} | {} | {chain} | [report]({}/report.md) |",
            e.scenario,
            e.hara_tests.join(", "),
            met(e),
            e.checks.len(),
            cell(&e.reason),
            summary.run_dir
        );
    }

    let failing: Vec<&Evaluation> = evaluations
        .iter()
        .copied()
        .filter(|e| e.verdict != Verdict::Pass)
        .collect();
    if !failing.is_empty() {
        let _ = writeln!(out, "\n## Not passed\n");
        for e in failing {
            let planned = if e.status == ScenarioStatus::Planned {
                " (planned)"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "### {} {}{planned}\n\n{}\n",
                marked(e.verdict),
                e.scenario,
                e.reason
            );
            for check in ordered(&e.checks) {
                if matches!(check.outcome, Outcome::Met { .. }) {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "- {} **{}** {}: {}",
                    outcome_mark(&check.outcome),
                    check.expectation.requirement(),
                    describe(&check.expectation),
                    outcome_detail(&check.outcome)
                );
            }
            for violation in &e.violations {
                let _ = writeln!(
                    out,
                    "- ✗ forbidden **{}** ({}): {}",
                    violation.rule, violation.requirement, violation.detail
                );
            }
            let _ = writeln!(out);
        }
    }
    let _ = writeln!(
        out,
        "\nScenarios marked *planned* check requirements that are not implemented yet; they are expected to fail and are reported anyway."
    );
    out
}
