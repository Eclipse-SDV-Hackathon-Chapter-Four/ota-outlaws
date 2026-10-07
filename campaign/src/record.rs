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

//! The taps: passive uProtocol listeners on the Guardian's input and output,
//! and OpenSOVD polling. They never send anything into the system.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use prost::Message;
use thermal_contract::transport::{self, uri, ZenohEndpoints};
use thermal_contract::{v1 as pb, BATTERY_TEMPERATURE, GUARDIAN_EVENTS};
use up_rust::{UListener, UMessage, UTransport};

use crate::recording::{GuardianEvent, Observation, Tap, Temperature, Writer};

/// Authority of the tool's own uEntity on the bus.
const AUTHORITY: &str = "campaign";

pub struct Recorder {
    start: Instant,
    writer: Mutex<Writer>,
    samples: AtomicU64,
    /// Milliseconds since `start` of the last sample, 0 if none yet.
    last_sample_ms: AtomicU64,
}

impl Recorder {
    pub fn create(path: &Path) -> std::io::Result<Arc<Self>> {
        Ok(Arc::new(Recorder {
            start: Instant::now(),
            writer: Mutex::new(Writer::create(path)?),
            samples: AtomicU64::new(0),
            last_sample_ms: AtomicU64::new(0),
        }))
    }

    pub fn log(&self, tap: Tap) {
        let t_ms = self.start.elapsed().as_millis() as u64;
        if matches!(tap, Tap::BatteryTemperature(_)) {
            self.samples.fetch_add(1, Ordering::Relaxed);
            self.last_sample_ms.store(t_ms.max(1), Ordering::Relaxed);
        }
        let observation = Observation { t_ms, tap };
        if let Err(error) = self.writer.lock().unwrap().write(&observation) {
            eprintln!("cannot write recording: {error}");
        }
    }

    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }

    /// Time since the last sample, or `None` before the first one.
    pub fn since_last_sample(&self) -> Option<Duration> {
        let last = self.last_sample_ms.load(Ordering::Relaxed);
        (last > 0).then(|| {
            self.start
                .elapsed()
                .saturating_sub(Duration::from_millis(last))
        })
    }
}

struct TemperatureListener(Arc<Recorder>);

#[async_trait]
impl UListener for TemperatureListener {
    async fn on_receive(&self, message: UMessage) {
        let Some(payload) = message.payload else {
            return;
        };
        match pb::BatteryTemperature::decode(payload) {
            Ok(sample) => self
                .0
                .log(Tap::BatteryTemperature(Temperature::from(&sample))),
            Err(error) => self.0.log(Tap::Injection {
                action: "undecodable_sample".to_owned(),
                detail: error.to_string(),
            }),
        }
    }
}

struct EventListener(Arc<Recorder>);

#[async_trait]
impl UListener for EventListener {
    async fn on_receive(&self, message: UMessage) {
        let Some(payload) = message.payload else {
            return;
        };
        if let Ok(event) = pb::GuardianEvent::decode(payload) {
            self.0.log(Tap::GuardianEvent(GuardianEvent::from(&event)));
        }
    }
}

pub struct SovdTap {
    /// For example `http://127.0.0.1:7690/sovd/v1`.
    pub url: String,
    pub entity: String,
    pub codes: Vec<String>,
}

/// Running taps; dropping them stops the recording.
pub struct Taps {
    _transport: Arc<dyn UTransport>,
    poller: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Taps {
    fn drop(&mut self) {
        if let Some(poller) = self.poller.take() {
            poller.abort();
        }
    }
}

pub async fn start(
    recorder: Arc<Recorder>,
    endpoints: &ZenohEndpoints,
    sovd: Option<SovdTap>,
) -> anyhow::Result<Taps> {
    let transport = transport::open(AUTHORITY, endpoints).await?;
    transport
        .register_listener(
            &uri(BATTERY_TEMPERATURE),
            None,
            Arc::new(TemperatureListener(Arc::clone(&recorder))),
        )
        .await
        .map_err(|status| anyhow::anyhow!("cannot subscribe to temperatures: {status:?}"))?;
    transport
        .register_listener(
            &uri(GUARDIAN_EVENTS),
            None,
            Arc::new(EventListener(Arc::clone(&recorder))),
        )
        .await
        .map_err(|status| anyhow::anyhow!("cannot subscribe to Guardian events: {status:?}"))?;
    let poller = sovd.map(|tap| tokio::spawn(poll_sovd(recorder, tap)));
    Ok(Taps {
        _transport: transport,
        poller,
    })
}

/// Polls every fault every 25 ms and records each change. The DFM keeps only
/// the current state, so the recording is the history.
async fn poll_sovd(recorder: Arc<Recorder>, tap: SovdTap) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("cannot create HTTP client: {error}");
            return;
        }
    };
    let base = format!(
        "{}/apps/{}/faults",
        tap.url.trim_end_matches('/'),
        tap.entity
    );
    let mut last: std::collections::BTreeMap<String, (Option<u16>, serde_json::Value)> =
        Default::default();
    loop {
        for code in &tap.codes {
            let current = match client.get(format!("{base}/{code}")).send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response
                        .json::<serde_json::Value>()
                        .await
                        .unwrap_or(serde_json::Value::Null);
                    (Some(status), body)
                }
                Err(_) => (None, serde_json::Value::Null),
            };
            if last.get(code) != Some(&current) {
                recorder.log(Tap::SovdFault {
                    code: code.clone(),
                    http_status: current.0,
                    body: current.1.clone(),
                });
                last.insert(code.clone(), current);
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
