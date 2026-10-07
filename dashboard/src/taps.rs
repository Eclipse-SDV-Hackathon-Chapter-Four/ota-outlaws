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

//! The taps: passive observers of the data that flows between components.
//! They feed the input and output logs. Like the campaign's taps, they never
//! send anything into the system.
//!
//! - KUKSA: a gRPC subscription to the battery signals in the Data Broker.
//! - uProtocol: listeners on the `BatteryTemperature` and `GuardianEvent`
//!   topics, decoded with the campaign's recording types.
//! - OpenSOVD: polls the fault list and logs every change of a fault.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use campaign::recording::{EventKind, GuardianEvent, Temperature};
use futures_util::StreamExt as _;
use prost::Message as _;
use serde::Serialize;
use thermal_contract::transport::{self, uri, ZenohEndpoints};
use thermal_contract::{v1 as pb, BATTERY_TEMPERATURE, GUARDIAN_EVENTS};
use up_rust::{UListener, UMessage};

use crate::sovd::Sovd;

#[allow(clippy::all)]
mod kuksa {
    pub mod val {
        pub mod v1 {
            tonic::include_proto!("kuksa.val.v1");
        }
    }
}

use kuksa::val::v1::{
    datapoint::Value as VssValue, val_client::ValClient, Field, SubscribeEntry, SubscribeRequest,
    View,
};

/// Entries kept per tap. At 10 frames per second this is about a minute.
const CAPACITY: usize = 600;
const RETRY: Duration = Duration::from_secs(2);
const SOVD_POLL: Duration = Duration::from_secs(1);

