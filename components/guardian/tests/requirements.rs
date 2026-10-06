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

//! Requirement tests for the Guardian core.
//!
//! Each test is named after the functional safety requirement it verifies, as
//! defined in `docs/explanation/safety-concept.md`. All tests use the shipped
//! configuration, so they also check that the configuration file is valid.

use guardian::{
    Event, EventKind, FaultCode, Guardian, GuardianConfig, Millis, Mitigation, MonitoringStatus,
    Sample, ThermalState,
};

const SHIPPED_CONFIG: &str = include_str!("../../../config/guardian/safety-params.toml");

/// Signal cycle of the source.
const CYCLE_MS: u64 = 100;
/// Tick interval of the runtime adapter.
const TICK_MS: u64 = 50;
/// Reaction time budget `T_react` from the safety concept.
const T_REACT_MS: u64 = 500;
/// The source clock runs with an offset to the Guardian clock, to show that the
/// core never compares the two.
const SOURCE_CLOCK_OFFSET_MS: u64 = 1_000_000;

fn config() -> GuardianConfig {
    GuardianConfig::from_toml_str(SHIPPED_CONFIG).expect("shipped configuration is valid")
}

/// Drives a Guardian like the runtime adapter does: a tick every 50 ms and a
/// sample every 100 ms. Collects all events.
struct Run {
    guardian: Guardian,
    now: u64,
    sequence: u64,
    events: Vec<Event>,
}

impl Run {
    fn new() -> Self {
        Self {
            guardian: Guardian::new(&config()),
            now: 0,
            sequence: 0,
            events: Vec::new(),
        }
    }

    /// Advances time by one cycle, then delivers a fresh sample.
    fn sample(&mut self, max_c: f32, avg_c: f32, min_c: f32) -> &mut Self {
        self.advance(CYCLE_MS);
        let source_timestamp_ms = self.now + SOURCE_CLOCK_OFFSET_MS;
        self.deliver(source_timestamp_ms, max_c, avg_c, min_c)
    }

    /// Delivers a sample with the given source timestamp without advancing time.
    fn deliver(
        &mut self,
        source_timestamp_ms: u64,
        max_c: f32,
        avg_c: f32,
        min_c: f32,
    ) -> &mut Self {
        self.sequence += 1;
        let sample = Sample {
            source_timestamp_ms,
            sequence: self.sequence,
            max_c,
            avg_c,
            min_c,
        };
        let events = self.guardian.on_sample(sample, Millis(self.now));
        self.events.extend(events);
        self
    }

    /// Delivers `count` fresh samples with constant values, one per cycle.
    fn samples(&mut self, count: usize, max_c: f32, avg_c: f32, min_c: f32) -> &mut Self {
        for _ in 0..count {
            self.sample(max_c, avg_c, min_c);
        }
        self
    }

    /// Advances time, ticking every 50 ms.
    fn advance(&mut self, duration_ms: u64) -> &mut Self {
        let end = self.now + duration_ms;
        while self.now < end {
            self.now = (self.now + TICK_MS).min(end);
            let events = self.guardian.on_tick(Millis(self.now));
            self.events.extend(events);
        }
        self
    }

    fn time_of_first(&self, matches: impl Fn(&EventKind) -> bool) -> Option<u64> {
        self.events
            .iter()
            .find(|e| matches(&e.kind))
            .map(|e| e.at.0)
    }

    fn fault_time(&self, fault: FaultCode) -> Option<u64> {
        self.time_of_first(
            |kind| matches!(kind, EventKind::FaultDetected { fault: f, .. } if *f == fault),
        )
    }

    fn thermal_change_time(&self, to: ThermalState) -> Option<u64> {
        self.time_of_first(
            |kind| matches!(kind, EventKind::ThermalStateChanged { to: t, .. } if *t == to),
        )
    }

