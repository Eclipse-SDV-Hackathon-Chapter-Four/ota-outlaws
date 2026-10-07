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

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{GuardianConfig, RecoveryConfig, ThermalConfig};
use crate::detectors::{FreshnessMonitor, StuckDetector};
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
    thermal: ThermalState,
    monitoring: MonitoringStatus,
    last_fresh_sample: Option<SampleRef>,
    freshness: FreshnessMonitor,
    stuck: StuckDetector,
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
            thermal: ThermalState::Clear,
            monitoring: MonitoringStatus::Ok,
            last_fresh_sample: None,
            freshness: FreshnessMonitor::new(&config.freshness),
            stuck: StuckDetector::new(&config.stuck),
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
    /// such samples arrive, FSR-2.2 or FSR-2.3 detects the loss of fresh data.
    /// Fresh samples whose quality is not `Valid` are reported (FSR-3.4), but not
    /// evaluated.
    pub fn on_sample(&mut self, sample: Sample, now: Millis) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.is_fresh(&sample) {
            self.reset_recovery();
            if self.is_repeated_frame(&sample) {
                self.freshness.record_repeated_frame();
            }
            return events;
        }
        // A long gap breaks recovery even when no tick ran during that gap.
        if self.freshness.is_stale(now) {
            self.reset_recovery();
        }
        self.last_fresh_sample = Some(sample.reference());
        self.freshness.record_fresh_sample(now);

        if sample.quality != Quality::Valid {
            self.reset_recovery();
            self.report_fault(FaultCode::QualityInvalid, now, &mut events);
            return events;
        }
        if self.stuck.observe(&sample, now) {
            self.reset_recovery();
            self.stuck_fault_max = Some(sample.max_c);
            self.report_fault(FaultCode::SignalStuck, now, &mut events);
        } else {
            self.recover_faults(&sample, now, &mut events);
        }
        self.evaluate_thermal(&sample, now, &mut events);
        events
    }

    /// Checks time-based conditions. Call it at least every few tens of
    /// milliseconds.
    pub fn on_tick(&mut self, now: Millis) -> Vec<Event> {
        let mut events = Vec::new();
        if self.freshness.is_stale(now) {
            self.reset_recovery();
            let fault = if self.freshness.source_repeats_itself() {
                FaultCode::CounterStuck
            } else {
                FaultCode::FreshnessLost
            };
            self.report_fault(fault, now, &mut events);
        }
        events
    }

    fn is_fresh(&self, sample: &Sample) -> bool {
        let advanced = self.last_fresh_sample.is_none_or(|last| {
            sample.source_timestamp_ms > last.source_timestamp_ms
                && sample.alive_counter != last.alive_counter
        });
        advanced && sample.has_finite_values()
    }

    /// A newer frame that carries the alive counter of the last fresh sample.
    fn is_repeated_frame(&self, sample: &Sample) -> bool {
        self.last_fresh_sample.is_some_and(|last| {
            sample.source_timestamp_ms > last.source_timestamp_ms
                && sample.alive_counter == last.alive_counter
        })
    }

    /// FSR-1.1/1.2 escalation; FSR-1.5 recovery with hysteresis.
    /// Never lower severity while monitoring is DEGRADED (FSR-2.5).
    fn evaluate_thermal(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        let mut assessed = if sample.max_c >= self.thermal_config.critical_c {
            ThermalState::Critical
        } else if sample.max_c >= self.thermal_config.warn_c {
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
                    if sample.max_c < self.thermal_config.warn_c - self.recovery.hysteresis_c =>
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
            self.emit(
                Some(change),
                now,
                EventKind::MitigationRequested {
                    mitigation: Mitigation::DriverWarningOvertemp,
                },
                events,
            );
        }
    }

    /// Reports a fault once and enters DEGRADED (SG-2).
    fn report_fault(&mut self, fault: FaultCode, now: Millis, events: &mut Vec<Event>) {
        if self.active_faults.contains_key(&fault) {
            return;
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
            return;
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
    }

    fn reset_recovery(&mut self) {
        self.fault_recovery.clear();
        self.thermal_recovery = None;
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
