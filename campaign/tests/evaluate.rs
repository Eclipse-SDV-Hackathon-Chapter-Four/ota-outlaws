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
use campaign::evaluate::{evaluate, Evaluation, LinkState, Outcome, Verdict};
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
                sample: None,
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
                body: {
                    let (fault_type, severity) = context().classes[code].clone();
                    serde_json::json!({
                        "status": {"testFailed": failed, "testFailedSinceLastClear": since_clear},
                        "environment_data": {
                            "session_id": SESSION,
                            "event_id": event_id.to_string(),
                            "fault_type": fault_type,
                            "severity": severity,
                        },
                    })
                },
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

    fn injection(&mut self, t_ms: u64, action: &str) {
        self.observations.push(Observation {
            t_ms,
            tap: Tap::Injection {
                action: action.to_owned(),
                detail: String::new(),
            },
        });
    }

    fn degraded_after(&mut self, dtc: &str, at: u64) -> u64 {
        let fault = self.event(
            at,
            0,
            EventKind::FaultDetected {
                dtc: dtc.into(),
                requirement: "FSR".into(),
            },
        );
        let degraded = self.event(
            at,
            fault,
            EventKind::MonitoringStatusChanged {
                previous: "OK".into(),
                current: "DEGRADED".into(),
            },
        );
        self.event(
            at,
            degraded,
            EventKind::MitigationRequested {
                mitigation: "DRIVER_WARNING_MONITORING_UNAVAILABLE".into(),
            },
        );
        self.sovd(at + 300, dtc, fault, true, true);
        fault
    }

    fn judge(&self, context: &Context, id: &str) -> Evaluation {
        let mut observations = self.observations.clone();
        observations.sort_by_key(|o| o.t_ms);
        evaluate(
            scenario(context, id),
            &observations,
            &context.budgets,
            &context.onset,
            &context.classes,
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

/// A `counter_stuck` run: 2 s nominal, 2 s of frames that repeat the alive
/// counter, then nominal again.
fn counter_stuck_run() -> Recording {
    let mut r = Recording::default();
    r.nominal(0, 2000);
    let frozen = r.counter;
    for t in (2000..4000).step_by(100) {
        r.sample_at(t, frozen, "VALID", 40.0, 32.0, 24.0);
    }
    r.nominal(4000, 9000);
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
        evaluate(
            scenario,
            &observations,
            &context.budgets,
            &context.onset,
            &context.classes,
        )
        .unwrap_or_else(|error| panic!("{}: {error}", scenario.id));
    }
}

#[test]
fn fault_codes_come_from_the_dfm_catalog() {
    let context = context();
    assert!(context
        .fault_codes
        .contains(&"BTG_TempFreshnessLost".to_owned()));
    // The watchdog reports BTG_GuardianHeartbeatLoss under the same entity.
    assert!(context
        .fault_codes
        .contains(&"BTG_GuardianHeartbeatLoss".to_owned()));
    assert_eq!(context.fault_codes.len(), 11);
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

#[test]
fn events_arriving_out_of_order_are_judged_in_guardian_order() {
    // Observed on the real chain: two events published in order reach the
    // tool 1 ms swapped. Causality comes from the event IDs, not arrival.
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    for observation in &mut r.observations {
        if let Tap::GuardianEvent(event) = &observation.tap {
            // Event 5 (DEGRADED -> OK) arrives before event 4 (recovered).
            if event.event_id == 5 {
                observation.t_ms -= 1;
            }
        }
    }

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
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
fn counter_stuck_at_n_stuck_repeated_frames_is_in_budget() {
    let context = context();
    let mut r = counter_stuck_run();
    // The 10th repeated frame arrives at 2900. Budget T_counter_stuck + T_react
    // = 10 × 100 + 100 ms after t0 = 2000.
    r.full_reaction("BTG_TempCounterStuck", 2900, 6000);

    let evaluation = r.judge(&context, "counter_stuck");

    assert_eq!(context.budgets["T_counter_stuck"], 1000);
    assert_eq!(evaluation.requirements["FSR-2.3"], Verdict::Pass);
}

#[test]
fn counter_stuck_after_budget_is_fail() {
    let context = context();
    let mut r = counter_stuck_run();
    r.full_reaction("BTG_TempCounterStuck", 3200, 6000);

    let evaluation = r.judge(&context, "counter_stuck");

    assert_eq!(evaluation.requirements["FSR-2.3"], Verdict::Fail);
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
    assert_eq!(evaluation.requirements["SG-4"], Verdict::Fail);
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
    onset.find(&samples, &[], &context.onset).map(|f| f.t_ms)
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

// --- HARA test scenarios ----------------------------------------------------------

#[test]
fn ts_03_startup_fault_in_time_is_pass() {
    let context = context();
    let mut r = Recording::default();
    r.injection(10, "start_guardian");
    // guardian_time_ms equals t_ms in this helper: 350 ms after its start.
    r.degraded_after("BTG_TempNoDataAtStartup", 350);

    let evaluation = r.judge(&context, "startup_without_source");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
}

#[test]
fn ts_03_late_startup_fault_is_fail() {
    let context = context();
    let mut r = Recording::default();
    r.degraded_after("BTG_TempNoDataAtStartup", 900);

    let evaluation = r.judge(&context, "startup_without_source");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.requirements["FSR-2.1"], Verdict::Fail);
}

#[test]
fn ts_03_with_samples_at_the_input_is_inconclusive() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 1000);
    r.degraded_after("BTG_TempNoDataAtStartup", 350);

    let evaluation = r.judge(&context, "startup_without_source");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
}

#[test]
fn ts_04_stream_end_is_the_onset() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 6000);
    // Last sample at 5900, t0 = 6000; detected 300 ms later.
    r.degraded_after("BTG_TempFreshnessLost", 6300);

    let evaluation = r.judge(&context, "source_shutdown");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
    assert_eq!(evaluation.onset.as_ref().unwrap().t_ms, 6000);
}

