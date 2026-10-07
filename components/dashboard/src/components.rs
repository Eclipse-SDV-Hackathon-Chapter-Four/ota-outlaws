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

//! The components of the Compose project, their state, memory, and settings
//! from Docker, and what their input and output logs are made of.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::docker::Docker;
use crate::taps::TapId;

const POLL: Duration = Duration::from_secs(2);
const COMPOSE_PROJECT: &str = "com.docker.compose.project";
const COMPOSE_SERVICE: &str = "com.docker.compose.service";

/// Where the lines of an input or output log come from.
#[derive(Debug, Clone, Copy)]
pub enum Source {
    /// Observed on the way: see [`crate::taps`].
    Tap(TapId),
    /// Lines of the container's own log that contain one of the patterns.
    Log(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub service: &'static str,
    pub title: &'static str,
    pub role: &'static str,
    pub input: &'static [Source],
    pub output: &'static [Source],
    /// Files inside the container shown with the settings.
    pub files: &'static [&'static str],
    /// Start order for "start all"; stop all goes the other way.
    pub rank: u8,
}

/// The components in the order of the signal chain.
pub const SPECS: &[Spec] = &[
    Spec {
        service: "kuksa-can-provider",
        title: "KUKSA CAN Provider",
        role: "Replays the recorded CAN trace and writes the decoded signals into the Data Broker",
        input: &[Source::Log(&[
            "canplayer",
            "Replayed",
            "dumpfile",
            "candump",
        ])],
        output: &[Source::Tap(TapId::Vss)],
        files: &[],
        rank: 4,
    },
    Spec {
        service: "kuksa-databroker",
        title: "KUKSA Data Broker",
        role: "Holds the VSS signals and serves them to subscribers over gRPC",
        input: &[Source::Tap(TapId::Vss)],
        output: &[
            Source::Tap(TapId::Vss),
            Source::Log(&["subscriber", "WARN", "ERROR"]),
        ],
        files: &[],
        rank: 1,
    },
    Spec {
        service: "vss-publisher",
        title: "VSS Publisher",
        role: "Turns the VSS signals into BatteryTemperature messages on uProtocol",
        input: &[Source::Tap(TapId::Vss)],
        output: &[Source::Tap(TapId::BatteryTemperature)],
        files: &[],
        rank: 5,
    },
    Spec {
        service: "zenoh",
        title: "Zenoh Router",
        role: "Routes the uProtocol messages between the services",
        input: &[
            Source::Tap(TapId::BatteryTemperature),
            Source::Tap(TapId::GuardianEvents),
        ],
        output: &[
            Source::Tap(TapId::BatteryTemperature),
            Source::Tap(TapId::GuardianEvents),
        ],
        files: &[],
        rank: 0,
    },
    Spec {
        service: "guardian",
        title: "Battery Thermal Guardian",
        role: "Judges temperature and data health, requests mitigations, reports faults to DFM",
        input: &[Source::Tap(TapId::BatteryTemperature)],
        output: &[
            Source::Tap(TapId::GuardianEvents),
            Source::Log(&["DFM record", "DFM unavailable"]),
        ],
        files: &["/etc/guardian/safety-params.toml"],
        rank: 6,
    },
    Spec {
        service: "watchdog",
        title: "Guardian Watchdog",
        role: "Watches the Guardian's heartbeat and reports its loss to DFM",
        input: &[Source::Log(&["heartbeat"])],
        output: &[Source::Log(&["DFM record", "DFM unavailable"])],
        files: &[],
        rank: 7,
    },
    Spec {
        service: "opensovd-dfm",
        title: "OpenSOVD DFM",
        role: "Diagnostic Fault Manager: receives the fault records and stores the fault states",
        input: &[Source::Log(&["Received new fault", "hash"])],
        output: &[
            Source::Tap(TapId::SovdFaults),
            Source::Log(&["stored", "ERROR", "WARN"]),
        ],
        files: &[],
        rank: 2,
    },
    Spec {
        service: "opensovd-gateway",
        title: "OpenSOVD Gateway",
        role: "Serves the stored faults over the SOVD REST API",
        input: &[Source::Log(&["Request finished"])],
        output: &[Source::Tap(TapId::SovdFaults)],
        files: &[],
        rank: 3,
    },
];

/// The input and output log of a service without a spec.
const UNKNOWN: Spec = Spec {
    service: "",
    title: "",
    role: "Not described by the dashboard; the logs are the container log",
    input: &[Source::Log(&[""])],
    output: &[Source::Log(&[""])],
    files: &[],
    rank: 9,
};

pub fn spec(service: &str) -> Spec {
    SPECS
        .iter()
        .find(|s| s.service == service)
        .copied()
        .unwrap_or(UNKNOWN)
}

