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

//! End-to-end test over uProtocol and Zenoh: a test publisher in the role of the
//! VSS Publisher sends `BatteryTemperature` messages, and the Guardian service
//! answers with `GuardianEvent` messages. No Data Broker is involved.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use guardian::GuardianConfig;
use guardian_service::runtime;
use guardian_service::transport::{self, uri, ZenohEndpoints};
use prost::Message;
use thermal_contract::v1 as pb;
use thermal_contract::{BATTERY_TEMPERATURE, GUARDIAN_EVENTS};
use tokio::sync::oneshot;
use up_rust::{UListener, UMessage, UMessageBuilder, UPayloadFormat, UTransport};

const SHIPPED_CONFIG: &str = include_str!("../../../config/guardian/safety-params.toml");

#[derive(Default)]
struct Collected(Mutex<Vec<pb::GuardianEvent>>);

#[async_trait]
impl UListener for Collected {
    async fn on_receive(&self, message: UMessage) {
        if let Some(payload) = message.payload {
            let event = pb::GuardianEvent::decode(payload).expect("valid GuardianEvent");
            self.0.lock().unwrap().push(event);
        }
    }
}

impl Collected {
    fn find(
        &self,
        matches: impl Fn(&pb::guardian_event::Kind) -> bool,
    ) -> Option<pb::GuardianEvent> {
        let events = self.0.lock().unwrap();
        events
            .iter()
            .find(|event| event.kind.as_ref().is_some_and(&matches))
            .cloned()
    }
}

struct Proxy {
    transport: Arc<dyn UTransport>,
    sequence: u64,
}

impl Proxy {
    async fn publish(&mut self, max_c: f32) {
        self.sequence += 1;
        let payload = pb::BatteryTemperature {
            max_c,
            avg_c: max_c - 5.0,
            min_c: max_c - 10.0,
            source_timestamp_ms: 1_000 + self.sequence * 100,
            sequence: self.sequence,
            alive_counter: (self.sequence % 256) as u32,
            quality: pb::Quality::Valid as i32,
        }
        .encode_to_vec();
        let message = UMessageBuilder::publish(uri(BATTERY_TEMPERATURE))
            .build_with_payload(payload, UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF)
            .unwrap();
        self.transport.send(message).await.unwrap();
    }
}

fn free_local_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("tcp/127.0.0.1:{}", listener.local_addr().unwrap().port())
}

async fn wait_for(
    collected: &Collected,
    timeout: Duration,
    matches: impl Fn(&pb::guardian_event::Kind) -> bool + Copy,
) -> Option<pb::GuardianEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Some(event) = collected.find(matches) {
            return Some(event);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guardian_reacts_to_temperature_and_to_missing_data_over_uprotocol() {
    let endpoint = free_local_endpoint();
    let guardian_side = transport::open(
        GUARDIAN_EVENTS.authority,
        &ZenohEndpoints {
            listen: vec![endpoint.clone()],
            ..ZenohEndpoints::default()
        },
    )
    .await
    .unwrap();
    let proxy_side = transport::open(
        BATTERY_TEMPERATURE.authority,
        &ZenohEndpoints {
            connect: vec![endpoint],
            ..ZenohEndpoints::default()
        },
    )
    .await
    .unwrap();

    let collected = Arc::new(Collected::default());
    proxy_side
        .register_listener(&uri(GUARDIAN_EVENTS), None, collected.clone())
        .await
        .unwrap();

    let config = GuardianConfig::from_toml_str(SHIPPED_CONFIG).unwrap();
    let warn_c = config.thermal.warn_c;
    let (stop, stopped) = oneshot::channel::<()>();
    let service = tokio::spawn(async move {
        runtime::run(guardian_side, &config, async {
            let _ = stopped.await;
        })
        .await
    });

    // Send samples above the warning threshold until the Guardian reports
    // WARNING. The first messages may be lost while Zenoh connects.
    let mut proxy = Proxy {
        transport: proxy_side,
        sequence: 0,
    };
    let warning = |kind: &pb::guardian_event::Kind| {
        matches!(kind, pb::guardian_event::Kind::ThermalStateChanged(change)
            if change.current == pb::ThermalState::Warning as i32)
    };
    let mut warned = None;
    for _ in 0..50 {
        proxy.publish(warn_c + 1.0).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        warned = collected.find(warning);
        if warned.is_some() {
            break;
        }
    }
    let warned = warned.expect("Guardian reports WARNING over uProtocol");
    assert!(warned.kind.is_some());

    // Stop sending: the Guardian must report the loss of fresh data and
    // request the "monitoring unavailable" warning.
    let freshness_lost = wait_for(&collected, Duration::from_secs(3), |kind| {
        matches!(kind, pb::guardian_event::Kind::FaultDetected(fault)
            if fault.dtc == "BTG_TempFreshnessLost" && fault.requirement == "FSR-2.2")
    })
    .await
    .expect("Guardian reports loss of fresh data over uProtocol");
    let mitigation = wait_for(&collected, Duration::from_secs(1), |kind| {
        matches!(kind, pb::guardian_event::Kind::MitigationRequested(request)
            if request.mitigation == pb::Mitigation::DriverWarningMonitoringUnavailable as i32)
    })
    .await
    .expect("Guardian requests the monitoring-unavailable warning");

    // The cause chain survives the transport: fault -> status change -> mitigation.
    let degraded = collected
        .find(|kind| matches!(kind, pb::guardian_event::Kind::MonitoringStatusChanged(_)))
        .expect("monitoring status change");
    assert_eq!(degraded.cause_event_id, freshness_lost.event_id);
    assert_eq!(mitigation.cause_event_id, degraded.event_id);

    stop.send(()).unwrap();
    service.await.unwrap().unwrap();
}
