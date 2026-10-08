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

//! uProtocol over Zenoh.

use std::sync::Arc;

use crate::Topic;
use anyhow::Context;
use up_rust::{UTransport, UUri};
use up_transport_zenoh::UPTransportZenoh;

/// Where the Zenoh session connects to and listens on, for example
/// `tcp/127.0.0.1:7447`. Empty lists use Zenoh's defaults.
#[derive(Debug, Clone, Default)]
pub struct ZenohEndpoints {
    pub connect: Vec<String>,
    pub listen: Vec<String>,
    /// Discover peers by multicast. Turned off when endpoints are given
    /// explicitly, for example in tests.
    pub multicast_scouting: bool,
    /// Zenoh session mode: `peer` (Zenoh's default) or `client`. A client
    /// talks only through the router it connects to, which works from
    /// outside a container network.
    pub mode: Option<String>,
}

impl ZenohEndpoints {
    /// Reads `ZENOH_CONNECT` and `ZENOH_LISTEN`, each a comma-separated list.
    /// Multicast scouting stays on, as in Zenoh's default configuration.
    pub fn from_env() -> Self {
        let list = |name: &str| {
            std::env::var(name)
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|endpoint| !endpoint.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            connect: list("ZENOH_CONNECT"),
            listen: list("ZENOH_LISTEN"),
            multicast_scouting: true,
            mode: std::env::var("ZENOH_MODE").ok(),
        }
    }
}

/// Opens a uProtocol transport for the uEntity `authority`.
pub async fn open(
    authority: &str,
    endpoints: &ZenohEndpoints,
) -> anyhow::Result<Arc<dyn UTransport>> {
    let mut config = zenoh::Config::default();
    let json_list = |endpoints: &[String]| {
        let quoted: Vec<String> = endpoints.iter().map(|e| format!("\"{e}\"")).collect();
        format!("[{}]", quoted.join(","))
    };
    if !endpoints.connect.is_empty() {
        config
            .insert_json5("connect/endpoints", &json_list(&endpoints.connect))
            .map_err(|e| anyhow::anyhow!("invalid ZENOH_CONNECT: {e}"))?;
    }
    if !endpoints.listen.is_empty() {
        config
            .insert_json5("listen/endpoints", &json_list(&endpoints.listen))
            .map_err(|e| anyhow::anyhow!("invalid ZENOH_LISTEN: {e}"))?;
    }
    config
        .insert_json5(
            "scouting/multicast/enabled",
            if endpoints.multicast_scouting {
                "true"
            } else {
                "false"
            },
        )
        .map_err(|e| anyhow::anyhow!("invalid scouting setting: {e}"))?;

    if let Some(mode) = &endpoints.mode {
        config
            .insert_json5("mode", &format!("\"{mode}\""))
            .map_err(|e| anyhow::anyhow!("invalid Zenoh mode {mode}: {e}"))?;
    }

    let transport = UPTransportZenoh::builder(authority)
        .map_err(|e| anyhow::anyhow!("invalid authority {authority}: {e}"))?
        .with_config(config)
        .build()
        .await
        .context("cannot open Zenoh session")?;
    Ok(Arc::new(transport))
}

/// The uProtocol URI of a topic from the contract.
pub fn uri(topic: Topic) -> UUri {
    UUri::try_from_parts(
        topic.authority,
        topic.ue_id,
        topic.ue_version_major,
        topic.resource_id,
    )
    .expect("topics in the contract are valid URIs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BATTERY_TEMPERATURE, GUARDIAN_EVENTS, GUARDIAN_HEARTBEAT, SUPERVISOR_EVENTS};

    #[test]
    fn topic_uris_match_the_contract_documentation() {
        assert_eq!(
            uri(BATTERY_TEMPERATURE).to_uri(false),
            "//battery-vss/9001/1/9001"
        );
        assert_eq!(uri(GUARDIAN_EVENTS).to_uri(false), "//guardian/9002/1/8001");
        assert_eq!(
            uri(GUARDIAN_HEARTBEAT).to_uri(false),
            "//guardian/9002/1/8002"
        );
        assert_eq!(
            uri(SUPERVISOR_EVENTS).to_uri(false),
            "//guardian-watchdog/9003/1/8001"
        );
    }
}
