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

//! Verdict rules of the Safety Concept, checked on synthetic recordings and
//! the shipped catalog. No Docker, no network.

use std::path::Path;

use campaign::catalog::{Scenario, Stimulus};
use campaign::evaluate::{evaluate, Evaluation, Outcome, Verdict};
use campaign::onset::Onset;
use campaign::recording::{EventKind, GuardianEvent, Observation, Tap, Temperature};
use campaign::Context;

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn context() -> Context {
    Context::load(repo()).expect("shipped catalog and parameters load")
}

fn scenario<'a>(context: &'a Context, id: &str) -> &'a Scenario {
    context.catalog.scenario(id).expect("scenario in catalog")
}

/// Builds a recording like the taps write it.
#[derive(Default)]
struct Recording {
    observations: Vec<Observation>,
    counter: u32,
    sequence: u64,
    next_event: u64,
}

const SESSION: &str = "session-1";

impl Recording {
    fn sample_at(&mut self, t_ms: u64, counter: u32, quality: &str, max: f32, avg: f32, min: f32) {
        self.sequence += 1;
        self.observations.push(Observation {
            t_ms,
            tap: Tap::BatteryTemperature(Temperature {
                sequence: self.sequence,
                source_timestamp_ms: 1_000_000 + t_ms,
                alive_counter: counter,
                quality: quality.to_owned(),
                max_c: max,
                avg_c: avg,
                min_c: min,
            }),
        });
    }

    /// Nominal samples every 100 ms from `from` to `to` (exclusive).
    fn nominal(&mut self, from: u64, to: u64) {
        let mut t = from;
        while t < to {
            self.counter = (self.counter + 1) % 256;
            self.sample_at(t, self.counter, "VALID", 40.0, 32.0, 24.0);
            t += 100;
        }
    }

    fn event(&mut self, t_ms: u64, cause: u64, kind: EventKind) -> u64 {
        self.next_event += 1;
        self.observations.push(Observation {
            t_ms,
            tap: Tap::GuardianEvent(GuardianEvent {
                session_id: SESSION.to_owned(),
                event_id: self.next_event,
                cause_event_id: cause,
                guardian_time_ms: t_ms,
                kind,
            }),
        });
        self.next_event
    }

    fn sovd(&mut self, t_ms: u64, code: &str, event_id: u64, failed: bool, since_clear: bool) {
        self.observations.push(Observation {
            t_ms,
            tap: Tap::SovdFault {
                code: code.to_owned(),
                http_status: Some(200),
                body: serde_json::json!({
                    "status": {"testFailed": failed, "testFailedSinceLastClear": since_clear},
                    "environment_data": {"session_id": SESSION, "event_id": event_id.to_string()},
                }),
            },
        });
    }

    /// The Guardian's full reaction to a fault: detection, DEGRADED, warning,
    /// OpenSOVD record, recovery, OK, and OpenSOVD passed.
    fn full_reaction(&mut self, dtc: &str, detected_at: u64, recovered_at: u64) {
        let fault = self.event(
            detected_at,
            0,
            EventKind::FaultDetected {
                dtc: dtc.into(),
                requirement: "FSR".into(),
            },
        );
        let degraded = self.event(
            detected_at,
            fault,
            EventKind::MonitoringStatusChanged {
                previous: "OK".into(),
                current: "DEGRADED".into(),
            },
        );
        self.event(
            detected_at,
            degraded,
            EventKind::MitigationRequested {
                mitigation: "DRIVER_WARNING_MONITORING_UNAVAILABLE".into(),
            },
        );
        self.sovd(detected_at + 400, dtc, fault, true, true);
        let recovered = self.event(
            recovered_at,
            fault,
            EventKind::FaultRecovered {
                dtc: dtc.into(),
                requirement: "FSR".into(),
            },
        );
        self.event(
            recovered_at,
            recovered,
            EventKind::MonitoringStatusChanged {
                previous: "DEGRADED".into(),
                current: "OK".into(),
            },
        );
        self.sovd(recovered_at + 300, dtc, fault, false, true);
    }

    fn judge(&self, context: &Context, id: &str) -> Evaluation {
        let mut observations = self.observations.clone();
        observations.sort_by_key(|o| o.t_ms);
        evaluate(
            scenario(context, id),
            &observations,
            &context.budgets,
            &context.onset,
        )
        .unwrap()
    }
}

fn failed_checks(evaluation: &Evaluation) -> Vec<String> {
    evaluation
        .checks
        .iter()
        .filter_map(|c| match &c.outcome {
            Outcome::Failed { detail } => Some(detail.clone()),
            _ => None,
        })
        .collect()
}

/// A `timeout` run: 2 s nominal, a 1.8 s gap, then nominal again.
fn timeout_run() -> Recording {
    let mut r = Recording::default();
    r.nominal(0, 2000);
    r.nominal(3800, 8000);
    r
}

// --- Catalog --------------------------------------------------------------------

