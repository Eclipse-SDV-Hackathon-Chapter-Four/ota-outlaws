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

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5); Codex / GPT-6 (gpt-6)

//! Starts and stops campaigns.
//!
//! The campaign tool drives Docker Compose itself: one project per scenario,
//! with bind mounts relative to the repository and ports published on the
//! Docker host's `127.0.0.1`, which it then connects to. So it does not run
//! inside the dashboard. The dashboard starts its own image (which contains
//! the campaign tool and the Docker CLI) as a separate `campaign-runner`
//! container:
//!
//! - the repository mounted read-write at the same path the Docker daemon
//!   knows it by, so that Compose's bind mounts resolve on the daemon side;
//! - host networking, so that `127.0.0.1` is the Docker host;
//! - the Docker socket.
//!
//! The campaign tool runs exactly as from a shell on the host and writes its
//! evidence to `runs/`, where the campaign tab reads it. It gets the
//! diagnostics image of the running stack (`DIAGNOSTICS_IMAGE`): the runner
//! has no registry login, and the image the stack runs is there locally.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _};
use campaign::catalog::{ScenarioStatus, Stimulus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::parse_log_line;
use crate::docker::Docker;
use crate::taps::Entry;

pub const RUNNER: &str = "campaign-runner";
const CAMPAIGN_BINARY: &str = "/usr/local/bin/campaign";
const COMPOSE_PROJECT: &str = "com.docker.compose.project";

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    /// Scenario IDs; empty for all scenarios the tool can run on its own.
    #[serde(default)]
    pub scenarios: Vec<String>,
    /// Rebuild the Guardian and VSS Publisher images first (the tool's
    /// default); off adds `--no-build`.
    #[serde(default)]
    pub build: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioInfo {
    pub id: String,
    pub description: String,
    pub status: String,
    pub hazard: Option<String>,
    pub safety_goal: Option<String>,
}

pub struct Launcher {
    pub docker: Docker,
    /// The Compose project of the dashboard, whose DFM image the runner uses.
    project: String,
    /// The repository inside the dashboard container.
    pub repo: PathBuf,
    /// The image of the dashboard's own container.
    image: Option<String>,
    /// The repository as the Docker daemon resolves bind mounts.
    host_repo: Option<String>,
    /// The Docker socket on the Docker host.
    socket_source: String,
    /// `uid:gid` of the repository's owner, so the evidence belongs to the
    /// user; `None` runs as root (Docker Desktop shows mounts as root).
    user: Option<String>,
    /// The socket's group, so a non-root runner may use it.
    group_add: Vec<String>,
}

impl Launcher {
    /// Reads image and mounts from the dashboard's own container (`own`,
    /// from `docker inspect`). `HOST_REPO_DIR` overrides the repository's
    /// host path.
    pub fn new(docker: Docker, project: String, repo: PathBuf, socket: &Path, own: &Value) -> Self {
        let mount_source = |target: &Path| {
            own["Mounts"].as_array().and_then(|mounts| {
                mounts
                    .iter()
                    .find(|m| m["Destination"].as_str() == Some(&target.to_string_lossy()))
                    .and_then(|m| m["Source"].as_str().map(str::to_owned))
            })
        };
        let host_repo = std::env::var("HOST_REPO_DIR")
            .ok()
            .or_else(|| mount_source(&repo).map(|source| daemon_path(&source)));
        let socket_source = mount_source(socket)
            .map(|source| daemon_path(&source))
            .unwrap_or_else(|| "/var/run/docker.sock".to_owned());
        let (user, group_add) = owner(&repo, socket);
        Launcher {
            docker,
            project,
            repo,
            image: own["Config"]["Image"].as_str().map(str::to_owned),
            host_repo,
            socket_source,
            user,
            group_add,
        }
    }

    /// Why campaigns cannot be started from here, if they cannot.
    pub fn unavailable(&self) -> Option<String> {
        if !Path::new(CAMPAIGN_BINARY).is_file() {
            return Some(format!(
                "this dashboard image has no campaign tool ({CAMPAIGN_BINARY}); rebuild it"
            ));
        }
        if self.image.is_none() {
            return Some("cannot read the dashboard's own image".to_owned());
        }
        if self.host_repo.is_none() {
            return Some(format!(
                "the repository is not mounted at {}; set HOST_REPO_DIR",
                self.repo.display()
            ));
        }
        None
    }

    /// The scenarios the campaign tool can run on its own (not `external`).
    pub fn scenarios(&self) -> anyhow::Result<Vec<ScenarioInfo>> {
        self.scenarios_for(false)
    }

    pub fn scenarios_for(&self, opendut: bool) -> anyhow::Result<Vec<ScenarioInfo>> {
        let context = campaign::Context::load(&self.repo)?;
        Ok(context
            .catalog
            .scenarios
            .iter()
            .filter(|s| match &s.stimulus {
                Stimulus::External => false,
                Stimulus::CanTrace {
                    isolate, watchdog, ..
                } => {
                    if opendut {
                        !watchdog && isolate.as_deref().is_none_or(|s| s == "can-link")
                    } else {
                        isolate.as_deref() != Some("can-link")
                    }
                }
                _ => true,
            })
            .map(|s| ScenarioInfo {
                id: s.id.clone(),
                description: s.description.clone(),
                status: match s.status {
                    ScenarioStatus::Implemented => "implemented",
                    ScenarioStatus::Planned => "planned",
                }
                .to_owned(),
                hazard: s.hazard.clone(),
                safety_goal: s.safety_goal.clone(),
            })
            .collect())
    }

    /// When the campaign runner stopped, if it exists and is not running.
    pub async fn stopped_at(&self) -> Option<std::time::SystemTime> {
        let inspect = self.docker.inspect(RUNNER).await.ok()?;
        let state = &inspect["State"];
        if state["Running"].as_bool() != Some(false) {
            return None;
        }
        humantime::parse_rfc3339(state["FinishedAt"].as_str()?).ok()
    }

    pub async fn status(&self) -> Value {
        let mut status = json!({
            "available": self.unavailable().is_none(),
            "unavailable": self.unavailable(),
            "host_repo": self.host_repo,
            "running": false,
            "exists": false,
        });
        if let Ok(inspect) = self.docker.inspect(RUNNER).await {
            let state = &inspect["State"];
            status["exists"] = json!(true);
            status["running"] = json!(state["Running"].as_bool().unwrap_or(false));
            status["state"] = state["Status"].clone();
            status["exit_code"] = state["ExitCode"].clone();
            status["started_at"] = state["StartedAt"].clone();
            status["finished_at"] = state["FinishedAt"].clone();
            status["args"] = inspect["Config"]["Cmd"].clone();
            let lines = self.docker.logs(RUNNER, 300).await.unwrap_or_default();
            let entries: Vec<Entry> = lines
                .iter()
                .filter_map(|line| parse_log_line(line, RUNNER))
                .collect();
            status["log"] = json!(entries);
        }
        status
    }

    pub async fn start(&self, request: &StartRequest) -> anyhow::Result<Vec<String>> {
        if let Some(reason) = self.unavailable() {
            bail!(reason);
        }
        if let Ok(inspect) = self.docker.inspect(RUNNER).await {
            if inspect["State"]["Running"].as_bool() == Some(true) {
                bail!("a campaign is already running; stop it first");
            }
        }
        let known = self.scenarios()?;
        for id in &request.scenarios {
            if !known.iter().any(|s| &s.id == id) {
                bail!("unknown scenario {id}");
            }
        }
        let args = runner_args(request);
        // The previous runner's log is gone with it; its evidence stays in runs/.
        self.docker.remove(RUNNER).await?;
        let diagnostics_image = self.diagnostics_image().await;
        let config = self.runner_config(&args, diagnostics_image.as_deref());
        self.docker
            .create(RUNNER, &config)
            .await
            .context("cannot create the campaign runner")?;
        self.docker.start(RUNNER).await?;
        Ok(vec![format!("campaign {}", args.join(" "))])
    }

    /// The image of the running DFM container of the project, if any.
    async fn diagnostics_image(&self) -> Option<String> {
        let containers = self
            .docker
            .containers(&format!("com.docker.compose.project={}", self.project))
            .await
            .ok()?;
        let container = containers
            .iter()
            .find(|c| c["Labels"]["com.docker.compose.service"] == "opensovd-dfm")?;
        // The list API can return a raw image ID when its original tag has
        // moved. Compose needs the configured image reference, not that ID.
        let inspect = self.docker.inspect(container["Id"].as_str()?).await.ok()?;
        inspect["Config"]["Image"].as_str().map(str::to_owned)
    }

    fn runner_config(&self, args: &[String], diagnostics_image: Option<&str>) -> Value {
        let host_repo = self.host_repo.clone().unwrap_or_default();
        let mut env = vec!["HOME=/tmp".to_owned(), "RUST_LOG=info".to_owned()];
        if let Some(image) = diagnostics_image {
            env.push(format!("DIAGNOSTICS_IMAGE={image}"));
        }
        let mut config = json!({
            "Image": self.image,
            "Entrypoint": [CAMPAIGN_BINARY],
            "Cmd": args,
            "WorkingDir": host_repo,
            "Env": env,
            "Labels": { "ota-outlaws.role": RUNNER },
            "HostConfig": {
                "NetworkMode": "host",
                "Mounts": [
                    { "Type": "bind", "Source": host_repo, "Target": host_repo },
                    { "Type": "bind", "Source": self.socket_source, "Target": "/var/run/docker.sock" },
                ],
                "GroupAdd": self.group_add,
            },
        });
        if let Some(user) = &self.user {
            config["User"] = json!(user);
        }
        config
    }

    /// Stops the runner and removes what the interrupted scenario left
    /// behind: the Compose projects `campaign-*`.
    pub async fn stop(&self) -> anyhow::Result<Vec<String>> {
        let mut done = Vec::new();
        if self.docker.inspect(RUNNER).await.is_ok() {
            self.docker.stop(RUNNER).await?;
            done.push("campaign runner stopped".to_owned());
        }
        for container in self.docker.containers(COMPOSE_PROJECT).await? {
            let project = container["Labels"][COMPOSE_PROJECT].as_str().unwrap_or("");
            if project.starts_with("campaign-") {
                if let Some(id) = container["Id"].as_str() {
                    self.docker.remove(id).await?;
                    let name = container["Names"][0].as_str().unwrap_or(id);
                    done.push(format!(
                        "removed container {}",
                        name.trim_start_matches('/')
                    ));
                }
            }
        }
        for kind in ["networks", "volumes"] {
            for object in self.docker.labeled(kind, COMPOSE_PROJECT).await? {
                let project = object["Labels"][COMPOSE_PROJECT].as_str().unwrap_or("");
                if project.starts_with("campaign-") {
                    if let Some(name) = object["Name"].as_str() {
                        self.docker.remove_object(kind, name).await?;
                        done.push(format!("removed {} {name}", &kind[..kind.len() - 1]));
                    }
                }
            }
        }
        Ok(done)
    }
}

pub fn runner_args(request: &StartRequest) -> Vec<String> {
    let mut args = vec!["run".to_owned()];
    if request.scenarios.is_empty() {
        args.push("--all".to_owned());
    } else {
        args.extend(request.scenarios.iter().cloned());
    }
    if !request.build {
        args.push("--no-build".to_owned());
    }
    args
}

/// The path of a bind-mount source as the Docker daemon knows it. Docker
/// Desktop on Windows reports Windows paths; its daemon sees the drives under
/// `/run/desktop/mnt/host/<drive>`.
pub fn daemon_path(source: &str) -> String {
    let bytes = source.as_bytes();
    let windows = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if windows {
        format!(
            "/run/desktop/mnt/host/{}/{}",
            (bytes[0] as char).to_ascii_lowercase(),
            source[3..].replace('\\', "/")
        )
    } else {
        source.to_owned()
    }
}

#[cfg(unix)]
fn owner(repo: &Path, socket: &Path) -> (Option<String>, Vec<String>) {
    use std::os::unix::fs::MetadataExt as _;
    match std::fs::metadata(repo) {
        Ok(meta) if meta.uid() != 0 => {
            let group = std::fs::metadata(socket)
                .map(|s| vec![s.gid().to_string()])
                .unwrap_or_default();
            (Some(format!("{}:{}", meta.uid(), meta.gid())), group)
        }
        _ => (None, Vec::new()),
    }
}

#[cfg(not(unix))]
fn owner(_repo: &Path, _socket: &Path) -> (Option<String>, Vec<String>) {
    (None, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_become_docker_desktop_paths() {
        assert_eq!(
            daemon_path(r"C:\Users\me\ota-outlaws"),
            "/run/desktop/mnt/host/c/Users/me/ota-outlaws"
        );
        assert_eq!(daemon_path("/home/me/ota-outlaws"), "/home/me/ota-outlaws");
    }

    #[test]
    fn arguments_for_all_or_some_scenarios() {
        let all = StartRequest {
            scenarios: vec![],
            build: false,
        };
        assert_eq!(runner_args(&all), ["run", "--all", "--no-build"]);
        let some = StartRequest {
            scenarios: vec!["counter_stuck".into(), "timeout".into()],
            build: true,
        };
        assert_eq!(runner_args(&some), ["run", "counter_stuck", "timeout"]);
    }

    #[test]
    fn runner_mounts_the_repository_at_its_host_path() {
        let own = json!({
            "Config": {"Image": "ota-outlaws/dashboard:dev"},
            "Mounts": [
                {"Source": r"C:\work\ota-outlaws", "Destination": "/repo"},
                {"Source": "/var/run/docker.sock", "Destination": "/var/run/docker.sock"}
            ]
        });
        let launcher = Launcher::new(
            Docker::new("/nonexistent.sock"),
            "ota-outlaws".to_owned(),
            PathBuf::from("/repo"),
            Path::new("/var/run/docker.sock"),
            &own,
        );
        let config = launcher.runner_config(
            &runner_args(&StartRequest {
                scenarios: vec![],
                build: false,
            }),
            Some("local/opensovd-demo-fork:verified"),
        );
        let host = "/run/desktop/mnt/host/c/work/ota-outlaws";
        assert_eq!(config["WorkingDir"], host);
        assert_eq!(config["HostConfig"]["NetworkMode"], "host");
        assert_eq!(config["HostConfig"]["Mounts"][0]["Source"], host);
        assert_eq!(config["HostConfig"]["Mounts"][0]["Target"], host);
        assert_eq!(config["Image"], "ota-outlaws/dashboard:dev");
        assert_eq!(config["Cmd"], json!(["run", "--all", "--no-build"]));
        assert!(config["Env"].as_array().unwrap().contains(&json!(
            "DIAGNOSTICS_IMAGE=local/opensovd-demo-fork:verified"
        )));
    }

    #[test]
    fn the_shipped_catalog_lists_runnable_scenarios() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let launcher = Launcher::new(
            Docker::new("/nonexistent.sock"),
            "ota-outlaws".to_owned(),
            repo,
            Path::new("/var/run/docker.sock"),
            &Value::Null,
        );
        let scenarios = launcher.scenarios().unwrap();
        assert!(scenarios.iter().any(|s| s.id == "counter_stuck"));
        assert!(!scenarios.iter().any(|s| s.id == "can_link_interruption"));
        let remote = launcher.scenarios_for(true).unwrap();
        assert!(remote.iter().any(|s| s.id == "can_link_interruption"));
        assert!(remote.iter().any(|s| s.id == "source_dropout_replay"));
        for id in ["transport_dropout", "guardian_crash", "guardian_hang"] {
            assert!(!remote.iter().any(|s| s.id == id));
        }
    }
}
