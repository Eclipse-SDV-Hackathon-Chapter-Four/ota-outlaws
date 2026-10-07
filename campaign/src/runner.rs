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

//! Runs one scenario in a fresh Docker Compose project: starts the signal
//! chain and the diagnostics, starts the taps, replays the scenario's CAN
//! trace once, waits until it has ended, and removes the project.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use thermal_contract::transport::ZenohEndpoints;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::catalog::{Scenario, Stimulus};
use crate::record::{self, Recorder, SovdTap};
use crate::recording::Tap;

/// Services started before the stimulus. The Guardian follows once the
/// source delivers data: started earlier, it would rightly report that no
/// data arrived after its start (FSR-2.1).
const CHAIN: &[&str] = &[
    "zenoh",
    "kuksa-databroker",
    "opensovd-dfm",
    "opensovd-gateway",
    "vss-publisher",
];

/// How long to keep recording after the last sample, so that OpenSOVD can
/// catch up with the last Guardian events.
const TAIL: Duration = Duration::from_secs(3);

pub struct Settings {
    pub repo: PathBuf,
    pub fault_codes: Vec<String>,
    pub entity: String,
}

pub struct Compose {
    project: String,
    files: Vec<PathBuf>,
    env: Vec<(String, String)>,
    repo: PathBuf,
}

impl Compose {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("docker");
        command.arg("compose").arg("-p").arg(&self.project);
        for file in &self.files {
            command.arg("-f").arg(file);
        }
        command.args(args).current_dir(&self.repo);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    async fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        let output = self
            .command(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("cannot run docker compose")?;
        if !output.status.success() {
            bail!(
                "docker compose {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    async fn output(&self, args: &[&str]) -> anyhow::Result<String> {
        let output = self.command(args).output().await?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr))
    }

