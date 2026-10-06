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

//! Battery Thermal Guardian executable.
//!
//! Environment:
//! - `GUARDIAN_CONFIG`: path to the safety parameters
//!   (default `config/guardian/safety-params.toml`)
//! - `ZENOH_CONNECT`, `ZENOH_LISTEN`: comma-separated Zenoh endpoints
//! - `RUST_LOG`: log filter (default `info`)

use anyhow::Context;
use guardian::GuardianConfig;
use guardian_service::runtime;
use guardian_service::transport::{self, ZenohEndpoints};
use thermal_contract::GUARDIAN_EVENTS;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG: &str = "config/guardian/safety-params.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = std::env::var("GUARDIAN_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG.into());
    let text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("cannot read {config_path}"))?;
    let config = GuardianConfig::from_toml_str(&text)?;
    info!(path = %config_path, "safety parameters loaded");

    let endpoints = ZenohEndpoints::from_env();
    let transport = transport::open(GUARDIAN_EVENTS.authority, &endpoints).await?;
    info!(?endpoints, "uProtocol transport ready");

    runtime::run(transport, &config, async {
        let _ = tokio::signal::ctrl_c().await;
        info!("shutting down");
    })
    .await
}
