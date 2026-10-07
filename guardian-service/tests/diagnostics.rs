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

// AI-assisted: Codex / GPT-6.1 Sol (gpt-6.1-sol)

//! Guardian -> iceoryx2 DFM -> OpenSOVD campaign. Requires Linux IPC.
use async_trait::async_trait;
use guardian::GuardianConfig;
use guardian_service::{
    diagnostics::{Diagnostics, DiagnosticsConfig},
    runtime,
    transport::{self, uri, ZenohEndpoints},
};
use prost::Message;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use thermal_contract::{v1 as pb, BATTERY_TEMPERATURE, GUARDIAN_EVENTS};
use tokio::sync::oneshot;
use up_rust::{UListener, UMessage, UMessageBuilder, UPayloadFormat};

#[derive(Default)]
struct Events(Mutex<Vec<(pb::GuardianEvent, Instant)>>);
#[async_trait]
impl UListener for Events {
    async fn on_receive(&self, message: UMessage) {
        if let Some(payload) = message.payload {
            self.0
                .lock()
                .unwrap()
                .push((pb::GuardianEvent::decode(payload).unwrap(), Instant::now()));
        }
    }
}
async fn restore_healthy_input(
    source_transport: &Arc<dyn up_rust::UTransport>,
    sequence: &mut u64,
    count: u32,
) {
    for i in 0..count {
        *sequence += 1;
        let sample = pb::BatteryTemperature {
            max_c: 35.0 + (i % 10) as f32 * 0.1,
            avg_c: 25.0,
            min_c: 20.0,
            source_timestamp_ms: *sequence * 100,
            sequence: *sequence,
            alive_counter: (*sequence % 256) as u32,
            quality: pb::Quality::Valid as i32,
        };
        source_transport
            .send(
                UMessageBuilder::publish(uri(BATTERY_TEMPERATURE))
                    .build_with_payload(
                        sample.encode_to_vec(),
                        UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn wait_for_runner(
    path: &std::path::Path,
    source: &Arc<dyn up_rust::UTransport>,
    sequence: &mut u64,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "runner did not create {}",
            path.display()
        );
        // Keep source timing independent of diagnostic-container pause/resume latency.
        restore_healthy_input(source, sequence, 1).await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the real isolated DFM/OpenSOVD Compose stack"]
async fn diagnostic_campaign() {
    let scenario = std::env::var("SCENARIO").unwrap_or_else(|_| "freshness".into());
    let evidence = PathBuf::from(std::env::var("EVIDENCE_DIR").unwrap_or_else(|_| "/tmp".into()));
    let configuration = DiagnosticsConfig::from_env();
    let session = configuration.session_id.clone();
    let url = format!(
        "{}/apps/{}/faults",
        std::env::var("SOVD_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:7690/sovd/v1".into())
            .trim_end_matches('/'),
        configuration.entity
    );
    let diagnostics = Diagnostics::start(configuration).unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(response) = client.get(&url).send().await {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                if json["items"].as_array().is_some_and(|a| a.len() == 4) {
                    break;
                }
            }
        }
        assert!(Instant::now() < deadline, "catalog never became available");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Baseline has no active faults, but is untested, not a falsely claimed PASS.
    let baseline: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert!(baseline["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["status"]["testFailed"] == false));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("tcp/127.0.0.1:{}", listener.local_addr().unwrap().port());
    drop(listener);
    let guardian_transport = transport::open(
        GUARDIAN_EVENTS.authority,
        &ZenohEndpoints {
            listen: vec![endpoint.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let source_transport = transport::open(
        BATTERY_TEMPERATURE.authority,
        &ZenohEndpoints {
            connect: vec![endpoint],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let events = Arc::new(Events::default());
    source_transport
        .register_listener(&uri(GUARDIAN_EVENTS), None, events.clone())
        .await
        .unwrap();
    if scenario != "outage" {
        std::fs::write(evidence.join("ready"), "ready").unwrap();
    }
    let observed = Arc::new(Mutex::new(std::collections::BTreeMap::<
        u64,
        (serde_json::Value, Instant),
    >::new()));
    let observations = Arc::clone(&observed);
    let poll_client = client.clone();
    let poll_url = url.clone();
    let poll_session = session.clone();
    let poller = tokio::spawn(async move {
        loop {
            for code in guardian_service::diagnostics::FAULTS {
                if let Ok(response) = poll_client
                    .get(format!("{}/{}", poll_url, code.dtc()))
                    .send()
                    .await
                {
                    if let Ok(json) = response.json::<serde_json::Value>().await {
                        if json["environment_data"]["session_id"] == poll_session {
                            if let Some(id) = json["environment_data"]["event_id"]
                                .as_str()
                                .and_then(|v| v.parse::<u64>().ok())
                            {
                                observations
                                    .lock()
                                    .unwrap()
                                    .entry(id)
                                    .or_insert((json, Instant::now()));
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });
    let config =
        GuardianConfig::from_toml_str(include_str!("../../config/guardian/safety-params.toml"))
            .unwrap();
    let (stop, stopped) = oneshot::channel();
    let service = tokio::spawn(async move {
        runtime::run_with_diagnostics(guardian_transport, &config, Some(diagnostics), async {
            let _ = stopped.await;
        })
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut sequence = 0_u64;
    if scenario == "outage" {
        // Complete initial monitor tests before pausing diagnostics. The upstream
        // IPC subscriber has a small overwrite buffer: this campaign isolates the
        // ordered Failed/Passed pair rather than startup-record overflow.
        restore_healthy_input(&source_transport, &mut sequence, 55).await;
        assert_eq!(
            events
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(e, _)| matches!(
                    e.kind,
                    Some(pb::guardian_event::Kind::FaultTestPassed(_))
                ))
                .count(),
            4
        );
        std::fs::write(evidence.join("ready"), "healthy baseline established").unwrap();
        wait_for_runner(&evidence.join("inject"), &source_transport, &mut sequence).await;
    }
    let sample_count = if scenario == "stuck" { 45 } else { 12 };
    let mut last_sent = Instant::now();
    for i in 0..sample_count {
        sequence += 1;
        let reference = (i % 10) as f32;
        let sample = pb::BatteryTemperature {
            max_c: if scenario == "stuck" {
                40.0
            } else {
                35.0 + reference * 0.1
            },
            avg_c: 25.0 + reference,
            min_c: 20.0 + reference,
            source_timestamp_ms: sequence * 100,
            sequence,
            alive_counter: if scenario == "counter" && i >= 5 {
                5
            } else {
                (sequence % 256) as u32
            },
            quality: if scenario == "quality" && i >= 5 {
                pb::Quality::Invalid as i32
            } else {
                pb::Quality::Valid as i32
            },
        };
        let message = UMessageBuilder::publish(uri(BATTERY_TEMPERATURE))
            .build_with_payload(
                sample.encode_to_vec(),
                UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
            )
            .unwrap();
        source_transport.send(message).await.unwrap();
        last_sent = Instant::now();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let expected = match scenario.as_str() {
        "stuck" => "BTG_TempSignalStuck",
        "counter" => "BTG_TempCounterStuck",
        "quality" => "BTG_TempQualityInvalid",
        _ => "BTG_TempFreshnessLost",
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let (fault, degraded, mitigation) = loop {
        let timed = events.0.lock().unwrap().clone();
        let captured: Vec<_> = timed.iter().map(|(e, _)| e.clone()).collect();
        let fault = captured.iter().find(|e| matches!(&e.kind, Some(pb::guardian_event::Kind::FaultDetected(f)) if f.dtc == expected));
        if let Some(fault) = fault {
            let degraded = captured.iter().find(|e| {
                e.cause_event_id == fault.event_id
                    && matches!(
                        e.kind,
                        Some(pb::guardian_event::Kind::MonitoringStatusChanged(_))
                    )
            });
            if let Some(degraded) = degraded {
                let mitigation = captured.iter().find(|e| e.cause_event_id == degraded.event_id && matches!(&e.kind, Some(pb::guardian_event::Kind::MitigationRequested(m)) if m.mitigation == pb::Mitigation::DriverWarningMonitoringUnavailable as i32));
                if let Some(mitigation) = mitigation {
                    break (fault.clone(), degraded.clone(), mitigation.clone());
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "missing detection/mitigation cause chain"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(fault.session_id, session);
    assert_eq!(mitigation.session_id, session);
    assert!(
        mitigation
            .guardian_time_ms
            .saturating_sub(fault.guardian_time_ms)
            <= 100
    );
    if scenario != "stuck" {
        assert!(
            last_sent.elapsed() <= Duration::from_millis(600),
            "freshness detection exceeded budget"
        );
    }
    std::fs::write(evidence.join("mitigated"), "mitigation request observed").unwrap();
    if scenario == "outage" {
        assert!(
            observed.lock().unwrap().get(&fault.event_id).is_none(),
            "fault must not be visible while diagnostics services are paused"
        );
        // Recover through the safety input while diagnostics remain paused.
        // Failed and Passed must both be delivered in order after resumption.
        restore_healthy_input(&source_transport, &mut sequence, 55).await;
        assert!(events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(e, _)| matches!(&e.kind,
            Some(pb::guardian_event::Kind::FaultRecovered(f)) if f.dtc == expected)));
        std::fs::write(evidence.join("recovery-queued"), "fault recovery observed").unwrap();
        wait_for_runner(
            &evidence.join("recovered"),
            &source_transport,
            &mut sequence,
        )
        .await;
        // Continue the healthy stream while outstanding paused HTTP requests expire.
        restore_healthy_input(&source_transport, &mut sequence, 15).await;
    }
    let started = Instant::now();
    let (diagnostic, visible_at) = loop {
        if let Some(observation) = observed.lock().unwrap().get(&fault.event_id).cloned() {
            break observation;
        }
        assert!(
            started.elapsed() < Duration::from_millis(2000),
            "diagnostic visibility exceeded budget"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let diagnostics_recovery_visibility_ms = started.elapsed().as_millis();
    let detected_at = events
        .0
        .lock()
        .unwrap()
        .iter()
        .find(|(e, _)| e.event_id == fault.event_id)
        .unwrap()
        .1;
    let visibility_ms = visible_at
        .saturating_duration_since(detected_at)
        .as_millis();
    if scenario != "outage" {
        assert!(
            visibility_ms <= 2000,
            "diagnostic visibility exceeded budget: {visibility_ms}ms"
        );
    }
    let detail = match &fault.kind {
        Some(pb::guardian_event::Kind::FaultDetected(f)) => f,
        _ => unreachable!(),
    };
    assert_eq!(
        diagnostic["environment_data"]["requirement"],
        detail.requirement
    );
    assert_eq!(
        diagnostic["environment_data"]["guardian_time_ms"],
        fault.guardian_time_ms.to_string()
    );
    let sample = detail.last_sample.as_ref().unwrap();
    assert_eq!(
        diagnostic["environment_data"]["sequence"],
        sample.sequence.to_string()
    );
    assert_eq!(
        diagnostic["environment_data"]["source_time_ms"],
        sample.source_timestamp_ms.to_string()
    );
    assert_eq!(
        diagnostic["environment_data"]["alive_counter"],
        sample.alive_counter.to_string()
    );
    if scenario != "outage" {
        restore_healthy_input(&source_transport, &mut sequence, 55).await;
    }
    let recovered = events.0.lock().unwrap().iter().map(|(e, _)| e)
        .find(|e| matches!(&e.kind, Some(pb::guardian_event::Kind::FaultRecovered(f)) if f.dtc == expected))
        .expect("missing fault recovery event").clone();
    assert_eq!(recovered.cause_event_id, fault.event_id);
    assert_eq!(recovered.session_id, session);
    assert!(events.0.lock().unwrap().iter().any(|(e, _)| matches!(&e.kind,
        Some(pb::guardian_event::Kind::MonitoringStatusChanged(m)) if m.previous == pb::MonitoringStatus::Degraded as i32 && m.current == pb::MonitoringStatus::Ok as i32)));
    let deadline = Instant::now() + Duration::from_secs(2);
    let cleared = loop {
        let json: serde_json::Value = client
            .get(format!("{url}/{expected}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if json["status"]["testFailed"] == false
            && json["environment_data"] == diagnostic["environment_data"]
        {
            break json;
        }
        assert!(
            Instant::now() < deadline,
            "Passed state never confirmed in OpenSOVD"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(cleared["status"]["testFailed"], false);
    assert_eq!(
        cleared["status"]["testFailedSinceLastClear"], true,
        "fault history must survive recovery"
    );
    assert_eq!(
        cleared["occurrence_counter"],
        diagnostic["occurrence_counter"]
    );
    assert_eq!(
        cleared["environment_data"], diagnostic["environment_data"],
        "DFM retains original failure provenance on Passed"
    );
    // Verify real service thermal recovery in addition to input-fault recovery.
    for i in 0..60 {
        sequence += 1;
        let sample = pb::BatteryTemperature {
            max_c: if i == 0 { 55.0 } else { 35.0 },
            avg_c: 25.0,
            min_c: 20.0,
            source_timestamp_ms: sequence * 100,
            sequence,
            alive_counter: (sequence % 256) as u32,
            quality: pb::Quality::Valid as i32,
        };
        source_transport
            .send(
                UMessageBuilder::publish(uri(BATTERY_TEMPERATURE))
                    .build_with_payload(
                        sample.encode_to_vec(),
                        UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF,
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let recovered_thermal = events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| {
                matches!(&e.kind,
            Some(pb::guardian_event::Kind::ThermalStateChanged(t)) if t.current < t.previous)
            })
            .count();
        if recovered_thermal == 2 {
            break;
        }
    }
    let captured = events.0.lock().unwrap().clone();
    std::fs::write(evidence.join("events.log"), format!("{captured:#?}")).unwrap();
    let lowering: Vec<_> = captured
        .iter()
        .filter_map(|(e, _)| match &e.kind {
            Some(pb::guardian_event::Kind::ThermalStateChanged(t)) if t.current < t.previous => {
                Some((t.previous, t.current))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        lowering,
        vec![
            (
                pb::ThermalState::Critical as i32,
                pb::ThermalState::Warning as i32
            ),
            (
                pb::ThermalState::Warning as i32,
                pb::ThermalState::Monitoring as i32
            )
        ]
    );
    let report = serde_json::json!({"scenario":scenario,"session_id":session,"hazard":"Thermal danger goes undetected when temperature input is unreliable", "safety_goal":"SG-2", "injected_fault":if scenario=="stuck" {"maximum frozen while reference signals change"} else {"source stops publishing"}, "diagnostics_outage":scenario=="outage", "detection":{"dtc":expected,"requirement":detail.requirement,"event_id":fault.event_id,"at_ms":fault.guardian_time_ms}, "degraded_event_id":degraded.event_id,"mitigation":{"event_id":mitigation.event_id,"cause_event_id":mitigation.cause_event_id,"at_ms":mitigation.guardian_time_ms,"action":"monitoring unavailable warning REQUESTED; actuator effect not tested"},"thermal_recovery":lowering,"fault_recovery":{"event_id":recovered.event_id,"at_ms":recovered.guardian_time_ms,"diagnostic":cleared},"diagnostic":diagnostic,"diagnostic_visibility_ms":visibility_ms,"recovery_visibility_ms":diagnostics_recovery_visibility_ms,"verdict":"PASS"});
    std::fs::write(
        evidence.join("report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    poller.abort();
    stop.send(()).unwrap();
    service.await.unwrap().unwrap();
}
