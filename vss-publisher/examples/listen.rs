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

//! Test listener: prints every temperature event the publisher sends.
//! Stands in for the Guardian while testing the publisher on its own.
//!
//!     cargo run --example listen

use std::sync::Arc;

use async_trait::async_trait;
use prost::Message;
use up_rust::{UListener, UMessage};
use vss_publisher::{
    make_uri_provider, open_up_transport, vss_battery_temp_uri, BatteryTemperature,
};

struct PrintListener;

#[async_trait]
impl UListener for PrintListener {
    async fn on_receive(&self, message: UMessage) {
        match message.payload {
            Some(payload) => match BatteryTemperature::decode(payload) {
                Ok(temperature) => println!("[listen] {temperature:?}"),
                Err(error) => println!("[listen] undecodable payload: {error}"),
            },
            None => println!("[listen] message without payload"),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let transport = open_up_transport(make_uri_provider("listen-test", 0x1010, 0x01)).await?;
    transport
        .register_listener(&vss_battery_temp_uri(), None, Arc::new(PrintListener))
        .await?;
    println!(
        "[listen] waiting for events on {}",
        vss_battery_temp_uri().to_uri(false)
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