#[test]
fn ts_06_dropout_behind_the_tap_is_pass_with_attribution() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 12000);
    r.injection(6000, "isolate");
    r.injection(8000, "reconnect");
    r.full_reaction("BTG_TempFreshnessLost", 6350, 9500);

    let evaluation = r.judge(&context, "transport_dropout");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
    assert_eq!(evaluation.requirements["EC-1"], Verdict::Pass);
}

#[test]
fn ts_06_without_samples_at_the_tap_attribution_is_inconclusive() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 6000);
    r.nominal(8000, 12000);
    r.injection(6000, "isolate");
    r.full_reaction("BTG_TempFreshnessLost", 6350, 9500);

    let evaluation = r.judge(&context, "transport_dropout");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
    assert_eq!(evaluation.requirements["EC-1"], Verdict::Inconclusive);
}

#[test]
fn ts_10_lowering_after_invalid_input_is_fail() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 3000);
    r.sample_at(3000, 31, "VALID", 47.0, 39.0, 31.0);
    r.event(
        3001,
        0,
        EventKind::ThermalStateChanged {
            previous: "MONITORING".into(),
            current: "WARNING".into(),
        },
    );
    r.sample_at(3100, 32, "INVALID", 20.0, 15.0, 10.0);
    r.sample_at(3200, 33, "VALID", 47.0, 39.0, 31.0);
    r.full_reaction("BTG_TempQualityInvalid", 3101, 4500);
    r.event(
        4600,
        0,
        EventKind::ThermalStateChanged {
            previous: "WARNING".into(),
            current: "MONITORING".into(),
        },
    );
    r.nominal(3300, 6000);

    let evaluation = r.judge(&context, "invalid_during_warning");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.requirements["FSR-3.6"], Verdict::Fail);
}

#[test]
fn samples_before_the_guardian_started_are_not_judged() {
    // A gap before the Guardian started is not a fault the Guardian could see.
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 1000);
    r.nominal(2000, 3000);
    r.injection(2500, "start_guardian");
    r.nominal(3000, 6000);

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
    assert!(evaluation.reason.contains("never showed"));
}

#[test]
fn detection_time_comes_from_the_guardian_when_delivery_is_delayed() {
    // TS-06 on the real chain: cut off the network, the Guardian detects the
    // fault in time, but its event reaches the tap only after reconnecting.
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 12000);
    r.injection(6000, "isolate");
    r.injection(8500, "reconnect");
    // An early event on time: Guardian clock = tool clock - 3000.
    r.event(
        3100,
        0,
        EventKind::ThermalStateChanged {
            previous: "CLEAR".into(),
            current: "MONITORING".into(),
        },
    );
    r.full_reaction("BTG_TempFreshnessLost", 8600, 10000);
    for observation in &mut r.observations {
        if let Tap::GuardianEvent(event) = &mut observation.tap {
            event.guardian_time_ms = if event.event_id == 2 {
                // The fault: detected at tool time 6350, delivered at 8600.
                3350
            } else {
                observation.t_ms - 3000
            };
        }
    }

    let evaluation = r.judge(&context, "transport_dropout");

    let fault = &evaluation.checks[0];
    assert_eq!(fault.latency_ms, Some(350), "{fault:#?}");
    assert_eq!(evaluation.requirements["FSR-2.2"], Verdict::Pass);
    match &fault.outcome {
        Outcome::Met { detail } => assert!(detail.contains("delivered"), "{detail}"),
        other => panic!("{other:?}"),
    }
}

