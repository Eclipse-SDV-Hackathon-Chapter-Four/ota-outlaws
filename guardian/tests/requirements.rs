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

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5); Codex / GPT-6.1 Sol (gpt-6.1-sol)

//! Requirement tests for the Guardian core.
//!
//! Each test is named after the functional safety requirement it verifies, as
//! defined in `docs/reference/hara.md`. All tests use the shipped
//! configuration, so they also check that the configuration file is valid.

use guardian::{
    Event, EventKind, FaultCode, Guardian, GuardianConfig, Millis, Mitigation, MonitoringStatus,
    Quality, Sample, ThermalState,
};

const SHIPPED_CONFIG: &str = include_str!("../../config/guardian/safety-params.toml");

/// Signal cycle of the source.
const CYCLE_MS: u64 = 100;
/// Tick interval of the runtime adapter.
const TICK_MS: u64 = 50;
/// Reaction time budget `T_react` from HARA DFR-1.
const T_REACT_MS: u64 = 100;
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
    alive_counter: u8,
    events: Vec<Event>,
}

impl Run {
    fn new() -> Self {
        Self {
            guardian: Guardian::new(&config()),
            now: 0,
            sequence: 0,
            alive_counter: 0,
            events: Vec::new(),
        }
    }

    /// Advances time by one cycle, then delivers a fresh sample.
    fn sample(&mut self, max_c: f32, avg_c: f32, min_c: f32) -> &mut Self {
        self.advance(CYCLE_MS);
        let source_timestamp_ms = self.now + SOURCE_CLOCK_OFFSET_MS;
        self.deliver(source_timestamp_ms, max_c, avg_c, min_c)
    }

    /// Advances time by one cycle, then delivers a sample with a fresh timestamp
    /// and the given alive counter and quality.
    fn frame(&mut self, alive_counter: u8, quality: Quality, max_c: f32) -> &mut Self {
        self.advance(CYCLE_MS);
        let source_timestamp_ms = self.now + SOURCE_CLOCK_OFFSET_MS;
        self.deliver_frame(
            source_timestamp_ms,
            alive_counter,
            quality,
            max_c,
            28.0,
            26.0,
        )
    }

    /// Delivers a sample with the given source timestamp and the next alive
    /// counter, without advancing time.
    fn deliver(
        &mut self,
        source_timestamp_ms: u64,
        max_c: f32,
        avg_c: f32,
        min_c: f32,
    ) -> &mut Self {
        let alive_counter = self.alive_counter.wrapping_add(1);
        self.deliver_frame(
            source_timestamp_ms,
            alive_counter,
            Quality::Valid,
            max_c,
            avg_c,
            min_c,
        )
    }

    fn deliver_frame(
        &mut self,
        source_timestamp_ms: u64,
        alive_counter: u8,
        quality: Quality,
        max_c: f32,
        avg_c: f32,
        min_c: f32,
    ) -> &mut Self {
        self.sequence += 1;
        self.alive_counter = alive_counter;
        let sample = Sample {
            source_timestamp_ms,
            sequence: self.sequence,
            alive_counter,
            quality,
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

    /// Driver warnings requested; `DiscardSample` is counted by
    /// [`Run::discarded`].
    fn mitigations(&self) -> Vec<Mitigation> {
        self.events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::MitigationRequested { mitigation }
                    if mitigation != Mitigation::DiscardSample =>
                {
                    Some(mitigation)
                }
                _ => None,
            })
            .collect()
    }

