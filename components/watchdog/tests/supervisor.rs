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

//! End-to-end test of HARA DFR-5 over uProtocol and Zenoh: the real watchdog
//! binary runs without a Guardian, so it must request the
//! monitoring-unavailable warning on its own topic. The test then sends
//! heartbeats in the Guardian's role and expects the restoration, linked to
//! the loss. No DFM runs; the watchdog's diagnostics worker only logs that.

use std::net::TcpListener;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use prost::Message;
use thermal_contract::transport::{self, uri, ZenohEndpoints};
use thermal_contract::{v1 as pb, GUARDIAN_HEARTBEAT, SUPERVISOR_EVENTS};
use up_rust::{UListener, UMessage, UMessageBuilder, UPayloadFormat};

use pb::supervisor_event::Kind;

#[derive(Default)]
struct Collected(Mutex<Vec<pb::SupervisorEvent>>);

#[async_trait]
impl UListener for Collected {
    async fn on_receive(&self, message: UMessage) {
        if let Some(payload) = message.payload {
            let event = pb::SupervisorEvent::decode(payload).expect("valid SupervisorEvent");
            self.0.lock().unwrap().push(event);
        }
    }
}

impl Collected {
    async fn wait_for(
        &self,
        timeout: Duration,
        matches: impl Fn(&Kind) -> bool,
    ) -> Option<pb::SupervisorEvent> {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            let found = self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.kind.as_ref().is_some_and(&matches))
                .cloned();
            if found.is_some() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }
}

/// Kills the watchdog when the test ends, also when it fails.
struct Watchdog(Child);

impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_local_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("tcp/127.0.0.1:{}", listener.local_addr().unwrap().port())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watchdog_requests_the_occupant_warning_when_the_guardian_is_silent() {
    let endpoint = free_local_endpoint();
    // In the Guardian's role, so this side may publish heartbeats.
    let guardian_side = transport::open(
        GUARDIAN_HEARTBEAT.authority,
        &ZenohEndpoints {
            listen: vec![endpoint.clone()],
            ..ZenohEndpoints::default()
        },
    )
    .await
    .unwrap();
    let collected = Arc::new(Collected::default());
    guardian_side
        .register_listener(&uri(SUPERVISOR_EVENTS), None, collected.clone())
        .await
        .unwrap();

    let repo = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let _watchdog = Watchdog(
        Command::new(env!("CARGO_BIN_EXE_watchdog"))
            .current_dir(repo)
            .env("ZENOH_CONNECT", &endpoint)
            .env("HEARTBEAT_TIMEOUT_MS", "300")
            .env("RUST_LOG", "warn")
            .spawn()
            .expect("watchdog binary starts"),
    );

    // No Guardian runs: the loss and the warning it causes.
    let lost = collected
        .wait_for(Duration::from_secs(15), |k| {
            matches!(k, Kind::GuardianLost(_))
        })
        .await
        .expect("GuardianLost");
    let warning = collected
        .wait_for(Duration::from_secs(2), |k| {
            matches!(k, Kind::MitigationRequested(_))
        })
        .await
        .expect("warning request");
    assert_eq!(
        warning.kind,
        Some(Kind::MitigationRequested(pb::MitigationRequested {
            mitigation: pb::Mitigation::DriverWarningMonitoringUnavailable as i32,
        }))
    );
    assert_eq!(warning.cause_event_id, lost.event_id);
    assert_eq!(warning.session_id, lost.session_id);
    assert!(matches!(
        &lost.kind,
        Some(Kind::GuardianLost(l)) if l.last_guardian_session_id.is_empty() && l.silence_ms > 300
    ));

    // The Guardian comes up: its heartbeats withdraw the warning.
    let restored = {
        let mut sequence = 0;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            sequence += 1;
            let heartbeat = pb::Heartbeat {
                session_id: "guardian-test".into(),
                sequence,
                guardian_time_ms: sequence * 100,
            };
            let message = UMessageBuilder::publish(uri(GUARDIAN_HEARTBEAT))
                .build_with_payload(
                    heartbeat.encode_to_vec(),
                    UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
                )
                .unwrap();
            guardian_side.send(message).await.unwrap();
            if let Some(event) = collected
                .wait_for(Duration::from_millis(100), |k| {
                    matches!(k, Kind::GuardianRestored(_))
                })
                .await
            {
                break event;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no GuardianRestored"
            );
        }
    };
    assert_eq!(restored.cause_event_id, lost.event_id);
    assert!(matches!(
        &restored.kind,
        Some(Kind::GuardianRestored(r)) if r.guardian_session_id == "guardian-test"
    ));
}