#[test]
fn shipped_catalog_loads_and_every_trace_exists() {
    let context = context();
    for scenario in &context.catalog.scenarios {
        if let Stimulus::CanTrace { trace, .. } = &scenario.stimulus {
            assert!(
                repo().join(trace).is_file(),
                "{}: missing {trace}",
                scenario.id
            );
        }
    }
}

#[test]
fn every_budget_in_the_catalog_resolves() {
    let context = context();
    // An empty recording judges nothing, but evaluating a recording with an
    // onset resolves every budget expression.
    for scenario in &context.catalog.scenarios {
        let mut r = timeout_run();
        r.sample_at(9000, 7, "INVALID", 250.0, 32.0, 24.0);
        r.sample_at(9100, 7, "VALID", 70.0, 90.0, 20.0);
        let mut observations = r.observations.clone();
        observations.sort_by_key(|o| o.t_ms);
        evaluate(scenario, &observations, &context.budgets, &context.onset)
            .unwrap_or_else(|error| panic!("{}: {error}", scenario.id));
    }
}

#[test]
fn fault_codes_come_from_the_dfm_catalog() {
    let context = context();
    assert!(context
        .fault_codes
        .contains(&"BTG_TempFreshnessLost".to_owned()));
    assert_eq!(context.fault_codes.len(), 4);
}

// --- PASS -----------------------------------------------------------------------

#[test]
fn complete_reaction_in_time_is_pass() {
    let context = context();
    let mut r = timeout_run();
    // t0 = 1900 + 100; detected 300 ms later.
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
    assert_eq!(evaluation.onset.as_ref().unwrap().t_ms, 2000);
    assert_eq!(evaluation.requirements["FSR-2.2"], Verdict::Pass);
    assert_eq!(evaluation.requirements["FSR-D.2"], Verdict::Pass);
    assert_eq!(evaluation.requirements["FSR-2.6"], Verdict::Pass);
}

#[test]
fn evaluation_is_deterministic() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);

    let a = serde_json::to_string(&r.judge(&context, "timeout")).unwrap();
    let b = serde_json::to_string(&r.judge(&context, "timeout")).unwrap();

    assert_eq!(a, b);
}

// --- FAIL -----------------------------------------------------------------------

#[test]
fn late_detection_is_fail() {
    let context = context();
    let mut r = timeout_run();
    // Budget T_stale + T_react = 800 ms after t0 = 2000.
    r.full_reaction("BTG_TempFreshnessLost", 2900, 5200);

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.requirements["FSR-2.2"], Verdict::Fail);
    assert!(failed_checks(&evaluation)[0].contains("late"));
}

#[test]
fn missing_fault_is_fail_and_still_judged() {
    let context = context();
    let r = timeout_run();

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert!(failed_checks(&evaluation)
        .iter()
        .all(|d| d.contains("no BTG_TempFreshnessLost")));
}

#[test]
fn degraded_without_warning_is_fail() {
    let context = context();
    let mut r = timeout_run();
    let fault = r.event(
        2300,
        0,
        EventKind::FaultDetected {
            dtc: "BTG_TempFreshnessLost".into(),
            requirement: "FSR-2.2".into(),
        },
    );
    r.event(
        2300,
        fault,
        EventKind::MonitoringStatusChanged {
            previous: "OK".into(),
            current: "DEGRADED".into(),
        },
    );

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert!(failed_checks(&evaluation)
        .iter()
        .any(|d| d.contains("without the monitoring-unavailable warning")));
}

#[test]
fn warning_must_be_caused_by_the_status_change() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    // Break the cause link of the warning (event 3 caused by event 2).
    for observation in &mut r.observations {
        if let Tap::GuardianEvent(event) = &mut observation.tap {
            if event.event_id == 3 {
                event.cause_event_id = 0;
            }
        }
    }

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
}

#[test]
fn lowering_thermal_state_while_degraded_is_forbidden() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    // Event ids 1..6 are the reaction; insert a lowering between DEGRADED
    // (event 2) and recovery (event 4) by ordering ids.
    r.observations
        .retain(|o| !matches!(&o.tap, Tap::GuardianEvent(e) if e.event_id >= 4));
    r.next_event = 3;
    r.event(
        2500,
        0,
        EventKind::ThermalStateChanged {
            previous: "WARNING".into(),
            current: "MONITORING".into(),
        },
    );

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.violations[0].rule, "no_lowering_while_degraded");
    assert_eq!(evaluation.requirements["FSR-2.5"], Verdict::Fail);
}

#[test]
fn fault_before_onset_is_a_false_alarm() {
    let context = context();
    let mut r = timeout_run();
    r.event(
        500,
        0,
        EventKind::FaultDetected {
            dtc: "BTG_TempSignalStuck".into(),
            requirement: "FSR-2.4".into(),
        },
    );
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.violations[0].rule, "no_fault_before_onset");
}