    /// Number of samples reported as discarded.
    fn discarded(&self) -> usize {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::MitigationRequested {
                        mitigation: Mitigation::DiscardSample
                    }
                )
            })
            .count()
    }

    fn active_fault_count(&self) -> usize {
        self.guardian.active_faults().count()
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
    // Heat plausibly (FSR-3.3) to just below the threshold, then reach it.
    let mut max = 40.0;
    while max < critical - 1.0 {
        run.sample(max, max - 5.0, max - 10.0);
        max += 1.0;
    }
    run.sample(critical, critical - 5.0, critical - 10.0);
    let t0 = run.now;

    let reacted = run
        .thermal_change_time(ThermalState::Critical)
        .expect("CRITICAL raised");

    assert!(reacted - t0 <= T_REACT_MS);
    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
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

// --- FSR-2.1: no data after startup ---------------------------------------------

#[test]
fn fsr_2_1_no_data_after_start_leads_to_degraded_within_budget() {
    // HARA TS-03: the Guardian starts, no temperature data arrives.
    let stale_timeout = config().freshness.stale_timeout_ms;
    let mut run = Run::new();

    run.advance(2_000);

    let detected = run
        .fault_time(FaultCode::NoDataAtStartup)
        .expect("startup fault");
    assert!(detected <= stale_timeout + T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(
        run.mitigations(),
        vec![Mitigation::DriverWarningMonitoringUnavailable]
    );
    assert_eq!(run.thermal(), ThermalState::Clear);
}

#[test]
fn fsr_2_1_first_sample_within_t_stale_is_no_fault() {
    let mut run = Run::new();

    run.samples(20, 30.0, 28.0, 26.0);

    assert_eq!(run.fault_time(FaultCode::NoDataAtStartup), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn fsr_2_1_startup_fault_recovers_when_data_arrives() {
    let mut run = Run::new();
    run.advance(1_000);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);

    run.samples(30, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
    assert_eq!(run.active_fault_count(), 0);
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
    // Nothing arrived at all, so this is not a stuck counter.
    assert_eq!(run.fault_time(FaultCode::CounterStuck), None);
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
    // A VSS Publisher that keeps repeating the last value while the source is dead.
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

/// Outcome of a stuck-maximum scenario.
struct StuckOutcome {
    /// How far the average moved between fault onset and detection. The real
    /// maximum moved by the same amount, unseen.
    hidden_error_c: f32,
    latency_ms: u64,
}

/// Heats the pack by one CAN step (1 °C) every `samples_per_step` samples,
/// then freezes the maximum while the pack keeps heating.
fn freeze_maximum_while_heating(samples_per_step: usize) -> StuckOutcome {
    let mut run = Run::new();
    let mut avg = 30.0_f32;
    for i in 0..2 * samples_per_step {
        if i % samples_per_step == 0 {
            avg += 1.0;
        }
        run.sample(avg + 40.0, avg, avg - 5.0);
    }
    // The maximum stays well above the rising average, so Min ≤ Avg ≤ Max
    // holds (FSR-3.1) until the stuck fault is detected.
    let stuck_max = avg + 40.0;
    let onset_avg = avg;
    let t0 = run.now + CYCLE_MS;

    // Twenty heating steps, but at least one minute.
    for i in 1..=(20 * samples_per_step).max(600) {
        if i % samples_per_step == 0 {
            avg += 1.0;
        }
        run.sample(stuck_max, avg, avg - 5.0);
        if let Some(detected) = run.fault_time(FaultCode::SignalStuck) {
            assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
            return StuckOutcome {
                hidden_error_c: avg - onset_avg,
                latency_ms: detected - t0,
            };
        }
    }
    panic!("stuck maximum not detected");
}

#[test]
fn fsr_2_4_stuck_maximum_with_fast_heating_is_detected_within_budget() {
    // 1 °C per second: the average moves by Δ_stuck before T_stuck has passed,
    // so T_stuck dominates.
    let stuck = config().stuck;

    let outcome = freeze_maximum_while_heating(10);

    assert!(outcome.latency_ms <= stuck.timeout_ms + T_REACT_MS);
    assert!(outcome.hidden_error_c <= stuck.min_reference_change_c + 1.0);
}

#[test]
fn fsr_2_4_stuck_maximum_with_very_slow_heating_bounds_hidden_error() {
    // 0.05 °C per second: detection takes long, but the hidden error stays
    // within Δ_stuck plus one CAN step.
    let stuck = config().stuck;

    let outcome = freeze_maximum_while_heating(200);

    assert!(outcome.hidden_error_c <= stuck.min_reference_change_c + 1.0);
    assert!(outcome.latency_ms > stuck.timeout_ms);
}

#[test]
fn fsr_2_4_stuck_maximum_is_not_reported_before_t_stuck() {
    // 10 °C per second: the average moves by Δ_stuck almost at once, but a
    // plateau of the maximum shorter than T_stuck is not a fault.
    let stuck = config().stuck;

    let outcome = freeze_maximum_while_heating(1);

    assert!(outcome.latency_ms >= stuck.timeout_ms);
    assert!(outcome.latency_ms <= stuck.timeout_ms + T_REACT_MS);
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
    // Negative case for HARA FSR-2.4: all values rise by one CAN step
    // (1 °C) every five seconds, so the maximum stays unchanged for longer than
    // T_stuck, but so do the others.
    let mut run = Run::new();
    let mut avg = 30.0;
    for _ in 0..20 {
        run.samples(50, avg + 5.0, avg, avg - 5.0);
        avg += 1.0;
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
    run.samples(8, 30.0, 28.0, 26.0);

    assert_eq!(run.thermal(), ThermalState::Warning);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_5_degraded_during_critical_keeps_critical() {
    let mut run = Run::new();
    run.samples(5, 60.0, 45.0, 40.0);

    run.advance(1_000);
    run.samples(8, 30.0, 28.0, 26.0);

    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_5_valid_sample_still_raises_thermal_state_while_degraded() {
    // Output model: a plausible critical sample always leads to CRITICAL.
    let mut run = Run::new();
    run.samples(5, 45.0, 40.0, 35.0);
    run.advance(1_000);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);

    // 11 °C in 1.1 s is a plausible rise (FSR-3.3).
    run.sample(56.0, 45.0, 40.0);

    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningOvertemp));
}

// --- FSR-2.3: stuck alive counter ---------------------------------------------

/// Delivers `count` frames that repeat the alive counter of the last fresh
/// sample, one per cycle.
fn repeat_frames(run: &mut Run, count: u32) {
    let frozen_counter = run.alive_counter;
    for _ in 0..count {
        run.frame(frozen_counter, Quality::Valid, 30.0);
    }
}

#[test]
fn fsr_2_3_repeated_frames_lead_to_counter_stuck_within_budget() {
    // A frozen source keeps sending the same frame. The timestamps stay fresh;
    // only the alive counter reveals it.
    let stuck_frames = config().freshness.stuck_repeated_frames;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    let t0 = run.now + CYCLE_MS;

    repeat_frames(&mut run, 20);

    let detected = run
        .fault_time(FaultCode::CounterStuck)
        .expect("counter stuck");
    assert!(detected - t0 <= u64::from(stuck_frames) * CYCLE_MS + T_REACT_MS);
    assert_eq!(run.fault_time(FaultCode::FreshnessLost), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_3_counter_stuck_exactly_at_n_stuck_repeated_frames() {
    let stuck_frames = config().freshness.stuck_repeated_frames;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    repeat_frames(&mut run, stuck_frames - 1);
    assert_eq!(run.fault_time(FaultCode::CounterStuck), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);

    repeat_frames(&mut run, 1);
    assert_eq!(run.fault_time(FaultCode::CounterStuck), Some(run.now));
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningMonitoringUnavailable));
}

#[test]
fn fsr_2_3_suspect_exactly_at_n_suspect_repeated_frames() {
    let suspect_frames = config().freshness.suspect_repeated_frames;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    repeat_frames(&mut run, suspect_frames - 1);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);

    repeat_frames(&mut run, 1);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
    assert_eq!(run.active_fault_count(), 0);
    assert!(run.mitigations().is_empty());
}

#[test]
fn dfr_8_suspect_recovers_only_after_n_recover_fresh_samples() {
    let suspect_frames = config().freshness.suspect_repeated_frames;
    let recover = config().recovery.valid_samples as usize;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    repeat_frames(&mut run, suspect_frames);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);

    run.samples(recover - 1, 30.0, 28.0, 26.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);

    run.sample(30.0, 28.0, 26.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.active_fault_count(), 0);
    assert!(run.mitigations().is_empty());
}

#[test]
fn dfr_8_repeated_frame_restarts_suspect_recovery() {
    let suspect_frames = config().freshness.suspect_repeated_frames;
    let recover = config().recovery.valid_samples as usize;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    repeat_frames(&mut run, suspect_frames);

    run.samples(recover - 1, 30.0, 28.0, 26.0);
    repeat_frames(&mut run, 1);
    run.samples(recover - 1, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
}

// --- HARA DFR-7 / TS-07: duplicated messages ----------------------------------

/// Delivers an exact copy of the last fresh sample: same source timestamp,
/// alive counter, quality, and payload.
fn duplicate_last(run: &mut Run, count: u32) {
    let source_timestamp_ms = run.now + SOURCE_CLOCK_OFFSET_MS;
    let alive_counter = run.alive_counter;
    for _ in 0..count {
        run.advance(CYCLE_MS);
        run.deliver_frame(
            source_timestamp_ms,
            alive_counter,
            Quality::Valid,
            30.0,
            28.0,
            26.0,
        );
    }
}

#[test]
fn ts_07_second_message_with_the_same_counter_sets_suspect() {
    // DFR-7: two messages with the same counter, the original and one duplicate.
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    duplicate_last(&mut run, 1);

    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
    assert_eq!(run.active_fault_count(), 0);
    assert!(run.mitigations().is_empty());
}

#[test]
fn ts_07_single_duplicate_causes_no_freshness_timeout_and_recovers() {
    let recover = config().recovery.valid_samples as usize;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    duplicate_last(&mut run, 1);
    run.samples(recover, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.fault_time(FaultCode::FreshnessLost), None);
    assert!(run.mitigations().is_empty());
}

#[test]
fn ts_07_tenth_message_with_the_same_counter_sets_degraded() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    // The original plus eight duplicates: nine messages.
    duplicate_last(&mut run, 8);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);

    duplicate_last(&mut run, 1);
    assert_eq!(run.fault_time(FaultCode::CounterStuck), Some(run.now));
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn ts_07_persistent_duplicates_lead_to_degraded() {
    let stuck_frames = config().freshness.stuck_repeated_frames;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    duplicate_last(&mut run, stuck_frames);

    assert_eq!(run.fault_time(FaultCode::CounterStuck), Some(run.now));
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningMonitoringUnavailable));
}

#[test]
fn ts_07_duplicates_do_not_advance_the_thermal_assessment() {
    let mut run = Run::new();
    ramp_to(&mut run, 44.0);
    let last = run.now + SOURCE_CLOCK_OFFSET_MS;
    let counter = run.alive_counter;

    run.advance(CYCLE_MS);
    run.deliver_frame(last, counter, Quality::Valid, 50.0, 42.0, 34.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

// --- HARA TS-08: out-of-order messages ----------------------------------------

#[test]
fn ts_08_out_of_order_sample_sets_suspect_and_is_ignored() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    let older = run.now + SOURCE_CLOCK_OFFSET_MS - CYCLE_MS;
    let newest = run.alive_counter;

    run.advance(CYCLE_MS);
    run.deliver_frame(
        older,
        newest.wrapping_sub(1),
        Quality::Valid,
        70.0,
        62.0,
        54.0,
    );
    // The source itself continues from its newest counter.
    run.alive_counter = newest;

    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
    assert_eq!(run.active_fault_count(), 0);
    assert!(run.mitigations().is_empty());

    let recover = config().recovery.valid_samples as usize;
    run.samples(recover, 30.0, 28.0, 26.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn fsr_2_3_single_repeated_frame_before_outage_is_freshness_lost() {
    // A duplicate followed by a complete outage: the source is not repeating
    // itself, the data simply stopped.
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    repeat_frames(&mut run, 1);

    run.advance(2_000);

    assert!(run.fault_time(FaultCode::FreshnessLost).is_some());
    assert_eq!(run.fault_time(FaultCode::CounterStuck), None);
}

#[test]
fn fsr_2_3_repeated_frames_below_n_stuck_before_outage_are_freshness_lost() {
    let stuck_frames = config().freshness.stuck_repeated_frames;
    let stale_timeout = config().freshness.stale_timeout_ms;
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    repeat_frames(&mut run, stuck_frames - 1);
    let last_frame = run.now;

    run.advance(2_000);

    // T_stale counts from the last repeated frame: the source sent until then.
    let detected = run
        .fault_time(FaultCode::FreshnessLost)
        .expect("freshness lost");
    assert!(detected - last_frame <= stale_timeout + T_REACT_MS);
    assert_eq!(run.fault_time(FaultCode::CounterStuck), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_2_3_repeated_frames_are_not_evaluated() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);
    let frozen_counter = run.alive_counter;

    run.frame(frozen_counter, Quality::Valid, 70.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn fsr_2_3_counter_wraparound_is_fresh() {
    let mut run = Run::new();

    // More than 256 frames, so the counter wraps from 255 to 0.
    run.samples(600, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.active_fault_count(), 0);
}

// --- FSR-3.4: quality flag ------------------------------------------------------

#[test]
fn fsr_3_4_invalid_quality_leads_to_degraded_immediately() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    let next = run.alive_counter.wrapping_add(1);
    run.frame(next, Quality::Invalid, 30.0);
    let t0 = run.now;

    let detected = run
        .fault_time(FaultCode::QualityInvalid)
        .expect("quality fault");
    assert!(detected - t0 <= T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(
        run.mitigations(),
        vec![Mitigation::DriverWarningMonitoringUnavailable]
    );
}

#[test]
fn fsr_3_4_quality_not_available_is_treated_as_invalid() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    let next = run.alive_counter.wrapping_add(1);
    run.frame(next, Quality::NotAvailable, 30.0);

    assert!(run.fault_time(FaultCode::QualityInvalid).is_some());
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_3_4_invalid_sample_does_not_change_thermal_state() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    let next = run.alive_counter.wrapping_add(1);
    run.frame(next, Quality::Invalid, 70.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn fsr_3_4_invalid_samples_still_show_the_source_is_alive() {
    let mut run = Run::new();
    run.samples(5, 30.0, 28.0, 26.0);

    for _ in 0..20 {
        let next = run.alive_counter.wrapping_add(1);
        run.frame(next, Quality::Invalid, 30.0);
    }

    assert_eq!(run.fault_time(FaultCode::FreshnessLost), None);
    assert_eq!(run.fault_time(FaultCode::CounterStuck), None);
}

// --- FSR-3.1 to FSR-3.3, FSR-3.6: plausibility and mitigation gating -----------

/// Heats from 30 °C to `to` at 1 °C per cycle, below `r_max`.
fn ramp_to(run: &mut Run, to: f32) {
    let mut max = 30.0;
    while max < to {
        max = (max + 1.0).min(to);
        run.sample(max, max - 8.0, max - 16.0);
    }
}

fn overtemp_requested(run: &Run) -> bool {
    run.mitigations()
        .contains(&Mitigation::DriverWarningOvertemp)
}

#[test]
fn fsr_3_1_average_above_maximum_is_invalid() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    run.sample(70.0, 90.0, 20.0);

    assert!(run.fault_time(FaultCode::OrderImplausible).is_some());
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
    assert!(!overtemp_requested(&run));
}

#[test]
fn fsr_3_1_minimum_above_average_or_maximum_is_invalid() {
    for (max, avg, min) in [(70.0, 40.0, 60.0), (40.0, 50.0, 70.0)] {
        let mut run = Run::new();
        run.samples(10, 30.0, 28.0, 26.0);

        run.sample(max, avg, min);

        assert!(
            run.fault_time(FaultCode::OrderImplausible).is_some(),
            "{max}/{avg}/{min}"
        );
    }
}

#[test]
fn fsr_3_1_equal_temperatures_are_plausible() {
    let mut run = Run::new();

    run.samples(10, 30.0, 30.0, 30.0);

    assert_eq!(run.fault_time(FaultCode::OrderImplausible), None);
}

#[test]
fn fsr_3_2_out_of_range_high_raises_warning_not_critical() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    let t0 = run.now + CYCLE_MS;

    run.sample(250.0, 80.0, 10.0);

    let detected = run.fault_time(FaultCode::OutOfRange).expect("out of range");
    assert!(detected - t0 <= T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(run.thermal(), ThermalState::Warning);
    assert!(!overtemp_requested(&run));
}

#[test]
fn fsr_3_2_range_bounds_are_plausible() {
    let plausibility = config().plausibility;
    let mut run = Run::new();

    ramp_to(&mut run, plausibility.max_c);

    assert_eq!(run.fault_time(FaultCode::OutOfRange), None);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn ts_20_isolated_spike_sets_suspect_and_warning_not_critical() {
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let t0 = run.now + CYCLE_MS;

    run.sample(70.0, 62.0, 54.0);

    let warning = run
        .thermal_change_time(ThermalState::Warning)
        .expect("warning");
    assert!(warning - t0 <= T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
    assert_eq!(run.fault_time(FaultCode::RateImplausible), None);
    // SUSPECT is a debounce: no monitoring-unavailable warning (TS-20).
    assert!(run.mitigations().is_empty());

    // The spike was discarded: valid nominal samples follow without a fault.
    let recover = config().recovery.valid_samples as usize;
    run.samples(recover, 40.0, 32.0, 24.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.thermal(), ThermalState::Warning);
    assert_eq!(run.active_fault_count(), 0);
}

#[test]
fn ts_20_warning_from_isolated_spike_is_caused_by_suspect() {
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);

    run.sample(70.0, 62.0, 54.0);

    let suspect = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::MonitoringStatusChanged {
                    to: MonitoringStatus::Suspect,
                    ..
                }
            )
        })
        .expect("suspect");
    let warning = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::ThermalStateChanged {
                    to: ThermalState::Warning,
                    ..
                }
            )
        })
        .expect("warning");
    assert_eq!(warning.cause, Some(suspect.id));
}

#[test]
fn ts_21_repeated_spikes_within_t_suspect_lead_to_degraded() {
    let plausibility = config().plausibility;
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let t0 = run.now + CYCLE_MS;

    // Each spike rises far more than r_max since the last valid sample.
    for _ in 0..plausibility.suspect_spikes {
        run.sample(100.0, 92.0, 84.0);
    }

    let detected = run
        .fault_time(FaultCode::RateImplausible)
        .expect("rate implausible");
    assert!(detected - t0 <= plausibility.suspect_window_ms + T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(run.thermal(), ThermalState::Warning);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningMonitoringUnavailable));
    assert!(!overtemp_requested(&run));
}