#[derive(Debug, Clone, Serialize)]
pub struct Memory {
    pub usage_bytes: u64,
    pub limit_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub service: String,
    pub title: String,
    pub role: String,
    pub container: Option<String>,
    pub container_id: Option<String>,
    /// Docker state: running, exited, paused, restarting, created; or
    /// `missing` when the project has no container for the service.
    pub state: String,
    pub status: String,
    pub health: Option<String>,
    pub started_at: Option<String>,
    pub restart_count: u64,
    pub memory: Option<Memory>,
    pub settings: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub project: String,
    pub components: Vec<Component>,
    /// Compose projects of running campaign scenarios (`campaign-*`).
    pub campaign_projects: Vec<String>,
    /// The containers of the campaign scenarios, whatever their state.
    pub campaign_containers: Vec<CampaignContainer>,
    pub error: Option<String>,
    pub updated_ms: u64,
}

/// A container of a campaign scenario's Compose project.
#[derive(Debug, Clone, Serialize, Default)]
pub struct CampaignContainer {
    pub project: String,
    pub service: String,
    pub state: String,
    pub status: String,
}

pub struct Components {
    pub docker: Docker,
    pub project: String,
    /// The dashboard's own service, left out of the list.
    pub own_service: String,
    pub snapshot: RwLock<Snapshot>,
}

