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

//! Guardian watchdog executable (FSR-2.7, HARA DFR-5).
//!
//! Subscribes to the Guardian's `Heartbeat`. When it stays away for longer
//! than `T_hb`, the watchdog requests the occupant warning
//! `DRIVER_WARNING_MONITORING_UNAVAILABLE` on its own `SupervisorEvent` topic
//! and reports `BTG_GuardianHeartbeatLoss` to DFM. Runs as its own process,
//! because a crashed or hung Guardian cannot report its own failure.
//!
//! Environment:
//! - `HEARTBEAT_TIMEOUT_MS`: `T_hb` (default 1500)
//! - `FAULT_CATALOG`, `SOVD_ENTITY`: as for the Guardian
//! - `ZENOH_CONNECT`, `ZENOH_LISTEN`, `ZENOH_MODE`: Zenoh endpoints
//! - `RUST_LOG`: log filter (default `info`)

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use prost::Message;
use thermal_contract::transport::{self, uri, ZenohEndpoints};
use thermal_contract::{v1 as pb, GUARDIAN_HEARTBEAT, SUPERVISOR_EVENTS};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use up_rust::{UListener, UMessage, UMessageBuilder, UPayloadFormat, UTransport};
use watchdog::diagnostics::{Diagnostics, DiagnosticsConfig, GuardianSeen, Report};
use watchdog::supervisor::Supervisor;
use watchdog::{HeartbeatMonitor, Transition};

/// `T_hb` from the Safety Concept: three heartbeat periods of 500 ms.
const DEFAULT_TIMEOUT_MS: u64 = 1_500;
/// How often the watchdog checks the timeout. Adds at most this much to the
/// detection time.
const CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// State shared by the heartbeat listener and the timeout check.
struct Shared {
    monitor: HeartbeatMonitor,
    supervisor: Supervisor,
    last: Option<GuardianSeen>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let timeout_ms = match std::env::var("HEARTBEAT_TIMEOUT_MS") {
        Ok(value) => value
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid HEARTBEAT_TIMEOUT_MS {value}: {e}"))?,
        Err(_) => DEFAULT_TIMEOUT_MS,
    };

    let endpoints = ZenohEndpoints::from_env();
    let transport = transport::open("guardian-watchdog", &endpoints).await?;
    info!(?endpoints, timeout_ms, "uProtocol transport ready");

    let diagnostics = Diagnostics::start(DiagnosticsConfig::from_env())?;
    info!(session_id = %diagnostics.session_id, "diagnostics worker started");

    let started = Instant::now();
    let shared = Arc::new(Mutex::new(Shared {
        monitor: HeartbeatMonitor::new(timeout_ms),
        supervisor: Supervisor::new(diagnostics.session_id.clone()),
        last: None,
    }));
    let warnings = WarningPublisher {
        transport: Arc::clone(&transport),
    };

    let source = uri(GUARDIAN_HEARTBEAT);
    let listener: Arc<dyn UListener> = Arc::new(HeartbeatListener {
        shared: Arc::clone(&shared),
        diagnostics: diagnostics.clone(),
        warnings: warnings.clone(),
        started,
    });
    transport
        .register_listener(&source, None, listener.clone())
        .await
        .map_err(|status| {
            anyhow::anyhow!("cannot subscribe to {}: {status:?}", source.to_uri(false))
        })?;
    info!(topic = %source.to_uri(false), "watching the Guardian heartbeat");

    let mut ticker = tokio::time::interval(CHECK_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            _ = ticker.tick() => {
                let now = elapsed_ms(started);
                let lost = {
                    let mut shared = shared.lock().expect("watchdog state");
                    match shared.monitor.on_tick(now) {
                        Transition::BecameLost => {
                            let last = shared.last.clone();
                            let silence_ms = shared.monitor.silence_ms(now);
                            let events = shared.supervisor.on_lost(last.as_ref(), silence_ms, now);
                            Some((events, Report::Lost { last, silence_ms }))
                        }
                        _ => None,
                    }
                };
                if let Some((events, report)) = lost {
                    warn!(?report, "Guardian heartbeat lost");
                    // The occupant warning first: diagnostics must not delay
                    // the safety reaction (HARA DFR-6).
                    warnings.publish(events).await;
                    diagnostics.report(report);
                }
            }
        }
    }

    info!("shutting down");
    diagnostics.stop();
    if let Err(status) = transport.unregister_listener(&source, None, listener).await {
        warn!(?status, "cannot unsubscribe from the Guardian heartbeat");
    }
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

struct HeartbeatListener {
    shared: Arc<Mutex<Shared>>,
    diagnostics: Diagnostics,
    warnings: WarningPublisher,
    started: Instant,
}

/// Publishes `SupervisorEvent`s on the watchdog's own topic (HARA DFR-5).
#[derive(Clone)]
struct WarningPublisher {
    transport: Arc<dyn UTransport>,
}

impl WarningPublisher {
    /// A failed publish is logged; the DFM report still follows.
    async fn publish(&self, events: Vec<pb::SupervisorEvent>) {
        for event in events {
            info!(id = event.event_id, cause = event.cause_event_id, kind = ?event.kind, "supervisor event");
            let message = UMessageBuilder::publish(uri(SUPERVISOR_EVENTS)).build_with_payload(
                event.encode_to_vec(),
                UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
            );
            match message {
                Ok(message) => {
                    match tokio::time::timeout(
                        Duration::from_millis(100),
                        self.transport.send(message),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        result => warn!(
                            ?result,
                            id = event.event_id,
                            "cannot publish supervisor event"
                        ),
                    }
                }
                Err(error) => {
                    warn!(%error, id = event.event_id, "cannot build supervisor event message")
                }
            }
        }
    }
}

#[async_trait]
impl UListener for HeartbeatListener {
    async fn on_receive(&self, message: UMessage) {
        let Some(payload) = message.payload else {
            warn!("heartbeat without payload");
            return;
        };
        let heartbeat = match pb::Heartbeat::decode(payload) {
            Ok(heartbeat) => heartbeat,
            Err(error) => {
                warn!(%error, "heartbeat rejected");
                return;
            }
        };
        let guardian = GuardianSeen {
            session_id: heartbeat.session_id,
            sequence: heartbeat.sequence,
        };
        let (events, report) = {
            let mut shared = self.shared.lock().expect("watchdog state");
            shared.last = Some(guardian.clone());
            let now = elapsed_ms(self.started);
            match shared.monitor.on_heartbeat(now) {
                Transition::FirstHeartbeat => (Vec::new(), Some(Report::TestPassed { guardian })),
                Transition::Recovered => (
                    shared.supervisor.on_restored(&guardian, now),
                    Some(Report::Recovered { guardian }),
                ),
                _ => (Vec::new(), None),
            }
        };
        self.warnings.publish(events).await;
        if let Some(report) = report {
            info!(?report, "Guardian heartbeat present");
            self.diagnostics.report(report);
        }
    }
}