    fn mitigations(&self) -> Vec<Mitigation> {
        self.events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::MitigationRequested { mitigation } => Some(mitigation),
                _ => None,
            })
            .collect()
    }

    fn thermal(&self) -> ThermalState {
        self.guardian.thermal_state()
    }

    fn monitoring(&self) -> MonitoringStatus {
        self.guardian.monitoring_status()
    }
}

// --- Thermal state basics --------------------------------------------------

#[test]
fn starts_in_clear_until_first_valid_sample() {
    let mut run = Run::new();
    assert_eq!(run.thermal(), ThermalState::Clear);

    run.sample(30.0, 28.0, 26.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

// --- FSR-1.1: WARNING threshold ---------------------------------------------

#[test]
fn fsr_1_1_reaching_warn_threshold_raises_warning_within_t_react() {
    let warn = config().thermal.warn_c;
    let mut run = Run::new();
    run.samples(5, warn - 1.0, 35.0, 30.0);
    run.sample(warn, 35.0, 30.0);
    let t0 = run.now;

    let reacted = run
        .thermal_change_time(ThermalState::Warning)
        .expect("WARNING raised");

    assert!(reacted - t0 <= T_REACT_MS);
    assert_eq!(run.thermal(), ThermalState::Warning);
}

#[test]
fn fsr_1_1_below_warn_threshold_stays_monitoring() {
    let warn = config().thermal.warn_c;
    let mut run = Run::new();

    run.samples(20, warn - 0.5, 35.0, 30.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
    assert!(run.mitigations().is_empty());
}

// --- FSR-1.2: CRITICAL threshold and driver warning ---------------------------

#[test]
fn fsr_1_2_reaching_critical_threshold_raises_critical_and_requests_mitigation() {
    let critical = config().thermal.critical_c;
    let mut run = Run::new();
    run.samples(3, 40.0, 35.0, 30.0);
    run.sample(critical, 40.0, 35.0);
    let t0 = run.now;

    let reacted = run
        .thermal_change_time(ThermalState::Critical)
        .expect("CRITICAL raised");

    assert!(reacted - t0 <= T_REACT_MS);
    assert_eq!(run.thermal(), ThermalState::Critical);
    assert_eq!(run.mitigations(), vec![Mitigation::DriverWarningOvertemp]);
}

#[test]
fn fsr_1_2_mitigation_request_is_linked_to_its_state_change() {
    let mut run = Run::new();
    run.sample(60.0, 40.0, 35.0);

    let change = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::ThermalStateChanged {
                    to: ThermalState::Critical,
                    ..
                }
            )
        })
        .expect("CRITICAL raised");
    let mitigation = run
        .events
        .iter()
        .find(|e| matches!(e.kind, EventKind::MitigationRequested { .. }))
        .expect("mitigation requested");

    assert_eq!(mitigation.cause, Some(change.id));
}

#[test]
fn fsr_1_2_critical_is_requested_only_once() {
    let mut run = Run::new();

    run.samples(10, 60.0, 40.0, 35.0);

    assert_eq!(run.mitigations(), vec![Mitigation::DriverWarningOvertemp]);
}

// --- FSR-2.2: loss of fresh data --------------------------------------------

