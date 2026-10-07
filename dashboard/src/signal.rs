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

//! The signal plot of one scenario run, reduced from its `recording.jsonl`.
//!
//! The page draws the temperatures the Guardian received over the uProtocol
//! interface, with the Guardian's state, its detections and mitigations,
//! the tool's injections, and the DTCs failed in OpenSOVD on the same time
//! axis. Everything comes from the recorded evidence; nothing is computed
//! that the recording does not contain.

use std::path::Path;

use campaign::recording::{EventKind, Observation, SupervisorKind, Tap};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    pub t_ms: u64,
    pub max_c: f32,
    pub avg_c: f32,
    pub min_c: f32,
    pub quality: String,
}

/// A change of the Guardian's thermal state or monitoring status.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub t_ms: u64,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    Detected,
    Recovered,
    Mitigation,
    Supervisor,
}

/// A Guardian or watchdog event worth a marker. Passed fault tests and state
/// changes are left out: the state has its own lane.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Marker {
    pub t_ms: u64,
    pub kind: MarkerKind,
    pub text: String,
}

/// Something the campaign tool did: start the source, stop a service, …
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Injection {
    pub t_ms: u64,
    pub action: String,
    pub detail: String,
}

/// A DTC whose `testFailed` flag changed in OpenSOVD.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DtcChange {
    pub t_ms: u64,
    pub code: String,
    pub failed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Signal {
    pub samples: Vec<Sample>,
    pub thermal: Vec<Change>,
    pub monitoring: Vec<Change>,
    pub markers: Vec<Marker>,
    pub injections: Vec<Injection>,
    pub dtcs: Vec<DtcChange>,
    /// The last observation.
    pub end_ms: u64,
}

/// Reads a recording. Lines that do not parse are skipped: the last line of
/// a running scenario may be half written.
pub fn read(path: &Path) -> std::io::Result<Signal> {
    let text = std::fs::read_to_string(path)?;
    let mut observations: Vec<Observation> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    observations.sort_by_key(|o| o.t_ms);
    Ok(reduce(&observations))
}

