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

//! The dashboard's HTTP API and the embedded web page.
//!
//! Requests that change something (start, stop, clear faults) must carry the
//! header `X-Dashboard: 1`. A browser only sends custom headers cross-origin
//! after a CORS preflight, which this server never allows, so another web
//! page cannot stop the stack through the visitor's browser.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::components::{spec, Components, Source};
use crate::docker::Docker;
use crate::launcher::{Launcher, StartRequest};
use crate::runs::{Activity, Runs};
use crate::sovd::Sovd;
use crate::taps::{Entry, Taps};

/// Lines read from a container log to filter an input or output log.
const LOG_SCAN: usize = 2000;
/// Entries returned per log.
const LOG_LIMIT: usize = 400;

pub struct App {
    pub components: Arc<Components>,
    pub taps: Arc<Taps>,
    pub sovd: Arc<Sovd>,
    pub runs: Runs,
    pub launcher: Launcher,
    pub remote: Option<crate::remote::Remote>,
    pub remote_selected: AtomicBool,
    pub backend_lock: tokio::sync::Mutex<()>,
}

type Shared = Arc<App>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_GATEWAY, format!("{error:#}"))
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(app: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/style.css", get(style))
        .route("/api/overview", get(overview))
        .route("/api/backend", post(select_backend))
        .route("/api/components/:service", get(component))
        .route("/api/components/:service/logs/:kind", get(logs))
        .route("/api/components/:service/:action", post(action))
        .route("/api/all/:action", post(all_action))
        .route("/api/sovd/faults", get(faults).delete(clear_all))
        .route("/api/sovd/faults/:code", get(fault).delete(clear_one))
        .route("/api/campaign/runner", get(runner))
        .route("/api/campaign/scenarios", get(scenarios))
        .route("/api/campaign/start", post(start_campaign))
        .route("/api/campaign/stop", post(stop_campaign))
        .route("/api/campaigns", get(campaigns))
        .route("/api/campaigns/:id", get(campaign))
        .route("/api/signal/*run_id", get(signal))
        .with_state(app)
}

async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../static/index.html"),
    )
}

async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../static/app.js"),
    )
}

async fn style() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../static/style.css"),
    )
}

fn guard(headers: &HeaderMap) -> Result<(), ApiError> {
    if headers.get("x-dashboard").is_some() {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "missing X-Dashboard header".to_owned(),
        ))
    }
}

