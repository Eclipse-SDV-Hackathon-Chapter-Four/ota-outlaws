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

//! The Guardian core: a deterministic state machine without I/O.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::config::{GuardianConfig, PlausibilityConfig, RecoveryConfig, ThermalConfig};
use crate::detectors::{FreshnessMonitor, Repetition, StuckDetector, TrendDetector};
use crate::model::{
    Event, EventId, EventKind, FaultCode, Millis, Mitigation, MonitoringStatus, Quality, Sample,
    SampleRef, ThermalState,
};

/// Battery Thermal Guardian core.
///
/// The caller feeds samples with [`Guardian::on_sample`] and calls
/// [`Guardian::on_tick`] periodically, so that missing samples are detected even
/// when nothing arrives. Both calls take the current local time and return the
/// events the adapters must publish. The same inputs always produce the same
/// events.
///
/// Recovery requires consecutive healthy samples and elapsed local time.
/// Thermal severity can fall only while monitoring is OK, with hysteresis.
#[derive(Debug, Clone)]
pub struct Guardian {
    thermal_config: ThermalConfig,
    plausibility: PlausibilityConfig,
    /// Maximum and source timestamp of the last valid sample (FSR-3.3).
    last_valid: Option<(f32, u64)>,
    /// Local time of the first call, the start for FSR-2.1.
    started_at: Option<Millis>,
    thermal: ThermalState,
    monitoring: MonitoringStatus,
    last_fresh_sample: Option<SampleRef>,
    /// Local arrival time of the last fresh sample (TS-24).
    last_fresh_at: Option<Millis>,
    /// `T_age`: maximum lateness of a sample relative to the last fresh one.
    max_age_ms: u64,
    freshness: FreshnessMonitor,
    stuck: StuckDetector,
    trend: TrendDetector,
    /// Whether the last valid sample showed a rising trend (FSR-1.3).
    trend_active: bool,
    /// While MITIGATING: start of the current observation window and the
    /// maximum at its start (FSR-1.7).
    mitigation_window: Option<(Millis, f32)>,
    /// Local times of recent rate-implausible samples, within `T_suspect`
    /// (FSR-3.5).
    recent_spikes: VecDeque<Millis>,
    /// Consecutive fresh, valid, healthy samples while SUSPECT (DFR-8).
    suspect_recovery_samples: u32,
    active_faults: BTreeMap<FaultCode, EventId>,
    recovery: RecoveryConfig,
    fault_recovery: BTreeMap<FaultCode, RecoveryWindow>,
    tested_faults: BTreeSet<FaultCode>,
    thermal_recovery: Option<(ThermalState, RecoveryWindow)>,
    stuck_fault_max: Option<f32>,
    stuck_timeout_ms: u64,
    next_event_id: u64,
}

impl Guardian {
    pub fn new(config: &GuardianConfig) -> Self {
        Self {
            thermal_config: config.thermal.clone(),
            plausibility: config.plausibility.clone(),
            last_valid: None,
            started_at: None,
            thermal: ThermalState::Clear,
            monitoring: MonitoringStatus::Ok,
            last_fresh_sample: None,
            last_fresh_at: None,
            max_age_ms: config.freshness.max_age_ms,
            freshness: FreshnessMonitor::new(&config.freshness),
            stuck: StuckDetector::new(&config.stuck),
            trend: TrendDetector::new(&config.thermal),
            trend_active: false,
            mitigation_window: None,
            recent_spikes: VecDeque::new(),
            suspect_recovery_samples: 0,
            active_faults: BTreeMap::new(),
            recovery: config.recovery.clone(),
            fault_recovery: BTreeMap::new(),
            tested_faults: BTreeSet::new(),
            thermal_recovery: None,
            stuck_fault_max: None,
            stuck_timeout_ms: config.stuck.timeout_ms,
            next_event_id: 1,
        }
    }

    pub fn thermal_state(&self) -> ThermalState {
        self.thermal
    }

