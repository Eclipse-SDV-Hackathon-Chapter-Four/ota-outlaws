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
use crate::dtc::{Dtc, Mapper};
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
use guardian::{Event, EventKind};
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

/// Every DTC the Guardian writes, in catalog order.
pub fn codes() -> Vec<&'static str> {
    Dtc::all().into_iter().map(Dtc::code).collect()
}

/// Fault type (`category`) and severity of each DTC, from the catalog. The
/// DFM does not store the category in its records, so the Guardian writes
/// both into each record's environment data.
pub fn classification(
    catalog: &std::path::Path,
) -> anyhow::Result<BTreeMap<String, (String, String)>> {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(catalog)?)?;
    json["faults"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing faults"))?
        .iter()
        .map(|fault| {
            let code = fault["id"]["Text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("fault without text ID"))?;
            let field = |name: &str| {
                fault[name]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow::anyhow!("{code} has no {name}"))
            };
            Ok((code.to_owned(), (field("category")?, field("severity")?)))
        })
        .collect()
}
const QUEUE_CAPACITY: usize = 16;
/// Pause after each DFM record. The DFM drains its IPC subscriber every 10 ms,
/// and the subscriber keeps only two records, overwriting the oldest: a burst,
/// such as the startup baselines or several monitors passing at once, would
/// lose records. Off the safety path; a burst of ten takes 200 ms.
const RECORD_SPACING: Duration = Duration::from_millis(20);

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
                .unwrap_or_else(|_| "deploy/diagnostics/catalog/battery_guardian.json".into())
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
        // own failure. So the catalog must contain every Guardian DTC, and
        // may contain codes owned by other reporters.
        let missing: Vec<_> = codes()
            .into_iter()
            .filter(|dtc| !ids.contains(dtc))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "catalog is missing Guardian DTCs: {missing:?}"
        );
        classification(&self.catalog)?;
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
                | EventKind::ThermalStateChanged { .. }
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

/// Environment data of one DFM record, at most eight entries of at most 64
/// characters (`fault_lib` limits).
pub fn metadata(
    event: &Event,
    dtc: Dtc,
    session: &str,
    (fault_type, severity): &(String, String),
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("session_id".into(), session.into()),
        ("event_id".into(), event.id.0.to_string()),
        ("guardian_time_ms".into(), event.at.0.to_string()),
        ("requirement".into(), dtc.requirement().into()),
        ("fault_type".into(), fault_type.clone()),
        ("severity".into(), severity.clone()),
    ]);
    let sample = match event.kind {
        EventKind::FaultDetected { last_sample, .. } => last_sample,
        EventKind::FaultRecovered { trigger, .. }
        | EventKind::FaultTestPassed { trigger, .. }
        | EventKind::ThermalStateChanged { trigger, .. } => Some(trigger),
        _ => None,
    };
    if let Some(sample) = sample {
        env.insert(
            "sample".into(),
            format!(
                "seq={} src={} ctr={}",
                sample.sequence, sample.source_timestamp_ms, sample.alive_counter
            ),
        );
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
    let classes = classification(&config.catalog)?;
    let mut mapper = Mapper::default();
    let mut reporters = BTreeMap::new();
    for dtc in Dtc::all() {
        let mut reporter = Reporter::new(
            &FaultId::Text(to_static_short_string(dtc.code())?),
            reporter_config.clone(),
        )?;
        // NotTested preserves uncertainty; startup does not claim a healthy monitor test.
        let record = reporter.create_record(LifecycleStage::NotTested);
        reporter
            .publish(&config.entity, record)
            .map_err(|e| anyhow::anyhow!("baseline: {e:?}"))?;
        thread::sleep(RECORD_SPACING);
        reporters.insert(dtc, reporter);
    }
    let mut pending: std::collections::VecDeque<(Event, crate::dtc::Record)> = Default::default();
    loop {
        if stats.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Retry only a rejected enqueue. Taking the next record after success
        // keeps Failed -> Passed -> Failed ordered without HTTP acknowledgment.
        if pending.is_empty() {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => {
                    for record in mapper.records(&event) {
                        pending.push_back((event.clone(), record));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        let Some((event, record)) = pending.front() else {
            continue;
        };
        let (dtc, failed) = (record.dtc, record.failed);
        let class = classes
            .get(dtc.code())
            .expect("catalog validated against the Guardian's DTCs");
        let env = metadata(event, dtc, &config.session_id, class);
        let reporter = reporters.get_mut(&dtc).expect("catalog validated");
        let mut dfm_record = reporter.create_record(if failed {
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
        dfm_record.env_data =
            MetadataVec::try_from(pairs.as_slice()).expect("at most eight metadata entries");
        match reporter.publish(&config.entity, dfm_record) {
            Ok(()) => {
                info!(event_id=event.id.0, dtc=dtc.code(), failed,
                    session_id=%config.session_id, "DFM record enqueued");
                pending.pop_front();
                thread::sleep(RECORD_SPACING);
            }
            Err(error) => {
                warn!(
                    event_id = event.id.0,
                    ?error,
                    "DFM enqueue rejected; retrying"
                );
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
                fault: guardian::FaultCode::FreshnessLost,
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
            catalog: "../../deploy/diagnostics/catalog/battery_guardian.json".into(),
            entity: "battery_guardian".into(),
            session_id: "test".into(),
        }
        .validate()
        .unwrap();
    }
    #[test]
    fn shipped_catalog_names_the_requirement_of_each_dtc() {
        // The catalog's summary starts with the requirement that detects the
        // DTC, for example "FSR-3.2: …". It must match the Guardian's.
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string("../../deploy/diagnostics/catalog/battery_guardian.json")
                .unwrap(),
        )
        .unwrap();
        for dtc in Dtc::all() {
            let summary = json["faults"]
                .as_array()
                .unwrap()
                .iter()
                .find(|fault| fault["id"]["Text"] == dtc.code())
                .and_then(|fault| fault["summary"].as_str())
                .unwrap_or_else(|| panic!("{} has no summary", dtc.code()));
            assert!(
                summary.starts_with(&format!("{}:", dtc.requirement())),
                "{}: catalog says '{summary}', the Guardian {}",
                dtc.code(),
                dtc.requirement()
            );
        }
    }
    #[test]
    fn metadata_fits_the_dfm_limits_and_carries_the_classification() {
        let event = Event {
            id: EventId(u64::MAX),
            cause: None,
            at: Millis(u64::MAX),
            kind: EventKind::FaultRecovered {
                fault: guardian::FaultCode::RateImplausible,
                trigger: guardian::SampleRef {
                    sequence: u64::MAX,
                    source_timestamp_ms: u64::MAX,
                    alive_counter: 255,
                },
            },
        };
        let class = ("Configuration".to_owned(), "Error".to_owned());
        let env = metadata(
            &event,
            Dtc::Input(guardian::FaultCode::RateImplausible),
            &"s".repeat(64),
            &class,
        );

        assert!(env.len() <= 8);
        assert!(
            env.iter().all(|(k, v)| k.len() <= 64 && v.len() <= 64),
            "{env:?}"
        );
        assert_eq!(env["fault_type"], "Configuration");
        assert_eq!(env["severity"], "Error");
        assert_eq!(env["requirement"], "FSR-3.3");
    }
}
