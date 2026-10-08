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

//! What the tool observed during a run, one JSON line per observation in
//! `recording.jsonl`. Every observation is stamped with the tool's own
//! monotonic clock, so all latencies are differences of these stamps.

use std::io::{BufRead, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thermal_contract::v1 as pb;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// Milliseconds since the recording started, on the tool's clock.
    pub t_ms: u64,
    #[serde(flatten)]
    pub tap: Tap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tap", rename_all = "snake_case")]
pub enum Tap {
    /// A `BatteryTemperature` message on the Guardian's input topic.
    BatteryTemperature(Temperature),
    /// A `GuardianEvent` message on the Guardian's event topic.
    GuardianEvent(GuardianEvent),
    /// A `SupervisorEvent` message on the watchdog's topic (HARA DFR-5).
    SupervisorEvent(SupervisorEvent),
    /// A change of one fault in OpenSOVD, or a failed poll (`body` is null).
    SovdFault {
        code: String,
        http_status: Option<u16>,
        body: serde_json::Value,
    },
    /// Something the tool did to the system, as a cross-check for the onset.
    Injection { action: String, detail: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Temperature {
    pub sequence: u64,
    pub source_timestamp_ms: u64,
    pub alive_counter: u32,
    pub quality: String,
    pub max_c: f32,
    pub avg_c: f32,
    pub min_c: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardianEvent {
    pub session_id: String,
    pub event_id: u64,
    /// 0 if the event has no cause.
    pub cause_event_id: u64,
    pub guardian_time_ms: u64,
    pub kind: EventKind,
    /// The sample the event refers to: the trigger of a thermal change, a
    /// recovery, or a passed test; the last fresh sample before a detected
    /// fault. Recordings from before this field have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<SampleRef>,
}

/// Identifies a `BatteryTemperature` message, as the contract's `SampleRef`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleRef {
    pub sequence: u64,
    pub source_timestamp_ms: u64,
    pub alive_counter: u32,
}

impl From<&pb::SampleRef> for SampleRef {
    fn from(sample: &pb::SampleRef) -> Self {
        SampleRef {
            sequence: sample.sequence,
            source_timestamp_ms: sample.source_timestamp_ms,
            alive_counter: sample.alive_counter,
        }
    }
}

impl EventKind {
    /// One line for reports, for example `FaultDetected BTG_TempCounterStuck (FSR-2.3)`.
    pub fn describe(&self) -> String {
        match self {
            EventKind::ThermalStateChanged { previous, current } => {
                format!("ThermalStateChanged {previous} → {current}")
            }
            EventKind::MonitoringStatusChanged { previous, current } => {
                format!("MonitoringStatusChanged {previous} → {current}")
            }
            EventKind::FaultDetected { dtc, requirement } => {
                format!("FaultDetected {dtc} ({requirement})")
            }
            EventKind::FaultRecovered { dtc, requirement } => {
                format!("FaultRecovered {dtc} ({requirement})")
            }
            EventKind::FaultTestPassed { dtc, requirement } => {
                format!("FaultTestPassed {dtc} ({requirement})")
            }
            EventKind::MitigationRequested { mitigation } => {
                format!("MitigationRequested {mitigation}")
            }
            EventKind::Unknown => "unknown event".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventKind {
    ThermalStateChanged { previous: String, current: String },
    MonitoringStatusChanged { previous: String, current: String },
    FaultDetected { dtc: String, requirement: String },
    FaultRecovered { dtc: String, requirement: String },
    FaultTestPassed { dtc: String, requirement: String },
    MitigationRequested { mitigation: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorEvent {
    pub session_id: String,
    pub event_id: u64,
    /// 0 if the event has no cause.
    pub cause_event_id: u64,
    pub watchdog_time_ms: u64,
    pub kind: SupervisorKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SupervisorKind {
    GuardianLost {
        last_guardian_session_id: String,
        silence_ms: u64,
    },
    MitigationRequested {
        mitigation: String,
    },
    GuardianRestored {
        guardian_session_id: String,
    },
    Unknown,
}

impl From<&pb::SupervisorEvent> for SupervisorEvent {
    fn from(message: &pb::SupervisorEvent) -> Self {
        use pb::supervisor_event::Kind;
        let kind = match &message.kind {
            Some(Kind::GuardianLost(lost)) => SupervisorKind::GuardianLost {
                last_guardian_session_id: lost.last_guardian_session_id.clone(),
                silence_ms: lost.silence_ms,
            },
            Some(Kind::MitigationRequested(request)) => SupervisorKind::MitigationRequested {
                mitigation: mitigation_name(request.mitigation),
            },
            Some(Kind::GuardianRestored(restored)) => SupervisorKind::GuardianRestored {
                guardian_session_id: restored.guardian_session_id.clone(),
            },
            None => SupervisorKind::Unknown,
        };
        SupervisorEvent {
            session_id: message.session_id.clone(),
            event_id: message.event_id,
            cause_event_id: message.cause_event_id,
            watchdog_time_ms: message.watchdog_time_ms,
            kind,
        }
    }
}

impl From<&pb::BatteryTemperature> for Temperature {
    fn from(message: &pb::BatteryTemperature) -> Self {
        Temperature {
            sequence: message.sequence,
            source_timestamp_ms: message.source_timestamp_ms,
            alive_counter: message.alive_counter,
            quality: short_name(message.quality().as_str_name(), "QUALITY_"),
            max_c: message.max_c,
            avg_c: message.avg_c,
            min_c: message.min_c,
        }
    }
}

impl From<&pb::GuardianEvent> for GuardianEvent {
    fn from(message: &pb::GuardianEvent) -> Self {
        use pb::guardian_event::Kind;
        let thermal = |raw: i32| {
            let name = pb::ThermalState::try_from(raw)
                .map(|state| state.as_str_name())
                .unwrap_or("UNKNOWN");
            short_name(name, "THERMAL_STATE_")
        };
        let monitoring = |raw: i32| {
            let name = pb::MonitoringStatus::try_from(raw)
                .map(|status| status.as_str_name())
                .unwrap_or("UNKNOWN");
            short_name(name, "MONITORING_STATUS_")
        };
        let sample = match &message.kind {
            Some(Kind::ThermalStateChanged(change)) => change.trigger.as_ref(),
            Some(Kind::FaultDetected(fault)) => fault.last_sample.as_ref(),
            Some(Kind::FaultRecovered(fault)) => fault.trigger.as_ref(),
            Some(Kind::FaultTestPassed(fault)) => fault.trigger.as_ref(),
            _ => None,
        }
        .map(SampleRef::from);
        let kind = match &message.kind {
            Some(Kind::ThermalStateChanged(change)) => EventKind::ThermalStateChanged {
                previous: thermal(change.previous),
                current: thermal(change.current),
            },
            Some(Kind::MonitoringStatusChanged(change)) => EventKind::MonitoringStatusChanged {
                previous: monitoring(change.previous),
                current: monitoring(change.current),
            },
            Some(Kind::FaultDetected(fault)) => EventKind::FaultDetected {
                dtc: fault.dtc.clone(),
                requirement: fault.requirement.clone(),
            },
            Some(Kind::FaultRecovered(fault)) => EventKind::FaultRecovered {
                dtc: fault.dtc.clone(),
                requirement: fault.requirement.clone(),
            },
            Some(Kind::FaultTestPassed(fault)) => EventKind::FaultTestPassed {
                dtc: fault.dtc.clone(),
                requirement: fault.requirement.clone(),
            },
            Some(Kind::MitigationRequested(request)) => EventKind::MitigationRequested {
                mitigation: mitigation_name(request.mitigation),
            },
            None => EventKind::Unknown,
        };
        GuardianEvent {
            session_id: message.session_id.clone(),
            event_id: message.event_id,
            cause_event_id: message.cause_event_id,
            guardian_time_ms: message.guardian_time_ms,
            kind,
            sample,
        }
    }
}

fn mitigation_name(raw: i32) -> String {
    short_name(
        pb::Mitigation::try_from(raw)
            .map(|m| m.as_str_name())
            .unwrap_or("UNKNOWN"),
        "MITIGATION_",
    )
}

fn short_name(name: &str, prefix: &str) -> String {
    name.strip_prefix(prefix).unwrap_or(name).to_owned()
}

/// Appends observations to `recording.jsonl`, flushing every line, so a crash
/// keeps everything recorded so far.
pub struct Writer {
    file: std::fs::File,
}

impl Writer {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Writer {
            file: std::fs::File::create(path)?,
        })
    }

    pub fn write(&mut self, observation: &Observation) -> std::io::Result<()> {
        let line = serde_json::to_string(observation)?;
        writeln!(self.file, "{line}")?;
        self.file.flush()
    }
}

pub fn read(path: &Path) -> std::io::Result<Vec<Observation>> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut observations = Vec::new();
    for (index, line) in file.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let observation = serde_json::from_str(&line).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}:{}: {error}", path.display(), index + 1),
            )
        })?;
        observations.push(observation);
    }
    observations.sort_by_key(|o: &Observation| o.t_ms);
    Ok(observations)
}