    pub fn monitoring_status(&self) -> MonitoringStatus {
        self.monitoring
    }

    /// Faults not yet recovered through sustained healthy observations.
    pub fn active_faults(&self) -> impl Iterator<Item = FaultCode> + '_ {
        self.active_faults.keys().copied()
    }

    /// Processes a received sample.
    ///
    /// Samples that are not fresh are ignored: repeated or older source
    /// timestamps, an unchanged alive counter, and non-finite values. If only
    /// such samples arrive, FSR-2.2 or FSR-2.3 detects the loss of fresh data:
    /// repeated frames, exact duplicates included, set SUSPECT and, once
    /// `N_stuck` arrived, DEGRADED (DFR-7, TS-07). An out-of-order sample sets
    /// SUSPECT at once (TS-08). SUSPECT returns to OK only after `N_recover`
    /// consecutive fresh, valid samples (DFR-8).
    ///
    /// Fresh samples whose quality is not `Valid` (FSR-3.4) or that are
    /// implausible (FSR-3.1 to FSR-3.3) are reported, but not evaluated. An
    /// isolated spike only sets SUSPECT; `N_suspect` spikes within `T_suspect`
    /// lead to DEGRADED (FSR-3.5, DFR-4). Invalid samples never lower the
    /// thermal state and never raise it to CRITICAL (FSR-3.6); an implausibly
    /// high one raises it to WARNING.
    pub fn on_sample(&mut self, sample: Sample, now: Millis) -> Vec<Event> {
        let mut events = Vec::new();
        self.started_at.get_or_insert(now);
        if let Err(cause) = self.screen(&sample, now, &mut events) {
            // Every sample that is not evaluated is reported as discarded,
            // linked to the event that explains why (HARA TS-06, TS-19, TS-20).
            self.emit(
                cause,
                now,
                EventKind::MitigationRequested {
                    mitigation: Mitigation::DiscardSample,
                },
                &mut events,
            );
            return events;
        }
        self.evaluate(&sample, now, &mut events);
        events
    }

    /// Decides whether a sample may be evaluated. `Err` carries the event that
    /// explains the rejection, if this sample caused one.
    fn screen(
        &mut self,
        sample: &Sample,
        now: Millis,
        events: &mut Vec<Event>,
    ) -> Result<(), Option<EventId>> {
        if !self.is_fresh(sample, now) {
            self.reset_recovery();
            let cause = match self.non_fresh_kind(sample, now) {
                Some(NonFresh::Repeated) => match self.freshness.record_repeated_frame(now) {
                    Repetition::Isolated => None,
                    Repetition::Suspect => self.set_suspect(now, events),
                    Repetition::Stuck => {
                        Some(self.report_fault(FaultCode::CounterStuck, now, events))
                    }
                },
                Some(NonFresh::OutOfOrder | NonFresh::Late) => self.set_suspect(now, events),
                None => None,
            };
            return Err(cause);
        }
        // A long gap breaks recovery even when no tick ran during that gap.
        if self.freshness.is_stale(now) {
            self.reset_recovery();
        }
        self.last_fresh_sample = Some(sample.reference());
        self.last_fresh_at = Some(now);
        self.freshness.record_fresh_sample(now);

        if sample.quality != Quality::Valid {
            self.reset_recovery();
            return Err(Some(self.report_fault(
                FaultCode::QualityInvalid,
                now,
                events,
            )));
        }
        if let Some((fault, may_be_real_heat)) = self.implausibility(sample) {
            self.reset_recovery();
            let cause = if fault == FaultCode::RateImplausible && !self.record_spike(now) {
                // FSR-3.5: an isolated spike is discarded and only debounced.
                self.set_suspect(now, events)
            } else {
                Some(self.report_fault(fault, now, events))
            };
            if may_be_real_heat {
                self.raise_to_warning(sample, cause, now, events);
            }
            return Err(cause);
        }
        Ok(())
    }

    /// Evaluates a fresh, valid, plausible sample.
    fn evaluate(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        let sample = *sample;
        self.last_valid = Some((sample.max_c, sample.source_timestamp_ms));
        self.trend_active = self.trend.observe(&sample);
        if self.stuck.observe(&sample, now) {
            self.reset_recovery();
            self.stuck_fault_max = Some(sample.max_c);
            self.report_fault(FaultCode::SignalStuck, now, events);
        } else {
            self.recover_faults(&sample, now, events);
            self.recover_suspect(now, events);
        }
        self.check_mitigation(&sample, now, events);
        self.evaluate_thermal(&sample, now, events);
    }

    /// Checks time-based conditions. Call it at least every few tens of
    /// milliseconds.
    pub fn on_tick(&mut self, now: Millis) -> Vec<Event> {
        let mut events = Vec::new();
        let started_at = *self.started_at.get_or_insert(now);
        // FSR-2.1: no fresh sample at all since the start. HARA TS-03 sets the
        // limit to T_stale, like FSR-2.2 once data has flowed.
        if self.last_fresh_sample.is_none()
            && now.since(started_at) > self.freshness.stale_timeout_ms()
        {
            self.report_fault(FaultCode::NoDataAtStartup, now, &mut events);
        }
        if self.freshness.is_stale(now) {
            self.reset_recovery();
            self.report_fault(FaultCode::FreshnessLost, now, &mut events);
        }
        events
    }

    /// FSR-3.1 to FSR-3.3. Returns the fault and whether the sample may
    /// indicate real heat (too high, or rising too fast).
    fn implausibility(&self, sample: &Sample) -> Option<(FaultCode, bool)> {
        let range = &self.plausibility;
        let in_range = |value: f32| (range.min_c..=range.max_c).contains(&value);
        if ![sample.max_c, sample.avg_c, sample.min_c]
            .into_iter()
            .all(in_range)
        {
            return Some((FaultCode::OutOfRange, sample.max_c > range.max_c));
        }
        if !(sample.min_c <= sample.avg_c && sample.avg_c <= sample.max_c) {
            return Some((FaultCode::OrderImplausible, false));
        }
        if let Some((last_max, last_time)) = self.last_valid {
            let seconds = sample.source_timestamp_ms.saturating_sub(last_time) as f32 / 1000.0;
            let rise = sample.max_c - last_max;
            // One resolution step is allowed on top: source timestamps can be
            // much closer than the signal cycle, for example in a burst.
            if rise > range.max_rise_c_per_s * seconds + range.resolution_c {
                return Some((FaultCode::RateImplausible, true));
            }
        }
        None
    }

    /// Raises the thermal state to WARNING for an invalid sample that may
    /// indicate real heat. Never to CRITICAL, never lowering (FSR-3.6).
    /// `cause` is the fault event of the invalid sample, so the evidence chain
    /// shows the WARNING comes from a sensor fault, not from overtemperature.
    fn raise_to_warning(
        &mut self,
        sample: &Sample,
        cause: Option<EventId>,
        now: Millis,
        events: &mut Vec<Event>,
    ) {
        if self.thermal.severity() >= ThermalState::Warning.severity() {
            return;
        }
        self.thermal_recovery = None;
        let from = self.thermal;
        self.thermal = ThermalState::Warning;
        self.emit(
            cause,
            now,
            EventKind::ThermalStateChanged {
                from,
                to: ThermalState::Warning,
                trigger: sample.reference(),
            },
            events,
        );
    }

    fn is_fresh(&self, sample: &Sample, now: Millis) -> bool {
        let advanced = self.last_fresh_sample.is_none_or(|last| {
            sample.source_timestamp_ms > last.source_timestamp_ms
                && sample.alive_counter != last.alive_counter
        });
        advanced && sample.has_finite_values() && !self.is_late(sample, now)
    }

    /// TS-24 (F-2): the sample arrived more than `T_age` later than its source
    /// timestamp implies, measured against the last fresh sample. This needs
    /// no synchronized clocks, but it cannot see a delay that was already
    /// present when that sample arrived (FSR-2.8).
    fn is_late(&self, sample: &Sample, now: Millis) -> bool {
        let (Some(last), Some(last_at)) = (self.last_fresh_sample, self.last_fresh_at) else {
            return false;
        };
        let source_gap = sample
            .source_timestamp_ms
            .saturating_sub(last.source_timestamp_ms);
        now.since(last_at).saturating_sub(source_gap) > self.max_age_ms
    }

    /// Classifies a sample that is not fresh. `None` for samples before the
    /// first fresh one and for non-finite values.
    fn non_fresh_kind(&self, sample: &Sample, now: Millis) -> Option<NonFresh> {
        let last = self.last_fresh_sample?;
        if !sample.has_finite_values() {
            return None;
        }
        if sample.alive_counter == last.alive_counter
            && sample.source_timestamp_ms >= last.source_timestamp_ms
        {
            Some(NonFresh::Repeated)
        } else if sample.source_timestamp_ms <= last.source_timestamp_ms {
            Some(NonFresh::OutOfOrder)
        } else if self.is_late(sample, now) {
            Some(NonFresh::Late)
        } else {
            None
        }
    }

    /// Records a rate-implausible sample. Returns true once `N_suspect` of
    /// them arrived within `T_suspect` (FSR-3.5).
    fn record_spike(&mut self, now: Millis) -> bool {
        let window = self.plausibility.suspect_window_ms;
        self.recent_spikes.retain(|at| now.since(*at) <= window);
        self.recent_spikes.push_back(now);
        self.recent_spikes.len() >= self.plausibility.suspect_spikes as usize
    }

    /// FSR-1.1/1.2 escalation; FSR-1.5 recovery with hysteresis.
    /// Never lower severity while monitoring is DEGRADED (FSR-2.5).
    fn evaluate_thermal(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        let mut assessed = if sample.max_c >= self.thermal_config.critical_c {
            ThermalState::Critical
        } else if sample.max_c >= self.thermal_config.warn_c || self.warning_criterion(sample) {
            ThermalState::Warning
        } else {
            ThermalState::Monitoring
        };
        if assessed.severity() > self.thermal.severity() {
            self.thermal_recovery = None;
        } else {
            let target = match self.thermal {
                ThermalState::Critical | ThermalState::Mitigating
                    if sample.max_c
                        < self.thermal_config.critical_c - self.recovery.hysteresis_c =>
                {
                    Some(ThermalState::Warning)
                }
                ThermalState::Warning
                    if !self.warning_criterion(sample)
                        && sample.max_c
                            < self.thermal_config.warn_c - self.recovery.hysteresis_c =>
                {
                    Some(ThermalState::Monitoring)
                }
                _ => None,
            };
            let Some(target) = target.filter(|_| self.monitoring == MonitoringStatus::Ok) else {
                self.thermal_recovery = None;
                return;
            };
            let (_, window) = self
                .thermal_recovery
                .get_or_insert((target, RecoveryWindow::new(now)));
            if !window.observe(now, &self.recovery) {
                return;
            }
            assessed = target;
            self.thermal_recovery = None;
        }

        let from = self.thermal;
        self.thermal = assessed;
        let change = self.emit(
            None,
            now,
            EventKind::ThermalStateChanged {
                from,
                to: assessed,
                trigger: sample.reference(),
            },
            events,
        );
        if assessed == ThermalState::Critical {
            self.request_overtemp_mitigation(sample, change, now, events);
        } else {
            self.mitigation_window = None;
        }
    }

    /// FSR-1.3 and FSR-1.4: criteria that raise WARNING below `θ_warn`.
    fn warning_criterion(&self, sample: &Sample) -> bool {
        // FSR-1.4: a large spread is a real local hot spot, not a sensor fault.
        let hot_spot = sample.max_c - sample.avg_c > self.thermal_config.hotspot_spread_c;
        self.trend_active || hot_spot
    }

    /// FSR-1.2 and FSR-1.6: requests the overtemperature mitigation, caused by
    /// `cause`, and moves from CRITICAL to MITIGATING.
    fn request_overtemp_mitigation(
        &mut self,
        sample: &Sample,
        cause: EventId,
        now: Millis,
        events: &mut Vec<Event>,
    ) {
        let request = self.emit(
            Some(cause),
            now,
            EventKind::MitigationRequested {
                mitigation: Mitigation::DriverWarningOvertemp,
            },
            events,
        );
        self.thermal = ThermalState::Mitigating;
        self.mitigation_window = Some((now, sample.max_c));
        self.emit(
            Some(request),
            now,
            EventKind::ThermalStateChanged {
                from: ThermalState::Critical,
                to: ThermalState::Mitigating,
                trigger: sample.reference(),
            },
            events,
        );
    }

    /// FSR-1.7: if the maximum rose by more than one resolution step over
    /// `T_mitigation` while MITIGATING, the mitigation failed: return to CRITICAL and request it
    /// again. Otherwise a new observation window starts.
    fn check_mitigation(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        if self.thermal != ThermalState::Mitigating {
            return;
        }
        let Some((since, start_max)) = self.mitigation_window else {
            return;
        };
        if now.since(since) < self.thermal_config.mitigation_timeout_ms {
            return;
        }
        // One resolution step is signal noise, not a rise.
        if sample.max_c <= start_max + self.plausibility.resolution_c {
            self.mitigation_window = Some((now, sample.max_c));
            return;
        }
        self.thermal = ThermalState::Critical;
        let failed = self.emit(
            None,
            now,
            EventKind::ThermalStateChanged {
                from: ThermalState::Mitigating,
                to: ThermalState::Critical,
                trigger: sample.reference(),
            },
            events,
        );
        self.request_overtemp_mitigation(sample, failed, now, events);
    }

    /// Reports a fault once and enters DEGRADED (SG-2).
    fn report_fault(&mut self, fault: FaultCode, now: Millis, events: &mut Vec<Event>) -> EventId {
        if let Some(&detected) = self.active_faults.get(&fault) {
            return detected;
        }
        let detected = self.emit(
            None,
            now,
            EventKind::FaultDetected {
                fault,
                last_sample: self.last_fresh_sample,
            },
            events,
        );
        self.active_faults.insert(fault, detected);
        if self.monitoring == MonitoringStatus::Degraded {
            return detected;
        }

        let from = self.monitoring;
        self.monitoring = MonitoringStatus::Degraded;
        let change = self.emit(
            Some(detected),
            now,
            EventKind::MonitoringStatusChanged {
                from,
                to: MonitoringStatus::Degraded,
            },
            events,
        );
        self.emit(
            Some(change),
            now,
            EventKind::MitigationRequested {
                mitigation: Mitigation::DriverWarningMonitoringUnavailable,
            },
            events,
        );
        detected
    }

    /// Switches from OK to SUSPECT (FSR-2.3, FSR-3.5, TS-07, TS-08). SUSPECT
    /// requests no mitigation; DEGRADED is left alone. Returns the status
    /// change event, if there was one.
    fn set_suspect(&mut self, now: Millis, events: &mut Vec<Event>) -> Option<EventId> {
        self.suspect_recovery_samples = 0;
        if self.monitoring != MonitoringStatus::Ok {
            return None;
        }
        self.monitoring = MonitoringStatus::Suspect;
        Some(self.emit(
            None,
            now,
            EventKind::MonitoringStatusChanged {
                from: MonitoringStatus::Ok,
                to: MonitoringStatus::Suspect,
            },
            events,
        ))
    }

    /// Returns from SUSPECT to OK after `N_recover` consecutive fresh, valid,
    /// healthy samples (DFR-8).
    fn recover_suspect(&mut self, now: Millis, events: &mut Vec<Event>) {
        if self.monitoring != MonitoringStatus::Suspect {
            return;
        }
        self.suspect_recovery_samples = self.suspect_recovery_samples.saturating_add(1);
        if self.suspect_recovery_samples < self.recovery.valid_samples {
            return;
        }
        self.suspect_recovery_samples = 0;
        self.monitoring = MonitoringStatus::Ok;
        self.emit(
            None,
            now,
            EventKind::MonitoringStatusChanged {
                from: MonitoringStatus::Suspect,
                to: MonitoringStatus::Ok,
            },
            events,
        );
    }

    fn reset_recovery(&mut self) {
        self.fault_recovery.clear();
        self.thermal_recovery = None;
        self.suspect_recovery_samples = 0;
    }

    fn recover_faults(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        let faults: Vec<_> = FaultCode::ALL
            .into_iter()
            .filter(|fault| {
                self.active_faults.contains_key(fault) || !self.tested_faults.contains(fault)
            })
            .collect();
        let mut last_recovered = None;
        for fault in faults {
            // A frozen maximum is not proven healthy just because references stop moving.
            if fault == FaultCode::SignalStuck
                && self
                    .stuck_fault_max
                    .is_some_and(|max| max.to_bits() == sample.max_c.to_bits())
            {
                self.fault_recovery.remove(&fault);
                continue;
            }
            let window = self
                .fault_recovery
                .entry(fault)
                .or_insert_with(|| RecoveryWindow::new(now));
            if !window.observe(now, &self.recovery) {
                continue;
            }
            // Both initial tests and recovery must observe a full stuck detection
            // interval followed by the sustained healthy confirmation period.
            // A single changed maximum must not clear a sensor that freezes again.
            if fault == FaultCode::SignalStuck
                && now.since(window.since)
                    < self
                        .stuck_timeout_ms
                        .saturating_add(self.recovery.min_duration_ms)
            {
                continue;
            }
            let detected = self.active_faults.remove(&fault);
            self.fault_recovery.remove(&fault);
            self.tested_faults.insert(fault);
            if let Some(detected) = detected {
                last_recovered = Some(self.emit(
                    Some(detected),
                    now,
                    EventKind::FaultRecovered {
                        fault,
                        trigger: sample.reference(),
                    },
                    events,
                ));
            } else {
                self.emit(
                    None,
                    now,
                    EventKind::FaultTestPassed {
                        fault,
                        trigger: sample.reference(),
                    },
                    events,
                );
            }
            if fault == FaultCode::SignalStuck {
                self.stuck_fault_max = None;
            }
        }
        if self.active_faults.is_empty() && self.monitoring == MonitoringStatus::Degraded {
            self.monitoring = MonitoringStatus::Ok;
            self.emit(
                last_recovered,
                now,
                EventKind::MonitoringStatusChanged {
                    from: MonitoringStatus::Degraded,
                    to: MonitoringStatus::Ok,
                },
                events,
            );
        }
    }

    fn emit(
        &mut self,
        cause: Option<EventId>,
        at: Millis,
        kind: EventKind,
        events: &mut Vec<Event>,
    ) -> EventId {
        let id = EventId(self.next_event_id);
        self.next_event_id += 1;
        events.push(Event {
            id,
            cause,
            at,
            kind,
        });
        id
    }
}

/// Why a sample is not fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonFresh {
    /// Same alive counter as the last fresh sample, source timestamp not
    /// older: a frozen source or a duplicated message (FSR-2.3, DFR-7).
    Repeated,
    /// Source timestamp not newer than the last fresh sample: delivered out
    /// of order (TS-08).
    OutOfOrder,
    /// Newer, but delivered more than `T_age` late (TS-24).
    Late,
}

#[derive(Debug, Clone)]
struct RecoveryWindow {
    since: Millis,
    samples: u32,
}
impl RecoveryWindow {
    fn new(now: Millis) -> Self {
        Self {
            since: now,
            samples: 0,
        }
    }
    fn observe(&mut self, now: Millis, config: &RecoveryConfig) -> bool {
        self.samples = self.samples.saturating_add(1);
        self.samples >= config.valid_samples && now.since(self.since) >= config.min_duration_ms
    }
}