#[test]
fn ts_21_spikes_further_apart_than_t_suspect_stay_suspect() {
    let plausibility = config().plausibility;
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let gap = (plausibility.suspect_window_ms / CYCLE_MS) as usize;

    for _ in 0..plausibility.suspect_spikes {
        run.sample(70.0, 62.0, 54.0);
        run.samples(gap, 40.0, 32.0, 24.0);
    }

    assert_eq!(run.fault_time(FaultCode::RateImplausible), None);
    assert_ne!(run.monitoring(), MonitoringStatus::Degraded);
}

#[test]
fn fsr_3_6_warning_from_invalid_data_is_caused_by_its_fault() {
    // The evidence chain must show that this WARNING comes from a sensor
    // fault, not from a real overtemperature.
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);

    run.sample(250.0, 80.0, 10.0);

    let fault = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::FaultDetected {
                    fault: FaultCode::OutOfRange,
                    ..
                }
            )
        })
        .expect("out of range");
    let warning = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::ThermalStateChanged {
                    to: ThermalState::Warning,
                    ..
                }
            )
        })
        .expect("warning");
    assert_eq!(warning.cause, Some(fault.id));
}

#[test]
fn fsr_1_1_warning_from_valid_data_has_no_fault_cause() {
    let mut run = Run::new();
    ramp_to(&mut run, 46.0);

    let warning = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::ThermalStateChanged {
                    to: ThermalState::Warning,
                    ..
                }
            )
        })
        .expect("warning");
    assert_eq!(warning.cause, None);
}