#[test]
fn events_of_two_sessions_are_forbidden() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    if let Tap::GuardianEvent(event) = &mut r
        .observations
        .iter_mut()
        .find(|o| matches!(o.tap, Tap::GuardianEvent(_)))
        .unwrap()
        .tap
    {
        event.session_id = "other".into();
    }

    let evaluation = r.judge(&context, "timeout");

    assert!(evaluation
        .violations
        .iter()
        .any(|v| v.rule == "one_session"));
}

#[test]
fn nominal_false_alarm_is_fail() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 5000);
    r.event(
        10,
        0,
        EventKind::ThermalStateChanged {
            previous: "CLEAR".into(),
            current: "MONITORING".into(),
        },
    );
    r.event(
        2000,
        0,
        EventKind::FaultDetected {
            dtc: "BTG_TempSignalStuck".into(),
            requirement: "FSR-2.4".into(),
        },
    );

    let evaluation = r.judge(&context, "normal");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.requirements["H-3"], Verdict::Fail);
}

// --- INCONCLUSIVE ---------------------------------------------------------------

#[test]
fn fault_that_never_reached_the_guardian_is_inconclusive() {
    let context = context();
    // counter_stuck, but the repeated frames were lost on the way.
    let mut r = Recording::default();
    r.nominal(0, 6000);

    let evaluation = r.judge(&context, "counter_stuck");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
    assert!(evaluation.reason.contains("never showed"));
    assert!(evaluation
        .requirements
        .values()
        .all(|v| *v == Verdict::Inconclusive));
}

#[test]
fn no_samples_at_all_is_inconclusive() {
    let context = context();

    let evaluation = Recording::default().judge(&context, "normal");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
}

#[test]
fn opensovd_unreachable_is_inconclusive_not_fail() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    r.observations
        .retain(|o| !matches!(o.tap, Tap::SovdFault { .. }));
    r.observations.push(Observation {
        t_ms: 0,
        tap: Tap::SovdFault {
            code: "BTG_TempFreshnessLost".into(),
            http_status: None,
            body: serde_json::Value::Null,
        },
    });

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
    assert_eq!(evaluation.requirements["FSR-2.2"], Verdict::Pass);
    assert_eq!(evaluation.requirements["FSR-D.2"], Verdict::Inconclusive);
}

// --- Window -----------------------------------------------------------------------

#[test]
fn freshness_fault_after_the_trace_ended_is_not_judged() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 5000);
    r.event(
        10,
        0,
        EventKind::ThermalStateChanged {
            previous: "CLEAR".into(),
            current: "MONITORING".into(),
        },
    );
    // The replay ended at 4900; the Guardian notices 300 ms later.
    r.event(
        5250,
        0,
        EventKind::FaultDetected {
            dtc: "BTG_TempFreshnessLost".into(),
            requirement: "FSR-2.2".into(),
        },
    );

    let evaluation = r.judge(&context, "normal");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
}

// --- Onset detectors ------------------------------------------------------------------

fn onset(r: &Recording, onset: Onset) -> Option<u64> {
    let context = context();
    let samples: Vec<_> = r
        .observations
        .iter()
        .filter_map(|o| match &o.tap {
            Tap::BatteryTemperature(s) => Some((o.t_ms, s)),
            _ => None,
        })
        .collect();
    onset.find(&samples, &context.onset).map(|f| f.t_ms)
}

#[test]
fn single_lost_frame_is_not_a_counter_jump() {
    let mut r = Recording::default();
    r.sample_at(0, 1, "VALID", 40.0, 32.0, 24.0);
    r.sample_at(200, 3, "VALID", 40.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::AliveCounterJumps), None);
    r.sample_at(300, 9, "VALID", 40.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::AliveCounterJumps), Some(300));
}

#[test]
fn counter_wraparound_is_not_a_jump() {
    let mut r = Recording::default();
    r.sample_at(0, 255, "VALID", 40.0, 32.0, 24.0);
    r.sample_at(100, 0, "VALID", 40.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::AliveCounterJumps), None);
}

#[test]
fn constant_temperature_is_not_a_frozen_maximum() {
    let mut r = Recording::default();
    r.nominal(0, 5000);
    assert_eq!(onset(&r, Onset::MaxFrozenWhileReferenceMoves), None);
}

#[test]
fn frozen_maximum_onset_is_the_start_of_the_plateau() {
    let mut r = Recording::default();
    r.sample_at(0, 1, "VALID", 41.0, 30.0, 22.0);
    r.sample_at(100, 2, "VALID", 42.0, 30.0, 22.0);
    r.sample_at(200, 3, "VALID", 42.0, 31.0, 23.0);
    r.sample_at(300, 4, "VALID", 42.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::MaxFrozenWhileReferenceMoves), Some(100));
}

#[test]
fn gap_needs_more_than_t_stale() {
    let mut r = Recording::default();
    r.sample_at(0, 1, "VALID", 40.0, 32.0, 24.0);
    r.sample_at(300, 2, "VALID", 40.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::Gap), None);
    r.sample_at(700, 3, "VALID", 40.0, 32.0, 24.0);
    assert_eq!(onset(&r, Onset::Gap), Some(400));
}
