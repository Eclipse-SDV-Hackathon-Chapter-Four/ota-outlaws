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

//! Asynchronous fault reporting. Only OpenSOVD readback confirms delivery.
use common::{
    fault::{FaultId, LifecyclePhase, LifecycleStage},
    ids::SourceId,
    types::MetadataVec,
};
use fault_lib::{
    catalog::FaultCatalogBuilder,
    reporter::{Reporter, ReporterApi, ReporterConfig},
    utils::to_static_short_string,
    FaultApi,
};
use guardian::{Event, EventKind, FaultCode};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tracing::{info, warn};

pub const FAULTS: [FaultCode; 4] = FaultCode::ALL;
const QUEUE_CAPACITY: usize = 16;

#[derive(Clone)]
pub struct DiagnosticsConfig {
    pub catalog: PathBuf,
    pub entity: String,
    /// Base including /sovd/v1, not the fault collection path.
    pub sovd_url: String,
    pub session_id: String,
}
impl DiagnosticsConfig {
    pub fn from_env() -> Self {
        Self {
            catalog: std::env::var("FAULT_CATALOG")
                .unwrap_or_else(|_| "diagnostics/catalog/battery_guardian.json".into())
                .into(),
            entity: std::env::var("SOVD_ENTITY").unwrap_or_else(|_| "battery_guardian".into()),
            sovd_url: std::env::var("SOVD_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:7690/sovd/v1".into()),
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.entity.is_empty()
                && self.entity.len() <= 64
                && self
                    .entity
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "invalid SOVD_ENTITY"
        );
        anyhow::ensure!(
            !self.session_id.is_empty() && self.session_id.len() <= 64,
            "invalid session ID"
        );
        let catalog = FaultCatalogBuilder::new()
            .json_file(self.catalog.clone())
            .map_err(|e| anyhow::anyhow!("catalog: {e:?}"))?
            .build();
        let _ = catalog;
        // Catch mismatches before launching the safety service.
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&self.catalog)?)?;
        anyhow::ensure!(
            json["id"] == self.entity,
            "catalog ID must equal SOVD_ENTITY"
        );
        let ids: std::collections::BTreeSet<_> = json["faults"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing faults"))?
            .iter()
            .filter_map(|f| f["id"]["Text"].as_str())
            .collect();
        anyhow::ensure!(
            ids == FAULTS.iter().map(|f| f.dtc()).collect(),
            "catalog must contain exactly the core fault codes"
        );
        anyhow::ensure!(
            reqwest::Url::parse(&self.sovd_url)?.scheme() == "http",
            "this IPC deployment uses an HTTP gateway URL"
        );
        Ok(())
    }
}
#[derive(Default)]
pub struct DiagnosticsStats {
    pub confirmed: AtomicU64,
    pub rejected: AtomicU64,
    pub overdue: AtomicU64,
    stop: AtomicBool,
}
/// Nonblocking producer. Overflow is explicit; never silently claim delivery.
pub struct Diagnostics {
    tx: mpsc::SyncSender<(Event, Instant)>,
    pub stats: Arc<DiagnosticsStats>,
    pub session_id: String,
}
impl Diagnostics {
    pub fn start(config: DiagnosticsConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let stats = Arc::new(DiagnosticsStats::default());
        let worker_stats = Arc::clone(&stats);
        let session_id = config.session_id.clone();
        thread::Builder::new()
            .name("guardian-diagnostics".into())
            .spawn(move || {
                if let Err(error) = worker(config, rx, &worker_stats) {
                    warn!(%error, "diagnostics worker failed");
                    worker_stats.rejected.fetch_add(1, Ordering::Relaxed);
                }
            })?;
        Ok(Self {
            tx,
            stats,
            session_id,
        })
    }
    pub fn report(&self, event: &Event) {
        if !matches!(
            event.kind,
            EventKind::FaultDetected { .. }
                | EventKind::FaultRecovered { .. }
                | EventKind::FaultTestPassed { .. }
        ) {
            return;
        }
        if let Err(error) = self.tx.try_send((event.clone(), Instant::now())) {
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            warn!(id=event.id.0, %error, "diagnostic evidence rejected");
        }
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        self.stats.stop.store(true, Ordering::Relaxed);
    }
}