#[test]
fn fsr_3_3_fast_plausible_rise_is_valid() {
    // 1.5 °C per 100 ms is 15 °C/s, below r_max.
    let mut run = Run::new();
    run.samples(5, 30.0, 22.0, 14.0);
    let mut max = 30.0;
    for _ in 0..10 {
        max += 1.5;
        run.sample(max, max - 8.0, max - 16.0);
    }

    assert_eq!(run.fault_time(FaultCode::RateImplausible), None);
}

#[test]
fn fsr_3_3_one_resolution_step_is_plausible_under_timestamp_jitter() {
    // Observed in the real chain: frames reach the Data Broker in a burst,
    // 2 ms apart. A rise of one CAN resolution step (1 °C) is not a spike.
    let mut run = Run::new();
    run.samples(5, 38.0, 31.0, 24.0);
    let last = run.now + SOURCE_CLOCK_OFFSET_MS;
    run.deliver(last + 2, 39.0, 32.0, 25.0);
    run.deliver(last + 4, 40.0, 32.0, 25.0);

    assert_eq!(run.fault_time(FaultCode::RateImplausible), None);
}

#[test]
fn fsr_3_3_larger_rise_within_jitter_is_a_spike() {
    let mut run = Run::new();
    run.samples(5, 38.0, 31.0, 24.0);
    let last = run.now + SOURCE_CLOCK_OFFSET_MS;
    run.deliver(last + 2, 41.0, 32.0, 25.0);

    // Isolated, so only SUSPECT (FSR-3.5).
    assert_eq!(run.monitoring(), MonitoringStatus::Suspect);
}