pub fn reduce(observations: &[Observation]) -> Signal {
    let mut signal = Signal::default();
    let mut failed: Vec<(String, bool)> = Vec::new();
    for observation in observations {
        let t_ms = observation.t_ms;
        signal.end_ms = signal.end_ms.max(t_ms);
        match &observation.tap {
            Tap::BatteryTemperature(sample) => signal.samples.push(Sample {
                t_ms,
                max_c: sample.max_c,
                avg_c: sample.avg_c,
                min_c: sample.min_c,
                quality: sample.quality.clone(),
            }),
            Tap::GuardianEvent(event) => {
                let text = format!("#{} {}", event.event_id, event.kind.describe());
                let marker = |kind| Marker {
                    t_ms,
                    kind,
                    text: text.clone(),
                };
                match &event.kind {
                    EventKind::ThermalStateChanged { current, .. } => signal.thermal.push(Change {
                        t_ms,
                        value: current.clone(),
                    }),
                    EventKind::MonitoringStatusChanged { current, .. } => {
                        signal.monitoring.push(Change {
                            t_ms,
                            value: current.clone(),
                        })
                    }
                    EventKind::FaultDetected { .. } => {
                        signal.markers.push(marker(MarkerKind::Detected))
                    }
                    EventKind::FaultRecovered { .. } => {
                        signal.markers.push(marker(MarkerKind::Recovered))
                    }
                    EventKind::MitigationRequested { .. } => {
                        signal.markers.push(marker(MarkerKind::Mitigation))
                    }
                    EventKind::FaultTestPassed { .. } | EventKind::Unknown => {}
                }
            }
            Tap::SupervisorEvent(event) => {
                let what = match &event.kind {
                    SupervisorKind::GuardianLost { silence_ms, .. } => {
                        format!("GuardianLost after {silence_ms} ms of silence")
                    }
                    SupervisorKind::MitigationRequested { mitigation } => {
                        format!("MitigationRequested {mitigation}")
                    }
                    SupervisorKind::GuardianRestored { .. } => "GuardianRestored".to_owned(),
                    SupervisorKind::Unknown => "unknown event".to_owned(),
                };
                signal.markers.push(Marker {
                    t_ms,
                    kind: MarkerKind::Supervisor,
                    text: format!("watchdog #{} {what}", event.event_id),
                });
            }
            Tap::SovdFault { code, body, .. } => {
                // A failed poll (no body) says nothing about the flag.
                let Some(now) = body["status"]["testFailed"].as_bool() else {
                    continue;
                };
                let before = match failed.iter_mut().find(|(c, _)| c == code) {
                    Some((_, flag)) => std::mem::replace(flag, now),
                    None => {
                        failed.push((code.clone(), now));
                        false
                    }
                };
                if before != now {
                    signal.dtcs.push(DtcChange {
                        t_ms,
                        code: code.clone(),
                        failed: now,
                    });
                }
            }
            Tap::Injection { action, detail } => signal.injections.push(Injection {
                t_ms,
                action: action.clone(),
                detail: detail.clone(),
            }),
        }
    }
    signal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observations(lines: &str) -> Vec<Observation> {
        lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    const RECORDING: &str = r#"{"t_ms":100,"tap":"injection","action":"start_can_provider","detail":"scenario hot_spot"}
{"t_ms":200,"tap":"battery_temperature","sequence":1,"source_timestamp_ms":1,"alive_counter":0,"quality":"VALID","max_c":40.0,"avg_c":32.0,"min_c":24.0}
{"t_ms":250,"tap":"sovd_fault","code":"BTG_TempOverheat","http_status":200,"body":{"status":{"testFailed":false}}}
{"t_ms":300,"tap":"guardian_event","session_id":"s","event_id":1,"cause_event_id":0,"guardian_time_ms":14,"kind":{"type":"ThermalStateChanged","previous":"CLEAR","current":"WARNING"}}
{"t_ms":310,"tap":"guardian_event","session_id":"s","event_id":2,"cause_event_id":1,"guardian_time_ms":15,"kind":{"type":"MitigationRequested","mitigation":"DRIVER_WARNING_OVERTEMP"}}
{"t_ms":320,"tap":"guardian_event","session_id":"s","event_id":3,"cause_event_id":0,"guardian_time_ms":16,"kind":{"type":"FaultTestPassed","dtc":"BTG_TempStuck","requirement":"FSR-2.4"}}
{"t_ms":400,"tap":"sovd_fault","code":"BTG_TempOverheat","http_status":200,"body":{"status":{"testFailed":true}}}
{"t_ms":450,"tap":"sovd_fault","code":"BTG_TempOverheat","http_status":null,"body":null}
{"t_ms":500,"tap":"sovd_fault","code":"BTG_TempOverheat","http_status":200,"body":{"status":{"testFailed":true}}}
{"t_ms":600,"tap":"sovd_fault","code":"BTG_TempOverheat","http_status":200,"body":{"status":{"testFailed":false}}}
{"t_ms":700,"tap":"supervisor_event","session_id":"w","event_id":1,"cause_event_id":0,"watchdog_time_ms":5,"kind":{"type":"GuardianLost","last_guardian_session_id":"s","silence_ms":800}}"#;

    #[test]
    fn reduces_a_recording_to_the_plot() {
        let signal = reduce(&observations(RECORDING));
        assert_eq!(signal.end_ms, 700);
        assert_eq!(signal.samples.len(), 1);
        assert_eq!(signal.samples[0].max_c, 40.0);
        assert_eq!(signal.injections[0].action, "start_can_provider");
        assert_eq!(
            signal.thermal,
            vec![Change {
                t_ms: 300,
                value: "WARNING".into()
            }]
        );
        // Passed fault tests get no marker; the watchdog's events do.
        let kinds: Vec<_> = signal.markers.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, vec![MarkerKind::Mitigation, MarkerKind::Supervisor]);
        assert_eq!(
            signal.markers[0].text,
            "#2 MitigationRequested DRIVER_WARNING_OVERTEMP"
        );
    }

    #[test]
    fn dtc_changes_only_when_test_failed_flips() {
        let signal = reduce(&observations(RECORDING));
        let changes: Vec<_> = signal.dtcs.iter().map(|d| (d.t_ms, d.failed)).collect();
        // The initial passed state, the failed poll at 450 ms, and the repeat
        // at 500 ms are no changes.
        assert_eq!(changes, vec![(400, true), (600, false)]);
    }

    #[test]
    fn skips_a_half_written_last_line() {
        let path =
            std::env::temp_dir().join(format!("dashboard-signal-{}.jsonl", std::process::id()));
        std::fs::write(
            &path,
            format!("{RECORDING}\n{{\"t_ms\":800,\"tap\":\"battery_tem"),
        )
        .unwrap();
        let signal = read(&path).unwrap();
        assert_eq!(signal.end_ms, 700);
        let _ = std::fs::remove_file(path);
    }
}