/// The battery signals the VSS Publisher reads, in the order the KUKSA CAN
/// Provider writes them. The alive counter comes last and completes a frame.
const VSS_SIGNALS: [(&str, &str); 5] = [
    ("Vehicle.Powertrain.TractionBattery.Temperature.Max", "max"),
    (
        "Vehicle.Powertrain.TractionBattery.Temperature.Average",
        "avg",
    ),
    ("Vehicle.Powertrain.TractionBattery.Temperature.Min", "min"),
    (
        "Vehicle.Powertrain.TractionBattery.BMS.SignalQuality",
        "quality",
    ),
    (
        "Vehicle.Powertrain.TractionBattery.BMS.AliveCounter",
        "counter",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TapId {
    Vss,
    BatteryTemperature,
    GuardianEvents,
    SovdFaults,
}

impl TapId {
    pub fn label(self) -> &'static str {
        match self {
            TapId::Vss => "KUKSA Data Broker: battery signals (gRPC subscription)",
            TapId::BatteryTemperature => "uProtocol: BatteryTemperature //battery-vss/9001/1/9001",
            TapId::GuardianEvents => "uProtocol: GuardianEvent //guardian/9002/1/8001",
            TapId::SovdFaults => "OpenSOVD: fault status changes (polled every second)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            TapId::Vss => "vss",
            TapId::BatteryTemperature => "BatteryTemperature",
            TapId::GuardianEvents => "GuardianEvent",
            TapId::SovdFaults => "sovd",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Unix time in milliseconds when the dashboard observed it.
    pub ts_ms: u64,
    pub source: String,
    pub text: String,
}

#[derive(Default)]
struct Buffer {
    entries: VecDeque<Entry>,
    status: String,
}

/// Ring buffers of the observations, one per tap.
#[derive(Default)]
pub struct Taps {
    buffers: Mutex<HashMap<TapId, Buffer>>,
}

impl Taps {
    pub fn push(&self, tap: TapId, text: String) {
        let mut buffers = self.buffers.lock().expect("tap buffers");
        let buffer = buffers.entry(tap).or_default();
        if buffer.entries.len() == CAPACITY {
            buffer.entries.pop_front();
        }
        buffer.entries.push_back(Entry {
            ts_ms: now_ms(),
            source: tap.short().to_owned(),
            text,
        });
    }

    pub fn set_status(&self, tap: TapId, status: impl Into<String>) {
        let mut buffers = self.buffers.lock().expect("tap buffers");
        buffers.entry(tap).or_default().status = status.into();
    }

    pub fn entries(&self, tap: TapId) -> Vec<Entry> {
        let buffers = self.buffers.lock().expect("tap buffers");
        buffers
            .get(&tap)
            .map(|b| b.entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn status(&self, tap: TapId) -> String {
        let buffers = self.buffers.lock().expect("tap buffers");
        buffers
            .get(&tap)
            .map(|b| b.status.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "starting".to_owned())
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

// --- uProtocol -------------------------------------------------------------

pub fn spawn_uprotocol(taps: Arc<Taps>, endpoints: ZenohEndpoints) {
    tokio::spawn(async move {
        for tap in [TapId::BatteryTemperature, TapId::GuardianEvents] {
            taps.set_status(tap, "connecting to Zenoh");
        }
        let transport = loop {
            match transport::open("dashboard", &endpoints).await {
                Ok(transport) => break transport,
                Err(error) => {
                    for tap in [TapId::BatteryTemperature, TapId::GuardianEvents] {
                        taps.set_status(tap, format!("Zenoh unavailable: {error:#}"));
                    }
                    tokio::time::sleep(RETRY).await;
                }
            }
        };
        let listeners: [(TapId, Arc<dyn UListener>); 2] = [
            (
                TapId::BatteryTemperature,
                Arc::new(TemperatureListener(Arc::clone(&taps))),
            ),
            (
                TapId::GuardianEvents,
                Arc::new(EventListener(Arc::clone(&taps))),
            ),
        ];
        for (tap, listener) in listeners {
            let topic = match tap {
                TapId::BatteryTemperature => BATTERY_TEMPERATURE,
                _ => GUARDIAN_EVENTS,
            };
            match transport
                .register_listener(&uri(topic), None, listener)
                .await
            {
                Ok(()) => taps.set_status(tap, "subscribed"),
                Err(status) => taps.set_status(tap, format!("cannot subscribe: {status:?}")),
            }
        }
        // Zenoh reconnects on its own; keep the transport and its listeners.
        std::future::pending::<()>().await;
        drop(transport);
    });
}

struct TemperatureListener(Arc<Taps>);

#[async_trait]
impl UListener for TemperatureListener {
    async fn on_receive(&self, message: UMessage) {
        let text = match message.payload.map(pb::BatteryTemperature::decode) {
            Some(Ok(decoded)) => describe_temperature(&Temperature::from(&decoded)),
            Some(Err(error)) => format!("undecodable payload: {error}"),
            None => "message without payload".to_owned(),
        };
        self.0.push(TapId::BatteryTemperature, text);
    }
}

struct EventListener(Arc<Taps>);

#[async_trait]
impl UListener for EventListener {
    async fn on_receive(&self, message: UMessage) {
        let text = match message.payload.map(pb::GuardianEvent::decode) {
            Some(Ok(decoded)) => describe_event(&GuardianEvent::from(&decoded)),
            Some(Err(error)) => format!("undecodable payload: {error}"),
            None => "message without payload".to_owned(),
        };
        self.0.push(TapId::GuardianEvents, text);
    }
}

pub fn describe_temperature(t: &Temperature) -> String {
    format!(
        "#{} max {:.1} °C, avg {:.1} °C, min {:.1} °C, counter {}, quality {}",
        t.sequence, t.max_c, t.avg_c, t.min_c, t.alive_counter, t.quality
    )
}

pub fn describe_event(event: &GuardianEvent) -> String {
    let kind = match &event.kind {
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
    };
    let cause = match event.cause_event_id {
        0 => String::new(),
        id => format!(", cause #{id}"),
    };
    let session: String = event.session_id.chars().take(8).collect();
    format!(
        "#{} {kind}{cause} (session {session}…, t={} ms)",
        event.event_id, event.guardian_time_ms
    )
}

// --- KUKSA ------------------------------------------------------------------

pub fn spawn_kuksa(taps: Arc<Taps>, address: String) {
    tokio::spawn(async move {
        loop {
            taps.set_status(TapId::Vss, format!("connecting to {address}"));
            if let Err(error) = subscribe_kuksa(&taps, &address).await {
                taps.set_status(TapId::Vss, format!("Data Broker unavailable: {error:#}"));
            }
            tokio::time::sleep(RETRY).await;
        }
    });
}

async fn subscribe_kuksa(taps: &Taps, address: &str) -> anyhow::Result<()> {
    let mut client = ValClient::connect(address.to_owned()).await?;
    let entries = VSS_SIGNALS
        .iter()
        .map(|(path, _)| SubscribeEntry {
            path: (*path).to_owned(),
            view: View::CurrentValue as i32,
            fields: vec![Field::Value as i32],
        })
        .collect();
    let mut stream = client
        .subscribe(SubscribeRequest { entries })
        .await?
        .into_inner();
    taps.set_status(TapId::Vss, "subscribed");
    let mut frame: BTreeMap<&str, String> = BTreeMap::new();
    while let Some(response) = stream.next().await {
        for update in response?.updates {
            let Some(entry) = update.entry else { continue };
            let Some(value) = entry.value.and_then(|dp| dp.value) else {
                continue;
            };
            let Some((_, name)) = VSS_SIGNALS.iter().find(|(path, _)| *path == entry.path) else {
                continue;
            };
            frame.insert(name, vss_value(&value));
            if *name == "counter" {
                taps.push(TapId::Vss, describe_frame(&frame));
            }
        }
    }
    anyhow::bail!("subscription ended")
}

fn vss_value(value: &VssValue) -> String {
    match value {
        VssValue::Float(v) => format!("{v:.1}"),
        VssValue::Double(v) => format!("{v:.1}"),
        VssValue::Int32(v) => v.to_string(),
        VssValue::Int64(v) => v.to_string(),
        VssValue::Uint32(v) => v.to_string(),
        VssValue::Uint64(v) => v.to_string(),
        VssValue::Bool(v) => v.to_string(),
        VssValue::String(v) => v.clone(),
    }
}

/// One line per CAN frame, in the order Max, Avg, Min, quality, counter.
fn describe_frame(frame: &BTreeMap<&str, String>) -> String {
    let get = |name: &str| frame.get(name).map(String::as_str).unwrap_or("?");
    format!(
        "Temperature.Max {} °C, Average {} °C, Min {} °C, BMS.SignalQuality {}, BMS.AliveCounter {}",
        get("max"),
        get("avg"),
        get("min"),
        get("quality"),
        get("counter")
    )
}

// --- OpenSOVD ---------------------------------------------------------------

pub fn spawn_sovd(taps: Arc<Taps>, sovd: Arc<Sovd>) {
    tokio::spawn(async move {
        let mut previous: Option<BTreeMap<String, String>> = None;
        loop {
            match sovd.raw_faults().await {
                Ok(items) => {
                    taps.set_status(TapId::SovdFaults, "polling");
                    let current: BTreeMap<String, String> = items
                        .iter()
                        .filter_map(|item| {
                            Some((item["code"].as_str()?.to_owned(), fault_state(item)))
                        })
                        .collect();
                    match &previous {
                        None => {
                            let failed = current
                                .values()
                                .filter(|s| s.starts_with("testFailed"))
                                .count();
                            taps.push(
                                TapId::SovdFaults,
                                format!(
                                    "first poll: {} faults in the catalog, {failed} currently failed",
                                    current.len()
                                ),
                            );
                        }
                        Some(previous) => {
                            for (code, state) in &current {
                                if previous.get(code) != Some(state) {
                                    taps.push(TapId::SovdFaults, format!("{code}: {state}"));
                                }
                            }
                            for code in previous.keys().filter(|c| !current.contains_key(*c)) {
                                taps.push(TapId::SovdFaults, format!("{code}: no longer listed"));
                            }
                        }
                    }
                    previous = Some(current);
                }
                Err(error) => {
                    taps.set_status(
                        TapId::SovdFaults,
                        format!("OpenSOVD unavailable: {error:#}"),
                    );
                    if previous.take().is_some() {
                        taps.push(
                            TapId::SovdFaults,
                            format!("OpenSOVD unavailable: {error:#}"),
                        );
                    }
                }
            }
            tokio::time::sleep(SOVD_POLL).await;
        }
    });
}

/// A short description of a fault's status; it changes whenever a status bit
/// or a counter changes.
fn fault_state(item: &serde_json::Value) -> String {
    let status = &item["status"];
    let flag = |name: &str| status[name].as_bool().unwrap_or(false);
    let failed = if flag("testFailed") {
        "testFailed"
    } else {
        "not failed"
    };
    let mut bits = Vec::new();
    for name in [
        "confirmedDtc",
        "pendingDtc",
        "testFailedSinceLastClear",
        "warningIndicatorRequested",
    ] {
        if flag(name) {
            bits.push(name);
        }
    }
    format!(
        "{failed} (mask {}, occurrences {}{}{})",
        status["mask"].as_str().unwrap_or("?"),
        item["occurrence_counter"],
        if bits.is_empty() { "" } else { ", " },
        bits.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_the_newest_entries() {
        let taps = Taps::default();
        for i in 0..CAPACITY + 5 {
            taps.push(TapId::Vss, i.to_string());
        }
        let entries = taps.entries(TapId::Vss);
        assert_eq!(entries.len(), CAPACITY);
        assert_eq!(entries[0].text, "5");
        assert_eq!(taps.status(TapId::GuardianEvents), "starting");
    }

    #[test]
    fn describes_events_with_their_cause() {
        let event = GuardianEvent {
            session_id: "0123456789abcdef".into(),
            event_id: 7,
            cause_event_id: 6,
            guardian_time_ms: 1200,
            kind: EventKind::MitigationRequested {
                mitigation: "DRIVER_WARNING_OVERTEMP".into(),
            },
        };
        assert_eq!(
            describe_event(&event),
            "#7 MitigationRequested DRIVER_WARNING_OVERTEMP, cause #6 (session 01234567…, t=1200 ms)"
        );
    }

    #[test]
    fn fault_state_changes_with_the_status() {
        let failed = serde_json::json!({
            "code": "BTG_X", "occurrence_counter": 2,
            "status": {"testFailed": true, "confirmedDtc": true, "mask": "0xAB"}
        });
        assert_eq!(
            fault_state(&failed),
            "testFailed (mask 0xAB, occurrences 2, confirmedDtc)"
        );
        let passed = serde_json::json!({
            "code": "BTG_X", "occurrence_counter": 2,
            "status": {"testFailed": false, "mask": "0x00"}
        });
        assert_eq!(
            fault_state(&passed),
            "not failed (mask 0x00, occurrences 2)"
        );
    }
}