// --- FSR-1.4: hot spot -------------------------------------------------------------

#[test]
fn fsr_1_4_hot_spot_below_warn_raises_warning_within_t_react() {
    let spread = config().thermal.hotspot_spread_c;
    let mut run = Run::new();
    // Spread just below Δ_hotspot, so the next step is a plausible rise.
    run.samples(10, 30.0 + spread - 1.0, 30.0, 24.0);
    let t0 = run.now + CYCLE_MS;

    // The maximum rises plausibly while the average stays: a local hot spot.
    run.sample(30.0 + spread + 1.0, 30.0, 24.0);

    let warning = run
        .thermal_change_time(ThermalState::Warning)
        .expect("warning from hot spot");
    assert!(warning - t0 <= T_REACT_MS);
    assert_eq!(run.active_fault_count(), 0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert!(run.mitigations().is_empty());
}

#[test]
fn fsr_1_4_spread_of_exactly_delta_hotspot_is_no_hot_spot() {
    let spread = config().thermal.hotspot_spread_c;
    let mut run = Run::new();

    run.samples(20, 30.0 + spread, 30.0, 24.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn fsr_1_4_hot_spot_warning_holds_while_the_spread_persists() {
    let recover = config().recovery.valid_samples as usize;
    let mut run = Run::new();
    run.samples(10, 36.0, 30.0, 24.0);
    run.sample(42.0, 30.0, 24.0);
    assert_eq!(run.thermal(), ThermalState::Warning);

    run.samples(30, 42.0, 30.0, 24.0);
    assert_eq!(run.thermal(), ThermalState::Warning);

    // Spread back below Δ_hotspot and below θ_warn − hysteresis.
    run.samples(recover + 1, 36.0, 30.0, 24.0);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

// --- FSR-1.6, FSR-1.7: MITIGATING and failed mitigation -----------------------------

/// Heats plausibly from 47 °C to `to` at 1 °C per cycle, spread 8 °C.
fn heat_to(run: &mut Run, to: f32) {
    run.samples(10, 47.0, 39.0, 31.0);
    let mut max = 47.0;
    while max < to {
        max += 1.0;
        run.sample(max, max - 8.0, max - 16.0);
    }
}

fn event_of(run: &Run, matches: impl Fn(&EventKind) -> bool) -> &Event {
    run.events.iter().find(|e| matches(&e.kind)).expect("event")
}

#[test]
fn fsr_1_6_critical_moves_to_mitigating_after_the_request() {
    let mut run = Run::new();

    heat_to(&mut run, 55.0);

    let critical = event_of(&run, |k| {
        matches!(
            k,
            EventKind::ThermalStateChanged {
                to: ThermalState::Critical,
                ..
            }
        )
    });
    let request = event_of(&run, |k| {
        matches!(
            k,
            EventKind::MitigationRequested {
                mitigation: Mitigation::DriverWarningOvertemp
            }
        )
    });
    let mitigating = event_of(&run, |k| {
        matches!(
            k,
            EventKind::ThermalStateChanged {
                from: ThermalState::Critical,
                to: ThermalState::Mitigating,
                ..
            }
        )
    });
    assert_eq!(request.cause, Some(critical.id));
    assert_eq!(mitigating.cause, Some(request.id));
    assert_eq!(mitigating.at, critical.at);
    assert_eq!(run.thermal(), ThermalState::Mitigating);
}

#[test]
fn fsr_1_6_mitigating_is_lowered_with_hysteresis() {
    let recover = config().recovery.valid_samples as usize;
    let mut run = Run::new();
    heat_to(&mut run, 55.0);

    run.samples(recover + 1, 52.0, 44.0, 36.0);

    assert_eq!(run.thermal(), ThermalState::Warning);
}

#[test]
fn fsr_1_7_still_rising_after_t_mitigation_repeats_the_request() {
    let timeout = config().thermal.mitigation_timeout_ms;
    let mut run = Run::new();
    heat_to(&mut run, 55.0);
    let requested = run.now;

    // Keeps rising slowly, 1 °C per second, for longer than T_mitigation.
    let mut max = 55.0;
    while run.now - requested < timeout + 1_000 {
        max += 0.1;
        run.sample(max, max - 8.0, max - 16.0);
    }

    let overtemp = run
        .mitigations()
        .into_iter()
        .filter(|m| *m == Mitigation::DriverWarningOvertemp)
        .count();
    assert_eq!(overtemp, 2);
    let failed = event_of(&run, |k| {
        matches!(
            k,
            EventKind::ThermalStateChanged {
                from: ThermalState::Mitigating,
                to: ThermalState::Critical,
                ..
            }
        )
    });
    assert!(failed.at.0 - requested >= timeout);
    assert!(failed.at.0 - requested <= timeout + T_REACT_MS);
    assert_eq!(run.thermal(), ThermalState::Mitigating);
}

#[test]
fn fsr_1_7_steady_temperature_while_mitigating_is_no_failure() {
    let timeout = config().thermal.mitigation_timeout_ms;
    let mut run = Run::new();
    heat_to(&mut run, 56.0);

    run.samples((3 * timeout / CYCLE_MS) as usize, 56.0, 48.0, 40.0);

    assert_eq!(run.mitigations(), vec![Mitigation::DriverWarningOvertemp]);
    assert_eq!(run.thermal(), ThermalState::Mitigating);
}

// --- DiscardSample (HARA TS-06, TS-19, TS-20) ---------------------------------------

#[test]
fn discard_sample_is_reported_for_each_sample_not_evaluated() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    assert_eq!(run.discarded(), 0);

    duplicate_last(&mut run, 1);
    assert_eq!(run.discarded(), 1);

    run.frame(run.alive_counter.wrapping_add(1), Quality::Invalid, 30.0);
    assert_eq!(run.discarded(), 2);

    run.sample(70.0, 90.0, 20.0);
    assert_eq!(run.discarded(), 3);
}

#[test]
fn discard_sample_is_caused_by_the_fault_it_triggered() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);

    run.frame(run.alive_counter.wrapping_add(1), Quality::Invalid, 30.0);

    let fault = event_of(&run, |k| matches!(k, EventKind::FaultDetected { .. }));
    let discard = event_of(&run, |k| {
        matches!(
            k,
            EventKind::MitigationRequested {
                mitigation: Mitigation::DiscardSample
            }
        )
    });
    assert_eq!(discard.cause, Some(fault.id));
}

#[test]
fn discard_sample_of_isolated_spike_is_caused_by_suspect() {
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);

    run.sample(70.0, 62.0, 54.0);

    let suspect = event_of(&run, |k| {
        matches!(
            k,
            EventKind::MonitoringStatusChanged {
                to: MonitoringStatus::Suspect,
                ..
            }
        )
    });
    let discard = event_of(&run, |k| {
        matches!(
            k,
            EventKind::MitigationRequested {
                mitigation: Mitigation::DiscardSample
            }
        )
    });
    assert_eq!(discard.cause, Some(suspect.id));
}

