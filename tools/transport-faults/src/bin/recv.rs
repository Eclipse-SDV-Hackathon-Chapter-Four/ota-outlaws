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


//! Receives the numbered messages and prints one CSV line per arrival:
//! `n,sent_ms,recv_ms`. Stops after `SECONDS` (default 25).
//!
//!     ZENOH_CONNECT=tcp/127.0.0.1:7457 cargo run --bin recv

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use transport_faults::{now_ms, open_transport, topic};
use up_rust::{UListener, UMessage};

#[derive(Deserialize)]
struct Msg {
    n: u64,
    sent_ms: u64,
}

struct Printer;

#[async_trait]
impl UListener for Printer {
    async fn on_receive(&self, message: UMessage) {
        let recv_ms = now_ms();
        if let Some(payload) = message.payload {
            if let Ok(msg) = serde_json::from_slice::<Msg>(&payload) {
                println!("{},{},{}", msg.n, msg.sent_ms, recv_ms);
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let seconds: u64 = std::env::var("SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(25);
    let transport = open_transport("tf-recv", 0x1021).await?;
    transport.register_listener(&topic(), None, Arc::new(Printer)).await?;
    eprintln!("[recv] listening on {}", topic().to_uri(false));
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    Ok(())
}
