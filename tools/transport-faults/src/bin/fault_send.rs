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

//! Stands in for the VSS Publisher and injects delivery faults.
//!
//! Sends `BatteryTemperature` messages (contracts/battery_thermal.proto) on the
//! real topic and, inside a time window, duplicates, reorders, drops or delays
//! them. The first manipulated message is written to the injection log with its
//! wall-clock time, so that a detection latency can be computed later.
//!
//!     ZENOH_CONNECT=tcp/127.0.0.1:7457 cargo run --bin fault_send -- \
//!         --fault duplicate --start-s 10 --duration-s 5 --correlation-id S10
//!
//! Options (all optional):
//!   --fault none|duplicate|reorder|drop|delay   default none
//!   --start-s 10 --duration-s 5 --total-s 30    fault window and run length
//!   --rate-hz 10      message rate (the CAN frame rate is 10 Hz)
//!   --every 1         manipulate every n-th message inside the window
//!   --delay-ms 1000   delay for --fault delay
//!   --seed 1234       seed for the (deterministic) temperature profile
//!   --correlation-id S00   written to the injection log
//!   --log inject.jsonl     injection log (JSON lines, appended)

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use prost::Message;
use thermal_contract::v1::{BatteryTemperature, Quality};
use thermal_contract::BATTERY_TEMPERATURE;
use transport_faults::{now_ms, open_transport};
use up_rust::{UMessage, UMessageBuilder, UPayloadFormat, UTransport, UUri};

struct Args {
    fault: String,
    start_s: f64,
    duration_s: f64,
    total_s: f64,
    rate_hz: f64,
    every: u64,
    delay_ms: u64,
    seed: u64,
    correlation_id: String,
    log: String,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut a = Args {
        fault: "none".into(),
        start_s: 10.0,
        duration_s: 5.0,
        total_s: 30.0,
        rate_hz: 10.0,
        every: 1,
        delay_ms: 1000,
        seed: 1234,
        correlation_id: "S00".into(),
        log: "inject.jsonl".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| anyhow::anyhow!("{flag} needs a value"));
        match flag.as_str() {
            "--fault" => a.fault = value()?,
            "--start-s" => a.start_s = value()?.parse()?,
            "--duration-s" => a.duration_s = value()?.parse()?,
            "--total-s" => a.total_s = value()?.parse()?,
            "--rate-hz" => a.rate_hz = value()?.parse()?,
            "--every" => a.every = value()?.parse::<u64>()?.max(1),
            "--delay-ms" => a.delay_ms = value()?.parse()?,
            "--seed" => a.seed = value()?.parse()?,
            "--correlation-id" => a.correlation_id = value()?,
            "--log" => a.log = value()?,
            other => anyhow::bail!("unknown option {other}"),
        }
    }
    if !["none", "duplicate", "reorder", "drop", "delay"].contains(&a.fault.as_str()) {
        anyhow::bail!("--fault must be none, duplicate, reorder, drop or delay");
    }
    Ok(a)
}

/// Small deterministic generator (xorshift), so the same seed gives the same run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Nominal profile: a slow, safe swing around 35 °C in 1 °C steps (the CAN
/// resolution). Average and minimum follow the maximum at fixed offsets, so a
/// flat peak never looks like a stuck maximum next to moving neighbours. The
/// seed adds a constant offset of 0 to 2 °C to the whole profile.
fn temperature(i: u64, rate_hz: f64, base: f64) -> (f32, f32, f32) {
    let t = i as f64 / rate_hz;
    let max = (base + 5.0 * (t * std::f64::consts::TAU / 30.0).sin()).round();
    (max as f32, (max - 5.0) as f32, (max - 10.0) as f32)
}

fn topic() -> UUri {
    let t = BATTERY_TEMPERATURE;
    UUri::try_from_parts(t.authority, t.ue_id, t.ue_version_major, t.resource_id)
        .expect("topics in the contract are valid URIs")
}

fn build(sample: &BatteryTemperature) -> anyhow::Result<UMessage> {
    Ok(UMessageBuilder::publish(topic())
        .build_with_payload(sample.encode_to_vec(), UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF)?)
}

fn log_line(path: &str, line: serde_json::Value) -> anyhow::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let transport: Arc<dyn UTransport> = open_transport("fault-send", 0x1030).await?;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let mut rng = Rng(args.seed.max(1));
    let base = 33.0 + (rng.next() % 3) as f64;
    let total = (args.total_s * args.rate_hz) as u64;
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / args.rate_hz));
    let mut held: Option<UMessage> = None;
    let mut first_hit: Option<(u64, u64)> = None; // (seq, wall ms)
    let (mut hits, mut sent) = (0u64, 0u64);

    log_line(&args.log, serde_json::json!({
        "event": "run_start", "correlation_id": args.correlation_id, "fault": args.fault,
        "window_s": [args.start_s, args.start_s + args.duration_s], "every": args.every,
        "delay_ms": args.delay_ms, "seed": args.seed, "rate_hz": args.rate_hz,
        "wall_ms": now_ms(),
    }))?;

    for i in 0..total {
        tick.tick().await;
        let t = i as f64 / args.rate_hz;
        let in_window = t >= args.start_s && t < args.start_s + args.duration_s;
        let (max_c, avg_c, min_c) = temperature(i, args.rate_hz, base);
        let sample = BatteryTemperature {
            max_c,
            avg_c,
            min_c,
            source_timestamp_ms: now_ms(),
            sequence: i + 1,
            alive_counter: (i % 256) as u32,
            quality: Quality::Valid as i32,
        };
        let message = build(&sample)?;

        let hit = in_window && args.fault != "none" && i % args.every == 0;
        if hit && first_hit.is_none() {
            first_hit = Some((sample.sequence, now_ms()));
            log_line(&args.log, serde_json::json!({
                "event": "fault_start", "correlation_id": args.correlation_id,
                "fault": args.fault, "first_seq": sample.sequence, "wall_ms": now_ms(),
            }))?;
        }
        if !hit {
            transport.send(message).await?;
            sent += 1;
            continue;
        }
        hits += 1;
        match args.fault.as_str() {
            "duplicate" => {
                transport.send(message.clone()).await?;
                transport.send(message).await?;
                sent += 2;
            }
            "reorder" => match held.take() {
                Some(older) => {
                    transport.send(message).await?;
                    transport.send(older).await?;
                    sent += 2;
                }
                None => held = Some(message),
            },
            "drop" => {} // the sequence keeps counting, so the receiver sees a gap
            "delay" => {
                let transport = Arc::clone(&transport);
                let delay = Duration::from_millis(args.delay_ms);
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = transport.send(message).await;
                });
                sent += 1;
            }
            _ => unreachable!(),
        }
    }
    if let Some(older) = held.take() {
        transport.send(older).await?; // do not lose the last held message
        sent += 1;
    }
    tokio::time::sleep(Duration::from_millis(args.delay_ms + 1500)).await;
    log_line(&args.log, serde_json::json!({
        "event": "run_end", "correlation_id": args.correlation_id, "fault": args.fault,
        "messages_planned": total, "messages_sent": sent, "manipulated": hits,
        "first_manipulated": first_hit.map(|(s, w)| serde_json::json!({"seq": s, "wall_ms": w})),
        "wall_ms": now_ms(),
    }))?;
    eprintln!("[fault_send] done: planned={total} sent={sent} manipulated={hits}");
    Ok(())
}
