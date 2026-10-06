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

//! Translation between the uProtocol payloads of `contracts/battery_thermal.proto`
//! and the types of the Guardian core.

use std::fmt;

use guardian::{
    Event, EventKind, Mitigation, MonitoringStatus, Quality, Sample, SampleRef, ThermalState,
};
use prost::Message;
use thermal_contract::v1 as pb;

/// A `BatteryTemperature` payload the Guardian cannot use.
#[derive(Debug)]
pub enum DecodeError {
    Protobuf(prost::DecodeError),
    /// The alive counter of a CAN frame is 0 to 255.
    AliveCounterOutOfRange(u32),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Protobuf(error) => {
                write!(f, "invalid BatteryTemperature payload: {error}")
            }
            DecodeError::AliveCounterOutOfRange(value) => {
                write!(f, "alive counter {value} is outside 0 to 255")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes a `BatteryTemperature` payload into a core sample.
///
/// A payload that cannot be decoded is rejected and never reaches the core. If
/// only such payloads arrive, the core detects the loss of fresh data (FSR-2.2).
pub fn decode_sample(payload: &[u8]) -> Result<Sample, DecodeError> {
    let message = pb::BatteryTemperature::decode(payload).map_err(DecodeError::Protobuf)?;
    let alive_counter = u8::try_from(message.alive_counter)
        .map_err(|_| DecodeError::AliveCounterOutOfRange(message.alive_counter))?;
    Ok(Sample {
        source_timestamp_ms: message.source_timestamp_ms,
        sequence: message.sequence,
        alive_counter,
        quality: quality(message.quality),
        max_c: message.max_c,
        avg_c: message.avg_c,
        min_c: message.min_c,
    })
}

/// Unspecified and unknown quality values are treated as `NotAvailable`: the
/// source does not vouch for the value, so the core reports it (FSR-3.4)
/// instead of using it.
fn quality(raw: i32) -> Quality {
    match pb::Quality::try_from(raw) {
        Ok(pb::Quality::Valid) => Quality::Valid,
        Ok(pb::Quality::Invalid) => Quality::Invalid,
        Ok(pb::Quality::NotAvailable | pb::Quality::Unspecified) | Err(_) => Quality::NotAvailable,
    }
}

/// Encodes a core event as a `GuardianEvent` payload.
pub fn encode_event(event: &Event) -> Vec<u8> {
    to_message(event).encode_to_vec()
}

fn to_message(event: &Event) -> pb::GuardianEvent {
    use pb::guardian_event::Kind;

    let kind = match &event.kind {
        EventKind::ThermalStateChanged { from, to, trigger } => {
            Kind::ThermalStateChanged(pb::ThermalStateChanged {
                previous: thermal_state(*from) as i32,
                current: thermal_state(*to) as i32,
                trigger: Some(sample_ref(*trigger)),
            })
        }
        EventKind::MonitoringStatusChanged { from, to } => {
            Kind::MonitoringStatusChanged(pb::MonitoringStatusChanged {
                previous: monitoring_status(*from) as i32,
                current: monitoring_status(*to) as i32,
            })
        }
        EventKind::FaultDetected { fault, last_sample } => Kind::FaultDetected(pb::FaultDetected {
            dtc: fault.dtc().to_owned(),
            requirement: fault.requirement().to_owned(),
            last_sample: last_sample.map(sample_ref),
        }),
        EventKind::MitigationRequested { mitigation } => {
            Kind::MitigationRequested(pb::MitigationRequested {
                mitigation: self::mitigation(*mitigation) as i32,
            })
        }
    };
    pb::GuardianEvent {
        event_id: event.id.0,
        cause_event_id: event.cause.map_or(0, |cause| cause.0),
        guardian_time_ms: event.at.0,
        kind: Some(kind),
    }
}

fn sample_ref(sample: SampleRef) -> pb::SampleRef {
    pb::SampleRef {
        sequence: sample.sequence,
        source_timestamp_ms: sample.source_timestamp_ms,
        alive_counter: u32::from(sample.alive_counter),
    }
}

fn thermal_state(state: ThermalState) -> pb::ThermalState {
    match state {
        ThermalState::Clear => pb::ThermalState::Clear,
        ThermalState::Monitoring => pb::ThermalState::Monitoring,
        ThermalState::Warning => pb::ThermalState::Warning,
        ThermalState::Critical => pb::ThermalState::Critical,
        ThermalState::Mitigating => pb::ThermalState::Mitigating,
    }
}

fn monitoring_status(status: MonitoringStatus) -> pb::MonitoringStatus {
    match status {
        MonitoringStatus::Ok => pb::MonitoringStatus::Ok,
        MonitoringStatus::Suspect => pb::MonitoringStatus::Suspect,
        MonitoringStatus::Degraded => pb::MonitoringStatus::Degraded,
    }
}

fn mitigation(mitigation: Mitigation) -> pb::Mitigation {
    match mitigation {
        Mitigation::DriverWarningOvertemp => pb::Mitigation::DriverWarningOvertemp,
        Mitigation::DriverWarningMonitoringUnavailable => {
            pb::Mitigation::DriverWarningMonitoringUnavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guardian::{EventId, FaultCode, Millis};

    fn temperature(alive_counter: u32, quality: pb::Quality) -> Vec<u8> {
        pb::BatteryTemperature {
            max_c: 50.0,
            avg_c: 40.0,
            min_c: 30.0,
            source_timestamp_ms: 1_234,
            sequence: 7,
            alive_counter,
            quality: quality as i32,
        }
        .encode_to_vec()
    }

    #[test]
    fn decodes_all_fields() {
        let sample = decode_sample(&temperature(255, pb::Quality::Valid)).unwrap();

        assert_eq!(
            sample,
            Sample {
                source_timestamp_ms: 1_234,
                sequence: 7,
                alive_counter: 255,
                quality: Quality::Valid,
                max_c: 50.0,
                avg_c: 40.0,
                min_c: 30.0,
            }
        );
    }

    #[test]
    fn rejects_alive_counter_above_255() {
        let result = decode_sample(&temperature(256, pb::Quality::Valid));

        assert!(matches!(
            result,
            Err(DecodeError::AliveCounterOutOfRange(256))
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            decode_sample(&[0xff, 0xff, 0xff]),
            Err(DecodeError::Protobuf(_))
        ));
    }

    #[test]
    fn maps_quality_values() {
        assert_eq!(quality(pb::Quality::Valid as i32), Quality::Valid);
        assert_eq!(quality(pb::Quality::Invalid as i32), Quality::Invalid);
        assert_eq!(
            quality(pb::Quality::NotAvailable as i32),
            Quality::NotAvailable
        );
    }

    #[test]
    fn unspecified_or_unknown_quality_is_not_available() {
        assert_eq!(
            quality(pb::Quality::Unspecified as i32),
            Quality::NotAvailable
        );
        assert_eq!(quality(42), Quality::NotAvailable);
    }

    #[test]
    fn encodes_fault_with_requirement_and_cause() {
        let event = Event {
            id: EventId(3),
            cause: Some(EventId(2)),
            at: Millis(500),
            kind: EventKind::FaultDetected {
                fault: FaultCode::CounterStuck,
                last_sample: None,
            },
        };

        let message = pb::GuardianEvent::decode(encode_event(&event).as_slice()).unwrap();

        assert_eq!(message.event_id, 3);
        assert_eq!(message.cause_event_id, 2);
        assert_eq!(message.guardian_time_ms, 500);
        assert_eq!(
            message.kind,
            Some(pb::guardian_event::Kind::FaultDetected(pb::FaultDetected {
                dtc: "BTG_TempCounterStuck".to_owned(),
                requirement: "FSR-2.3".to_owned(),
                last_sample: None,
            }))
        );
    }

    #[test]
    fn event_without_cause_has_cause_zero() {
        let event = Event {
            id: EventId(1),
            cause: None,
            at: Millis(0),
            kind: EventKind::MitigationRequested {
                mitigation: Mitigation::DriverWarningOvertemp,
            },
        };

        let message = pb::GuardianEvent::decode(encode_event(&event).as_slice()).unwrap();

        assert_eq!(message.cause_event_id, 0);
    }
}
