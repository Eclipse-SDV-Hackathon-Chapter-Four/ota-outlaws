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

//! Dashboard for the Battery Thermal Guardian stack.
//!
//! Shows the state, memory, and settings of every component of the Compose
//! project, starts and stops them, shows what goes into and comes out of
//! each one, lists the DTCs in OpenSOVD, and shows the campaign reports.
//!
//! Environment (defaults fit the Compose service):
//! - `LISTEN`: address to serve on (`0.0.0.0:8080`)
//! - `DOCKER_SOCKET`: Docker Engine socket (`/var/run/docker.sock`)
//! - `DASHBOARD_PROJECT`: Compose project, if it cannot be read from the
//!   dashboard's own container (`ota-outlaws`)
//! - `SOVD_URL`, `SOVD_ENTITY`: OpenSOVD (`http://opensovd-gateway:7690/sovd/v1`,
//!   `battery_guardian`)
//! - `FAULT_CATALOG`: DFM fault catalog (`/etc/dashboard/catalog/battery_guardian.json`)
//! - `DATABROKER_ADDR`: KUKSA Data Broker (`http://kuksa-databroker:55555`)
//! - `REPO_DIR`: the repository, mounted read-only (`/repo`); the campaign
//!   runner mounts it read-write from the host, see [`launcher`]
//! - `HOST_REPO_DIR`: the repository's path on the Docker host, if it cannot
//!   be read from the dashboard's own mounts
//! - `RUNS_DIR`: the campaign tool's evidence directory (`/repo/runs`)
//! - `ZENOH_CONNECT`, `ZENOH_LISTEN`, `ZENOH_MODE`: Zenoh endpoints
//! - `RUST_LOG`: log filter (`info`)

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use thermal_contract::transport::ZenohEndpoints;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

mod api;
mod components;
mod docker;
mod launcher;
mod remote;
mod runs;
mod signal;
mod sovd;
mod taps;

use components::Components;
use docker::Docker;
use sovd::{Catalog, Sovd};
use taps::Taps;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let socket = PathBuf::from(env("DOCKER_SOCKET", "/var/run/docker.sock"));
    let docker = Docker::new(&socket);
    let own = own_container(&docker).await;
    let (project, own_service) = own_project(&own);
    info!(%project, %own_service, "watching Compose project");

    let catalog_path = PathBuf::from(env(
        "FAULT_CATALOG",
        "/etc/dashboard/catalog/battery_guardian.json",
    ));
    let catalog = Catalog::load(&catalog_path).unwrap_or_else(|error| {
        warn!(%error, "no fault catalog; severities come from OpenSOVD's numbers");
        Catalog::default()
    });
    let sovd = Arc::new(Sovd::new(
        env("SOVD_URL", "http://opensovd-gateway:7690/sovd/v1"),
        env("SOVD_ENTITY", "battery_guardian"),
        catalog,
    ));

    let taps = Arc::new(Taps::default());
    taps::spawn_uprotocol(Arc::clone(&taps), ZenohEndpoints::from_env());
    taps::spawn_kuksa(
        Arc::clone(&taps),
        env("DATABROKER_ADDR", "http://kuksa-databroker:55555"),
    );
    taps::spawn_sovd(Arc::clone(&taps), Arc::clone(&sovd));

    let repo = PathBuf::from(env("REPO_DIR", "/repo"));
    let launcher =
        launcher::Launcher::new(docker.clone(), project.clone(), repo.clone(), &socket, &own);
    if let Some(reason) = launcher.unavailable() {
        warn!(%reason, "campaigns cannot be started from the dashboard");
    }

    let components = Arc::new(Components {
        docker,
        project,
        own_service,
        snapshot: RwLock::new(Default::default()),
    });
    components.spawn_poller();

    let app = Arc::new(api::App {
        components,
        taps,
        sovd,
        runs: runs::Runs {
            dir: std::env::var("RUNS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| repo.join("runs")),
        },
        launcher,
        remote: remote::Remote::from_env()?,
        remote_selected: std::sync::atomic::AtomicBool::new(
            env("DASHBOARD_BACKEND", "compose") == "opendut",
        ),
        backend_lock: tokio::sync::Mutex::new(()),
    });
    let address: SocketAddr = env("LISTEN", "0.0.0.0:8080")
        .parse()
        .context("invalid LISTEN address")?;
    info!(%address, "dashboard ready");
    axum::Server::bind(&address)
        .serve(api::router(app).into_make_service())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// `docker inspect` of the dashboard's own container, or null outside a
/// container. Docker sets the hostname to the short container ID. When
/// Docker Desktop starts, it starts this container before its API answers,
/// so the inspect is retried for about a minute.
async fn own_container(docker: &Docker) -> serde_json::Value {
    let Ok(hostname) = std::env::var("HOSTNAME") else {
        return serde_json::Value::Null;
    };
    const ATTEMPTS: u32 = 12;
    for attempt in 1..=ATTEMPTS {
        match docker.inspect(&hostname).await {
            Ok(inspect) => return inspect,
            Err(error) if attempt == ATTEMPTS => {
                warn!(%error, "cannot inspect own container");
            }
            Err(error) => {
                warn!(%error, attempt, "cannot inspect own container yet; retrying");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
    serde_json::Value::Null
}

/// The Compose project and service of the dashboard's own container.
fn own_project(own: &serde_json::Value) -> (String, String) {
    let labels = &own["Config"]["Labels"];
    match (
        labels["com.docker.compose.project"].as_str(),
        labels["com.docker.compose.service"].as_str(),
    ) {
        (Some(project), Some(service)) => (project.to_owned(), service.to_owned()),
        _ => (
            env("DASHBOARD_PROJECT", "ota-outlaws"),
            "dashboard".to_owned(),
        ),
    }
}