#[test]
fn fsr_2_2_missing_samples_lead_to_degraded_within_budget() {
    let stale_timeout = config().freshness.stale_timeout_ms;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    // t0: the first expected sample is missing (last arrival plus one cycle).
    let t0 = run.now + CYCLE_MS;

    run.advance(2_000);

    let detected = run
        .fault_time(FaultCode::FreshnessLost)
        .expect("freshness fault");
    assert!(detected - t0 <= stale_timeout + T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(
        run.mitigations(),
        vec![Mitigation::DriverWarningMonitoringUnavailable]
    );
}

#[test]
fn fsr_2_2_no_fault_while_fresh_samples_arrive() {
    let mut run = Run::new();

    run.samples(100, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.fault_time(FaultCode::FreshnessLost), None);
}

#[test]
fn fsr_2_2_delay_shorter_than_timeout_is_tolerated() {
    let stale_timeout = config().freshness.stale_timeout_ms;
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    // The next sample arrives late, but within T_stale.
    run.advance(stale_timeout - CYCLE_MS);
    run.samples(5, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn fsr_2_2_repeated_source_timestamp_does_not_count_as_fresh() {
    // A publisher that keeps repeating the last value while the source is dead.
    let mut run = Run::new();
    run.sample(30.0, 28.0, 26.0);
    let frozen_source_timestamp = run.now + SOURCE_CLOCK_OFFSET_MS;

    for _ in 0..20 {
        run.advance(CYCLE_MS);
        run.deliver(frozen_source_timestamp, 30.0, 28.0, 26.0);
    }

    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert!(run.fault_time(FaultCode::FreshnessLost).is_some());
}

#[test]
fn fsr_2_2_older_source_timestamp_does_not_count_as_fresh() {
    let mut run = Run::new();
    run.sample(30.0, 28.0, 26.0);
    let old_source_timestamp = run.now + SOURCE_CLOCK_OFFSET_MS - 1;

    for _ in 0..20 {
        run.advance(CYCLE_MS);
        run.deliver(old_source_timestamp, 70.0, 28.0, 26.0);
    }

    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    // The old samples were ignored, so their high value raised nothing.
    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn fsr_2_2_non_finite_values_do_not_count_as_fresh() {
    let mut run = Run::new();
    run.sample(30.0, 28.0, 26.0);

    run.samples(20, f32::NAN, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert!(run.fault_time(FaultCode::FreshnessLost).is_some());
}

#[test]
fn fsr_2_2_fault_is_reported_once() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    run.advance(5_000);

    let reports = run
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FaultDetected {
                    fault: FaultCode::FreshnessLost,
                    ..
                }
            )
        })
        .count();
    assert_eq!(reports, 1);
}

#[test]
fn fsr_2_2_fault_references_last_fresh_sample() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);
    let last_sequence = run.sequence;

    run.advance(1_000);

    let last_sample = run
        .events
        .iter()
        .find_map(|e| match e.kind {
            EventKind::FaultDetected { last_sample, .. } => last_sample,
            _ => None,
        })
        .expect("fault references a sample");
    assert_eq!(last_sample.sequence, last_sequence);
}

// --- FSR-2.4: stuck maximum --------------------------------------------------

