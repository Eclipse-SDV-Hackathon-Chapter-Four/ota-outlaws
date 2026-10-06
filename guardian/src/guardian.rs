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

//! The Guardian core: a deterministic state machine without I/O.

use std::collections::BTreeSet;

use crate::config::{GuardianConfig, ThermalConfig};
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
/// The thermal state is never lowered and DEGRADED is never left, because
/// recovery is not implemented. Each scenario starts with a new Guardian
/// (assumption A-4). Which requirements are implemented is recorded in the
/// status column of `docs/explanation/safety-concept.md`.
#[derive(Debug, Clone)]
pub struct Guardian {
    thermal_config: ThermalConfig,
    thermal: ThermalState,
    monitoring: MonitoringStatus,
    last_fresh_sample: Option<SampleRef>,
    freshness: FreshnessMonitor,
    stuck: StuckDetector,
    active_faults: BTreeSet<FaultCode>,
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
            active_faults: BTreeSet::new(),
            next_event_id: 1,
        }
    }

    pub fn thermal_state(&self) -> ThermalState {
        self.thermal
    }

    pub fn monitoring_status(&self) -> MonitoringStatus {
        self.monitoring
    }

    /// Faults detected so far. Faults stay active, because recovery is not
    /// implemented yet.
    pub fn active_faults(&self) -> impl Iterator<Item = FaultCode> + '_ {
        self.active_faults.iter().copied()
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
            if self.is_repeated_frame(&sample) {
                self.freshness.record_repeated_frame();
            }
            return events;
        }
        self.last_fresh_sample = Some(sample.reference());
        self.freshness.record_fresh_sample(now);

        if sample.quality != Quality::Valid {
            self.report_fault(FaultCode::QualityInvalid, now, &mut events);
            return events;
        }
        if self.stuck.observe(&sample, now) {
            self.report_fault(FaultCode::SignalStuck, now, &mut events);
        }
        self.evaluate_thermal(&sample, now, &mut events);
        events
    }

    /// Checks time-based conditions. Call it at least every few tens of
    /// milliseconds.
    pub fn on_tick(&mut self, now: Millis) -> Vec<Event> {
        let mut events = Vec::new();
        if self.freshness.is_stale(now) {
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

    /// FSR-1.1 and FSR-1.2. The thermal state is only ever raised here, so it is
    /// never lowered while monitoring is DEGRADED (FSR-2.5).
    fn evaluate_thermal(&mut self, sample: &Sample, now: Millis, events: &mut Vec<Event>) {
        let assessed = if sample.max_c >= self.thermal_config.critical_c {
            ThermalState::Critical
        } else if sample.max_c >= self.thermal_config.warn_c {
            ThermalState::Warning
        } else {
            ThermalState::Monitoring
        };
        if assessed.severity() <= self.thermal.severity() {
            return;
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
        if !self.active_faults.insert(fault) {
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