pub fn metadata(event: &Event, session: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("session_id".into(), session.into()),
        ("event_id".into(), event.id.0.to_string()),
        ("guardian_time_ms".into(), event.at.0.to_string()),
    ]);
    let details = match event.kind {
        EventKind::FaultDetected { fault, last_sample } => Some((fault, last_sample)),
        EventKind::FaultRecovered { fault, trigger }
        | EventKind::FaultTestPassed { fault, trigger } => Some((fault, Some(trigger))),
        _ => None,
    };
    if let Some((fault, last_sample)) = details {
        env.insert("requirement".into(), fault.requirement().into());
        if let Some(sample) = last_sample {
            env.insert("sequence".into(), sample.sequence.to_string());
            env.insert(
                "source_time_ms".into(),
                sample.source_timestamp_ms.to_string(),
            );
            env.insert("alive_counter".into(), sample.alive_counter.to_string());
        }
    }
    env
}
fn worker(
    config: DiagnosticsConfig,
    rx: mpsc::Receiver<(Event, Instant)>,
    stats: &DiagnosticsStats,
) -> anyhow::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(250))
        .build()?;
    let _api = loop {
        if stats.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let catalog = FaultCatalogBuilder::new()
            .json_file(config.catalog.clone())
            .map_err(|e| anyhow::anyhow!("catalog: {e:?}"))?
            .build();
        match FaultApi::try_new(catalog) {
            Ok(api) => break api,
            Err(error) => {
                warn!(?error, "DFM unavailable; safety service continues");
                thread::sleep(Duration::from_millis(100));
            }
        }
    };
    let reporter_config = ReporterConfig {
        source: SourceId {
            entity: to_static_short_string("BatteryThermalGuardian")?,
            ecu: None,
            domain: None,
            sw_component: None,
            instance: Some(to_static_short_string(&config.session_id)?),
        },
        lifecycle_phase: LifecyclePhase::Running,
        default_env_data: MetadataVec::new(),
    };
    let mut reporters = BTreeMap::new();
    for fault in FAULTS {
        let mut reporter = Reporter::new(
            &FaultId::Text(to_static_short_string(fault.dtc())?),
            reporter_config.clone(),
        )?;
        // NotTested preserves uncertainty; startup does not claim a healthy monitor test.
        let record = reporter.create_record(LifecycleStage::NotTested);
        reporter
            .publish(&config.entity, record)
            .map_err(|e| anyhow::anyhow!("baseline: {e:?}"))?;
        reporters.insert(fault, reporter);
    }
    let mut pending: VecDeque<(Event, Instant, bool)> = VecDeque::new();
    loop {
        if stats.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let received = if pending.len() < QUEUE_CAPACITY {
            rx.recv_timeout(Duration::from_millis(50))
        } else {
            thread::sleep(Duration::from_millis(50));
            Err(mpsc::RecvTimeoutError::Timeout)
        };
        match received {
            Ok((event, queued_at)) => {
                // Keep Failed -> Passed -> Failed ordered, including across an outage.
                pending.push_back((event, queued_at, false));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some((event, queued_at, overdue)) = pending.front_mut() {
            let (fault, failed) = match event.kind {
                EventKind::FaultDetected { fault, .. } => (fault, true),
                EventKind::FaultRecovered { fault, .. }
                | EventKind::FaultTestPassed { fault, .. } => (fault, false),
                _ => unreachable!("only fault lifecycle events are queued"),
            };
            let env = metadata(event, &config.session_id);
            let reporter = reporters.get_mut(&fault).expect("catalog validated");
            let mut record = reporter.create_record(if failed {
                LifecycleStage::Failed
            } else {
                LifecycleStage::Passed
            });
            let pairs: Vec<_> = env
                .iter()
                .map(|(k, v)| {
                    (
                        to_static_short_string(k).expect("short metadata key"),
                        to_static_short_string(v).expect("short metadata value"),
                    )
                })
                .collect();
            record.env_data =
                MetadataVec::try_from(pairs.as_slice()).expect("at most seven metadata entries");
            if let Err(error) = reporter.publish(&config.entity, record) {
                warn!(?error, "DFM enqueue failed; retrying");
            }
            let url = format!(
                "{}/apps/{}/faults/{}",
                config.sovd_url.trim_end_matches('/'),
                config.entity,
                fault.dtc()
            );
            let readback = client
                .get(url)
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json::<serde_json::Value>());
            let confirmed = readback.as_ref().is_ok_and(|v| {
                if failed {
                    v["status"]["testFailed"] == true
                        && env
                            .iter()
                            .all(|(k, value)| v["environment_data"][k] == value.as_str())
                } else {
                    // DFM intentionally retains failure environment data on Passed.
                    // Confirm recovery against its original detected fault, not replacement metadata.
                    v["status"]["testFailed"] == false
                        && match event.kind {
                            EventKind::FaultRecovered { .. } => event.cause.is_some_and(|cause| {
                                v["environment_data"]["session_id"] == config.session_id
                                    && v["environment_data"]["event_id"]
                                        .as_str()
                                        .and_then(|id| id.parse::<u64>().ok())
                                        == Some(cause.0)
                            }),
                            EventKind::FaultTestPassed { .. } => true,
                            _ => false,
                        }
                }
            });
            if confirmed {
                stats.confirmed.fetch_add(1, Ordering::Relaxed);
                info!(event_id=event.id.0, dtc=fault.dtc(), failed, session_id=%config.session_id, visibility_ms=queued_at.elapsed().as_millis(), "diagnostic readback confirmed");
                pending.pop_front();
                continue;
            }
            if !*overdue && queued_at.elapsed() > Duration::from_millis(200) {
                *overdue = true;
                stats.overdue.fetch_add(1, Ordering::Relaxed);
                warn!(
                    event_id = event.id.0,
                    dtc = fault.dtc(),
                    "diagnostic confirmation overdue; retaining evidence and retrying"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guardian::{EventId, Millis};
    #[test]
    fn full_queue_rejects_evidence_without_blocking() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let diagnostics = Diagnostics {
            tx,
            stats: Arc::new(DiagnosticsStats::default()),
            session_id: "test".into(),
        };
        let event = Event {
            id: EventId(1),
            cause: None,
            at: Millis(300),
            kind: EventKind::FaultDetected {
                fault: FaultCode::FreshnessLost,
                last_sample: None,
            },
        };
        diagnostics.report(&event);
        diagnostics.report(&event);
        assert_eq!(diagnostics.stats.rejected.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn shipped_catalog_is_exactly_the_core_fault_set() {
        DiagnosticsConfig {
            catalog: "../diagnostics/catalog/battery_guardian.json".into(),
            entity: "battery_guardian".into(),
            sovd_url: "http://localhost:7690/sovd/v1".into(),
            session_id: "test".into(),
        }
        .validate()
        .unwrap();
    }
}