// --- HARA TS-24: late-arriving stale message ----------------------------------------

#[test]
fn ts_24_late_sample_is_not_fresh_and_does_not_update_the_assessment() {
    let max_age = config().freshness.max_age_ms;
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let last_source = run.now + SOURCE_CLOCK_OFFSET_MS;

    // The next sample is held back for longer than T_age, nothing else arrives.
    run.advance(max_age + 500);
    assert!(run.fault_time(FaultCode::FreshnessLost).is_some());
    let discarded = run.discarded();
    run.deliver(last_source + CYCLE_MS, 50.0, 42.0, 34.0);

    assert_eq!(run.discarded(), discarded + 1);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);

    // Fresh samples resume and monitoring recovers.
    let recover = config().recovery.valid_samples as usize;
    run.samples(recover + 1, 40.0, 32.0, 24.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

#[test]
fn ts_24_delay_below_t_age_is_accepted() {
    let max_age = config().freshness.max_age_ms;
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let last_source = run.now + SOURCE_CLOCK_OFFSET_MS;

    run.advance(max_age - 200);
    run.deliver(last_source + CYCLE_MS, 40.0, 32.0, 24.0);

    assert_eq!(run.discarded(), 0);
}

#[test]
fn ts_24_source_outage_is_not_a_late_sample() {
    // The source itself paused: its timestamps advance with the gap.
    let max_age = config().freshness.max_age_ms;
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);

    run.advance(max_age * 3);
    run.samples(20, 40.0, 32.0, 24.0);

    assert_eq!(run.discarded(), 0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
}

// --- HARA TS-26: upper-scale saturation -----------------------------------------

#[test]
fn ts_26_saturated_maximum_is_degraded_and_warning_not_critical() {
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);
    let t0 = run.now + CYCLE_MS;

    run.sample(255.0, 32.0, 24.0);

    let detected = run.fault_time(FaultCode::OutOfRange).expect("out of range");
    assert!(detected - t0 <= T_REACT_MS);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(run.thermal(), ThermalState::Warning);
    assert!(run
        .mitigations()
        .contains(&Mitigation::DriverWarningMonitoringUnavailable));
    assert!(!overtemp_requested(&run));
}

// --- FSR-1.3 / HARA TS-25: rising trend below θ_warn ----------------------------

/// Rises from 20 °C by `step` °C per cycle for `cycles` cycles.
fn drift(run: &mut Run, step: f32, cycles: usize) {
    let mut max = 20.0;
    for _ in 0..cycles {
        max += step;
        run.sample(max, max - 4.0, max - 8.0);
    }
}

#[test]
fn ts_25_sustained_rise_below_warn_raises_warning_within_budget() {
    let thermal = config().thermal;
    let mut run = Run::new();
    run.samples(5, 20.0, 16.0, 12.0);
    let t0 = run.now;

    // 1.2 °C/s for 8 s: stays below θ_warn.
    drift(&mut run, 0.12, 80);

    let warning = run
        .thermal_change_time(ThermalState::Warning)
        .expect("warning from trend");
    assert!(warning - t0 <= thermal.trend_duration_ms + T_REACT_MS);
    assert_eq!(run.thermal(), ThermalState::Warning);
    assert!(!overtemp_requested(&run));
    assert_eq!(run.active_fault_count(), 0);
}

#[test]
fn ts_25_drift_at_can_resolution_raises_warning() {
    // The campaign's drift trace: +1 °C every 600 ms, integer values.
    let mut run = Run::new();
    run.samples(50, 30.0, 22.0, 14.0);
    for max in 31..43 {
        let max = max as f32;
        run.samples(6, max, max - 8.0, max - 16.0);
    }

    assert_eq!(run.thermal(), ThermalState::Warning);
    assert_eq!(run.active_fault_count(), 0);
    assert!(run.mitigations().is_empty());
}

#[test]
fn ts_25_short_fast_rise_after_a_plateau_is_no_trend() {
    // Seen in the campaign (invalid_during_warning): 5 s at 30 °C, then
    // +1 °C per 100 ms. The average over T_trend is high, but the rise is not
    // sustained; only θ_warn may raise WARNING here.
    let mut run = Run::new();
    run.samples(50, 30.0, 22.0, 14.0);
    for max in 31..45 {
        let max = max as f32;
        run.sample(max, max - 8.0, max - 16.0);
    }
    assert_eq!(run.thermal(), ThermalState::Monitoring);

    run.sample(45.0, 37.0, 29.0);
    assert_eq!(run.thermal(), ThermalState::Warning);
}

