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

//! Runs the Guardian core: feeds it received samples and ticks, and publishes
//! its events.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use guardian::{Event, Guardian, GuardianConfig, Millis, Sample};
use thermal_contract::{BATTERY_TEMPERATURE, GUARDIAN_EVENTS, GUARDIAN_HEARTBEAT};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tracing::{debug, info, warn};
use up_rust::{UListener, UMessage, UMessageBuilder, UPayloadFormat, UTransport};

use crate::convert::{decode_sample, encode_event_for_session, encode_heartbeat};
use crate::diagnostics::Diagnostics;
use crate::transport::uri;

/// How often the core checks time-based conditions, such as missing samples.
pub const TICK_INTERVAL: Duration = Duration::from_millis(50);

/// `T_hb_period`: how often the Guardian publishes a `Heartbeat` (FSR-2.7).
pub const HEARTBEAT_PERIOD: Duration = Duration::from_millis(500);

/// Received samples waiting for the core. A full queue drops samples, which
/// the core then detects as missing data.
const SAMPLE_QUEUE: usize = 1024;

/// Subscribes to `BatteryTemperature` messages, runs the core until `shutdown`
/// completes, and publishes every core event as a `GuardianEvent`.
///
/// All core calls happen on this task, in the order the samples arrived, so the
/// core stays single-threaded and deterministic.
pub async fn run(
    transport: Arc<dyn UTransport>,
    config: &GuardianConfig,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    run_with_diagnostics(transport, config, None, shutdown).await
}

pub async fn run_with_diagnostics(
    transport: Arc<dyn UTransport>,
    config: &GuardianConfig,
    diagnostics: Option<Diagnostics>,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let session_id = diagnostics
        .as_ref()
        .map(|d| d.session_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let (samples_tx, mut samples_rx) = mpsc::channel(SAMPLE_QUEUE);
    let source = uri(BATTERY_TEMPERATURE);
    let listener: Arc<dyn UListener> = Arc::new(SampleListener {
        samples: samples_tx,
    });
    transport
        .register_listener(&source, None, listener.clone())
        .await
        .map_err(|status| {
            anyhow::anyhow!("cannot subscribe to {}: {status:?}", source.to_uri(false))
        })?;
    info!(topic = %source.to_uri(false), "subscribed to battery temperature");

    let mut guardian = Guardian::new(config);
    let publisher = EventPublisher {
        transport: transport.clone(),
        session_id,
    };
    let started = Instant::now();
    let now = || Millis(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));

    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // The heartbeat is driven by the same loop that runs the core, not by a
    // separate task: if the core hangs, the heartbeat stops too, so the
    // watchdog sees a hang and not only a crash (HARA TS-23).
    let mut heartbeat = tokio::time::interval(HEARTBEAT_PERIOD);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut heartbeat_sequence: u64 = 0;
    tokio::pin!(shutdown);

    loop {
        let events = tokio::select! {
            () = &mut shutdown => break,
            _ = ticker.tick() => guardian.on_tick(now()),
            Some(sample) = samples_rx.recv() => guardian.on_sample(sample, now()),
            _ = heartbeat.tick() => {
                heartbeat_sequence += 1;
                publisher.publish_heartbeat(heartbeat_sequence, now().0).await;
                Vec::new()
            }
        };
        for event in &events {
            if let Some(diagnostics) = &diagnostics {
                diagnostics.report(event);
            }
            publisher.publish(event).await;
        }
    }

    if let Err(status) = transport.unregister_listener(&source, None, listener).await {
        warn!(?status, "cannot unsubscribe from battery temperature");
    }
    Ok(())
}

struct SampleListener {
    samples: mpsc::Sender<Sample>,
}

#[async_trait]
impl UListener for SampleListener {
    async fn on_receive(&self, message: UMessage) {
        let Some(payload) = message.payload else {
            warn!("battery temperature message without payload");
            return;
        };
        match decode_sample(&payload) {
            Ok(sample) => {
                if self.samples.try_send(sample).is_err() {
                    warn!("sample queue full or closed, sample dropped");
                }
            }
            Err(error) => warn!(%error, "battery temperature message rejected"),
        }
    }
}

struct EventPublisher {
    transport: Arc<dyn UTransport>,
    session_id: String,
}

impl EventPublisher {
    /// Logs and publishes one event. A failed publish is logged; it does not
    /// stop the Guardian, whose state stays correct.
    async fn publish(&self, event: &Event) {
        info!(id = event.id.0, cause = ?event.cause.map(|c| c.0), at_ms = event.at.0, kind = ?event.kind, "guardian event");
        let message = UMessageBuilder::publish(uri(GUARDIAN_EVENTS)).build_with_payload(
            encode_event_for_session(event, &self.session_id),
            UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
        );
        match message {
            Ok(message) => {
                match tokio::time::timeout(Duration::from_millis(100), self.transport.send(message))
                    .await
                {
                    Ok(Ok(())) => {}
                    result => warn!(?result, id = event.id.0, "cannot publish guardian event"),
                }
            }
            Err(error) => warn!(%error, id = event.id.0, "cannot build guardian event message"),
        }
    }

    /// Publishes one `Heartbeat`. A failed publish is logged; a missing
    /// heartbeat is exactly what the watchdog is there to notice.
    async fn publish_heartbeat(&self, sequence: u64, guardian_time_ms: u64) {
        debug!(sequence, "guardian heartbeat");
        let message = UMessageBuilder::publish(uri(GUARDIAN_HEARTBEAT)).build_with_payload(
            encode_heartbeat(&self.session_id, sequence, guardian_time_ms),
            UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
        );
        match message {
            Ok(message) => {
                match tokio::time::timeout(Duration::from_millis(100), self.transport.send(message))
                    .await
                {
                    Ok(Ok(())) => {}
                    result => warn!(?result, sequence, "cannot publish guardian heartbeat"),
                }
            }
            Err(error) => warn!(%error, sequence, "cannot build guardian heartbeat message"),
        }
    }
}
