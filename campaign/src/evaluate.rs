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
use crate::recording::{EventKind, GuardianEvent, Observation, SampleRef, Tap, Temperature};

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
    /// Hazard → safety goal → fault → detection → mitigation → DTC → verdict,
    /// linked by the Guardian's session and event IDs.
    pub chain: Chain,
    /// Every Guardian event, in the Guardian's order.
    pub timeline: Vec<TimelineEntry>,
}

/// Whether a link of the evidence chain is backed by evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// Observed and linked to the link before it.
    Present,
    /// The scenario expects it, but it was not observed or not linked.
    Missing,
    /// The scenario expects none, and none was observed.
    NotExpected,
    /// Observed although the scenario expects none: a false alarm.
    Unexpected,
}

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub link: String,
    pub state: LinkState,
    pub evidence: String,
}

/// A detection by the Guardian: a fault, or a thermal state raised to
/// WARNING or more.
#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    pub event_id: u64,
    pub event: String,
    pub dtc: Option<String>,
    pub requirement: Option<String>,
    /// When the Guardian detected it, on the tool's clock.
    pub t_ms: u64,
    /// When its event reached the tap.
    pub delivered_ms: u64,
    pub guardian_time_ms: u64,
    /// From the onset to the detection.
    pub latency_ms: Option<u64>,
    /// The last fresh sample before a fault, or the trigger of a thermal change.
    pub sample: Option<SampleRef>,
    /// The `FaultRecovered` event caused by this detection, and when.
    pub recovered_event_id: Option<u64>,
    pub recovered_ms: Option<u64>,
}

/// A mitigation the Guardian requested, with the events that led to it.
#[derive(Debug, Clone, Serialize)]
pub struct MitigationEvidence {
    pub event_id: u64,
    pub mitigation: String,
    pub t_ms: u64,
    pub delivered_ms: u64,
    /// From the onset to the request.
    pub latency_ms: Option<u64>,
    /// The detection it goes back to through `cause_event_id`, if any.
    pub detection_event_id: Option<u64>,
    /// From the detection (or the first known cause) to the request.
    pub cause_chain: Vec<String>,
}