#[test]
fn fsr_2_4_frozen_maximum_while_average_moves_leads_to_degraded_within_budget() {
    let stuck = config().stuck;
    let mut run = Run::new();
    // Nominal heating: all values rise by 0.5 °C per second.
    let mut avg = 30.0;
    for _ in 0..5 {
        avg += 0.5;
        run.samples(10, avg + 5.0, avg, avg - 5.0);
    }
    // From here on, the maximum is stuck while the pack keeps heating.
    let stuck_max = avg + 5.0;
    let t0 = run.now + CYCLE_MS;
    for _ in 0..10 {
        avg += 0.5;
        run.samples(10, stuck_max, avg, avg - 5.0);
    }

    let detected = run.fault_time(FaultCode::SignalStuck).expect("stuck fault");
    assert!(detected - t0 <= stuck.timeout_ms + T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_4_battery_at_constant_temperature_is_not_stuck() {
    let mut run = Run::new();

    run.samples(200, 30.0, 28.0, 26.0);

    assert_eq!(run.fault_time(FaultCode::SignalStuck), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn fsr_2_4_slow_nominal_heating_is_not_stuck() {
    // Negative test from the safety concept: all values rise by one CAN
    // resolution step (0.5 °C) every two seconds.
    let mut run = Run::new();
    let mut avg = 30.0;
    for _ in 0..30 {
        run.samples(20, avg + 5.0, avg, avg - 5.0);
        avg += 0.5;
    }

    assert_eq!(run.fault_time(FaultCode::SignalStuck), None);
}

#[test]
fn fsr_2_4_small_movement_of_other_signals_is_not_stuck() {
    let delta = config().stuck.min_reference_change_c;
    let mut run = Run::new();
    run.sample(30.0, 28.0, 26.0);

    run.samples(100, 30.0, 28.0 + delta / 2.0, 26.0);

    assert_eq!(run.fault_time(FaultCode::SignalStuck), None);
}

// --- FSR-2.5: never lower the thermal state while DEGRADED -------------------

#[test]
fn fsr_2_5_degraded_during_warning_keeps_warning() {
    let mut run = Run::new();
    run.samples(5, 50.0, 40.0, 35.0);
    assert_eq!(run.thermal(), ThermalState::Warning);

    run.advance(1_000);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    // Data comes back with a low temperature.
    run.samples(20, 30.0, 28.0, 26.0);

    assert_eq!(run.thermal(), ThermalState::Warning);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_5_degraded_during_critical_keeps_critical() {
    let mut run = Run::new();
    run.samples(5, 60.0, 45.0, 40.0);

    run.advance(1_000);
    run.samples(20, 30.0, 28.0, 26.0);

    assert_eq!(run.thermal(), ThermalState::Critical);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_5_valid_sample_still_raises_thermal_state_while_degraded() {
    // Output model: a plausible critical sample always leads to CRITICAL.
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);
    run.advance(1_000);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);

    run.sample(60.0, 45.0, 40.0);

    assert_eq!(run.thermal(), ThermalState::Critical);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningOvertemp));
}

// --- FSR-D.1: fault codes identify the detecting requirement ------------------

#[test]
fn fsr_d_1_fault_codes_identify_detecting_requirement() {
    assert_eq!(FaultCode::FreshnessLost.requirement(), "FSR-2.2");
    assert_eq!(FaultCode::SignalStuck.requirement(), "FSR-2.4");
}

#[test]
fn fsr_d_1_fault_and_mitigation_form_a_causal_chain() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);
    run.advance(1_000);

    let fault = run
        .events
        .iter()
        .find(|e| matches!(e.kind, EventKind::FaultDetected { .. }))
        .expect("fault");
    let degraded = run
        .events
        .iter()
        .find(|e| matches!(e.kind, EventKind::MonitoringStatusChanged { .. }))
        .expect("status change");
    let mitigation = run
        .events
        .iter()
        .find(|e| matches!(e.kind, EventKind::MitigationRequested { .. }))
        .expect("mitigation");

    assert_eq!(degraded.cause, Some(fault.id));
    assert_eq!(mitigation.cause, Some(degraded.id));
}

#[test]
fn second_fault_is_reported_without_new_mitigation() {
    let mut run = Run::new();
    let mut avg = 30.0;
    run.sample(40.0, avg, avg - 5.0);
    for _ in 0..40 {
        avg += 0.1;
        run.sample(40.0, avg, avg - 5.0);
    }
    assert!(run.fault_time(FaultCode::SignalStuck).is_some());

    run.advance(1_000);

    assert!(run.fault_time(FaultCode::FreshnessLost).is_some());
    assert_eq!(
        run.mitigations(),
        vec![Mitigation::DriverWarningMonitoringUnavailable]
    );
}

// --- Determinism ---------------------------------------------------------------

#[test]
fn same_inputs_produce_identical_events() {
    let scenario = || {
        let mut run = Run::new();
        run.samples(10, 40.0, 35.0, 30.0);
        run.samples(10, 50.0, 40.0, 35.0);
        run.advance(1_000);
        run.samples(5, 60.0, 45.0, 40.0);
        run.events
    };

    assert_eq!(scenario(), scenario());
}

// --- Configuration ---------------------------------------------------------------

#[test]
fn config_rejects_warn_threshold_not_below_critical() {
    let text = SHIPPED_CONFIG.replace("warn_c = 45.0", "warn_c = 55.0");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_zero_stale_timeout() {
    let text = SHIPPED_CONFIG.replace("stale_timeout_ms = 300", "stale_timeout_ms = 0");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_unknown_parameters() {
    let text = format!("{SHIPPED_CONFIG}\n[unknown]\nvalue = 1\n");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}
