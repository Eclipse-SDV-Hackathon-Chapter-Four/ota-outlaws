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


//! Shared helpers: a Zenoh transport that talks only to one router.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use up_rust::{LocalUriProvider, StaticUriProvider, UTransport, UUri};
use up_transport_zenoh::UPTransportZenoh;

pub const TOPIC_RESOURCE_ID: u16 = 0x9001;

pub fn topic() -> UUri {
    UUri::try_from_parts("tf-test", 0x9001, 0x01, TOPIC_RESOURCE_ID).unwrap()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Opens a transport as a Zenoh *client* of `ZENOH_CONNECT`. Multicast scouting
/// is off, so every message must pass through the router, where the fault is
/// injected.
pub async fn open_transport(name: &str, entity_id: u32) -> anyhow::Result<Arc<dyn UTransport>> {
    let provider: Arc<dyn LocalUriProvider> = Arc::new(StaticUriProvider::new(name, entity_id, 0x01));
    let endpoint = std::env::var("ZENOH_CONNECT")
        .map_err(|_| anyhow::anyhow!("ZENOH_CONNECT is not set, e.g. tcp/127.0.0.1:7457"))?;
    let mut config = zenoh::Config::default();
    let set = |config: &mut zenoh::Config, key: &str, value: &str| {
        config.insert_json5(key, value).map_err(|e| anyhow::anyhow!("zenoh config {key}: {e}"))
    };
    set(&mut config, "mode", "\"client\"")?;
    set(&mut config, "connect/endpoints", &format!("[\"{endpoint}\"]"))?;
    set(&mut config, "scouting/multicast/enabled", "false")?;
    let transport = UPTransportZenoh::builder(provider.get_authority())
        .map_err(|e| anyhow::anyhow!("invalid authority: {e}"))?
        .with_config(config)
        .build()
        .await?;
    Ok(Arc::new(transport))
}