/// What OpenSOVD showed for one detection: the records whose environment
/// data carry this run's session and the detection's event ID.
#[derive(Debug, Clone, Serialize)]
pub struct DtcEvidence {
    pub dtc: String,
    pub detection_event_id: u64,
    pub symptom: Option<String>,
    /// The first poll that showed the DTC failed for this event.
    pub failed_ms: Option<u64>,
    /// From the event reaching the tap to `failed_ms`.
    pub latency_ms: Option<u64>,
    pub fault_type: Option<String>,
    pub severity: Option<String>,
    /// Status bits and counters of the last record of this event.
    pub status: serde_json::Value,
    pub occurrence_counter: Option<u64>,
    pub environment_data: serde_json::Value,
    /// A later record of the same event shows the test passed again.
    pub passed_later: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chain {
    /// No link is missing. Says nothing about timing: that is the verdict.
    pub complete: bool,
    pub links: Vec<Link>,
    pub detections: Vec<Detection>,
    pub mitigations: Vec<MitigationEvidence>,
    pub diagnostics: Vec<DtcEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub session_id: String,
    pub event_id: u64,
    pub cause_event_id: u64,
    pub delivered_ms: u64,
    pub guardian_time_ms: u64,
    pub event: String,
    pub sample: Option<SampleRef>,
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
    /// The samples the Guardian could have received: from [`GUARDIAN_READY`].
    samples: Vec<(u64, &'a Temperature)>,
    /// Every recorded sample, also those before the Guardian was ready.
    all_samples: Vec<(u64, &'a Temperature)>,
    ready: Option<u64>,
    events: Vec<(u64, &'a GuardianEvent)>,
    sovd: Vec<(u64, &'a str, Option<u16>, &'a serde_json::Value)>,
    injections: Vec<(u64, &'a str)>,
    session_id: Option<String>,
    window_end: Option<u64>,
}

/// The tool logs this injection right before it starts the Guardian. Only for
/// the timeline: the time until [`GUARDIAN_READY`] is the container's start.
pub const GUARDIAN_START: &str = "start_guardian";

/// The tool logs this injection once the Guardian has logged its
/// subscription. Samples before it may not have reached the Guardian and are
/// not judged. Docker delivers the log line with a delay, so the marker can
/// come late, never early.
pub const GUARDIAN_READY: &str = "guardian_ready";

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
        let all_samples = samples.clone();
        let ready = injections
            .iter()
            .rev()
            .find(|(_, a)| *a == GUARDIAN_READY)
            .map(|(t, _)| *t);
        if let Some(ready) = ready {
            samples.retain(|(t, _)| *t >= ready);
        }
        let session_id = events.first().map(|(_, e)| e.session_id.clone());
        let window_end = samples.last().map(|(t, _)| t + cycle_ms);
        Run {
            samples,
            all_samples,
            ready,
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

    /// Where a latency is measured from: the first `after` at the Guardian's
    /// input, or t0 without one. `None` if `after` never showed.
    fn reference(&self, after: &Option<Onset>, t0: u64, params: &OnsetParams) -> Option<u64> {
        match after {
            Some(onset) => onset
                .find(&self.samples, &self.injections, params)
                .map(|f| f.t_ms),
            None => Some(t0),
        }
    }

    /// OpenSOVD records of `dtc`, whoever reported it. For faults without a
    /// Guardian event, such as the watchdog's heartbeat loss.
    fn sovd_records_of(&self, dtc: &str) -> Vec<(u64, &'a serde_json::Value)> {
        self.sovd
            .iter()
            .filter(|(_, code, status, _)| *code == dtc && *status == Some(200))
            .map(|(t, _, _, body)| (*t, *body))
            .collect()
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

    /// OpenSOVD records of any DTC that belong to this run's `event`.
    fn sovd_for_event(&self, event: &GuardianEvent) -> Vec<(u64, &'a str, &'a serde_json::Value)> {
        let event_id = event.event_id.to_string();
        self.sovd
            .iter()
            .filter(|(_, _, status, body)| {
                *status == Some(200)
                    && body["environment_data"]["session_id"] == event.session_id.as_str()
                    && body["environment_data"]["event_id"] == event_id.as_str()
            })
            .map(|(t, code, _, body)| (*t, *code, *body))
            .collect()
    }
}

/// A raise of the thermal state to WARNING or more: the Guardian's detection
/// of a thermal hazard.
fn raises(kind: &EventKind) -> bool {
    matches!(kind, EventKind::ThermalStateChanged { previous, current }
        if severity(current) > severity(previous) && severity(current) >= 2)
}

fn seconds(ms: u64) -> String {
    format!("{:.2} s", ms as f64 / 1000.0)
}

/// Builds the evidence chain of a run. Every link after the fault is found
/// through the Guardian's IDs, never through timing alone: mitigations go
/// back to their detection through `cause_event_id`, and OpenSOVD records
/// belong to a detection through the session and event ID in their
/// environment data.
fn evidence_chain(
    scenario: &Scenario,
    run: &Run<'_>,
    onset: Option<&Found>,
    verdict: Verdict,
    reason: &str,
) -> Chain {
    let t0 = onset.map(|f| f.t_ms);
    let session = run.session_id.as_deref();
    let of_session = |e: &GuardianEvent| Some(e.session_id.as_str()) == session;
    // After the onset, and within the judged window: faults after it come
    // from the end of the stimulus.
    let in_scope =
        |t: u64| t0.is_some_and(|t0| t >= t0) && (run.window_end.is_none() || run.in_window(t));
    let by_id: BTreeMap<u64, &GuardianEvent> = run
        .events
        .iter()
        .filter(|(_, e)| of_session(e))
        .map(|(_, e)| (e.event_id, *e))
        .collect();

    let mut detections: Vec<Detection> = run
        .events
        .iter()
        .filter(|(t, e)| {
            of_session(e)
                && in_scope(*t)
                && (matches!(e.kind, EventKind::FaultDetected { .. }) || raises(&e.kind))
        })
        .map(|(arrival, e)| {
            let t = run.detection_time(*arrival, e);
            let (dtc, requirement) = match &e.kind {
                EventKind::FaultDetected { dtc, requirement } => {
                    (Some(dtc.clone()), Some(requirement.clone()))
                }
                _ => (None, None),
            };
            let recovered = run.events.iter().find(|(_, r)| {
                of_session(r)
                    && r.cause_event_id == e.event_id
                    && matches!(r.kind, EventKind::FaultRecovered { .. })
            });
            Detection {
                event_id: e.event_id,
                event: e.kind.describe(),
                dtc,
                requirement,
                t_ms: t,
                delivered_ms: *arrival,
                guardian_time_ms: e.guardian_time_ms,
                latency_ms: t0.map(|t0| t.saturating_sub(t0)),
                sample: e.sample,
                recovered_event_id: recovered.map(|(_, r)| r.event_id),
                recovered_ms: recovered.map(|(t, r)| run.detection_time(*t, r)),
            }
        })
        .collect();
    detections.sort_by_key(|d| d.event_id);
    let detection_ids: BTreeSet<u64> = detections.iter().map(|d| d.event_id).collect();

    let mut mitigations: Vec<MitigationEvidence> = run
        .events
        .iter()
        .filter(|(_, e)| of_session(e))
        .filter_map(|(arrival, e)| {
            let EventKind::MitigationRequested { mitigation } = &e.kind else {
                return None;
            };
            let mut causes = vec![*e];
            let mut cause = e.cause_event_id;
            while cause != 0 && causes.len() < 64 {
                let Some(event) = by_id.get(&cause) else {
                    break;
                };
                causes.push(event);
                cause = event.cause_event_id;
            }
            causes.reverse();
            let detection = causes
                .iter()
                .position(|c| detection_ids.contains(&c.event_id));
            if detection.is_none() && !in_scope(*arrival) {
                return None;
            }
            let t = run.detection_time(*arrival, e);
            Some(MitigationEvidence {
                event_id: e.event_id,
                mitigation: mitigation.clone(),
                t_ms: t,
                delivered_ms: *arrival,
                latency_ms: t0.map(|t0| t.saturating_sub(t0)),
                detection_event_id: detection.map(|i| causes[i].event_id),
                cause_chain: causes[detection.unwrap_or(0)..]
                    .iter()
                    .map(|c| format!("#{} {}", c.event_id, c.kind.describe()))
                    .collect(),
            })
        })
        .collect();
    mitigations.sort_by_key(|m| m.event_id);

    let mut diagnostics = Vec::new();
    for detection in &detections {
        let Some(event) = by_id.get(&detection.event_id) else {
            continue;
        };
        let records = run.sovd_for_event(event);
        let mut codes: Vec<&str> = records.iter().map(|(_, code, _)| *code).collect();
        codes.sort_unstable();
        codes.dedup();
        if codes.is_empty() {
            // A detected fault that never showed in OpenSOVD is evidence too.
            if let Some(dtc) = &detection.dtc {
                diagnostics.push(DtcEvidence {
                    dtc: dtc.clone(),
                    detection_event_id: detection.event_id,
                    symptom: None,
                    failed_ms: None,
                    latency_ms: None,
                    fault_type: None,
                    severity: None,
                    status: serde_json::Value::Null,
                    occurrence_counter: None,
                    environment_data: serde_json::Value::Null,
                    passed_later: false,
                });
            }
            continue;
        }
        for code in codes {
            let of_code: Vec<&(u64, &str, &serde_json::Value)> =
                records.iter().filter(|(_, c, _)| *c == code).collect();
            let failed = of_code
                .iter()
                .find(|(_, _, body)| body["status"]["testFailed"] == true);
            let last = of_code.last().map(|(_, _, body)| *body);
            let shown = failed.map(|(_, _, body)| *body).or(last);
            let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
            diagnostics.push(DtcEvidence {
                dtc: code.to_owned(),
                detection_event_id: detection.event_id,
                symptom: shown.and_then(|b| text(&b["symptom"])),
                failed_ms: failed.map(|(t, _, _)| *t),
                latency_ms: failed.map(|(t, _, _)| t.saturating_sub(detection.delivered_ms)),
                fault_type: shown.and_then(|b| text(&b["environment_data"]["fault_type"])),
                severity: shown.and_then(|b| text(&b["environment_data"]["severity"])),
                status: last
                    .map(|b| b["status"].clone())
                    .unwrap_or(serde_json::Value::Null),
                occurrence_counter: last.and_then(|b| b["occurrence_counter"].as_u64()),
                environment_data: shown
                    .map(|b| b["environment_data"].clone())
                    .unwrap_or(serde_json::Value::Null),
                passed_later: failed.is_some_and(|(t, _, _)| {
                    of_code
                        .iter()
                        .any(|(later, _, body)| later > t && body["status"]["testFailed"] == false)
                }),
            });
        }
    }

    let expects = |f: &dyn Fn(&Expectation) -> bool| scenario.expectations.iter().any(f);
    // A thermal expectation below WARNING (the nominal MONITORING) is normal
    // operation, not a detection.
    let detection_expected = expects(&|e| {
        matches!(
            e,
            Expectation::Fault { .. }
                | Expectation::Degraded { .. }
                | Expectation::Sovd { .. }
                | Expectation::Recovery { .. }
                | Expectation::StartupFault { .. }
                | Expectation::DriverWarningOvertemp { .. }
                | Expectation::OvertempDtc { .. }
        ) || matches!(e, Expectation::Thermal { state, .. } if severity(state) >= 2)
    });
    let mitigation_expected = expects(&|e| {
        matches!(
            e,
            Expectation::Degraded { .. } | Expectation::DriverWarningOvertemp { .. }
        )
    });
    let diagnostics_expected = expects(&|e| {
        matches!(
            e,
            Expectation::Sovd { .. }
                | Expectation::Recovery { .. }
                | Expectation::OvertempDtc { .. }
        )
    });
    let nominal = scenario.fault_class == "None";

    let mut links = Vec::new();
    let assigned = |name: &str, value: &Option<String>| match value {
        Some(id) => link(name, LinkState::Present, id.clone()),
        None if nominal => link(name, LinkState::NotExpected, "nominal scenario".to_owned()),
        None => link(
            name,
            LinkState::Missing,
            "not assigned in the scenario catalog".to_owned(),
        ),
    };
    links.push(assigned("Hazard", &scenario.hazard));
    links.push(assigned("Safety goal", &scenario.safety_goal));
    links.push(match onset {
        Some(found) => link(
            "Fault",
            LinkState::Present,
            format!(
                "{} ({}); t0 = {}: {}",
                scenario.description,
                scenario.fault_class,
                seconds(found.t_ms),
                found.description
            ),
        ),
        None => link(
            "Fault",
            LinkState::Missing,
            format!(
                "{} ({}); never showed at the Guardian's input",
                scenario.description, scenario.fault_class
            ),
        ),
    });

    let detected: Vec<String> = detections
        .iter()
        .map(|d| match d.latency_ms {
            Some(latency) => format!("#{} {} after {}", d.event_id, d.event, seconds(latency)),
            None => format!("#{} {}", d.event_id, d.event),
        })
        .collect();
    links.push(match (detection_expected, detected.is_empty()) {
        (true, false) => link("Detection", LinkState::Present, detected.join("; ")),
        (false, false) => link("Detection", LinkState::Unexpected, detected.join("; ")),
        (true, true) => link(
            "Detection",
            LinkState::Missing,
            "no detection by the Guardian after the onset".to_owned(),
        ),
        (false, true) => link(
            "Detection",
            LinkState::NotExpected,
            "none expected, none observed".to_owned(),
        ),
    });

    let linked: Vec<String> = mitigations
        .iter()
        .filter(|m| m.detection_event_id.is_some())
        .map(|m| {
            let via = if m.cause_chain.len() > 1 {
                format!(
                    ", via {}",
                    m.cause_chain[..m.cause_chain.len() - 1].join(" → ")
                )
            } else {
                String::new()
            };
            let after = m
                .latency_ms
                .map(|l| format!(" after {}", seconds(l)))
                .unwrap_or_default();
            format!("#{} {}{after}{via}", m.event_id, m.mitigation)
        })
        .collect();
    let unlinked = mitigations.iter().any(|m| m.detection_event_id.is_none());
    links.push(match (mitigation_expected, linked.is_empty()) {
        (_, false) => link("Mitigation", LinkState::Present, linked.join("; ")),
        (true, true) if unlinked => link(
            "Mitigation",
            LinkState::Missing,
            "mitigation requested, but not caused by a detection".to_owned(),
        ),
        (true, true) => link(
            "Mitigation",
            LinkState::Missing,
            "no mitigation requested after the detection".to_owned(),
        ),
        (false, true) => link(
            "Mitigation",
            LinkState::NotExpected,
            "none expected, none requested".to_owned(),
        ),
    });

    let visible: Vec<String> = diagnostics
        .iter()
        .filter_map(|d| {
            let latency = d.latency_ms?;
            Some(format!(
                "{} failed in OpenSOVD {} after event #{} ({}, {})",
                d.dtc,
                seconds(latency),
                d.detection_event_id,
                d.severity.as_deref().unwrap_or("severity unknown"),
                d.fault_type.as_deref().unwrap_or("fault type unknown"),
            ))
        })
        .collect();
    links.push(match (diagnostics_expected, visible.is_empty()) {
        (_, false) => link("DTC in OpenSOVD", LinkState::Present, visible.join("; ")),
        (true, true) if !run.sovd_reachable() => link(
            "DTC in OpenSOVD",
            LinkState::Missing,
            "OpenSOVD never answered".to_owned(),
        ),
        (true, true) => link(
            "DTC in OpenSOVD",
            LinkState::Missing,
            "no DTC failed in OpenSOVD with this run's session and event ID".to_owned(),
        ),
        (false, true) => link(
            "DTC in OpenSOVD",
            LinkState::NotExpected,
            "none expected, none shown".to_owned(),
        ),
    });

    let verdict_text = match verdict {
        Verdict::Pass => "PASS",
        Verdict::Fail => "FAIL",
        Verdict::Inconclusive => "INCONCLUSIVE",
    };
    links.push(link(
        "Verdict",
        LinkState::Present,
        format!("{verdict_text}: {reason}"),
    ));

    Chain {
        complete: links.iter().all(|l| l.state != LinkState::Missing),
        links,
        detections,
        mitigations,
        diagnostics,
    }
}

fn link(name: &str, state: LinkState, evidence: String) -> Link {
    Link {
        link: name.to_owned(),
        state,
        evidence,
    }
}

/// Every Guardian event: the run's session first, each in event-ID order.
fn timeline(run: &Run<'_>) -> Vec<TimelineEntry> {
    let mut entries: Vec<TimelineEntry> = run
        .events
        .iter()
        .map(|(t, e)| TimelineEntry {
            session_id: e.session_id.clone(),
            event_id: e.event_id,
            cause_event_id: e.cause_event_id,
            delivered_ms: *t,
            guardian_time_ms: e.guardian_time_ms,
            event: e.kind.describe(),
            sample: e.sample,
        })
        .collect();
    let first = run.session_id.clone().unwrap_or_default();
    entries.sort_by(|a, b| {
        (a.session_id != first, &a.session_id, a.event_id).cmp(&(
            b.session_id != first,
            &b.session_id,
            b.event_id,
        ))
    });
    entries
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
    let early = early_onset(scenario, &run, params);
    // A fault that began before the Guardian was ready proves nothing.
    let judged = if early.is_none() {
        onset.as_ref()
    } else {
        None
    };

    let mut checks = Vec::new();
    if let Some(found) = judged {
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
    let violations = match judged {
        Some(found) => forbidden(scenario, found.t_ms, &run),
        None => Vec::new(),
    };

    let (verdict, reason) = if observations.is_empty() {
        // Not even OpenSOVD was polled: the run itself failed, for example
        // because the stack did not start. Nothing about the Guardian is known.
        (
            Verdict::Inconclusive,
            "nothing was recorded: the run itself failed".to_owned(),
        )
    } else if run.samples.is_empty() && scenario.onset != Onset::NoInput {
        (
            Verdict::Inconclusive,
            "no sample reached the Guardian's input".to_owned(),
        )
    } else if let (Some(found), Some(ready)) = (&early, run.ready) {
        (
            Verdict::Inconclusive,
            format!(
                "the fault began at {:.2} s, before the Guardian was ready at {:.2} s",
                found.t_ms as f64 / 1000.0,
                ready as f64 / 1000.0
            ),
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
    if judged.is_none() || observations.is_empty() {
        requirements.clear();
        for expectation in &scenario.expectations {
            requirements.insert(expectation.requirement().to_owned(), Verdict::Inconclusive);
        }
    }

    let onset = early.or(onset);
    let chain = evidence_chain(scenario, &run, onset.as_ref(), verdict, &reason);

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
        chain,
        timeline: timeline(&run),
    })
}

/// The onset in all recorded samples, if it came before the Guardian was
/// ready. `none` and `no_input` mean the Guardian's first sample or none, so
/// they cannot come early.
fn early_onset(scenario: &Scenario, run: &Run<'_>, params: &OnsetParams) -> Option<Found> {
    let ready = run.ready?;
    if matches!(scenario.onset, Onset::None | Onset::NoInput) {
        return None;
    }
    scenario
        .onset
        .find(&run.all_samples, &run.injections, params)
        .filter(|found| found.t_ms < ready)
}

/// The onset cannot be judged because it never reached the Guardian's input.
fn never_showed(after: &Option<Onset>) -> Outcome {
    Outcome::Unobservable {
        detail: format!(
            "'{}' never showed at the Guardian's input",
            after.clone().unwrap_or(Onset::None)
        ),
    }
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
        Expectation::SovdFault { dtc, budget, .. } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            result.outcome = if !run.sovd_reachable() {
                Outcome::Unobservable {
                    detail: "OpenSOVD never answered".to_owned(),
                }
            } else {
                let failed_record = run
                    .sovd_records_of(dtc)
                    .into_iter()
                    .find(|(t, body)| *t >= t0 && body["status"]["testFailed"] == true);
                match failed_record {
                    None => failed(format!("{dtc} never failed in OpenSOVD after the onset")),
                    Some((t, _)) => {
                        let latency = t.saturating_sub(t0);
                        result.t_ms = Some(t);
                        result.latency_ms = Some(latency);
                        let text = format!("{dtc} testFailed in OpenSOVD");
                        if latency <= budget {
                            Outcome::Met { detail: text }
                        } else {
                            failed(format!("{text}, late"))
                        }
                    }
                }
            };
        }
        Expectation::SovdRecovery { dtc, .. } => {
            result.outcome = if !run.sovd_reachable() {
                Outcome::Unobservable {
                    detail: "OpenSOVD never answered".to_owned(),
                }
            } else {
                let records = run.sovd_records_of(dtc);
                let first_failed = records
                    .iter()
                    .find(|(t, body)| *t >= t0 && body["status"]["testFailed"] == true)
                    .map(|(t, _)| *t);
                match first_failed {
                    None => failed(format!("{dtc} never failed in OpenSOVD after the onset")),
                    Some(t_failed) => {
                        let passed = records.iter().find(|(t, body)| {
                            *t > t_failed
                                && body["status"]["testFailed"] == false
                                && body["status"]["testFailedSinceLastClear"] == true
                        });
                        match passed {
                            None => failed(format!(
                                "{dtc} never showed as passed with its history after it failed"
                            )),
                            Some((t, _)) => {
                                result.t_ms = Some(*t);
                                Outcome::Met {
                                    detail: format!("{dtc} passed again, history kept"),
                                }
                            }
                        }
                    }
                }
            };
        }
        Expectation::InputQuality { quality, .. } => {
            let seen = run
                .samples
                .iter()
                .find(|(t, sample)| *t >= t0 && sample.quality == *quality);
            result.outcome = match seen {
                Some((t, sample)) => {
                    result.t_ms = Some(*t);
                    Outcome::Met {
                        detail: format!(
                            "a sample with quality {quality} reached the input (sequence {})",
                            sample.sequence
                        ),
                    }
                }
                None => failed(format!(
                    "no sample with quality {quality} reached the Guardian's input after the onset"
                )),
            };
        }
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
            result.outcome = match run.reference(after, t0, params) {
                None => never_showed(after),
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
        Expectation::DriverWarningOvertemp { budget, after, .. } => {
            let budget = resolve_budget(budget, budgets)?;
            result.budget_ms = Some(budget);
            result.outcome = match run.reference(after, t0, params) {
                None => never_showed(after),
                Some(reference) => {
                    let critical = run.events.iter().find(|(t, e)| {
                        *t >= reference
                            && matches!(&e.kind, EventKind::ThermalStateChanged { current, .. } if current == "CRITICAL")
                    });
                    match critical {
                        None => failed("thermal state never reached CRITICAL".to_owned()),
                        Some((_, change)) => match run.caused_by(change, |k| {
                            matches!(k, EventKind::MitigationRequested { mitigation } if mitigation == "DRIVER_WARNING_OVERTEMP")
                        }) {
                            None => failed(format!(
                                "no DRIVER_WARNING_OVERTEMP caused by event #{}",
                                change.event_id
                            )),
                            Some((t, warning)) => {
                                let latency = t.saturating_sub(reference);
                                result.t_ms = Some(t);
                                result.latency_ms = Some(latency);
                                let text = format!(
                                    "event #{} DRIVER_WARNING_OVERTEMP (cause #{})",
                                    warning.event_id, change.event_id
                                );
                                if latency <= budget {
                                    Outcome::Met { detail: text }
                                } else {
                                    failed(format!("{text}, late"))
                                }
                            }
                        },
                    }
                }
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
