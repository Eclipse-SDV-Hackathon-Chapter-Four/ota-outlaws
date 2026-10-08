/*
 * Copyright (c) 2026 Contributors to the Eclipse Foundation
 *
 * See the NOTICE file(s) distributed with this work for additional
 * information regarding copyright ownership.
 *
 * This program and the accompanying materials are made available under the
 * terms of the Eclipse Public License 2.0 which is available at
 * https://www.eclipse.org/legal/epl-2.0
 *
 * SPDX-License-Identifier: EPL-2.0
 */

// AI-assisted: Claude Code / Claude Sonnet 5.5 (claude-sonnet-5-5)

//! Container events: which container of which Compose project was started,
//! stopped, paused, or killed, and when. The Live chain view shows them, so one
//! can follow how a campaign builds and tears down its chains.
//!
//! The Docker events API streams without end, but with an `until` in the past
//! it answers with the events of that window and closes. The dashboard asks
//! for the last second once a second.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::docker::Docker;

const KEEP: usize = 500;
const COMPOSE_PROJECT: &str = "com.docker.compose.project";
const COMPOSE_SERVICE: &str = "com.docker.compose.service";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub ts_ms: u64,
    pub project: String,
    pub service: String,
    /// What happened, in words: started, exited, stopped, killed, paused, resumed, restarted.
    pub action: String,
    pub detail: String,
}

#[derive(Default)]
pub struct Events {
    buffer: Mutex<VecDeque<Entry>>,
}

impl Events {
    pub fn entries(&self) -> Vec<Entry> {
        self.buffer
            .lock()
            .map(|buffer| buffer.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn push(&self, entry: Entry) {
        if let Ok(mut buffer) = self.buffer.lock() {
            if buffer.len() == KEEP {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }
    }
}

/// Reads one line of the Docker events stream. Only containers that belong to
/// a Compose project and only the actions a person wants to see.
pub fn parse(line: &str) -> Option<(String, Entry)> {
    let event: Value = serde_json::from_str(line).ok()?;
    if event["Type"] != "container" {
        return None;
    }
    let attributes = &event["Actor"]["Attributes"];
    let project = attributes[COMPOSE_PROJECT].as_str()?.to_owned();
    let service = attributes[COMPOSE_SERVICE].as_str()?.to_owned();
    let (action, detail) = match event["Action"].as_str()? {
        "start" => ("started", String::new()),
        "die" => (
            "exited",
            format!(
                "exit code {}",
                attributes["exitCode"].as_str().unwrap_or("?")
            ),
        ),
        "stop" => ("stopped", String::new()),
        "kill" => (
            "killed",
            format!("signal {}", attributes["signal"].as_str().unwrap_or("?")),
        ),
        "pause" => ("paused", String::new()),
        "unpause" => ("resumed", String::new()),
        "restart" => ("restarted", String::new()),
        "oom" => ("ran out of memory", String::new()),
        _ => return None,
    };
    let nanos = event["timeNano"].as_u64().unwrap_or(0);
    let key = format!("{}{}{}", event["id"], nanos, event["Action"]);
    Some((
        key,
        Entry {
            ts_ms: nanos / 1_000_000,
            project,
            service,
            action: action.to_owned(),
            detail,
        },
    ))
}

fn timestamp(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:09}", since.as_secs(), since.subsec_nanos())
}

/// Polls the Docker events once a second and keeps the last 500.
pub fn spawn(docker: Docker) -> Arc<Events> {
    let events = Arc::new(Events::default());
    let shared = Arc::clone(&events);
    tokio::spawn(async move {
        let mut since = SystemTime::now();
        let mut seen: VecDeque<String> = VecDeque::new();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let until = SystemTime::now();
            if let Ok(lines) = docker.events(&timestamp(since), &timestamp(until)).await {
                since = until;
                for line in lines {
                    let Some((key, entry)) = parse(&line) else {
                        continue;
                    };
                    if seen.contains(&key) {
                        continue;
                    }
                    if seen.len() == 200 {
                        seen.pop_front();
                    }
                    seen.push_back(key);
                    shared.push(entry);
                }
            }
        }
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(action: &str, extra: &str) -> String {
        format!(
            r#"{{"Type":"container","Action":"{action}","id":"abc","Actor":{{"ID":"abc","Attributes":{{"com.docker.compose.project":"campaign-normal","com.docker.compose.service":"guardian"{extra}}}}},"time":1,"timeNano":1790000000123000000}}"#
        )
    }

    #[test]
    fn a_start_names_project_service_and_time() {
        let (_, entry) = parse(&line("start", "")).unwrap();
        assert_eq!(entry.project, "campaign-normal");
        assert_eq!(entry.service, "guardian");
        assert_eq!(entry.action, "started");
        assert_eq!(entry.ts_ms, 1_790_000_000_123);
    }

    #[test]
    fn an_exit_carries_its_code_and_a_kill_its_signal() {
        let (_, die) = parse(&line("die", r#","exitCode":"137""#)).unwrap();
        assert_eq!(
            (die.action.as_str(), die.detail.as_str()),
            ("exited", "exit code 137")
        );
        let (_, kill) = parse(&line("kill", r#","signal":"9""#)).unwrap();
        assert_eq!(kill.detail, "signal 9");
    }

    #[test]
    fn other_actions_and_containers_without_a_project_are_ignored() {
        assert!(parse(&line("exec_start: sh", "")).is_none());
        assert!(parse(r#"{"Type":"network","Action":"connect"}"#).is_none());
        assert!(parse(
            r#"{"Type":"container","Action":"start","Actor":{"Attributes":{"name":"x"}}}"#
        )
        .is_none());
    }
}