// --- Overtemperature DTCs and classification ----------------------------------

/// Heating run: WARNING (event 1), CRITICAL (2) with its mitigation (3), then
/// cooling to WARNING (4) and MONITORING (5), with OpenSOVD records.
fn heating_run(classify: bool) -> Recording {
    let mut r = Recording::default();
    r.nominal(0, 2000);
    r.sample_at(2000, 21, "VALID", 45.0, 37.0, 29.0);
    r.sample_at(3000, 31, "VALID", 55.0, 47.0, 39.0);
    r.nominal(3100, 9000);
    let warning = r.event(
        2001,
        0,
        EventKind::ThermalStateChanged {
            previous: "MONITORING".into(),
            current: "WARNING".into(),
        },
    );
    let critical = r.event(
        3001,
        0,
        EventKind::ThermalStateChanged {
            previous: "WARNING".into(),
            current: "CRITICAL".into(),
        },
    );
    r.event(
        3001,
        critical,
        EventKind::MitigationRequested {
            mitigation: "DRIVER_WARNING_OVERTEMP".into(),
        },
    );
    r.event(
        5000,
        0,
        EventKind::ThermalStateChanged {
            previous: "CRITICAL".into(),
            current: "WARNING".into(),
        },
    );
    r.event(
        7000,
        0,
        EventKind::ThermalStateChanged {
            previous: "WARNING".into(),
            current: "MONITORING".into(),
        },
    );
    r.sovd(2400, "BTG_TempOverTempWarning", warning, true, true);
    r.sovd(3400, "BTG_TempOverTempCritical", critical, true, true);
    r.sovd(5300, "BTG_TempOverTempCritical", critical, false, true);
    r.sovd(7300, "BTG_TempOverTempWarning", warning, false, true);
    if !classify {
        for observation in &mut r.observations {
            if let Tap::SovdFault { body, .. } = &mut observation.tap {
                body["environment_data"]["severity"] = "Error".into();
            }
        }
    }
    r
}

#[test]
fn overtemperature_dtcs_with_history_are_pass() {
    let context = context();

    let evaluation = heating_run(true).judge(&context, "heating");

    assert_eq!(
        evaluation.verdict,
        Verdict::Pass,
        "{:#?}",
        evaluation.checks
    );
}

#[test]
fn wrong_severity_in_opensovd_is_fail() {
    let context = context();

    let evaluation = heating_run(false).judge(&context, "heating");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert!(failed_checks(&evaluation)
        .iter()
        .any(|d| d.contains("expected Hardware and Warn")));
}

#[test]
fn late_driver_warning_overtemp_is_fail() {
    let context = context();
    let mut r = heating_run(true);
    for observation in &mut r.observations {
        if let Tap::GuardianEvent(GuardianEvent {
            kind: EventKind::MitigationRequested { .. },
            ..
        }) = &observation.tap
        {
            observation.t_ms = 3500;
        }
    }

    let evaluation = r.judge(&context, "heating");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert!(failed_checks(&evaluation)
        .iter()
        .any(|d| d.contains("DRIVER_WARNING_OVERTEMP") && d.ends_with("late")));
}

#[test]
fn overtemperature_dtc_not_passed_after_cooling_is_fail() {
    let context = context();
    let mut r = heating_run(true);
    r.observations.retain(|o| {
        !matches!(&o.tap, Tap::SovdFault { code, body, .. }
            if code == "BTG_TempOverTempWarning" && body["status"]["testFailed"] == false)
    });

    let evaluation = r.judge(&context, "heating");

    assert_eq!(evaluation.verdict, Verdict::Fail);
}

#[test]
fn input_fault_without_classification_is_fail() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    for observation in &mut r.observations {
        if let Tap::SovdFault { body, .. } = &mut observation.tap {
            body["environment_data"]
                .as_object_mut()
                .unwrap()
                .remove("fault_type");
        }
    }

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(evaluation.verdict, Verdict::Fail);
    assert_eq!(evaluation.requirements["FSR-D.2"], Verdict::Fail);
}

// --- Evidence chain -------------------------------------------------------------

