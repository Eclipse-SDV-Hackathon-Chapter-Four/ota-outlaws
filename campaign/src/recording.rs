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
                mitigation: short_name(
                    pb::Mitigation::try_from(request.mitigation)
                        .map(|m| m.as_str_name())
                        .unwrap_or("UNKNOWN"),
                    "MITIGATION_",
                ),
            },
            None => EventKind::Unknown,
        };
        GuardianEvent {
            session_id: message.session_id.clone(),
            event_id: message.event_id,
            cause_event_id: message.cause_event_id,
            guardian_time_ms: message.guardian_time_ms,
            kind,
        }
    }
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