fn is_remote(app: &App) -> bool {
    app.remote_selected.load(Ordering::SeqCst)
}
fn remote(app: &App) -> Result<&crate::remote::Remote, ApiError> {
    app.remote.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Set OPENDUT_CONTROLLER_URL to the host bench service".into(),
        )
    })
}
fn evidence(app: &App) -> Runs {
    Runs {
        dir: if is_remote(app) {
            app.launcher.repo.join("runs/opendut-dashboard")
        } else {
            app.runs.dir.clone()
        },
    }
}
async fn select_backend(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    guard(&headers)?;
    let _lock = app.backend_lock.lock().await;
    let selection = match body["backend"].as_str() {
        Some("compose") => false,
        Some("opendut") => {
            remote(&app)?;
            true
        }
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "Unknown backend".into())),
    };
    if is_remote(&app) != selection {
        let status = if is_remote(&app) {
            // A missing adapter cannot confirm that its controller has cleaned up.
            remote(&app)?.get("/status").await?
        } else {
            app.launcher.status().await
        };
        if status["running"] == true || status["recovery"] == true {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Wait for the active campaign and cleanup before switching backends".into(),
            ));
        }
        app.remote_selected.store(selection, Ordering::SeqCst);
    }
    Ok(Json(
        json!({ "backend": if selection { "opendut" } else { "compose" } }),
    ))
}
async fn overview(State(app): State<Shared>) -> ApiResult {
    if is_remote(&app) {
        let runtime = remote(&app)?.get("/runtime").await.unwrap_or_else(|error| json!({
            "runner": { "available": false, "unavailable": format!("{error:#}"), "running": false },
            "error": format!("{error:#}"), "error_source": "controller", "components": [], "updated_ms": crate::taps::now_ms(),
        }));
        let runner = &runtime["runner"];
        let current = runner["run_id"]
            .as_str()
            .and_then(|id| evidence(&app).campaign(id, &Activity::default()).ok());
        let live = if runner["running"] == true {
            Some(json!({
                "id": runner["run_id"], "phase": runner["phase"],
                "scenario": current.as_ref().and_then(|c| c.scenarios.iter().find(|s| s.state == crate::runs::State::Running)).map(|s| &s.id),
                "done": current.as_ref().map(|c| c.scenarios.iter().filter(|s| s.report.is_some()).count()).unwrap_or(0),
                "total": current.as_ref().map(|c| c.scenarios.len()).unwrap_or(0),
            }))
        } else {
            None
        };
        let mut components = runtime["components"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for c in &mut components {
            let service = c["service"].as_str().unwrap_or("").to_owned();
            c["title"] = json!(spec(&service).title);
            c["role"] = json!(spec(&service).role);
        }
        return Ok(Json(
            json!({ "backend": "opendut", "opendut_configured": app.remote.is_some(),
                "snapshot": { "project": "AutoSD / Ankaios / OpenDUT", "components": components,
                    "campaign_projects": [], "error": runtime["error"], "error_source": runtime["error_source"], "updated_ms": runtime["updated_ms"] },
                "campaign": { "runs_dir": evidence(&app).dir, "runner": runner, "running": live, "projects": [] },
                "sovd": { "url": "AutoSD peer B via host controller", "catalog_version": app.sovd.catalog.version }
            }),
        ));
    }
    let snapshot = app.components.snapshot.read().await.clone();
    let current = app.runs.current(&activity(&app).await);
    let campaign = json!({
        "runs_dir": evidence(&app).dir.display().to_string(),
        "running": current.as_ref().map(|c| json!({
            "id": c.id,
            "mode": c.mode,
            "scenario": c.scenarios.iter().find(|s| s.state == crate::runs::State::Running).map(|s| s.id.clone()),
            "done": c.scenarios.iter().filter(|s| s.state == crate::runs::State::Done).count(),
            "total": c.scenarios.len(),
        })),
        "projects": snapshot.campaign_projects,
        "runner": app.launcher.status().await,
    });
    Ok(Json(json!({
        "backend": "compose", "opendut_configured": app.remote.is_some(),
        "snapshot": snapshot,
        "campaign": campaign,
        "sovd": { "url": app.sovd.faults_url(), "catalog_version": app.sovd.catalog.version },
    })))
}

async fn component(State(app): State<Shared>, Path(service): Path<String>) -> ApiResult {
    if is_remote(&app) {
        let runtime = remote(&app)?.get("/runtime").await?;
        let mut component = runtime["components"]
            .as_array()
            .and_then(|cs| cs.iter().find(|c| c["service"] == service))
            .cloned()
            .ok_or_else(|| {
                ApiError(
                    StatusCode::NOT_FOUND,
                    "Peer B workload is not active".into(),
                )
            })?;
        component["title"] = json!(spec(&service).title);
        component["role"] = json!(spec(&service).role);
        return Ok(Json(
            json!({ "component": component, "input": [], "output": [], "files": [] }),
        ));
    }
    let component = {
        let snapshot = app.components.snapshot.read().await;
        snapshot
            .components
            .iter()
            .find(|c| c.service == service)
            .cloned()
    };
    let Some(component) = component else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no component {service}"),
        ));
    };
    let spec = spec(&service);
    let mut files = Vec::new();
    if let (Some(id), "running") = (&component.container_id, component.state.as_str()) {
        for path in spec.files {
            files.push(match app.components.docker.file(id, path).await {
                Ok(content) => json!({ "path": path, "content": content }),
                Err(error) => json!({ "path": path, "error": format!("{error:#}") }),
            });
        }
    }
    Ok(Json(json!({
        "component": component,
        "input": describe_sources(&app, &service, spec.input),
        "output": describe_sources(&app, &service, spec.output),
        "files": files,
    })))
}

fn describe_sources(app: &App, service: &str, sources: &[Source]) -> Vec<Value> {
    sources
        .iter()
        .map(|source| match source {
            Source::Tap(tap) => json!({
                "kind": "tap",
                "label": tap.label(),
                "status": app.taps.status(*tap),
            }),
            Source::Log(patterns) => {
                let label = if patterns.iter().all(|p| p.is_empty()) {
                    format!("container log of {service}")
                } else {
                    format!(
                        "container log of {service}, lines containing {}",
                        patterns
                            .iter()
                            .map(|p| format!("\"{p}\""))
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )
                };
                json!({ "kind": "log", "label": label, "status": "read on request" })
            }
        })
        .collect()
}

