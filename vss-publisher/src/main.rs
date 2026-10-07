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
// AI-assisted: Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5); Claude Code / Claude Opus 5.5 (claude-opus-5-5)
//
// Derived from Eclipse-SDV-Hackathon-Chapter-Four/Doctor-Whodunit,
// branch example-first-steps, demo/services/src/bin/vss_bridge.rs.

//! VSS Bridge — KUKSA Databroker → uProtocol/Zenoh
//!
//! Subscribes to battery temperature VSS signals from kuksa-databroker via gRPC
//! and publishes one `BatteryTemperature` message (contracts/battery_thermal.proto)
//! per Data Broker update over uProtocol/Zenoh to the Guardian.
//!
//! Environment:
//! - `DATABROKER_ADDR`: Data Broker address (default `http://kuksa-databroker:55555`)
//! - `VSS_ALIVE_COUNTER_PATH`, `VSS_QUALITY_PATH`: VSS paths of the CAN frame's
//!   alive counter and quality flag (defaults: the paths of `can/vss_dbc.json`)
//! - `ZENOH_CONNECT`, `ZENOH_LISTEN`: Zenoh endpoints

use std::sync::Arc;

use tokio_stream::StreamExt;
use tracing::{info, warn};
use vss_publisher::{
    make_uri_provider, now_ms, open_up_transport, publish_temperature, vss_battery_temp_uri,
    BatteryTemperature, Quality,
};

// Generated from proto/kuksa/val/v1/
mod kuksa {
    pub mod val {
        pub mod v1 {
            tonic::include_proto!("kuksa.val.v1");
        }
    }
}

use kuksa::val::v1::{
    datapoint::Value, val_client::ValClient, Datapoint, Field, SubscribeEntry, SubscribeRequest,
    View,
};

const VSS_TEMP_MAX: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Max";
const VSS_TEMP_AVG: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Average";
const VSS_TEMP_MIN: &str = "Vehicle.Powertrain.TractionBattery.Temperature.Min";
const VSS_ALIVE_COUNTER: &str = "Vehicle.Powertrain.TractionBattery.BMS.AliveCounter";
const VSS_QUALITY: &str = "Vehicle.Powertrain.TractionBattery.BMS.SignalQuality";

/// Raw values of the CAN quality flag (Quality Enum in docs/reference/architecture.md).
const CAN_QUALITY_INVALID: u32 = 0x00;
const CAN_QUALITY_VALID: u32 = 0x80;
const CAN_QUALITY_ERROR_NOT_AVAILABLE: u32 = 0xFF;

/// Latest values of the subscribed signals.
#[derive(Debug, Default)]
struct Frame {
    temp_max: Option<f32>,
    temp_avg: Option<f32>,
    temp_min: Option<f32>,
    alive_counter: Option<u32>,
    /// Raw CAN value.
    quality: Option<u32>,
}

impl Frame {
    /// Builds the message once all signals of the frame are known.
    fn to_message(&self, sequence: u64, source_timestamp_ms: u64) -> Option<BatteryTemperature> {
        Some(BatteryTemperature {
            max_c: self.temp_max?,
            avg_c: self.temp_avg?,
            min_c: self.temp_min?,
            source_timestamp_ms,
            sequence,
            alive_counter: self.alive_counter?,
            quality: contract_quality(self.quality?) as i32,
        })
    }
}

/// Translates the raw CAN quality flag into the contract's quality.
fn contract_quality(raw: u32) -> Quality {
    match raw {
        CAN_QUALITY_VALID => Quality::Valid,
        CAN_QUALITY_INVALID => Quality::Invalid,
        CAN_QUALITY_ERROR_NOT_AVAILABLE => Quality::NotAvailable,
        _ => Quality::Unspecified,
    }
}

fn as_f32(value: &Value) -> Option<f32> {
    match *value {
        Value::Float(f) => Some(f),
        Value::Double(d) => Some(d as f32),
        Value::Uint32(u) => Some(u as f32),
        Value::Uint64(u) => Some(u as f32),
        Value::Int32(i) => Some(i as f32),
        Value::Int64(i) => Some(i as f32),
        _ => None,
    }
}

fn as_u32(value: &Value) -> Option<u32> {
    match *value {
        Value::Uint32(u) => Some(u),
        Value::Uint64(u) => u32::try_from(u).ok(),
        Value::Int32(i) => u32::try_from(i).ok(),
        Value::Int64(i) => u32::try_from(i).ok(),
        _ => None,
    }
}