    /// Disconnects a service's container from the project network, or
    /// reconnects it.
    async fn network(&self, verb: &str, service: &str) -> anyhow::Result<()> {
        let container = self.output(&["ps", "-q", service]).await?;
        let container = container.lines().next().unwrap_or_default().trim();
        if container.is_empty() {
            bail!("service {service} is not running");
        }
        let network = format!("{}_default", self.project);
        let output = Command::new("docker")
            .args(["network", verb, &network, container])
            .output()
            .await?;
        if !output.status.success() {
            bail!(
                "docker network {verb} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

fn free_port() -> anyhow::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

/// Builds the images of the services the campaign changes. Done once per
/// campaign, not per scenario.
pub async fn build(repo: &Path) -> anyhow::Result<()> {
    let status = Command::new("docker")
        .args(["compose", "build", "guardian", "vss-publisher"])
        .current_dir(repo)
        .status()
        .await?;
    if !status.success() {
        bail!("docker compose build failed");
    }
    Ok(())
}

/// One injection beyond the trace, scheduled after the source started.
enum Action {
    Pause(Vec<String>, Duration),
    Stop(Vec<String>),
    Isolate(String, Duration),
}

struct Plan {
    /// Trace path in the repository, and its duration. `None`: no source.
    trace: Option<(String, Duration)>,
    /// Recording time without a source.
    no_source_for: Duration,
    actions: Vec<(Duration, Action)>,
}

fn plan(scenario: &Scenario, repo: &Path) -> anyhow::Result<Plan> {
    let ms = |value: Option<u64>| Duration::from_millis(value.unwrap_or(0));
    match &scenario.stimulus {
        Stimulus::External => bail!(
            "scenario {} has an external stimulus; use `campaign observe`",
            scenario.id
        ),
        Stimulus::NoSource { duration_ms } => Ok(Plan {
            trace: None,
            no_source_for: Duration::from_millis(*duration_ms),
            actions: Vec::new(),
        }),
        Stimulus::CanTrace {
            trace,
            pause,
            pause_after_ms,
            pause_for_ms,
            stop,
            stop_after_ms,
            isolate,
            isolate_after_ms,
            isolate_for_ms,
        } => {
            let path = repo.join(trace);
            if !path.is_file() {
                bail!("trace {} not found", path.display());
            }
            let mut actions = Vec::new();
            if !pause.is_empty() {
                actions.push((
                    ms(*pause_after_ms),
                    Action::Pause(pause.clone(), ms(*pause_for_ms)),
                ));
            }
            if !stop.is_empty() {
                actions.push((ms(*stop_after_ms), Action::Stop(stop.clone())));
            }
            if let Some(service) = isolate {
                actions.push((
                    ms(*isolate_after_ms),
                    Action::Isolate(service.clone(), ms(*isolate_for_ms)),
                ));
            }
            actions.sort_by_key(|(after, _)| *after);
            Ok(Plan {
                trace: Some((trace.clone(), trace_duration(&path)?)),
                no_source_for: Duration::ZERO,
                actions,
            })
        }
    }
}

/// Runs the scenario and records it into `recording`. Returns when the
/// recording is complete and the Compose project is removed.
pub async fn run(
    scenario: &Scenario,
    settings: &Settings,
    run_dir: &Path,
    recorder: Arc<Recorder>,
) -> anyhow::Result<()> {
    let plan = plan(scenario, &settings.repo)?;
    let zenoh_port = free_port()?;
    let sovd_port = free_port()?;
    let provider = match &plan.trace {
        Some((trace, _)) => format!(
            "\x20 kuksa-can-provider:\n\
             \x20   container_name: !reset null\n\
             \x20   restart: \"no\"\n\
             \x20   environment:\n\
             \x20     CANDUMP_FILE: \"/campaign/{trace}\"\n\
             \x20   command: [\"--dumpfile\", \"/campaign/{trace}\"]\n\
             \x20   volumes:\n\
             \x20     - \"{repo}:/campaign:ro\"\n",
            repo = settings.repo.display(),
        ),
        None => "\x20 kuksa-can-provider:\n\x20   container_name: !reset null\n".to_owned(),
    };
    let override_file = run_dir.join("compose.campaign.yml");
    std::fs::write(
        &override_file,
        format!(
            "# Generated by the campaign tool for scenario {id}.\n\
             services:\n\
             \x20 zenoh:\n\
             \x20   container_name: !reset null\n\
             \x20   ports: !override [\"127.0.0.1:{zenoh_port}:7447\"]\n\
             \x20 kuksa-databroker:\n\
             \x20   container_name: !reset null\n\
             \x20 vss-publisher:\n\
             \x20   container_name: !reset null\n\
             \x20 guardian:\n\
             \x20   container_name: !reset null\n\
             {provider}",
            id = scenario.id,
        ),
    )?;
    let compose = Compose {
        project: format!("campaign-{}", run_dir_name(run_dir)),
        files: vec![settings.repo.join("docker-compose.yml"), override_file],
        env: vec![
            ("SOVD_PORT".into(), sovd_port.to_string()),
            ("KUKSA_HOST_PORT".into(), free_port()?.to_string()),
        ],
        repo: settings.repo.clone(),
    };

    let result = drive(
        scenario, settings, &compose, recorder, zenoh_port, sovd_port, plan,
    )
    .await;

    let logs = compose
        .output(&["logs", "--no-color"])
        .await
        .unwrap_or_default();
    let _ = std::fs::write(run_dir.join("services.log"), logs);
    let _ = compose.run(&["down", "-v", "--remove-orphans"]).await;
    result
}

fn injection(recorder: &Recorder, action: &str, detail: String) {
    recorder.log(Tap::Injection {
        action: action.to_owned(),
        detail,
    });
}

async fn drive(
    scenario: &Scenario,
    settings: &Settings,
    compose: &Compose,
    recorder: Arc<Recorder>,
    zenoh_port: u16,
    sovd_port: u16,
    plan: Plan,
) -> anyhow::Result<()> {
    let mut chain = vec!["up", "-d", "--no-build"];
    chain.extend_from_slice(CHAIN);
    compose.run(&chain).await?;

    let sovd_url = format!("http://127.0.0.1:{sovd_port}/sovd/v1");
    wait_for_sovd(&sovd_url, &settings.entity).await?;

    let taps = record::start(
        Arc::clone(&recorder),
        &ZenohEndpoints {
            connect: vec![format!("tcp/127.0.0.1:{zenoh_port}")],
            mode: Some("client".to_owned()),
            ..Default::default()
        },
        Some(SovdTap {
            url: sovd_url,
            entity: settings.entity.clone(),
            codes: settings.fault_codes.clone(),
        }),
    )
    .await?;
    // Let the subscriptions reach the router before the first sample.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let source_started = Instant::now();
    if plan.trace.is_some() {
        injection(
            &recorder,
            "start_can_provider",
            format!("scenario {}", scenario.id),
        );
        compose
            .run(&["up", "-d", "--no-build", "kuksa-can-provider"])
            .await?;
        wait_for_samples(&recorder).await?;
    }

    injection(
        &recorder,
        crate::evaluate::GUARDIAN_START,
        "the Guardian starts".to_owned(),
    );
    compose.run(&["up", "-d", "--no-build", "guardian"]).await?;
    wait_for_log(compose, "guardian", "subscribed to battery temperature").await?;
    injection(
        &recorder,
        crate::evaluate::GUARDIAN_READY,
        "the Guardian has subscribed".to_owned(),
    );

    let Some((_, trace_duration)) = plan.trace else {
        tokio::time::sleep(plan.no_source_for).await;
        tokio::time::sleep(TAIL).await;
        drop(taps);
        return Ok(());
    };

    for (after, action) in plan.actions {
        let elapsed = source_started.elapsed();
        if after > elapsed {
            tokio::time::sleep(after - elapsed).await;
        }
        match action {
            Action::Pause(services, duration) => {
                let mut args = vec!["pause"];
                args.extend(services.iter().map(String::as_str));
                compose.run(&args).await?;
                injection(&recorder, "pause", services.join(", "));
                tokio::time::sleep(duration).await;
                args[0] = "unpause";
                compose.run(&args).await?;
                injection(&recorder, "unpause", services.join(", "));
            }
            Action::Stop(services) => {
                let mut args = vec!["stop", "-t", "0"];
                args.extend(services.iter().map(String::as_str));
                compose.run(&args).await?;
                injection(&recorder, "stop", services.join(", "));
            }
            Action::Isolate(service, duration) => {
                compose.network("disconnect", &service).await?;
                injection(&recorder, "isolate", service.clone());
                tokio::time::sleep(duration).await;
                compose.network("connect", &service).await?;
                injection(&recorder, "reconnect", service);
            }
        }
    }

    // The trace has ended when samples stopped for a while after its
    // duration, or at the latest well after it.
    let deadline = source_started + trace_duration + Duration::from_secs(30);
    loop {
        let quiet = recorder
            .since_last_sample()
            .is_some_and(|since| since > Duration::from_millis(1500));
        if source_started.elapsed() > trace_duration && quiet {
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(TAIL).await;
    drop(taps);
    if recorder.samples() == 0 {
        eprintln!("warning: no sample reached the taps");
    }
    Ok(())
}

async fn wait_for_samples(recorder: &Recorder) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while recorder.samples() == 0 {
        if Instant::now() > deadline {
            bail!("no sample reached the tap after starting the source");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

async fn wait_for_sovd(url: &str, entity: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(response) = client
            .get(format!("{url}/apps/{entity}/faults"))
            .send()
            .await
        {
            if response.status().is_success() {
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            bail!("OpenSOVD did not become ready at {url}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Waits until a service logs `text`. Follows the log instead of polling it,
/// so the wait ends as soon as Docker delivers the line.
async fn wait_for_log(compose: &Compose, service: &str, text: &str) -> anyhow::Result<()> {
    let mut child = compose
        .command(&["logs", "--follow", "--no-color", service])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("cannot run docker compose logs")?;
    let stdout = child
        .stdout
        .take()
        .context("no output of docker compose logs")?;
    let mut lines = BufReader::new(stdout).lines();
    let found = tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(line) = lines.next_line().await? {
            if line.contains(text) {
                return Ok(true);
            }
        }
        Ok::<_, std::io::Error>(false)
    })
    .await;
    match found {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => bail!("{service} stopped before it logged '{text}'"),
        Ok(Err(error)) => Err(error).context("cannot read docker compose logs"),
        Err(_) => bail!("{service} did not log '{text}'"),
    }
}

/// Time of the last frame of an ASC trace.
pub fn trace_duration(path: &Path) -> anyhow::Result<Duration> {
    let text = std::fs::read_to_string(path)?;
    let last = text
        .lines()
        .filter_map(|line| line.split_whitespace().next()?.parse::<f64>().ok())
        .fold(0.0_f64, f64::max);
    Ok(Duration::from_secs_f64(last))
}

fn run_dir_name(run_dir: &Path) -> String {
    run_dir
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
