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

// AI-assisted: Codex / GPT-6.1 Sol (gpt-6.1-sol); Claude Code / Claude Opus 5.5 (claude-opus-5-5)

//! Asynchronous DFM fault reporting, independent of diagnostic verification.
//!
//! TODO: OpenSOVD readback, event correlation, visibility timing and persistence
//! confirmation belong in a future evidence collector. IPC enqueue success here
//! does not establish that DFM stored a record or OpenSOVD exposed it.
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
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tracing::{info, warn};

pub const FAULTS: [FaultCode; FaultCode::ALL.len()] = FaultCode::ALL;
const QUEUE_CAPACITY: usize = 16;

#[derive(Clone)]
pub struct DiagnosticsConfig {
    pub catalog: PathBuf,
    pub entity: String,
    pub session_id: String,
}
impl DiagnosticsConfig {
    pub fn from_env() -> Self {
        Self {
            catalog: std::env::var("FAULT_CATALOG")
                .unwrap_or_else(|_| "diagnostics/catalog/battery_guardian.json".into())
                .into(),
            entity: std::env::var("SOVD_ENTITY").unwrap_or_else(|_| "battery_guardian".into()),
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
        // The SOVD app is shared: the watchdog reports BTG_GuardianHeartbeatLoss
        // under the same entity, because a crashed Guardian cannot report its
        // own failure. So the catalog must contain every core fault code, and
        // may contain codes owned by other reporters.
        let missing: Vec<_> = FAULTS
            .iter()
            .map(|f| f.dtc())
            .filter(|dtc| !ids.contains(dtc))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "catalog is missing core fault codes: {missing:?}"
        );
        Ok(())
    }
}
#[derive(Default)]
pub struct DiagnosticsStats {
    pub rejected: AtomicU64,
    stop: AtomicBool,
}
/// Nonblocking producer. Overflow is explicit; never silently claim delivery.
pub struct Diagnostics {
    tx: mpsc::SyncSender<Event>,
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
        if let Err(error) = self.tx.try_send(event.clone()) {
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
    rx: mpsc::Receiver<Event>,
    stats: &DiagnosticsStats,
) -> anyhow::Result<()> {
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
    let mut pending = None;
    loop {
        if stats.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Retry only a rejected enqueue. Taking the next event after success
        // keeps Failed -> Passed -> Failed ordered without HTTP acknowledgment.
        let event = match pending.take() {
            Some(event) => event,
            None => match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            },
        };
        let (fault, failed) = match event.kind {
            EventKind::FaultDetected { fault, .. } => (fault, true),
            EventKind::FaultRecovered { fault, .. } | EventKind::FaultTestPassed { fault, .. } => {
                (fault, false)
            }
            _ => unreachable!("only fault lifecycle events are queued"),
        };
        let env = metadata(&event, &config.session_id);
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
        match reporter.publish(&config.entity, record) {
            Ok(()) => {
                info!(event_id=event.id.0, dtc=fault.dtc(), failed,
                    session_id=%config.session_id, "DFM record enqueued");
            }
            Err(error) => {
                warn!(
                    event_id = event.id.0,
                    ?error,
                    "DFM enqueue rejected; retrying"
                );
                pending = Some(event);
                thread::sleep(Duration::from_millis(50));
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
    fn shipped_catalog_contains_the_core_fault_set() {
        DiagnosticsConfig {
            catalog: "../diagnostics/catalog/battery_guardian.json".into(),
            entity: "battery_guardian".into(),
            session_id: "test".into(),
        }
        .validate()
        .unwrap();
    }
}
