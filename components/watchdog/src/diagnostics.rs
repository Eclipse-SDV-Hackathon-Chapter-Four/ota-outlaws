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

//! DFM reporting of `BTG_GuardianHeartbeatLoss`.
//!
//! Same interface as the Guardian's own reporting
//! (`components/guardian-service/src/diagnostics.rs`): `fault_lib` over iceoryx2 IPC to
//! DFM, under the same SOVD entity, so the fault appears next to the
//! Guardian's faults in OpenSOVD. As there, an enqueue success does not
//! establish that DFM stored the record.

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
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tracing::{info, warn};

/// Diagnostic trouble code reported by the watchdog.
pub const DTC: &str = "BTG_GuardianHeartbeatLoss";
/// The requirement the watchdog implements.
pub const REQUIREMENT: &str = "FSR-2.7";
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
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&self.catalog)?)?;
        anyhow::ensure!(
            json["id"] == self.entity,
            "catalog ID must equal SOVD_ENTITY"
        );
        let present = json["faults"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing faults"))?
            .iter()
            .any(|f| f["id"]["Text"].as_str() == Some(DTC));
        anyhow::ensure!(present, "catalog must contain {DTC}");
        Ok(())
    }
}

/// What the watchdog observed about the Guardian.
#[derive(Debug, Clone)]
pub enum Report {
    /// First heartbeat after the watchdog started: the first healthy test.
    TestPassed { guardian: GuardianSeen },
    /// No heartbeat for longer than `T_hb`.
    Lost {
        last: Option<GuardianSeen>,
        silence_ms: u64,
    },
    /// Heartbeats arrive again, for example from a restarted Guardian.
    Recovered { guardian: GuardianSeen },
}

/// The last heartbeat the watchdog received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianSeen {
    pub session_id: String,
    pub sequence: u64,
}

/// Nonblocking producer. A full queue drops the report and logs it.
#[derive(Clone)]
pub struct Diagnostics {
    tx: mpsc::SyncSender<Report>,
    stop: Arc<AtomicBool>,
    pub session_id: String,
}

impl Diagnostics {
    pub fn start(config: DiagnosticsConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let session_id = config.session_id.clone();
        thread::Builder::new()
            .name("watchdog-diagnostics".into())
            .spawn(move || {
                if let Err(error) = worker(config, rx, &worker_stop) {
                    warn!(%error, "watchdog diagnostics worker failed");
                }
            })?;
        Ok(Self {
            tx,
            stop,
            session_id,
        })
    }

    pub fn report(&self, report: Report) {
        if let Err(error) = self.tx.try_send(report) {
            warn!(%error, "watchdog diagnostic report rejected");
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Lifecycle stage and environment data of a report.
pub fn record_content(
    report: &Report,
    watchdog_session: &str,
) -> (LifecycleStage, BTreeMap<String, String>) {
    let mut env = BTreeMap::from([
        ("requirement".to_owned(), REQUIREMENT.to_owned()),
        (
            "watchdog_session_id".to_owned(),
            watchdog_session.to_owned(),
        ),
    ]);
    let stage = match report {
        Report::TestPassed { guardian } | Report::Recovered { guardian } => {
            insert_guardian(&mut env, guardian);
            LifecycleStage::Passed
        }
        Report::Lost { last, silence_ms } => {
            if let Some(guardian) = last {
                insert_guardian(&mut env, guardian);
            }
            env.insert("silence_ms".into(), silence_ms.to_string());
            LifecycleStage::Failed
        }
    };
    (stage, env)
}

fn insert_guardian(env: &mut BTreeMap<String, String>, guardian: &GuardianSeen) {
    env.insert("guardian_session_id".into(), guardian.session_id.clone());
    env.insert("heartbeat_sequence".into(), guardian.sequence.to_string());
}

fn worker(
    config: DiagnosticsConfig,
    rx: mpsc::Receiver<Report>,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let _api = loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let catalog = FaultCatalogBuilder::new()
            .json_file(config.catalog.clone())
            .map_err(|e| anyhow::anyhow!("catalog: {e:?}"))?
            .build();
        match FaultApi::try_new(catalog) {
            Ok(api) => break api,
            Err(error) => {
                warn!(?error, "DFM unavailable; watchdog keeps watching");
                thread::sleep(Duration::from_millis(100));
            }
        }
    };
    let reporter_config = ReporterConfig {
        source: SourceId {
            entity: to_static_short_string("BatteryThermalGuardianWatchdog")?,
            ecu: None,
            domain: None,
            sw_component: None,
            instance: Some(to_static_short_string(&config.session_id)?),
        },
        lifecycle_phase: LifecyclePhase::Running,
        default_env_data: MetadataVec::new(),
    };
    let mut reporter = Reporter::new(
        &FaultId::Text(to_static_short_string(DTC)?),
        reporter_config,
    )?;
    // NotTested until the first heartbeat: startup does not claim a healthy Guardian.
    let record = reporter.create_record(LifecycleStage::NotTested);
    reporter
        .publish(&config.entity, record)
        .map_err(|e| anyhow::anyhow!("baseline: {e:?}"))?;

    let mut pending = None;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Retry only a rejected enqueue, so Failed -> Passed stays ordered.
        let report = match pending.take() {
            Some(report) => report,
            None => match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(report) => report,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            },
        };
        let (stage, env) = record_content(&report, &config.session_id);
        let mut record = reporter.create_record(stage);
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
            MetadataVec::try_from(pairs.as_slice()).expect("at most eight metadata entries");
        match reporter.publish(&config.entity, record) {
            Ok(()) => info!(dtc = DTC, ?stage, "DFM record enqueued"),
            Err(error) => {
                warn!(?error, "DFM enqueue rejected; retrying");
                pending = Some(report);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guardian() -> GuardianSeen {
        GuardianSeen {
            session_id: "g1".into(),
            sequence: 42,
        }
    }

    #[test]
    fn loss_is_failed_with_the_last_heartbeat_and_the_silence() {
        let (stage, env) = record_content(
            &Report::Lost {
                last: Some(guardian()),
                silence_ms: 1_600,
            },
            "w1",
        );

        assert_eq!(stage, LifecycleStage::Failed);
        assert_eq!(env["requirement"], "FSR-2.7");
        assert_eq!(env["watchdog_session_id"], "w1");
        assert_eq!(env["guardian_session_id"], "g1");
        assert_eq!(env["heartbeat_sequence"], "42");
        assert_eq!(env["silence_ms"], "1600");
    }

    #[test]
    fn loss_before_any_heartbeat_has_no_guardian_session() {
        let (stage, env) = record_content(
            &Report::Lost {
                last: None,
                silence_ms: 1_501,
            },
            "w1",
        );

        assert_eq!(stage, LifecycleStage::Failed);
        assert!(!env.contains_key("guardian_session_id"));
    }

    #[test]
    fn test_passed_and_recovery_are_passed() {
        for report in [
            Report::TestPassed {
                guardian: guardian(),
            },
            Report::Recovered {
                guardian: guardian(),
            },
        ] {
            assert_eq!(record_content(&report, "w1").0, LifecycleStage::Passed);
        }
    }

    #[test]
    fn shipped_catalog_contains_the_heartbeat_fault() {
        DiagnosticsConfig {
            catalog: "../../deploy/diagnostics/catalog/battery_guardian.json".into(),
            entity: "battery_guardian".into(),
            session_id: "test".into(),
        }
        .validate()
        .unwrap();
    }
}