fn link_state(evaluation: &Evaluation, link: &str) -> LinkState {
    evaluation
        .chain
        .links
        .iter()
        .find(|l| l.link == link)
        .unwrap_or_else(|| panic!("no link {link}"))
        .state
}

#[test]
fn full_reaction_gives_a_complete_chain_linked_by_ids() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);

    let evaluation = r.judge(&context, "timeout");
    let chain = &evaluation.chain;

    assert!(chain.complete, "{:#?}", chain.links);
    for link in [
        "Hazard",
        "Safety goal",
        "Fault",
        "Detection",
        "Mitigation",
        "DTC in OpenSOVD",
        "Verdict",
    ] {
        assert_eq!(link_state(&evaluation, link), LinkState::Present, "{link}");
    }
    let detection = &chain.detections[0];
    assert_eq!(detection.dtc.as_deref(), Some("BTG_TempFreshnessLost"));
    assert_eq!(detection.latency_ms, Some(300));
    assert_eq!(detection.recovered_event_id, Some(4));
    let mitigation = &chain.mitigations[0];
    assert_eq!(
        mitigation.mitigation,
        "DRIVER_WARNING_MONITORING_UNAVAILABLE"
    );
    assert_eq!(mitigation.detection_event_id, Some(detection.event_id));
    // FaultDetected → MonitoringStatusChanged → MitigationRequested.
    assert_eq!(mitigation.cause_chain.len(), 3);
    let dtc = &chain.diagnostics[0];
    assert_eq!(dtc.latency_ms, Some(400));
    assert!(dtc.passed_later);
    assert_eq!(
        dtc.fault_type.as_deref(),
        Some(context.classes["BTG_TempFreshnessLost"].0.as_str())
    );
    assert_eq!(evaluation.timeline.len(), 5);
}

#[test]
fn a_fault_without_warning_or_dtc_leaves_those_links_missing() {
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

    assert!(!evaluation.chain.complete);
    assert_eq!(link_state(&evaluation, "Detection"), LinkState::Present);
    assert_eq!(link_state(&evaluation, "Mitigation"), LinkState::Missing);
    assert_eq!(
        link_state(&evaluation, "DTC in OpenSOVD"),
        LinkState::Missing
    );
    assert!(evaluation.chain.diagnostics[0].failed_ms.is_none());
}

#[test]
fn a_mitigation_not_caused_by_the_detection_is_not_linked() {
    let context = context();
    let mut r = timeout_run();
    r.full_reaction("BTG_TempFreshnessLost", 2300, 5200);
    // The warning (event 3) loses its cause.
    for observation in &mut r.observations {
        if let Tap::GuardianEvent(event) = &mut observation.tap {
            if event.event_id == 3 {
                event.cause_event_id = 0;
            }
        }
    }

    let evaluation = r.judge(&context, "timeout");

    assert_eq!(link_state(&evaluation, "Mitigation"), LinkState::Missing);
    assert_eq!(evaluation.chain.mitigations[0].detection_event_id, None);
}

#[test]
fn a_nominal_run_expects_no_detection_mitigation_or_dtc() {
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

    let evaluation = r.judge(&context, "normal");

    assert!(evaluation.chain.complete, "{:#?}", evaluation.chain.links);
    for link in ["Detection", "Mitigation", "DTC in OpenSOVD"] {
        assert_eq!(
            link_state(&evaluation, link),
            LinkState::NotExpected,
            "{link}"
        );
    }
}

#[test]
fn a_false_alarm_is_an_unexpected_detection() {
    let context = context();
    let mut r = Recording::default();
    r.nominal(0, 5000);
    r.event(
        2000,
        0,
        EventKind::FaultDetected {
            dtc: "BTG_TempSignalStuck".into(),
            requirement: "FSR-2.4".into(),
        },
    );

    let evaluation = r.judge(&context, "normal");

    assert_eq!(link_state(&evaluation, "Detection"), LinkState::Unexpected);
    assert_eq!(evaluation.verdict, Verdict::Fail);
}

#[test]
fn a_run_that_recorded_nothing_is_inconclusive_not_fail() {
    let context = context();

    // The stack did not start: not a single observation.
    let evaluation = Recording::default().judge(&context, "startup_without_source");

    assert_eq!(evaluation.verdict, Verdict::Inconclusive);
    assert!(evaluation.reason.contains("nothing was recorded"));
    assert!(evaluation
        .requirements
        .values()
        .all(|v| *v == Verdict::Inconclusive));
    assert!(!evaluation.chain.complete);
}