impl Components {
    pub fn spawn_poller(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let snapshot = this.collect().await;
                *this.snapshot.write().await = snapshot;
                tokio::time::sleep(POLL).await;
            }
        });
    }

    async fn collect(&self) -> Snapshot {
        let mut snapshot = Snapshot {
            project: self.project.clone(),
            updated_ms: crate::taps::now_ms(),
            ..Default::default()
        };
        let containers = match self
            .docker
            .containers(&format!("{COMPOSE_PROJECT}={}", self.project))
            .await
        {
            Ok(containers) => containers,
            Err(error) => {
                snapshot.error = Some(format!("{error:#}"));
                snapshot.components = SPECS.iter().map(|s| missing(s.service)).collect();
                return snapshot;
            }
        };
        let mut by_service: BTreeMap<String, Value> = BTreeMap::new();
        for container in containers {
            if let Some(service) = container["Labels"][COMPOSE_SERVICE].as_str() {
                if service != self.own_service {
                    by_service.insert(service.to_owned(), container);
                }
            }
        }
        let mut services: Vec<String> = SPECS.iter().map(|s| s.service.to_owned()).collect();
        services.extend(
            by_service
                .keys()
                .filter(|s| !SPECS.iter().any(|spec| spec.service == s.as_str()))
                .cloned(),
        );
        let futures = services.iter().map(|service| async {
            match by_service.get(service) {
                Some(container) => self.describe(service, container).await,
                None => missing(service),
            }
        });
        snapshot.components = futures_util::future::join_all(futures).await;
        // A service of the spec list that the project does not define
        // (for example the watchdog on a branch without it) is left out.
        snapshot
            .components
            .retain(|c| c.state != "missing" || by_service.is_empty() || is_core(&c.service));
        let campaign = self.campaign_containers().await;
        snapshot.campaign_projects = campaign_projects(&campaign);
        snapshot.campaign_containers = campaign;
        snapshot
    }

    async fn describe(&self, service: &str, container: &Value) -> Component {
        let id = container["Id"].as_str().unwrap_or_default().to_owned();
        let spec = spec(service);
        let state = container["State"].as_str().unwrap_or("unknown").to_owned();
        let inspect = self.docker.inspect(&id).await.unwrap_or(Value::Null);
        let memory = if state == "running" {
            self.docker
                .stats(&id)
                .await
                .ok()
                .and_then(|stats| memory(&stats))
        } else {
            None
        };
        Component {
            service: service.to_owned(),
            title: if spec.title.is_empty() {
                service.to_owned()
            } else {
                spec.title.to_owned()
            },
            role: spec.role.to_owned(),
            container: container["Names"][0]
                .as_str()
                .map(|n| n.trim_start_matches('/').to_owned()),
            container_id: Some(id),
            state,
            status: container["Status"].as_str().unwrap_or_default().to_owned(),
            health: inspect["State"]["Health"]["Status"]
                .as_str()
                .map(str::to_owned),
            started_at: inspect["State"]["StartedAt"].as_str().map(str::to_owned),
            restart_count: inspect["RestartCount"].as_u64().unwrap_or(0),
            memory,
            settings: settings(&inspect),
        }
    }

    async fn campaign_containers(&self) -> Vec<CampaignContainer> {
        let Ok(containers) = self.docker.containers(COMPOSE_PROJECT).await else {
            return Vec::new();
        };
        containers
            .iter()
            .filter_map(|c| {
                let project = c["Labels"][COMPOSE_PROJECT].as_str()?;
                project.starts_with("campaign-").then(|| CampaignContainer {
                    project: project.to_owned(),
                    service: c["Labels"][COMPOSE_SERVICE]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    state: c["State"].as_str().unwrap_or("unknown").to_owned(),
                    status: c["Status"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect()
    }

    /// The container of a service of this project, if it exists.
    pub async fn container_id(&self, service: &str) -> Option<String> {
        let snapshot = self.snapshot.read().await;
        snapshot
            .components
            .iter()
            .find(|c| c.service == service)
            .and_then(|c| c.container_id.clone())
    }

    /// Starts, stops, or restarts one service. After DFM starts, the
    /// services that share its IPC (or PID) namespace are restarted: they
    /// would otherwise keep the namespace of the stopped DFM.
    pub async fn act(&self, service: &str, action: &str) -> anyhow::Result<Vec<String>> {
        let Some(id) = self.container_id(service).await else {
            anyhow::bail!("{service} has no container; create it with docker compose up -d");
        };
        let mut done = vec![format!("{action} {service}")];
        match action {
            "start" => self.docker.start(&id).await?,
            "stop" => self.docker.stop(&id).await?,
            "restart" => self.docker.restart(&id).await?,
            other => anyhow::bail!("unknown action {other}"),
        }
        if action != "stop" {
            for (dependent, dependent_id) in self.namespace_dependents(&id).await {
                self.docker.restart(&dependent_id).await?;
                done.push(format!(
                    "restart {dependent} (shares the namespace of {service})"
                ));
            }
        }
        self.refresh().await;
        Ok(done)
    }

    /// Starts or stops every service, in start order or the reverse.
    pub async fn act_all(&self, action: &str) -> Vec<String> {
        let mut specs: Vec<(u8, String)> = {
            let snapshot = self.snapshot.read().await;
            snapshot
                .components
                .iter()
                .filter(|c| c.container_id.is_some())
                .map(|c| (spec(&c.service).rank, c.service.clone()))
                .collect()
        };
        specs.sort();
        if action == "stop" {
            specs.reverse();
        }
        let mut results = Vec::new();
        for (_, service) in specs {
            let Some(id) = self.container_id(&service).await else {
                continue;
            };
            let result = match action {
                "start" => self.docker.start(&id).await,
                "stop" => self.docker.stop(&id).await,
                other => Err(anyhow::anyhow!("unknown action {other}")),
            };
            results.push(match result {
                Ok(()) => format!("{action} {service}: ok"),
                Err(error) => format!("{action} {service}: {error:#}"),
            });
        }
        self.refresh().await;
        results
    }

    async fn refresh(&self) {
        let snapshot = self.collect().await;
        *self.snapshot.write().await = snapshot;
    }

    async fn namespace_dependents(&self, owner_id: &str) -> Vec<(String, String)> {
        let owner = format!("container:{owner_id}");
        let candidates: Vec<(String, String)> = {
            let snapshot = self.snapshot.read().await;
            snapshot
                .components
                .iter()
                .filter(|c| matches!(c.state.as_str(), "running" | "restarting"))
                .filter_map(|c| Some((c.service.clone(), c.container_id.clone()?)))
                .collect()
        };
        let mut dependents = Vec::new();
        for (service, id) in candidates {
            if id == owner_id {
                continue;
            }
            let Ok(inspect) = self.docker.inspect(&id).await else {
                continue;
            };
            let host = &inspect["HostConfig"];
            if host["IpcMode"] == owner.as_str() || host["PidMode"] == owner.as_str() {
                dependents.push((service, id));
            }
        }
        dependents
    }
}

/// Services every stack has; shown as missing when they are not created.
fn is_core(service: &str) -> bool {
    service != "watchdog"
}

fn missing(service: &str) -> Component {
    let spec = spec(service);
    Component {
        service: service.to_owned(),
        title: spec.title.to_owned(),
        role: spec.role.to_owned(),
        container: None,
        container_id: None,
        state: "missing".to_owned(),
        status: "no container".to_owned(),
        health: None,
        started_at: None,
        restart_count: 0,
        memory: None,
        settings: Value::Null,
    }
}

/// Memory in use without the page cache, as `docker stats` shows it.
pub fn memory(stats: &Value) -> Option<Memory> {
    let memory = &stats["memory_stats"];
    let usage = memory["usage"].as_u64()?;
    let cache = memory["stats"]["inactive_file"]
        .as_u64()
        .or_else(|| memory["stats"]["total_inactive_file"].as_u64())
        .unwrap_or(0);
    Some(Memory {
        usage_bytes: usage.saturating_sub(cache),
        limit_bytes: memory["limit"].as_u64().unwrap_or(0),
    })
}

/// The settings shown for a container: image, command, environment, ports,
/// restart policy, mounts, and namespaces.
pub fn settings(inspect: &Value) -> Value {
    if inspect.is_null() {
        return Value::Null;
    }
    let config = &inspect["Config"];
    let host = &inspect["HostConfig"];
    let join = |value: &Value| -> Option<String> {
        let parts: Vec<&str> = value.as_array()?.iter().filter_map(Value::as_str).collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    };
    let ports: Vec<String> = host["PortBindings"]
        .as_object()
        .map(|bindings| {
            bindings
                .iter()
                .flat_map(|(container_port, binds)| {
                    binds.as_array().into_iter().flatten().map(move |b| {
                        let ip = b["HostIp"].as_str().filter(|s| !s.is_empty());
                        let port = b["HostPort"].as_str().unwrap_or("");
                        match ip {
                            Some(ip) => format!("{ip}:{port} → {container_port}"),
                            None => format!("{port} → {container_port}"),
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mounts: Vec<String> = inspect["Mounts"]
        .as_array()
        .map(|mounts| {
            mounts
                .iter()
                .map(|m| {
                    let source = m["Name"]
                        .as_str()
                        .or_else(|| m["Source"].as_str())
                        .unwrap_or("");
                    let mode = if m["RW"].as_bool() == Some(false) {
                        " (read-only)"
                    } else {
                        ""
                    };
                    format!(
                        "{} {source} → {}{mode}",
                        m["Type"].as_str().unwrap_or(""),
                        m["Destination"].as_str().unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let env: Vec<String> = config["Env"]
        .as_array()
        .map(|env| {
            env.iter()
                .filter_map(Value::as_str)
                // Set by every base image; not a setting of the component.
                .filter(|e| !e.starts_with("PATH=") && !e.starts_with("HOSTNAME="))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({
        "image": config["Image"],
        "entrypoint": join(&config["Entrypoint"]),
        "command": join(&config["Cmd"]),
        "env": env,
        "ports": ports,
        "restart_policy": host["RestartPolicy"]["Name"],
        "mounts": mounts,
        "ipc_mode": host["IpcMode"],
        "pid_mode": host["PidMode"],
        "networks": inspect["NetworkSettings"]["Networks"]
            .as_object()
            .map(|n| n.keys().cloned().collect::<Vec<_>>()),
    })
}

/// The campaign projects with a running container, sorted.
fn campaign_projects(containers: &[CampaignContainer]) -> Vec<String> {
    let mut projects: Vec<String> = containers
        .iter()
        .filter(|c| c.state == "running")
        .map(|c| c.project.clone())
        .collect();
    projects.sort();
    projects.dedup();
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_excludes_the_page_cache() {
        let v2 = serde_json::json!({"memory_stats": {"usage": 1000, "limit": 4000, "stats": {"inactive_file": 200}}});
        let m = memory(&v2).unwrap();
        assert_eq!((m.usage_bytes, m.limit_bytes), (800, 4000));
        let v1 = serde_json::json!({"memory_stats": {"usage": 1000, "limit": 4000, "stats": {"total_inactive_file": 300}}});
        assert_eq!(memory(&v1).unwrap().usage_bytes, 700);
        assert!(memory(&serde_json::json!({"memory_stats": {}})).is_none());
    }

    #[test]
    fn settings_from_inspect() {
        let inspect = serde_json::json!({
            "Config": {"Image": "ota-outlaws/guardian:dev", "Cmd": ["--x", "1"],
                       "Env": ["PATH=/bin", "SOVD_ENTITY=battery_guardian"]},
            "HostConfig": {"RestartPolicy": {"Name": "unless-stopped"}, "IpcMode": "container:abc",
                           "PortBindings": {"7690/tcp": [{"HostIp": "127.0.0.1", "HostPort": "7690"}]}},
            "Mounts": [{"Type": "bind", "Source": "/repo/catalog", "Destination": "/catalog", "RW": false}]
        });
        let s = settings(&inspect);
        assert_eq!(s["command"], "--x 1");
        assert_eq!(
            s["env"],
            serde_json::json!(["SOVD_ENTITY=battery_guardian"])
        );
        assert_eq!(s["ports"][0], "127.0.0.1:7690 → 7690/tcp");
        assert_eq!(s["mounts"][0], "bind /repo/catalog → /catalog (read-only)");
        assert_eq!(s["restart_policy"], "unless-stopped");
    }

    #[test]
    fn diagnostics_start_before_their_reporters() {
        let rank = |s: &str| spec(s).rank;
        assert!(rank("opensovd-dfm") < rank("opensovd-gateway"));
        assert!(rank("opensovd-dfm") < rank("guardian"));
        assert!(rank("zenoh") < rank("vss-publisher"));
        assert!(rank("kuksa-databroker") < rank("kuksa-can-provider"));
    }
}
