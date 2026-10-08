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

//! The dashboard's HTTP API and the embedded web page.
//!
//! Requests that change something (start, stop, clear faults) must carry the
//! header `X-Dashboard: 1`. A browser only sends custom headers cross-origin
//! after a CORS preflight, which this server never allows, so another web
//! page cannot stop the stack through the visitor's browser.

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
    pub events: Arc<crate::events::Events>,
    pub taps: Arc<Taps>,
    pub sovd: Arc<Sovd>,
    pub runs: Runs,
    pub launcher: Launcher,
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
        .route("/api/events", get(events))
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

async fn overview(State(app): State<Shared>) -> ApiResult {
    let snapshot = app.components.snapshot.read().await.clone();
    let current = app.runs.current(&activity(&app).await);
    let campaign = json!({
        "runs_dir": app.runs.dir.display().to_string(),
        "running": current.as_ref().map(|c| json!({
            "id": c.id,
            "mode": c.mode,
            "scenario": c.scenarios.iter().find(|s| s.state == crate::runs::State::Running).map(|s| s.id.clone()),
            "done": c.scenarios.iter().filter(|s| s.state == crate::runs::State::Done).count(),
            "total": c.scenarios.len(),
        })),
        "projects": snapshot.campaign_projects,
        "containers": snapshot.campaign_containers,
        "runner": app.launcher.status().await,
    });
    Ok(Json(json!({
        "snapshot": snapshot,
        "campaign": campaign,
        "sovd": { "url": app.sovd.faults_url(), "catalog_version": app.sovd.catalog.version },
    })))
}

async fn events(State(app): State<Shared>) -> ApiResult {
    Ok(Json(json!({ "events": app.events.entries() })))
}

async fn component(State(app): State<Shared>, Path(service): Path<String>) -> ApiResult {
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
    let items = app.sovd.faults().await?;
    Ok(Json(json!({
        "entity": app.sovd.entity,
        "url": app.sovd.faults_url(),
        "catalog_version": app.sovd.catalog.version,
        "items": items,
    })))
}

async fn fault(State(app): State<Shared>, Path(code): Path<String>) -> Result<Response, ApiError> {
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
        "runs_dir": app.runs.dir.display().to_string(),
        "campaigns": app.runs.list(&activity(&app).await),
    })))
}

async fn campaign(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult {
    let view = app
        .runs
        .campaign(&id, &activity(&app).await)
        .map_err(|error| ApiError(StatusCode::NOT_FOUND, format!("{error:#}")))?;
    Ok(Json(
        serde_json::to_value(view).map_err(anyhow::Error::from)?,
    ))
}

/// The signal plot of one scenario run; `run_id` as in its manifest.
async fn signal(State(app): State<Shared>, Path(run_id): Path<String>) -> ApiResult {
    let not_found = |error: anyhow::Error| ApiError(StatusCode::NOT_FOUND, format!("{error:#}"));
    let dir = app
        .runs
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
    Ok(Json(app.launcher.status().await))
}

async fn scenarios(State(app): State<Shared>) -> ApiResult {
    let scenarios = app
        .launcher
        .scenarios()
        .map_err(|error| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    Ok(Json(
        json!({ "scenarios": scenarios, "hara_titles": app.launcher.hara_titles() }),
    ))
}

async fn start_campaign(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<StartRequest>,
) -> ApiResult {
    guard(&headers)?;
    let done = app
        .launcher
        .start(&request)
        .await
        .map_err(|error| ApiError(StatusCode::CONFLICT, format!("{error:#}")))?;
    Ok(Json(json!({ "done": done })))
}

async fn stop_campaign(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    guard(&headers)?;
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
