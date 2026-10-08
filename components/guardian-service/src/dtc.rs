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

//! Which DFM records a Guardian event produces: the input faults of the core,
//! and the overtemperature DTCs derived from thermal state changes
//! (`docs/reference/hara.md`). Pure, so it is unit-tested
//! without a DFM.

use guardian::{Event, EventId, EventKind, FaultCode, ThermalState};

/// A diagnostic trouble code of the Guardian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dtc {
    /// An input fault detected by the core.
    Input(FaultCode),
    /// The maximum cell temperature crossed `θ_warn` (FSR-1.1).
    OverTempWarning,
    /// The maximum cell temperature crossed `θ_crit` (FSR-1.2).
    OverTempCritical,
}

impl Dtc {
    /// Every DTC, in catalog order.
    pub fn all() -> Vec<Dtc> {
        FaultCode::ALL
            .into_iter()
            .map(Dtc::Input)
            .chain([Dtc::OverTempWarning, Dtc::OverTempCritical])
            .collect()
    }

    pub fn code(self) -> &'static str {
        match self {
            Dtc::Input(fault) => fault.dtc(),
            Dtc::OverTempWarning => "BTG_TempOverTempWarning",
            Dtc::OverTempCritical => "BTG_TempOverTempCritical",
        }
    }

    /// The requirement that detects the DTC.
    pub fn requirement(self) -> &'static str {
        match self {
            Dtc::Input(fault) => fault.requirement(),
            Dtc::OverTempWarning => "FSR-1.1",
            Dtc::OverTempCritical => "FSR-1.2",
        }
    }
}

/// One DFM record to write: the DTC failed or passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub dtc: Dtc,
    pub failed: bool,
}

/// Turns Guardian events into DFM records. Remembers which overtemperature
/// DTCs it reported as failed, so that it reports only those as passed.
#[derive(Debug, Default)]
pub struct Mapper {
    warning: Option<EventId>,
    critical: Option<EventId>,
}

fn level(state: ThermalState) -> u8 {
    state.severity()
}

impl Mapper {
    pub fn records(&mut self, event: &Event) -> Vec<Record> {
        let record = |dtc, failed| Record { dtc, failed };
        match event.kind {
            EventKind::FaultDetected { fault, .. } => vec![record(Dtc::Input(fault), true)],
            EventKind::FaultRecovered { fault, .. } | EventKind::FaultTestPassed { fault, .. } => {
                vec![record(Dtc::Input(fault), false)]
            }
            EventKind::ThermalStateChanged { from, to, .. } => {
                let warning = level(ThermalState::Warning);
                let critical = level(ThermalState::Critical);
                let mut records = Vec::new();
                if level(to) > level(from) {
                    // A WARNING caused by a fault event comes from an invalid
                    // sample (FSR-3.2, FSR-3.3): a sensor fault, already
                    // reported, not an overtemperature.
                    let real = event.cause.is_none();
                    if real && level(to) >= warning && self.warning.is_none() {
                        self.warning = Some(event.id);
                        records.push(record(Dtc::OverTempWarning, true));
                    }
                    if real && level(to) >= critical && self.critical.is_none() {
                        self.critical = Some(event.id);
                        records.push(record(Dtc::OverTempCritical, true));
                    }
                } else {
                    if level(to) < critical && self.critical.take().is_some() {
                        records.push(record(Dtc::OverTempCritical, false));
                    }
                    if level(to) < warning && self.warning.take().is_some() {
                        records.push(record(Dtc::OverTempWarning, false));
                    }
                }
                records
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guardian::{Millis, SampleRef};

    fn change(id: u64, cause: Option<u64>, from: ThermalState, to: ThermalState) -> Event {
        Event {
            id: EventId(id),
            cause: cause.map(EventId),
            at: Millis(id * 100),
            kind: EventKind::ThermalStateChanged {
                from,
                to,
                trigger: SampleRef {
                    sequence: id,
                    source_timestamp_ms: id * 100,
                    alive_counter: id as u8,
                },
            },
        }
    }

    fn failed(dtc: Dtc) -> Record {
        Record { dtc, failed: true }
    }

    fn passed(dtc: Dtc) -> Record {
        Record { dtc, failed: false }
    }

    use ThermalState::{Critical, Monitoring, Warning};

    #[test]
    fn heating_and_cooling_fail_and_pass_both_dtcs_in_order() {
        let mut mapper = Mapper::default();

        assert_eq!(
            mapper.records(&change(1, None, Monitoring, Warning)),
            vec![failed(Dtc::OverTempWarning)]
        );
        assert_eq!(
            mapper.records(&change(2, None, Warning, Critical)),
            vec![failed(Dtc::OverTempCritical)]
        );
        assert_eq!(
            mapper.records(&change(3, None, Critical, Warning)),
            vec![passed(Dtc::OverTempCritical)]
        );
        assert_eq!(
            mapper.records(&change(4, None, Warning, Monitoring)),
            vec![passed(Dtc::OverTempWarning)]
        );
    }

    #[test]
    fn jump_to_critical_fails_both_dtcs() {
        let mut mapper = Mapper::default();

        assert_eq!(
            mapper.records(&change(1, None, Monitoring, Critical)),
            vec![failed(Dtc::OverTempWarning), failed(Dtc::OverTempCritical)]
        );
    }

    #[test]
    fn warning_from_invalid_data_is_no_overtemperature() {
        let mut mapper = Mapper::default();

        assert!(mapper
            .records(&change(2, Some(1), Monitoring, Warning))
            .is_empty());
        // Lowering later does not pass a DTC that never failed.
        assert!(mapper
            .records(&change(3, None, Warning, Monitoring))
            .is_empty());
    }

    #[test]
    fn critical_after_invalid_warning_is_overtemperature() {
        // An invalid high sample raised WARNING; then valid data reached θ_crit.
        let mut mapper = Mapper::default();
        mapper.records(&change(2, Some(1), Monitoring, Warning));

        assert_eq!(
            mapper.records(&change(3, None, Warning, Critical)),
            vec![failed(Dtc::OverTempWarning), failed(Dtc::OverTempCritical)]
        );
    }

    #[test]
    fn repeated_escalation_reports_each_dtc_once() {
        let mut mapper = Mapper::default();
        mapper.records(&change(1, None, Monitoring, Critical));

        assert!(mapper
            .records(&change(2, None, Critical, Critical))
            .is_empty());
    }

    #[test]
    fn input_faults_pass_through() {
        let mut mapper = Mapper::default();
        let detected = Event {
            id: EventId(1),
            cause: None,
            at: Millis(0),
            kind: EventKind::FaultDetected {
                fault: FaultCode::OutOfRange,
                last_sample: None,
            },
        };

        assert_eq!(
            mapper.records(&detected),
            vec![failed(Dtc::Input(FaultCode::OutOfRange))]
        );
    }

    #[test]
    fn every_dtc_has_a_distinct_code() {
        let codes: std::collections::BTreeSet<_> = Dtc::all().iter().map(|d| d.code()).collect();
        assert_eq!(codes.len(), Dtc::all().len());
    }
}
