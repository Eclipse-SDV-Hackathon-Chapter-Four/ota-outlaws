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
//
// AI-assisted: Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5)
//
// Derived from Eclipse-SDV-Hackathon-Chapter-Four/Doctor-Whodunit,
// branch example-first-steps, demo/services/src/bin/vss_bridge.rs.

//! VSS Bridge — KUKSA Databroker → uProtocol/Zenoh
//!
//! Subscribes to battery temperature VSS signals from kuksa-databroker via gRPC,
//! assembles BatteryTempEvent, and publishes over uProtocol/Zenoh so the Guardian
//! receives the same message format as the sim path.

use std::sync::Arc;

use vss_publisher::{
    make_uri_provider, now_ms, open_up_transport, publish_json_event,
    vss_battery_temp_uri, BatteryTempEvent,
};
use tokio_stream::StreamExt;
use tracing::{info, warn};

// Generated from proto/kuksa/val/v1/
mod kuksa {
    pub mod val {
        pub mod v1 {
            tonic::include_proto!("kuksa.val.v1");
        }
    }
}

use kuksa::val::v1::{datapoint::Value, val_client::ValClient, Field, SubscribeEntry, SubscribeRequest, View};

const VSS_TEMP_MAX: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Max";
const VSS_TEMP_AVG: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Average";
const VSS_TEMP_MIN: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Min";

#[derive(Debug, Default)]
struct SignalState {
    temp_max: f32,
    temp_avg: f32,
    temp_min: f32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "vss_bridge=info,info".to_string()),
        )
        .init();

    let databroker_addr = std::env::var("DATABROKER_ADDR")
        .unwrap_or_else(|_| "http://kuksa-databroker:55555".to_string());

    info!("[VssBridge] Connecting to databroker at {}", databroker_addr);
    let mut client = ValClient::connect(databroker_addr.clone()).await
        .map_err(|e| anyhow::anyhow!("databroker connect failed: {}", e))?;
    info!("[VssBridge] Connected to kuksa-databroker");

    let transport = open_up_transport(make_uri_provider("vss-bridge", 0x1002, 0x01)).await?;
    info!("[VssBridge] uProtocol transport ready, publishing to {}",
        vss_battery_temp_uri().to_uri(false));

    let entries = [VSS_TEMP_MAX, VSS_TEMP_AVG, VSS_TEMP_MIN]
        .iter()
        .map(|&path| SubscribeEntry {
            path: path.to_string(),
            view: View::CurrentValue as i32,
            fields: vec![Field::Value as i32],
        })
        .collect();

    let mut stream = client
        .subscribe(SubscribeRequest { entries })
        .await
        .map_err(|e| anyhow::anyhow!("subscribe failed: {}", e))?
        .into_inner();

    info!("[VssBridge] Subscribed to VSS temperature signals");

    let mut state = SignalState::default();

    while let Some(result) = stream.next().await {
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                warn!("[VssBridge] Stream error: {}", e);
                break;
            }
        };

        for update in response.updates {
            let entry = match update.entry {
                Some(e) => e,
                None => continue,
            };

            let fval = match entry.value.and_then(|dp| dp.value) {
                Some(Value::Float(f))  => f,
                Some(Value::Double(d)) => d as f32,
                Some(Value::Uint32(u)) => u as f32,
                Some(Value::Uint64(u)) => u as f32,
                _ => continue,
            };

            match entry.path.as_str() {
                VSS_TEMP_MAX => state.temp_max = fval,
                VSS_TEMP_AVG => state.temp_avg = fval,
                VSS_TEMP_MIN => state.temp_min = fval,
                _            => {}
            }
        }

        if state.temp_max == 0.0 {
            continue;
        }

        let event = BatteryTempEvent {
            temp_max: state.temp_max,
            temp_avg: state.temp_avg,
            temp_min: state.temp_min,
            timestamp_ms: now_ms(),
        };

        info!(
            "[VssBridge] TempMax={:.1} TempAvg={:.1} TempMin={:.1}",
            event.temp_max, event.temp_avg, event.temp_min
        );

        let _ = publish_json_event(Arc::clone(&transport), vss_battery_temp_uri(), &event).await;
    }

    warn!("[VssBridge] Stream ended");
    Ok(())
}
