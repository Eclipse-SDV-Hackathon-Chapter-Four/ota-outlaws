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

//! Judges a recording against a scenario's expectations. A pure function of
//! its inputs: no I/O, no clock. The verdict rules are those of the Safety
//! Concept (scenario verdicts).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::catalog::{Expectation, Scenario, ScenarioStatus};
use crate::onset::{Found, Onset, OnsetParams};
use crate::recording::{EventKind, GuardianEvent, Observation, Tap, Temperature};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Inconclusive,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    /// The expected reaction was observed in time.
    Met { detail: String },
    /// The reaction is missing, late, or forbidden: FAIL.
    Failed { detail: String },
    /// The evidence needed to judge is missing: INCONCLUSIVE.
    Unobservable { detail: String },
}

impl Outcome {
    fn verdict(&self) -> Verdict {
        match self {
            Outcome::Met { .. } => Verdict::Pass,
            Outcome::Failed { .. } => Verdict::Fail,
            Outcome::Unobservable { .. } => Verdict::Inconclusive,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub expectation: Expectation,
    /// Tool time of the observation that met or decided the check.
    pub t_ms: Option<u64>,
    /// Time from the reference (onset or causing event) to that observation.
    pub latency_ms: Option<u64>,
    pub budget_ms: Option<u64>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub rule: String,
    pub requirement: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub scenario: String,
    /// HARA test scenarios this scenario implements.
    pub hara_tests: Vec<String>,
    pub status: ScenarioStatus,
    pub verdict: Verdict,
    pub reason: String,
    pub onset: Option<Found>,
    pub session_id: Option<String>,
    /// The judged period ends with the last sample plus one cycle. Faults
    /// after it come from the end of the stimulus, not from the scenario.
    pub window_end_ms: Option<u64>,
    pub checks: Vec<Check>,
    pub violations: Vec<Violation>,
    pub requirements: BTreeMap<String, Verdict>,
    pub samples: usize,
    pub guardian_events: usize,
}

/// Fault type and severity of each DTC, from the DFM catalog.
pub type Classes = BTreeMap<String, (String, String)>;

/// Whether an OpenSOVD record carries the catalog's fault type and severity
/// in its environment data.
fn classified(body: &serde_json::Value, dtc: &str, classes: &Classes) -> Result<(), String> {
    let Some((fault_type, severity)) = classes.get(dtc) else {
        return Err(format!("{dtc} is not in the DFM catalog"));
    };
    let env = &body["environment_data"];
    if env["fault_type"] == fault_type.as_str() && env["severity"] == severity.as_str() {
        Ok(())
    } else {
        Err(format!(
            "{dtc} has fault type {} and severity {} in OpenSOVD, expected {fault_type} and {severity}",
            env["fault_type"], env["severity"]
        ))
    }
}

/// Values for budget expressions such as `T_stale + T_react`.
pub type Budgets = BTreeMap<String, u64>;

pub fn resolve_budget(expression: &str, budgets: &Budgets) -> Result<u64, String> {
    expression
        .split('+')
        .map(str::trim)
        .map(|term| {
            term.parse::<u64>()
                .ok()
                .or_else(|| budgets.get(term).copied())
                .ok_or_else(|| format!("unknown budget term '{term}' in '{expression}'"))
        })
        .sum()
}

struct Run<'a> {
    samples: Vec<(u64, &'a Temperature)>,
    events: Vec<(u64, &'a GuardianEvent)>,
    sovd: Vec<(u64, &'a str, Option<u16>, &'a serde_json::Value)>,
    injections: Vec<(u64, &'a str)>,
    session_id: Option<String>,
    window_end: Option<u64>,
}

/// The tool logs this injection right before it starts the Guardian. Samples
/// before it cannot have reached the Guardian and are not judged.
pub const GUARDIAN_START: &str = "start_guardian";

/// A delivery this much later than the detection is shown in the report.
const DELIVERY_DELAY_NOTICE_MS: u64 = 50;

impl<'a> Run<'a> {
    fn new(observations: &'a [Observation], cycle_ms: u64) -> Self {
        let mut samples = Vec::new();
        let mut events = Vec::new();
        let mut sovd = Vec::new();
        let mut injections = Vec::new();
        for observation in observations {
            match &observation.tap {
                Tap::BatteryTemperature(sample) => samples.push((observation.t_ms, sample)),
                Tap::GuardianEvent(event) => events.push((observation.t_ms, event)),
                Tap::SovdFault {
                    code,
                    http_status,
                    body,
                } => sovd.push((observation.t_ms, code.as_str(), *http_status, body)),
                Tap::Injection { action, .. } => {
                    injections.push((observation.t_ms, action.as_str()))
                }
            }
        }
        if let Some((start, _)) = injections.iter().rev().find(|(_, a)| *a == GUARDIAN_START) {
            samples.retain(|(t, _)| t >= start);
        }
        let session_id = events.first().map(|(_, e)| e.session_id.clone());
        let window_end = samples.last().map(|(t, _)| t + cycle_ms);
        Run {
            samples,
            events,
            sovd,
            injections,
            session_id,
            window_end,
        }
    }

    /// Maps the Guardian's own time of an event onto the tool's clock. The
    /// offset is the smallest difference between arrival and Guardian time
    /// over all events: the events that arrived without delay.
    fn detection_time(&self, arrival: u64, event: &GuardianEvent) -> u64 {
        let offset = self
            .events
            .iter()
            .filter(|(_, e)| e.session_id == event.session_id)
            .map(|(t, e)| *t as i64 - e.guardian_time_ms as i64)
            .min();
        match offset {
            Some(offset) => {
                let mapped = (event.guardian_time_ms as i64 + offset).max(0) as u64;
                mapped.min(arrival)
            }
            None => arrival,
        }
    }

    fn in_window(&self, t: u64) -> bool {
        self.window_end.is_some_and(|end| t <= end)
    }

    fn sovd_reachable(&self) -> bool {
        self.sovd
            .iter()
            .any(|(_, _, status, _)| *status == Some(200))
    }

    fn first_fault(&self, dtc: &str, after: u64) -> Option<(u64, &'a GuardianEvent)> {
        self.events.iter().copied().find(|(t, e)| {
            *t >= after && matches!(&e.kind, EventKind::FaultDetected { dtc: d, .. } if d == dtc)
        })
    }

    fn caused_by(
        &self,
        cause: &GuardianEvent,
        matches: impl Fn(&EventKind) -> bool,
    ) -> Option<(u64, &'a GuardianEvent)> {
        self.events
            .iter()
            .copied()
            .find(|(_, e)| e.cause_event_id == cause.event_id && matches(&e.kind))
    }

    /// OpenSOVD records of `dtc` that belong to this run's `event`.
    fn sovd_records(&self, dtc: &str, event: &GuardianEvent) -> Vec<(u64, &'a serde_json::Value)> {
        let event_id = event.event_id.to_string();
        self.sovd
            .iter()
            .filter(|(_, code, status, body)| {
                *code == dtc
                    && *status == Some(200)
                    && body["environment_data"]["session_id"] == event.session_id.as_str()
                    && body["environment_data"]["event_id"] == event_id.as_str()
            })
            .map(|(t, _, _, body)| (*t, *body))
            .collect()
    }
}

fn severity(state: &str) -> u8 {
    match state {
        "CLEAR" => 0,
        "MONITORING" => 1,
        "WARNING" => 2,
        "CRITICAL" | "MITIGATING" => 3,
        _ => 0,
    }
}

pub fn evaluate(
    scenario: &Scenario,
    observations: &[Observation],
    budgets: &Budgets,
    params: &OnsetParams,
    classes: &Classes,
) -> Result<Evaluation, String> {
    let run = Run::new(observations, params.cycle_ms);
    let onset = scenario.onset.find(&run.samples, &run.injections, params);

    let mut checks = Vec::new();
    if let Some(found) = &onset {
        for expectation in &scenario.expectations {
            checks.push(check(
                expectation,
                found.t_ms,
                &run,
                budgets,
                params,
                classes,
            )?);
        }
    }
    let violations = match &onset {
        Some(found) => forbidden(scenario, found.t_ms, &run),
        None => Vec::new(),
    };

    let (verdict, reason) = if run.samples.is_empty() && scenario.onset != Onset::NoInput {
        (
            Verdict::Inconclusive,
            "no sample reached the Guardian's input".to_owned(),
        )
    } else if onset.is_none() {
        (
            Verdict::Inconclusive,
            format!(
                "the fault never showed at the Guardian's input (onset '{}')",
                scenario.onset
            ),
        )
    } else if !violations.is_empty() {
        (
            Verdict::Fail,
            format!("{} forbidden reaction(s)", violations.len()),
        )
    } else {
        let worst = checks
            .iter()
            .map(|c| c.outcome.verdict())
            .max()
            .unwrap_or(Verdict::Pass);
        let reason = match worst {
            Verdict::Pass => "every expected reaction observed in time".to_owned(),
            Verdict::Fail => "an expected reaction is missing or late".to_owned(),
            Verdict::Inconclusive => "evidence incomplete".to_owned(),
        };
        (worst, reason)
    };

    let mut requirements: BTreeMap<String, Verdict> = BTreeMap::new();
    for c in &checks {
        let entry = requirements
            .entry(c.expectation.requirement().to_owned())
            .or_insert(Verdict::Pass);
        *entry = (*entry).max(c.outcome.verdict());
    }
    for v in &violations {
        requirements.insert(v.requirement.clone(), Verdict::Fail);
    }
    if onset.is_none() {
        for expectation in &scenario.expectations {
            requirements.insert(expectation.requirement().to_owned(), Verdict::Inconclusive);
        }
    }

    Ok(Evaluation {
        scenario: scenario.id.clone(),
        hara_tests: scenario.hara_tests.clone(),
        status: scenario.status,
        verdict,
        reason,
        onset,
        session_id: run.session_id.clone(),
        window_end_ms: run.window_end,
        checks,
        violations,
        requirements,
        samples: run.samples.len(),
        guardian_events: run.events.len(),
    })
}

fn check(
    expectation: &Expectation,
    t0: u64,
    run: &Run<'_>,
    budgets: &Budgets,
    params: &OnsetParams,
    classes: &Classes,
) -> Result<Check, String> {
    let mut result = Check {
        expectation: expectation.clone(),
        t_ms: None,
        latency_ms: None,
        budget_ms: None,
        outcome: Outcome::Failed {
            detail: String::new(),
        },
    };
    let failed = |detail: String| Outcome::Failed { detail };
    let missing_fault = |dtc: &str| failed(format!("no {dtc} reported after the onset"));

    match expectation {
        Expectation::Fault { dtc, budget, .. } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            result.outcome = match run.first_fault(dtc, t0) {
                Some((arrival, event)) => {
                    // When the Guardian detected the fault, on the tool's
                    // clock. Its event may reach the tap much later, for
                    // example when the Guardian is cut off the network.
                    let detected = run.detection_time(arrival, event);
                    result.t_ms = Some(detected);
                    let latency = detected.saturating_sub(t0);
                    result.latency_ms = Some(latency);
                    let mut text = format!("event #{} {dtc}", event.event_id);
                    if arrival > detected + DELIVERY_DELAY_NOTICE_MS {
                        text += &format!(
                            ", delivered to the tap {:.2} s after detection",
                            (arrival - detected) as f64 / 1000.0
                        );
                    }
                    if latency <= budget {
                        Outcome::Met { detail: text }
                    } else {
                        failed(format!("{text}, late"))
                    }
                }
                None => missing_fault(dtc),
            };
        }
        Expectation::Degraded { dtc, .. } => {
            result.outcome = match run.first_fault(dtc, t0) {
                None => missing_fault(dtc),
                Some((_, fault)) => {
                    let degraded = run.caused_by(fault, |k| {
                        matches!(k, EventKind::MonitoringStatusChanged { current, .. } if current == "DEGRADED")
                    });
                    match degraded {
                        None => failed(format!(
                            "no change to DEGRADED caused by event #{}",
                            fault.event_id
                        )),
                        Some((t_degraded, status)) => {
                            let warning = run.caused_by(status, |k| {
                                matches!(k, EventKind::MitigationRequested { mitigation } if mitigation == "DRIVER_WARNING_MONITORING_UNAVAILABLE")
                            });
                            match warning {
                                None => failed(format!(
                                    "DEGRADED (event #{}) without the monitoring-unavailable warning",
                                    status.event_id
                                )),
                                Some((t, mitigation)) => {
                                    result.t_ms = Some(t);
                                    result.latency_ms = Some(t.saturating_sub(t_degraded));
                                    Outcome::Met {
                                        detail: format!(
                                            "event #{} DEGRADED (cause #{}), event #{} warning (cause #{})",
                                            status.event_id,
                                            fault.event_id,
                                            mitigation.event_id,
                                            status.event_id
                                        ),
                                    }
                                }
                            }
                        }
                    }
                }
            };
        }
        Expectation::Sovd { dtc, budget, .. } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            result.outcome = match run.first_fault(dtc, t0) {
                None => missing_fault(dtc),
                Some(_) if !run.sovd_reachable() => Outcome::Unobservable {
                    detail: "OpenSOVD never answered".to_owned(),
                },
                Some((t_fault, fault)) => {
                    let visible = run
                        .sovd_records(dtc, fault)
                        .into_iter()
                        .find(|(_, body)| body["status"]["testFailed"] == true);
                    match visible {
                        None => failed(format!(
                            "{dtc} of event #{} never failed in OpenSOVD",
                            fault.event_id
                        )),
                        Some((t, body)) if classified(body, dtc, classes).is_err() => {
                            result.t_ms = Some(t);
                            failed(classified(body, dtc, classes).unwrap_err())
                        }
                        Some((t, _)) => {
                            let latency = t.saturating_sub(t_fault);
                            result.t_ms = Some(t);
                            result.latency_ms = Some(latency);
                            let text = format!(
                                "{dtc} testFailed with session and event #{}",
                                fault.event_id
                            );
                            if latency <= budget {
                                Outcome::Met { detail: text }
                            } else {
                                failed(format!("{text}, late"))
                            }
                        }
                    }
                }
            };
        }
        Expectation::Recovery { dtc, .. } => {
            result.outcome = match run.first_fault(dtc, t0) {
                None => missing_fault(dtc),
                Some((_, fault)) => {
                    let recovered = run.caused_by(
                        fault,
                        |k| matches!(k, EventKind::FaultRecovered { dtc: d, .. } if d == dtc),
                    );
                    match recovered {
                        None => failed(format!(
                            "{dtc} of event #{} never recovered",
                            fault.event_id
                        )),
                        Some((t_recovered, recovered)) => {
                            // Guardian order, not arrival order: events can
                            // reach the tap a few milliseconds swapped.
                            let ok = run.events.iter().find(|(_, e)| {
                                e.event_id > recovered.event_id
                                    && matches!(&e.kind, EventKind::MonitoringStatusChanged { previous, current } if previous == "DEGRADED" && current == "OK")
                            });
                            let passed =
                                run.sovd_records(dtc, fault).into_iter().find(|(t, body)| {
                                    *t >= t_recovered
                                        && body["status"]["testFailed"] == false
                                        && body["status"]["testFailedSinceLastClear"] == true
                                });
                            match (ok, passed) {
                                (None, _) => failed(format!(
                                    "event #{} recovered, but monitoring never returned to OK",
                                    recovered.event_id
                                )),
                                (Some(_), None) if !run.sovd_reachable() => Outcome::Unobservable {
                                    detail: "OpenSOVD never answered".to_owned(),
                                },
                                (Some(_), None) => failed(format!(
                                    "event #{} recovered, but OpenSOVD never showed {dtc} as passed with its history",
                                    recovered.event_id
                                )),
                                (Some((t_ok, status)), Some(_)) => {
                                    result.t_ms = Some(*t_ok);
                                    Outcome::Met {
                                        detail: format!(
                                            "event #{} recovered (cause #{}), event #{} DEGRADED → OK, OpenSOVD passed with history",
                                            recovered.event_id, fault.event_id, status.event_id
                                        ),
                                    }
                                }
                            }
                        }
                    }
                }
            };
        }
        Expectation::Thermal {
            state,
            budget,
            after,
            ..
        } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            let reference = match after {
                Some(onset) => onset
                    .find(&run.samples, &run.injections, params)
                    .map(|f| f.t_ms),
                None => Some(t0),
            };
            result.outcome = match reference {
                None => Outcome::Unobservable {
                    detail: format!(
                        "'{}' never showed at the Guardian's input",
                        after.clone().unwrap_or(Onset::None)
                    ),
                },
                Some(reference) => {
                    let reached = run.events.iter().find(|(t, e)| {
                        *t >= reference
                            && matches!(&e.kind, EventKind::ThermalStateChanged { current, .. } if severity(current) >= severity(state))
                    });
                    match reached {
                        None => failed(format!("thermal state never reached {state}")),
                        Some((t, event)) => {
                            let latency = t - reference;
                            result.t_ms = Some(*t);
                            result.latency_ms = Some(latency);
                            let text = match &event.kind {
                                EventKind::ThermalStateChanged { previous, current } => {
                                    format!("event #{} {previous} → {current}", event.event_id)
                                }
                                _ => unreachable!(),
                            };
                            if latency <= budget {
                                Outcome::Met { detail: text }
                            } else {
                                failed(format!("{text}, late"))
                            }
                        }
                    }
                }
            };
        }
        Expectation::OvertempWarning { .. } => {
            let critical = run.events.iter().find(|(t, e)| {
                *t >= t0
                    && matches!(&e.kind, EventKind::ThermalStateChanged { current, .. } if current == "CRITICAL")
            });
            result.outcome = match critical {
                None => failed("thermal state never reached CRITICAL".to_owned()),
                Some((_, change)) => match run.caused_by(change, |k| {
                    matches!(k, EventKind::MitigationRequested { mitigation } if mitigation == "DRIVER_WARNING_OVERTEMP")
                }) {
                    None => failed(format!(
                        "no overtemperature warning caused by event #{}",
                        change.event_id
                    )),
                    Some((t, warning)) => {
                        result.t_ms = Some(t);
                        Outcome::Met {
                            detail: format!(
                                "event #{} overtemperature warning (cause #{})",
                                warning.event_id, change.event_id
                            ),
                        }
                    }
                },
            };
        }
        Expectation::NotThermal { state, .. } => {
            let reached = run.events.iter().find(|(t, e)| {
                *t >= t0
                    && run.in_window(*t)
                    && matches!(&e.kind, EventKind::ThermalStateChanged { current, .. } if severity(current) >= severity(state))
            });
            result.outcome = match reached {
                None => Outcome::Met {
                    detail: format!("thermal state stayed below {state}"),
                },
                Some((t, event)) => {
                    result.t_ms = Some(*t);
                    failed(format!("event #{} reached {state}", event.event_id))
                }
            };
        }
        Expectation::StartupFault { dtc, budget, .. } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            result.outcome = if !run.samples.is_empty() {
                Outcome::Unobservable {
                    detail: format!("{} samples reached the Guardian's input", run.samples.len()),
                }
            } else {
                match run.first_fault(dtc, 0) {
                    None => failed(format!("no {dtc} reported")),
                    Some((t, event)) => {
                        // Measured on the Guardian's own clock from its start:
                        // the tool cannot observe the start itself.
                        result.t_ms = Some(t);
                        result.latency_ms = Some(event.guardian_time_ms);
                        let text = format!(
                            "event #{} {dtc} {} ms after the Guardian started",
                            event.event_id, event.guardian_time_ms
                        );
                        if event.guardian_time_ms <= budget {
                            Outcome::Met { detail: text }
                        } else {
                            failed(format!("{text}, late"))
                        }
                    }
                }
            };
        }
        Expectation::SamplesContinue { .. } => {
            let within = run
                .samples
                .iter()
                .filter(|(t, _)| *t > t0 && *t <= t0 + 1000)
                .count();
            result.outcome = if within >= 5 {
                Outcome::Met {
                    detail: format!(
                        "{within} samples reached the tap in the second after the onset: source and publisher alive"
                    ),
                }
            } else {
                Outcome::Unobservable {
                    detail: format!(
                        "only {within} samples at the tap after the onset: the loss cannot be placed behind the tap"
                    ),
                }
            };
        }
        Expectation::NotLowered { .. } => {
            let lowered = run.events.iter().find(|(t, e)| {
                *t >= t0
                    && run.in_window(*t)
                    && matches!(&e.kind, EventKind::ThermalStateChanged { previous, current } if severity(current) < severity(previous))
            });
            result.outcome = match lowered {
                None => Outcome::Met {
                    detail: "thermal state not lowered".to_owned(),
                },
                Some((t, event)) => {
                    result.t_ms = Some(*t);
                    failed(format!(
                        "event #{} lowered the thermal state",
                        event.event_id
                    ))
                }
            };
        }
        Expectation::OvertempDtc {
            dtc, state, budget, ..
        } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            // The change to `state` from valid data: no fault as its cause.
            let change = run.events.iter().copied().find(|(t, e)| {
                *t >= t0
                    && e.cause_event_id == 0
                    && matches!(&e.kind, EventKind::ThermalStateChanged { previous, current }
                        if severity(current) >= severity(state) && severity(previous) < severity(state))
            });
            result.outcome = match change {
                None => failed(format!(
                    "thermal state never reached {state} from valid data"
                )),
                Some(_) if !run.sovd_reachable() => Outcome::Unobservable {
                    detail: "OpenSOVD never answered".to_owned(),
                },
                Some((t_change, event)) => {
                    let records = run.sovd_records(dtc, event);
                    let visible = records
                        .iter()
                        .find(|(_, body)| body["status"]["testFailed"] == true);
                    match visible {
                        None => failed(format!(
                            "{dtc} of event #{} never failed in OpenSOVD",
                            event.event_id
                        )),
                        Some((t, body)) => {
                            let latency = t.saturating_sub(t_change);
                            result.t_ms = Some(*t);
                            result.latency_ms = Some(latency);
                            let lowered = run.events.iter().any(|(_, e)| {
                                e.event_id > event.event_id
                                    && matches!(&e.kind, EventKind::ThermalStateChanged { previous, current }
                                        if severity(previous) >= severity(state) && severity(current) < severity(state))
                            });
                            let passed = records.iter().any(|(_, b)| {
                                b["status"]["testFailed"] == false
                                    && b["status"]["testFailedSinceLastClear"] == true
                            });
                            if let Err(detail) = classified(body, dtc, classes) {
                                failed(detail)
                            } else if latency > budget {
                                failed(format!(
                                    "{dtc} of event #{} failed in OpenSOVD, late",
                                    event.event_id
                                ))
                            } else if lowered && !passed {
                                failed(format!(
                                    "the thermal state was lowered below {state}, but OpenSOVD never showed {dtc} as passed"
                                ))
                            } else {
                                let (fault_type, sev) = &classes[dtc.as_str()];
                                Outcome::Met {
                                    detail: format!(
                                        "{dtc} failed for event #{} ({fault_type}, {sev}){}",
                                        event.event_id,
                                        if lowered {
                                            ", passed after cooling, history kept"
                                        } else {
                                            ""
                                        }
                                    ),
                                }
                            }
                        }
                    }
                }
            };
        }
        Expectation::NoFault { .. } => {
            let fault = run.events.iter().find(|(t, e)| {
                run.in_window(*t) && matches!(e.kind, EventKind::FaultDetected { .. })
            });
            result.outcome = match fault {
                None => Outcome::Met {
                    detail: "no fault reported".to_owned(),
                },
                Some((t, event)) => {
                    result.t_ms = Some(*t);
                    let dtc = match &event.kind {
                        EventKind::FaultDetected { dtc, .. } => dtc.clone(),
                        _ => unreachable!(),
                    };
                    failed(format!("event #{} {dtc} (false alarm)", event.event_id))
                }
            };
        }
    }
    Ok(result)
}