#[test]
fn ts_25_slow_rise_is_no_trend() {
    let mut run = Run::new();
    run.samples(5, 20.0, 16.0, 12.0);

    // 0.5 °C/s for 10 s, below r_trend.
    drift(&mut run, 0.05, 100);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn ts_25_warning_from_trend_is_lowered_after_the_rise_stops() {
    let recovery = config().recovery;
    let mut run = Run::new();
    run.samples(5, 20.0, 16.0, 12.0);
    drift(&mut run, 0.12, 80);
    assert_eq!(run.thermal(), ThermalState::Warning);

    // Held constant: the trend ends once the window no longer spans the rise.
    let hold = (config().thermal.trend_duration_ms / CYCLE_MS) as usize;
    run.samples(hold, 29.6, 25.6, 21.6);
    run.samples(recovery.valid_samples as usize + 1, 29.6, 25.6, 21.6);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn ts_25_gap_does_not_turn_a_slow_rise_into_a_trend() {
    let mut run = Run::new();
    run.samples(10, 20.0, 16.0, 12.0);

    // 6 °C higher after a 10 s gap: 0.6 °C/s, below r_trend.
    run.advance(10_000);
    run.samples(60, 26.0, 22.0, 18.0);

    assert_eq!(run.thermal(), ThermalState::Monitoring);
}

#[test]
fn fsr_3_3_sustained_high_value_becomes_valid_and_escalates() {
    // A real runaway that keeps a high value: the first samples are rejected
    // as a spike, but once the implied rise since the last valid sample is
    // below r_max, the value is valid and CRITICAL follows (fail toward warning).
    let mut run = Run::new();
    run.samples(10, 40.0, 32.0, 24.0);

    run.samples(50, 100.0, 92.0, 84.0);

    assert!(run.fault_time(FaultCode::RateImplausible).is_some());
    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    assert!(overtemp_requested(&run));
}

#[test]
fn fsr_3_6_invalid_sample_never_lowers_the_thermal_state() {
    let mut run = Run::new();
    ramp_to(&mut run, 47.0);
    assert_eq!(run.thermal(), ThermalState::Warning);

    // Invalid samples that would look cool: maximum below the average.
    run.samples(30, 20.0, 40.0, 10.0);

    assert_eq!(run.thermal(), ThermalState::Warning);
}

#[test]
fn fsr_3_6_invalid_input_never_raises_critical_or_mitigation() {
    for (max, avg, min) in [
        (250.0, 80.0, 10.0),
        (70.0, 90.0, 20.0),
        (160.0, 160.0, 160.0),
    ] {
        let mut run = Run::new();
        run.samples(10, 40.0, 32.0, 24.0);

        run.sample(max, avg, min);

        assert!(
            run.thermal().severity() < ThermalState::Critical.severity(),
            "{max}/{avg}/{min}"
        );
        assert!(!overtemp_requested(&run), "{max}/{avg}/{min}");
    }
}

#[test]
fn fsr_3_6_valid_critical_sample_still_requests_mitigation() {
    // Positive control for the gating: valid data must still escalate.
    let mut run = Run::new();

    ramp_to(&mut run, 56.0);

    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    assert!(overtemp_requested(&run));
    assert_eq!(run.active_fault_count(), 0);
}

#[test]
fn plausibility_faults_recover_after_valid_samples() {
    let mut run = Run::new();
    run.samples(10, 30.0, 28.0, 26.0);
    run.sample(70.0, 90.0, 20.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);

    run.samples(30, 30.0, 28.0, 26.0);

    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.active_fault_count(), 0);
}

// --- FSR-D.1: fault codes identify the detecting requirement ------------------

#[test]
fn fsr_d_1_fault_codes_identify_detecting_requirement() {
    assert_eq!(FaultCode::FreshnessLost.requirement(), "FSR-2.2");
    assert_eq!(FaultCode::SignalStuck.requirement(), "FSR-2.4");
    assert_eq!(FaultCode::CounterStuck.requirement(), "FSR-2.3");
    assert_eq!(FaultCode::QualityInvalid.requirement(), "FSR-3.4");
    assert_eq!(FaultCode::OrderImplausible.requirement(), "FSR-3.1");
    assert_eq!(FaultCode::OutOfRange.requirement(), "FSR-3.2");
    assert_eq!(FaultCode::RateImplausible.requirement(), "FSR-3.3");
    assert_eq!(FaultCode::NoDataAtStartup.requirement(), "FSR-2.1");
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
fn config_rejects_zero_suspect_repeated_frames() {
    let text = SHIPPED_CONFIG.replace("suspect_repeated_frames = 1", "suspect_repeated_frames = 0");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_single_stuck_repeated_frame() {
    let text = SHIPPED_CONFIG.replace("stuck_repeated_frames = 9", "stuck_repeated_frames = 1");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_suspect_after_stuck() {
    let text = SHIPPED_CONFIG.replace(
        "suspect_repeated_frames = 1",
        "suspect_repeated_frames = 10",
    );

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_single_suspect_spike() {
    let text = SHIPPED_CONFIG.replace("suspect_spikes = 3", "suspect_spikes = 1");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn config_rejects_zero_trend_parameters() {
    for (from, to) in [
        ("trend_rise_c_per_s = 1.0", "trend_rise_c_per_s = 0.0"),
        ("trend_duration_ms = 5000", "trend_duration_ms = 0"),
        ("suspect_window_ms = 1000", "suspect_window_ms = 0"),
    ] {
        let text = SHIPPED_CONFIG.replace(from, to);

        assert!(GuardianConfig::from_toml_str(&text).is_err(), "{to}");
    }
}

#[test]
fn config_rejects_zero_hotspot_mitigation_and_age_parameters() {
    for (from, to) in [
        ("hotspot_spread_c = 10.0", "hotspot_spread_c = 0.0"),
        ("mitigation_timeout_ms = 10000", "mitigation_timeout_ms = 0"),
        ("max_age_ms = 1000", "max_age_ms = 0"),
    ] {
        let text = SHIPPED_CONFIG.replace(from, to);

        assert!(GuardianConfig::from_toml_str(&text).is_err(), "{to}");
    }
}

#[test]
fn config_rejects_unknown_parameters() {
    let text = format!("{SHIPPED_CONFIG}\n[unknown]\nvalue = 1\n");

    assert!(GuardianConfig::from_toml_str(&text).is_err());
}

#[test]
fn recovery_thermal_requires_hysteresis_and_sustained_valid_data() {
    let mut run = Run::new();
    run.sample(60.0, 40.0, 30.0);
    // Boundary is not below hysteresis. FSR-1.6: CRITICAL moves on to
    // MITIGATING once the mitigation is requested.
    run.samples(30, 53.0, 45.0, 40.0);
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    run.samples(10, 52.0, 45.0, 40.0);
    assert_eq!(run.thermal(), ThermalState::Mitigating);
    run.sample(52.0, 45.0, 40.0);
    assert_eq!(run.thermal(), ThermalState::Warning);
    // Spreads stay below Δ_hotspot, so only the threshold criterion applies.
    run.samples(30, 43.0, 35.0, 30.0);
    assert_eq!(run.thermal(), ThermalState::Warning);
    run.samples(11, 42.0, 35.0, 30.0);
    assert_eq!(run.thermal(), ThermalState::Monitoring);
    // Escalation remains immediate: heat plausibly to just below θ_crit, then
    // the first sample at θ_crit raises CRITICAL.
    for max in 43..55 {
        run.sample(max as f32, max as f32 - 8.0, max as f32 - 16.0);
    }
    assert_eq!(run.thermal(), ThermalState::Warning);
    run.sample(55.0, 47.0, 39.0);
    // FSR-1.6: CRITICAL moves on to MITIGATING once the mitigation is requested.
    assert_eq!(run.thermal(), ThermalState::Mitigating);
}

#[test]
fn recovery_fault_cause_chain_and_recurrence() {
    let mut run = Run::new();
    run.sample(40.0, 30.0, 25.0).advance(400);
    run.samples(10, 40.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    run.sample(40.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.guardian.active_faults().count(), 0);
    let recovered = run
        .events
        .iter()
        .find(|e| {
            matches!(
                e.kind,
                EventKind::FaultRecovered {
                    fault: FaultCode::FreshnessLost,
                    ..
                }
            )
        })
        .unwrap();
    assert!(run.events.iter().any(|e| Some(e.id) == recovered.cause
        && matches!(
            e.kind,
            EventKind::FaultDetected {
                fault: FaultCode::FreshnessLost,
                ..
            }
        )));
    assert!(run.events.iter().any(|e| e.cause == Some(recovered.id)
        && matches!(
            e.kind,
            EventKind::MonitoringStatusChanged {
                to: MonitoringStatus::Ok,
                ..
            }
        )));
    run.advance(400);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(
                e.kind,
                EventKind::FaultDetected {
                    fault: FaultCode::FreshnessLost,
                    ..
                }
            ))
            .count(),
        2
    );
}

#[test]
fn recovery_invalid_duplicate_and_gaps_break_healthy_window() {
    let mut run = Run::new();
    run.sample(50.0, 35.0, 30.0).advance(400);
    run.samples(8, 40.0, 30.0, 25.0);
    run.frame(run.alive_counter.wrapping_add(1), Quality::Invalid, 40.0);
    run.samples(8, 40.0, 30.0, 25.0);
    run.frame(run.alive_counter, Quality::Valid, 40.0);
    run.samples(8, 40.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(run.thermal(), ThermalState::Warning);
    // No tick is needed to break recovery across a long gap.
    run.now += 400;
    run.deliver(run.now + SOURCE_CLOCK_OFFSET_MS, 40.0, 30.0, 25.0);
    run.samples(8, 40.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    run.samples(3, 40.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.guardian.active_faults().count(), 0);
}

#[test]
fn recovery_stuck_max_requires_movement_and_all_faults_clear_before_ok() {
    let mut run = Run::new();
    run.sample(40.0, 30.0, 25.0);
    run.samples(35, 40.0, 33.0, 25.0);
    assert!(run
        .guardian
        .active_faults()
        .any(|f| f == FaultCode::SignalStuck));
    run.advance(400); // Also freshness lost.
    run.samples(20, 40.0, 30.0, 25.0); // References return; maximum still frozen.
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    assert_eq!(
        run.guardian.active_faults().collect::<Vec<_>>(),
        vec![FaultCode::SignalStuck]
    );
    run.samples(41, 41.0, 30.0, 25.0);
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.guardian.active_faults().count(), 0);
}

#[test]
fn recovery_rejects_unsafe_configuration() {
    let mut c = config();
    c.recovery.hysteresis_c = f32::NAN;
    assert!(c.validate().is_err());
    c.recovery.hysteresis_c = 2.0;
    c.recovery.valid_samples = 1;
    assert!(c.validate().is_err());
    c.recovery.valid_samples = 10;
    c.recovery.min_duration_ms = 0;
    assert!(c.validate().is_err());
}

#[test]
fn recovery_initial_monitor_tests_require_observation_before_pass() {
    let mut run = Run::new();
    run.samples(10, 35.0, 30.0, 25.0);
    assert!(!run
        .events
        .iter()
        .any(|e| matches!(e.kind, EventKind::FaultTestPassed { .. })));
    run.sample(35.0, 30.0, 25.0);
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::FaultTestPassed { .. }))
            .count(),
        7
    );
    run.samples(31, 35.0, 30.0, 25.0);
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::FaultTestPassed { .. }))
            .count(),
        8
    );
    assert!(!run
        .events
        .iter()
        .any(|e| matches!(e.kind, EventKind::FaultRecovered { .. })));
}