async fn logs(
    State(app): State<Shared>,
    Path((service, kind)): Path<(String, String)>,
) -> ApiResult {
    if is_remote(&app) {
        let runtime = remote(&app)?.get("/runtime").await?;
        let lines = if kind == "container" {
            runtime["logs"][&service]
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else {
            vec![json!(
                "Input/output protocol evidence is available in the live campaign signal plot."
            )]
        };
        let entries: Vec<Value> = lines
            .into_iter()
            .map(|line| {
                json!({"ts_ms": runtime["updated_ms"],
            "source": format!("opendut:{service}"), "text": line })
            })
            .collect();
        return Ok(Json(
            json!({"service": service, "kind": kind, "entries": entries, "sources": [], "errors": []}),
        ));
    }
    let spec = spec(&service);
    let sources: &[Source] = match kind.as_str() {
        "input" => spec.input,
        "output" => spec.output,
        "container" => &[Source::Log(&[""])],
        other => return Err(ApiError(StatusCode::NOT_FOUND, format!("no log {other}"))),
    };
    let container = app.components.container_id(&service).await;
    let mut entries: Vec<Entry> = Vec::new();
    let mut errors = Vec::new();
    for source in sources {
        match source {
            Source::Tap(tap) => entries.extend(app.taps.entries(*tap)),
            Source::Log(patterns) => match &container {
                Some(id) => {
                    match container_log(&app.components.docker, id, &service, patterns).await {
                        Ok(lines) => entries.extend(lines),
                        Err(error) => errors.push(format!("{error:#}")),
                    }
                }
                None => errors.push(format!("{service} has no container")),
            },
        }
    }
    entries.sort_by_key(|e| e.ts_ms);
    let skip = entries.len().saturating_sub(LOG_LIMIT);
    Ok(Json(json!({
        "kind": kind,
        "sources": describe_sources(&app, &service, sources),
        "entries": &entries[skip..],
        "errors": errors,
    })))
}

async fn container_log(
    docker: &Docker,
    id: &str,
    service: &str,
    patterns: &[&str],
) -> anyhow::Result<Vec<Entry>> {
    let all = patterns.iter().all(|p| p.is_empty());
    let tail = if all { LOG_LIMIT } else { LOG_SCAN };
    let lines = docker.logs(id, tail).await?;
    Ok(lines
        .iter()
        .filter_map(|line| parse_log_line(line, service))
        .filter(|e| all || patterns.iter().any(|p| e.text.contains(p)))
        .collect())
}

/// `<RFC 3339 timestamp> <text>` as Docker writes it with `timestamps=1`.
pub fn parse_log_line(line: &str, service: &str) -> Option<Entry> {
    let (stamp, text) = line.split_once(' ')?;
    let ts_ms = humantime::parse_rfc3339(stamp)
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let text = strip_ansi(text);
    if text.trim().is_empty() {
        return None;
    }
    Some(Entry {
        ts_ms,
        source: format!("log:{service}"),
        text,
    })
}

/// Removes terminal color codes (`ESC [ … letter`).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

async fn action(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path((service, action)): Path<(String, String)>,
) -> ApiResult {
    guard(&headers)?;
    if is_remote(&app) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Ankaios owns the measured workload; use campaign start/stop".into(),
        ));
    }
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no action {action}"),
        ));
    }
    let done = app.components.act(&service, &action).await?;
    Ok(Json(json!({ "done": done })))
}

async fn all_action(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(action): Path<String>,
) -> ApiResult {
    guard(&headers)?;
    if is_remote(&app) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Ankaios owns the measured workload; use campaign start/stop".into(),
        ));
    }
    if !matches!(action.as_str(), "start" | "stop") {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no action {action}"),
        ));
    }
    Ok(Json(
        json!({ "done": app.components.act_all(&action).await }),
    ))
}

async fn faults(State(app): State<Shared>) -> ApiResult {
    let items = if is_remote(&app) {
        let runtime = remote(&app)?.get("/runtime").await?;
        if let Some(error) = runtime["sovd_error"].as_str() {
            return Err(ApiError(StatusCode::BAD_GATEWAY, error.into()));
        }
        let raw = &runtime["faults"];
        let list = raw
            .as_array()
            .or_else(|| raw["faults"].as_array())
            .or_else(|| raw["items"].as_array())
            .ok_or_else(|| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No live AutoSD diagnostics; see the campaign reports".into(),
                )
            })?;
        list.iter().cloned().map(|f| app.sovd.enrich(f)).collect()
    } else {
        app.sovd.faults().await?
    };
    Ok(Json(json!({
        "entity": app.sovd.entity,
        "url": if is_remote(&app) { "AutoSD peer B via host controller".to_owned() } else { app.sovd.faults_url() },
        "catalog_version": app.sovd.catalog.version,
        "items": items,
    })))
}

async fn fault(State(app): State<Shared>, Path(code): Path<String>) -> Result<Response, ApiError> {
    if is_remote(&app) {
        let body = remote(&app)?
            .get(&format!("/faults/{}", crate::docker::encode(&code)))
            .await?;
        return Ok(Json(app.sovd.enrich(body)).into_response());
    }
    let (status, body) = app.sovd.fault(&code).await?;
    let body = if (200..300).contains(&status) {
        app.sovd.enrich(body)
    } else {
        body
    };
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    Ok((status, Json(body)).into_response())
}

async fn clear_all(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    guard(&headers)?;
    clear(&app, None).await
}

