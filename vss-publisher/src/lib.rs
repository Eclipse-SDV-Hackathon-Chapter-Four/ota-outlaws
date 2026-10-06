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
// branch example-first-steps, demo/services/src/{lib.rs,bin/vss_bridge.rs}.

//! Event type, topic URI and uProtocol transport helpers for the VSS publisher.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use up_rust::{
    LocalUriProvider, StaticUriProvider, UMessageBuilder, UPayloadFormat,
    UTransport, UUri,
};
use up_transport_zenoh::UPTransportZenoh;

// =============================================================================
// Resource IDs
// =============================================================================

pub const RID_BATTERY_TEMP_EVENT: u16 = 0x9001;

// =============================================================================
// Event type
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryTempEvent {
    pub temp_max: f32,
    pub temp_avg: f32,
    pub temp_min: f32,
    pub timestamp_ms: u64,
}

// =============================================================================
// URI builders
// =============================================================================

pub fn vss_battery_temp_uri() -> UUri {
    UUri::try_from_parts("battery-vss", 0x9001, 0x01, RID_BATTERY_TEMP_EVENT).unwrap()
}

// =============================================================================
// Transport helpers
// =============================================================================

pub fn make_uri_provider(
    authority: &str,
    entity_id: u32,
    major_version: u8,
) -> Arc<dyn LocalUriProvider> {
    Arc::new(StaticUriProvider::new(authority, entity_id, major_version))
}

pub async fn open_up_transport(
    uri_provider: Arc<dyn LocalUriProvider>,
) -> anyhow::Result<Arc<dyn UTransport>> {
    UPTransportZenoh::try_init_log_from_env();
    let mut config = zenoh::Config::default();
    if let Ok(endpoint) = std::env::var("ZENOH_CONNECT") {
        config
            .insert_json5("connect/endpoints", &format!("[\"{}\"]", endpoint))
            .map_err(|e| anyhow::anyhow!("Zenoh config: {}", e))?;
    }
    if let Ok(endpoint) = std::env::var("ZENOH_LISTEN") {
        config
            .insert_json5("listen/endpoints", &format!("[\"{}\"]", endpoint))
            .map_err(|e| anyhow::anyhow!("Zenoh listen config: {}", e))?;
    }
    let transport = UPTransportZenoh::builder(uri_provider.get_authority())
        .expect("invalid authority name")
        .with_config(config)
        .build()
        .await
        .map(Arc::new)?;
    Ok(transport)
}

pub async fn publish_json_event<T: Serialize>(
    transport: Arc<dyn UTransport>,
    topic: UUri,
    data: &T,
) -> Result<(), up_rust::UStatus> {
    use up_rust::communication::UPayload;
    let bytes = serde_json::to_vec(data)
        .map_err(|e| up_rust::UStatus::fail_with_code(up_rust::UCode::INVALID_ARGUMENT, e.to_string()))?;
    let payload = UPayload::new(bytes, UPayloadFormat::UPAYLOAD_FORMAT_JSON);
    let fmt = payload.payload_format();
    let message = UMessageBuilder::publish(topic)
        .build_with_payload(payload.payload(), fmt)
        .map_err(|e| up_rust::UStatus::fail_with_code(up_rust::UCode::INVALID_ARGUMENT, e.to_string()))?;
    transport.send(message).await
}

// =============================================================================
// Utilities
// =============================================================================

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