/// Time the Data Broker received the value, in milliseconds since the epoch.
fn timestamp_ms(datapoint: &Datapoint) -> Option<u64> {
    let ts = datapoint.timestamp.as_ref()?;
    let ms = ts
        .seconds
        .checked_mul(1000)?
        .checked_add(i64::from(ts.nanos) / 1_000_000)?;
    u64::try_from(ms).ok()
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
    let counter_path =
        std::env::var("VSS_ALIVE_COUNTER_PATH").unwrap_or_else(|_| VSS_ALIVE_COUNTER.to_string());
    let quality_path =
        std::env::var("VSS_QUALITY_PATH").unwrap_or_else(|_| VSS_QUALITY.to_string());

    info!(
        "[VssBridge] Connecting to databroker at {}",
        databroker_addr
    );
    let mut client = ValClient::connect(databroker_addr.clone())
        .await
        .map_err(|e| anyhow::anyhow!("databroker connect failed: {}", e))?;
    info!("[VssBridge] Connected to kuksa-databroker");

    let transport = open_up_transport(make_uri_provider("vss-bridge", 0x1002, 0x01)).await?;
    info!(
        "[VssBridge] uProtocol transport ready, publishing to {}",
        vss_battery_temp_uri().to_uri(false)
    );

    let entries = [
        VSS_TEMP_MAX,
        VSS_TEMP_AVG,
        VSS_TEMP_MIN,
        counter_path.as_str(),
        quality_path.as_str(),
    ]
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

    let mut frame = Frame::default();
    let mut sequence: u64 = 0;

    while let Some(result) = stream.next().await {
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                warn!("[VssBridge] Stream error: {}", e);
                break;
            }
        };

        // The KUKSA CAN Provider writes the signals of a CAN frame one by one,
        // in DBC order, so the alive counter arrives last. Its update marks a
        // complete frame: publishing only then gives one message per frame
        // (assumption A-2). The newest datapoint timestamp is the source
        // timestamp of the message.
        let mut source_timestamp_ms = None;
        let mut frame_complete = false;
        for update in response.updates {
            let Some(entry) = update.entry else { continue };
            let Some(datapoint) = entry.value else {
                continue;
            };
            source_timestamp_ms = source_timestamp_ms.max(timestamp_ms(&datapoint));
            let Some(value) = datapoint.value else {
                continue;
            };

            let path = entry.path.as_str();
            if path == VSS_TEMP_MAX {
                frame.temp_max = as_f32(&value);
            } else if path == VSS_TEMP_AVG {
                frame.temp_avg = as_f32(&value);
            } else if path == VSS_TEMP_MIN {
                frame.temp_min = as_f32(&value);
            } else if path == counter_path {
                frame.alive_counter = as_u32(&value);
                frame_complete = true;
            } else if path == quality_path {
                frame.quality = as_u32(&value);
            }
        }

        if !frame_complete {
            continue;
        }
        // Fallback if the Data Broker sent no timestamp: the publish time.
        let source_timestamp_ms = source_timestamp_ms.unwrap_or_else(now_ms);
        let Some(message) = frame.to_message(sequence + 1, source_timestamp_ms) else {
            continue;
        };
        sequence += 1;

        info!(
            "[VssBridge] #{} TempMax={:.1} TempAvg={:.1} TempMin={:.1} counter={} quality={}",
            message.sequence,
            message.max_c,
            message.avg_c,
            message.min_c,
            message.alive_counter,
            message.quality
        );

        if let Err(status) =
            publish_temperature(Arc::clone(&transport), vss_battery_temp_uri(), &message).await
        {
            warn!("[VssBridge] publish failed: {:?}", status);
        }
    }

    warn!("[VssBridge] Stream ended");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_frame() -> Frame {
        Frame {
            temp_max: Some(50.0),
            temp_avg: Some(40.0),
            temp_min: Some(30.0),
            alive_counter: Some(17),
            quality: Some(CAN_QUALITY_VALID),
        }
    }

    #[test]
    fn no_message_until_all_signals_are_known() {
        let without_counter = Frame {
            alive_counter: None,
            ..complete_frame()
        };
        let without_quality = Frame {
            quality: None,
            ..complete_frame()
        };
        let without_min = Frame {
            temp_min: None,
            ..complete_frame()
        };

        assert!(without_counter.to_message(1, 0).is_none());
        assert!(without_quality.to_message(1, 0).is_none());
        assert!(without_min.to_message(1, 0).is_none());
    }

    #[test]
    fn uses_counter_and_quality_from_the_frame() {
        let message = complete_frame().to_message(5, 1_000).unwrap();

        assert_eq!(message.alive_counter, 17);
        assert_eq!(message.quality, Quality::Valid as i32);
        assert_eq!(message.sequence, 5);
        assert_eq!(message.source_timestamp_ms, 1_000);
    }

    #[test]
    fn translates_can_quality_values() {
        assert_eq!(contract_quality(0x80), Quality::Valid);
        assert_eq!(contract_quality(0x00), Quality::Invalid);
        assert_eq!(contract_quality(0xFF), Quality::NotAvailable);
        assert_eq!(contract_quality(0x01), Quality::Unspecified);
    }

    #[test]
    fn converts_datapoint_timestamp_to_milliseconds() {
        let datapoint = Datapoint {
            timestamp: Some(prost_types::Timestamp {
                seconds: 12,
                nanos: 345_678_901,
            }),
            value: None,
        };

        assert_eq!(timestamp_ms(&datapoint), Some(12_345));
    }

    #[test]
    fn missing_timestamp_is_none() {
        let datapoint = Datapoint {
            timestamp: None,
            value: None,
        };

        assert_eq!(timestamp_ms(&datapoint), None);
    }
}
