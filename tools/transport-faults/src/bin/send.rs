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


//! Sends numbered messages at a fixed rate.
//!
//!     ZENOH_CONNECT=tcp/127.0.0.1:7457 cargo run --bin send -- 100 100
//!                                                          (count, interval ms)

use serde::Serialize;
use transport_faults::{now_ms, open_transport, topic};
use up_rust::{UMessageBuilder, UPayloadFormat};

#[derive(Serialize)]
struct Msg {
    n: u64,
    sent_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let count: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);
    let interval_ms: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);

    let transport = open_transport("tf-send", 0x1020).await?;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let mut tick = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    for n in 0..count {
        tick.tick().await;
        let bytes = serde_json::to_vec(&Msg { n, sent_ms: now_ms() })?;
        let message = UMessageBuilder::publish(topic())
            .build_with_payload(bytes, UPayloadFormat::UPAYLOAD_FORMAT_JSON)?;
        transport.send(message).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    eprintln!("[send] done, {count} messages");
    Ok(())
}