#[test]
fn recovery_stuck_max_that_jumps_once_and_freezes_remains_degraded() {
    let mut run = Run::new();
    for i in 0..40 {
        run.sample(40.0, 25.0 + (i % 10) as f32, 20.0);
    }
    assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    let recovery_count = run
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FaultRecovered {
                    fault: FaultCode::SignalStuck,
                    ..
                }
            )
        })
        .count();
    for i in 0..40 {
        run.sample(41.0, 25.0 + (i % 10) as f32, 20.0);
        assert_eq!(run.monitoring(), MonitoringStatus::Degraded);
    }
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(
                e.kind,
                EventKind::FaultRecovered {
                    fault: FaultCode::SignalStuck,
                    ..
                }
            ))
            .count(),
        recovery_count
    );
    for i in 0..45 {
        run.sample(42.0 + i as f32 * 0.1, 25.0, 20.0);
    }
    assert_eq!(run.monitoring(), MonitoringStatus::Ok);
    assert_eq!(run.guardian.active_faults().count(), 0);
}

#[test]
fn config_rejects_implausible_plausibility_limits() {
    let shipped = include_str!("../../config/guardian/safety-params.toml");
    for (from, to) in [
        ("max_c = 125.0", "max_c = -5.0"),
        ("max_rise_c_per_s = 20.0", "max_rise_c_per_s = 0.0"),
        ("resolution_c = 1.0", "resolution_c = -1.0"),
        ("max_c = 125.0", "max_c = 50.0"),
    ] {
        assert!(shipped.contains(from), "{from}");
        let text = shipped.replace(from, to);
        assert!(GuardianConfig::from_toml_str(&text).is_err(), "{to}");
    }
}