/// Reactions that are forbidden in every scenario.
fn forbidden(scenario: &Scenario, t0: u64, run: &Run<'_>) -> Vec<Violation> {
    let mut violations = Vec::new();

    let sessions: BTreeSet<&str> = run
        .events
        .iter()
        .map(|(_, e)| e.session_id.as_str())
        .collect();
    if sessions.len() > 1 {
        violations.push(Violation {
            rule: "one_session".to_owned(),
            requirement: "A-4".to_owned(),
            detail: format!("events of {} Guardian sessions in one run", sessions.len()),
        });
    }

    let mut monitoring = "OK".to_owned();
    let mut ordered: Vec<_> = run.events.iter().map(|(_, e)| *e).collect();
    ordered.sort_by_key(|e| e.event_id);
    for event in ordered {
        match &event.kind {
            EventKind::MonitoringStatusChanged { current, .. } => monitoring = current.clone(),
            EventKind::ThermalStateChanged { previous, current }
                if monitoring == "DEGRADED" && severity(current) < severity(previous) =>
            {
                violations.push(Violation {
                    rule: "no_lowering_while_degraded".to_owned(),
                    requirement: "FSR-2.5".to_owned(),
                    detail: format!(
                        "event #{} lowered {previous} → {current} while DEGRADED",
                        event.event_id
                    ),
                });
            }
            _ => {}
        }
    }

    if scenario.onset != Onset::None {
        if let Some((_, event)) = run
            .events
            .iter()
            .find(|(t, e)| *t < t0 && matches!(e.kind, EventKind::FaultDetected { .. }))
        {
            violations.push(Violation {
                rule: "no_fault_before_onset".to_owned(),
                requirement: "SG-4".to_owned(),
                detail: format!(
                    "event #{} reported a fault before the onset (false alarm)",
                    event.event_id
                ),
            });
        }
    }
    violations
}