async fn clear_one(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> ApiResult {
    guard(&headers)?;
    clear(&app, Some(&code)).await
}

async fn clear(app: &App, code: Option<&str>) -> ApiResult {
    if is_remote(app) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Diagnostics clearing is disabled during measured OpenDUT campaigns".into(),
        ));
    }
    let (status, body) = app.sovd.clear(code).await?;
    if !(200..300).contains(&status) {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            format!("OpenSOVD answered {status}: {body}"),
        ));
    }
    Ok(Json(
        json!({ "status": status, "cleared": code.unwrap_or("all") }),
    ))
}

async fn campaigns(State(app): State<Shared>) -> ApiResult {
    Ok(Json(json!({
        "runs_dir": evidence(&app).dir.display().to_string(),
        "campaigns": evidence(&app).list(&activity(&app).await),
    })))
}

async fn campaign(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let mut view = evidence(&app)
        .campaign(&id, &activity(&app).await)
        .map_err(|error| ApiError(StatusCode::NOT_FOUND, format!("{error:#}")))?;
    if is_remote(&app) {
        let status = remote(&app)?.status().await;
        if status["running"] == true && status["run_id"] == id {
            view.state = crate::runs::State::Running;
        }
    }
    Ok(Json(
        serde_json::to_value(view).map_err(anyhow::Error::from)?,
    ))
}

/// The signal plot of one scenario run; `run_id` as in its manifest.
async fn signal(State(app): State<Shared>, Path(run_id): Path<String>) -> ApiResult {
    let not_found = |error: anyhow::Error| ApiError(StatusCode::NOT_FOUND, format!("{error:#}"));
    let dir = evidence(&app)
        .run_dir(run_id.trim_start_matches('/'))
        .map_err(not_found)?;
    let signal = crate::signal::read(&dir.join("recording.jsonl"))
        .map_err(|error| not_found(anyhow::Error::from(error).context("no recording yet")))?;
    Ok(Json(
        serde_json::to_value(signal).map_err(anyhow::Error::from)?,
    ))
}

/// What shows a running campaign: its Compose projects, and when the
/// dashboard's campaign runner stopped.
async fn activity(app: &App) -> Activity {
    if is_remote(app) {
        let status = match remote(app) {
            Ok(r) => r.status().await,
            Err(_) => return Activity::default(),
        };
        return Activity {
            projects: vec![],
            quiet_since: status["finished_at"]
                .as_str()
                .and_then(|s| humantime::parse_rfc3339(s).ok()),
        };
    }
    Activity {
        projects: app
            .components
            .snapshot
            .read()
            .await
            .campaign_projects
            .clone(),
        quiet_since: app.launcher.stopped_at().await,
    }
}

async fn runner(State(app): State<Shared>) -> ApiResult {
    Ok(Json(if is_remote(&app) {
        remote(&app)?.status().await
    } else {
        app.launcher.status().await
    }))
}

async fn scenarios(State(app): State<Shared>) -> ApiResult {
    let scenarios = app
        .launcher
        .scenarios_for(is_remote(&app))
        .map_err(|error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    Ok(Json(json!({ "scenarios": scenarios })))
}

async fn start_campaign(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<StartRequest>,
) -> ApiResult {
    guard(&headers)?;
    let _lock = app.backend_lock.lock().await;
    if is_remote(&app) {
        return Ok(Json(
            remote(&app)?
                .post("/start", json!({ "scenarios": request.scenarios }))
                .await?,
        ));
    }
    let done = app
        .launcher
        .start(&request)
        .await
        .map_err(|error| ApiError(StatusCode::CONFLICT, format!("{error:#}")))?;
    Ok(Json(json!({ "done": done })))
}

async fn stop_campaign(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    guard(&headers)?;
    let _lock = app.backend_lock.lock().await;
    if is_remote(&app) {
        return Ok(Json(remote(&app)?.post("/stop", json!({})).await?));
    }
    Ok(Json(json!({ "done": app.launcher.stop().await? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_log_lines() {
        let entry = parse_log_line(
            "2026-10-07T14:20:00.702813000Z \u{1b}[2m2026\u{1b}[0m INFO ready",
            "guardian",
        )
        .unwrap();
        assert_eq!(entry.ts_ms, 1_791_382_800_702);
        assert_eq!(entry.text, "2026 INFO ready");
        assert_eq!(entry.source, "log:guardian");
        assert!(parse_log_line("not a log line", "x").is_none());
    }

    #[test]
    fn mutating_requests_need_the_header() {
        let mut headers = HeaderMap::new();
        assert!(guard(&headers).is_err());
        headers.insert("x-dashboard", "1".parse().unwrap());
        assert!(guard(&headers).is_ok());
    }
}
