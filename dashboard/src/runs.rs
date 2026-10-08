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

//! Reads the campaign tool's evidence directory (`runs/`).
//!
//! The campaign tool runs on the host; the dashboard only reads what it
//! writes:
//!
//! ```text
//! runs/<campaign>/plan.json              scenarios of the campaign, written first
//! runs/<campaign>/<scenario>/manifest.json   written when the scenario starts
//! runs/<campaign>/<scenario>/recording.jsonl grows while it runs
//! runs/<campaign>/<scenario>/report.json     written when it has been judged
//! runs/<campaign>/campaign.md            written when the campaign is done
//! runs/<id>-observe-<scenario>/…         an `observe` run: one scenario, no subdirectory
//! ```
//!
//! A scenario is running while its Compose project (`campaign-<run>-<scenario>`)
//! has containers, or its recording changed in the last few seconds. When the
//! dashboard's own campaign runner has stopped, changes from before it
//! stopped no longer count.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context as _};
use serde::Serialize;
use serde_json::Value;

/// A recording that changed this recently belongs to a running scenario.
const ACTIVE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// In the plan, not started yet.
    Pending,
    Running,
    /// Judged; the report exists.
    Done,
    /// Started, not judged, and no longer running: the tool was stopped.
    Incomplete,
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignSummary {
    pub id: String,
    pub mode: String,
    pub state: State,
    pub scenarios: usize,
    pub pass: usize,
    pub fail: usize,
    pub inconclusive: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioView {
    pub id: String,
    pub state: State,
    pub manifest: Option<Value>,
    pub report: Option<Value>,
    /// Lines in `recording.jsonl`: observations so far.
    pub observations: usize,
    /// `error.txt`: the run itself failed; the scenario is still judged.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignView {
    pub id: String,
    pub mode: String,
    pub state: State,
    pub started_at: Option<String>,
    pub scenarios: Vec<ScenarioView>,
    pub campaign_md: Option<String>,
}

pub struct Runs {
    pub dir: PathBuf,
}

/// What tells that a campaign is running.
#[derive(Debug, Clone, Default)]
pub struct Activity {
    /// Compose projects of campaign scenarios with running containers.
    pub projects: Vec<String>,
    /// The campaign runner stopped at this time: files changed before it do
    /// not show a running campaign.
    pub quiet_since: Option<SystemTime>,
}

impl Runs {
    /// Campaigns, newest first. Directory names start with a timestamp.
    pub fn list(&self, activity: &Activity) -> Vec<CampaignSummary> {
        let mut ids: Vec<String> = std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids.iter()
            .filter_map(|id| self.campaign(id, activity).ok())
            .map(|view| summarize(&view))
            .collect()
    }

    /// The running campaign, if any.
    pub fn current(&self, activity: &Activity) -> Option<CampaignView> {
        self.list(activity)
            .into_iter()
            .find(|c| c.state == State::Running)
            .and_then(|c| self.campaign(&c.id, activity).ok())
    }

    pub fn campaign(&self, id: &str, activity: &Activity) -> anyhow::Result<CampaignView> {
        if !valid_name(id) {
            bail!("invalid campaign id {id:?}");
        }
        let dir = self.dir.join(id);
        if !dir.is_dir() {
            bail!("no campaign {id}");
        }
        // An `observe` run is a single scenario without a subdirectory.
        if dir.join("manifest.json").is_file() {
            let scenario = scenario(&dir, None, activity);
            let started_at = started_at(&scenario);
            return Ok(CampaignView {
                id: id.to_owned(),
                mode: "observe".to_owned(),
                state: scenario.state,
                started_at,
                scenarios: vec![scenario],
                campaign_md: None,
            });
        }

        let plan: Option<Value> = read_json(&dir.join("plan.json"));
        let mut names: Vec<String> = plan
            .as_ref()
            .and_then(|p| p["scenarios"].as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        // Without a plan (older campaigns), the started scenarios in the
        // order they started.
        let mut started: Vec<(SystemTime, String)> = std::fs::read_dir(&dir)
            .context("cannot read campaign directory")?
            .filter_map(Result::ok)
            .filter(|e| e.path().join("manifest.json").is_file())
            .map(|e| {
                let modified = e
                    .path()
                    .join("manifest.json")
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (modified, e.file_name().to_string_lossy().into_owned())
            })
            .collect();
        started.sort();
        for (_, name) in started {
            if !names.contains(&name) {
                names.push(name);
            }
        }

        let campaign_md = std::fs::read_to_string(dir.join("campaign.md")).ok();
        let mut scenarios: Vec<ScenarioView> = names
            .iter()
            .map(|name| {
                let scenario_dir = dir.join(name);
                if scenario_dir.join("manifest.json").is_file() {
                    scenario(&scenario_dir, Some(name), activity)
                } else {
                    ScenarioView {
                        id: name.clone(),
                        state: State::Pending,
                        manifest: None,
                        report: None,
                        observations: 0,
                        error: None,
                    }
                }
            })
            .collect();

        let any_running = scenarios.iter().any(|s| s.state == State::Running);
        let state = if campaign_md.is_some() {
            State::Done
        } else if any_running || recently_modified(&dir, activity.quiet_since) {
            State::Running
        } else {
            State::Incomplete
        };
        if state == State::Incomplete {
            // The tool was stopped: what never started will not start.
            for s in scenarios.iter_mut().filter(|s| s.state == State::Pending) {
                s.state = State::Incomplete;
            }
        }
        let started_at = plan
            .as_ref()
            .and_then(|p| p["started_at"].as_str().map(str::to_owned))
            .or_else(|| scenarios.iter().find_map(started_at));
        Ok(CampaignView {
            id: id.to_owned(),
            mode: "run".to_owned(),
            state,
            started_at,
            scenarios,
            campaign_md,
        })
    }
}

impl Runs {
    /// The directory of one scenario run, by the `run_id` of its manifest:
    /// `<campaign>/<scenario>`, or `<campaign>` for an `observe` run.
    pub fn run_dir(&self, run_id: &str) -> anyhow::Result<PathBuf> {
        let parts: Vec<&str> = run_id.split('/').collect();
        if parts.len() > 2 || !parts.iter().all(|p| valid_name(p)) {
            bail!("invalid run id {run_id:?}");
        }
        let dir = parts.iter().fold(self.dir.clone(), |dir, p| dir.join(p));
        if !dir.join("manifest.json").is_file() {
            bail!("no run {run_id}");
        }
        Ok(dir)
    }
}

/// One path component under `runs/`: no separators, nothing hidden or `..`.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

fn scenario(dir: &Path, name: Option<&str>, activity: &Activity) -> ScenarioView {
    let manifest: Option<Value> = read_json(&dir.join("manifest.json"));
    let report: Option<Value> = read_json(&dir.join("report.json"));
    let id = name
        .map(str::to_owned)
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m["scenario"].as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let recording = dir.join("recording.jsonl");
    let observations = std::fs::read(&recording)
        .map(|bytes| bytecount(&bytes))
        .unwrap_or(0);
    let project = manifest
        .as_ref()
        .and_then(|m| m["run_id"].as_str())
        .filter(|run_id| run_id.contains('/'))
        .map(|run_id| project_name(&run_id.replace('/', "-")))
        .unwrap_or_else(|| project_name(&id));
    let state = if report.is_some() {
        State::Done
    } else if activity.projects.contains(&project)
        || activity.projects.contains(&project_name(&id))
        || modified_within(&recording, ACTIVE, activity.quiet_since)
    {
        State::Running
    } else {
        State::Incomplete
    };
    ScenarioView {
        id,
        state,
        manifest,
        report,
        observations,
        error: std::fs::read_to_string(dir.join("error.txt")).ok(),
    }
}

/// The Compose project the campaign tool runs a scenario in
/// (`runner::run_dir_name`).
pub fn project_name(scenario: &str) -> String {
    let name: String = scenario
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("campaign-{name}")
}

fn summarize(view: &CampaignView) -> CampaignSummary {
    let verdicts = || {
        view.scenarios
            .iter()
            .filter_map(|s| s.report.as_ref()?["verdict"].as_str())
    };
    CampaignSummary {
        id: view.id.clone(),
        mode: view.mode.clone(),
        state: view.state,
        scenarios: view.scenarios.len(),
        pass: verdicts().filter(|v| *v == "PASS").count(),
        fail: verdicts().filter(|v| *v == "FAIL").count(),
        inconclusive: verdicts().filter(|v| *v == "INCONCLUSIVE").count(),
    }
}

fn started_at(scenario: &ScenarioView) -> Option<String> {
    scenario
        .manifest
        .as_ref()
        .and_then(|m| m["started_at"].as_str().map(str::to_owned))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn bytecount(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

/// Whether the file changed within `window`, and after `quiet_since`.
fn modified_within(path: &Path, window: Duration, quiet_since: Option<SystemTime>) -> bool {
    let Ok(modified) = path.metadata().and_then(|m| m.modified()) else {
        return false;
    };
    let recent = modified.elapsed().is_ok_and(|age| age < window);
    recent && quiet_since.is_none_or(|quiet| modified > quiet)
}

/// Whether anything directly in the campaign directory, or in one of its
/// scenario directories, changed recently. Covers the gaps between
/// scenarios, when the tool builds images or judges a run.
fn recently_modified(dir: &Path, quiet_since: Option<SystemTime>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        modified_within(&path, ACTIVE * 4, quiet_since)
            || (path.is_dir()
                && std::fs::read_dir(&path)
                    .map(|inner| {
                        inner
                            .filter_map(Result::ok)
                            .any(|e| modified_within(&e.path(), ACTIVE, quiet_since))
                    })
                    .unwrap_or(false))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_runs(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dashboard-runs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn project_names_match_the_campaign_runner() {
        assert_eq!(project_name("counter_stuck"), "campaign-counter-stuck");
    }

    #[test]
    fn running_project_is_attributed_to_its_campaign() {
        let dir = temp_runs("namespaced");
        for id in ["run_a", "run_b"] {
            write(
                &dir.join(id).join("counter_stuck/manifest.json"),
                &format!(r#"{{"run_id":"{id}/counter_stuck","scenario":"counter_stuck"}}"#),
            );
        }
        let runs = Runs { dir };
        let active = Activity {
            projects: vec!["campaign-run-a-counter-stuck".to_owned()],
            quiet_since: Some(SystemTime::now() + Duration::from_secs(1)),
        };
        assert_eq!(
            runs.campaign("run_a", &active).unwrap().scenarios[0].state,
            State::Running
        );
        assert_eq!(
            runs.campaign("run_b", &active).unwrap().scenarios[0].state,
            State::Incomplete
        );
    }

    #[test]
    fn finished_campaign_with_verdicts() {
        let dir = temp_runs("finished");
        let campaign = dir.join("20261007-120000");
        write(
            &campaign.join("plan.json"),
            r#"{"campaign_id":"20261007-120000","started_at":"2026-10-07T12:00:00Z","scenarios":["a","b"]}"#,
        );
        for (name, verdict) in [("a", "PASS"), ("b", "FAIL")] {
            write(
                &campaign.join(name).join("manifest.json"),
                r#"{"scenario":"x"}"#,
            );
            write(
                &campaign.join(name).join("report.json"),
                &format!(r#"{{"verdict":"{verdict}"}}"#),
            );
            write(&campaign.join(name).join("recording.jsonl"), "{}\n{}\n");
        }
        write(&campaign.join("campaign.md"), "# done");
        let runs = Runs { dir };
        let view = runs
            .campaign("20261007-120000", &Activity::default())
            .unwrap();
        assert_eq!(view.state, State::Done);
        assert_eq!(view.started_at.as_deref(), Some("2026-10-07T12:00:00Z"));
        assert_eq!(view.scenarios[0].observations, 2);
        let list = runs.list(&Activity::default());
        assert_eq!((list[0].pass, list[0].fail, list[0].scenarios), (1, 1, 2));
        assert!(runs.current(&Activity::default()).is_none());
    }

    #[test]
    fn running_scenario_and_pending_rest() {
        let dir = temp_runs("running");
        let campaign = dir.join("20261007-130000");
        write(
            &campaign.join("plan.json"),
            r#"{"scenarios":["counter_stuck","timeout"]}"#,
        );
        write(&campaign.join("counter_stuck").join("manifest.json"), "{}");
        let runs = Runs { dir };
        let active = Activity {
            projects: vec!["campaign-counter-stuck".to_owned()],
            quiet_since: None,
        };
        let view = runs.current(&active).expect("running campaign");
        assert_eq!(view.state, State::Running);
        assert_eq!(view.scenarios[0].state, State::Running);
        assert_eq!(view.scenarios[1].state, State::Pending);
    }

    #[test]
    fn a_stopped_runner_ends_a_recent_campaign() {
        let dir = temp_runs("stopped");
        let campaign = dir.join("20261007-140000");
        write(
            &campaign.join("plan.json"),
            r#"{"scenarios":["timeout","spike"]}"#,
        );
        write(&campaign.join("timeout").join("manifest.json"), "{}");
        write(
            &campaign.join("timeout").join("recording.jsonl"),
            "{}
",
        );
        let runs = Runs { dir };
        let stopped = Activity {
            projects: vec![],
            quiet_since: Some(SystemTime::now() + Duration::from_secs(1)),
        };
        let view = runs.campaign("20261007-140000", &stopped).unwrap();
        assert_eq!(view.state, State::Incomplete);
        assert_eq!(view.scenarios[0].state, State::Incomplete);
        assert_eq!(view.scenarios[1].state, State::Incomplete);
        assert!(runs.current(&stopped).is_none());
    }

    #[test]
    fn rejects_paths_outside_the_runs_directory() {
        let runs = Runs {
            dir: temp_runs("reject"),
        };
        assert!(runs.campaign("../etc", &Activity::default()).is_err());
        assert!(runs.campaign("..", &Activity::default()).is_err());
    }

    #[test]
    fn finds_scenario_and_observe_runs() {
        let dir = temp_runs("run-dir");
        write(&dir.join("c1").join("hot_spot").join("manifest.json"), "{}");
        write(&dir.join("c2-observe-spike").join("manifest.json"), "{}");
        let runs = Runs { dir: dir.clone() };
        assert_eq!(
            runs.run_dir("c1/hot_spot").unwrap(),
            dir.join("c1").join("hot_spot")
        );
        assert_eq!(
            runs.run_dir("c2-observe-spike").unwrap(),
            dir.join("c2-observe-spike")
        );
        assert!(runs.run_dir("c1").is_err(), "a campaign is no run");
        assert!(runs.run_dir("c1/../c1/hot_spot").is_err());
        assert!(runs.run_dir("c1/..").is_err());
        assert!(runs.run_dir("c1/hot_spot/x").is_err());
        assert!(runs.run_dir("").is_err());
    }
}
