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

//! `campaign` command line. Run from the repository root.
//!
//! ```text
//! campaign run <scenario>... | --all [--no-build] [--out runs]
//!              [--runtime-hook PATH] [--run-id ID]
//! campaign observe <scenario> [--seconds 60] [--zenoh tcp/127.0.0.1:7447]
//!                             [--sovd http://127.0.0.1:7690/sovd/v1]
//! campaign evaluate <run-dir>...
//! ```
//!
//! Exit code 0 if every scenario with status `implemented` passed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context as _};
use campaign::catalog::{ScenarioStatus, Stimulus};
use campaign::evaluate::{evaluate, Evaluation, Verdict};
use campaign::record::{self, Recorder, SovdTap};
use campaign::report::{self, Manifest, Report, Summary};
use campaign::runner::{self, Settings};
use campaign::{Context, CATALOG, SAFETY_PARAMS};
use sha2::{Digest, Sha256};
use thermal_contract::transport::ZenohEndpoints;

const ENTITY: &str = "battery_guardian";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut record_exit = true;
    let code = match dispatch(&args).await {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(error) => {
            // A rejected attempt to reuse evidence must not replace the
            // status of the process that owns that evidence directory.
            record_exit = !error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
            });
            eprintln!("error: {error:#}");
            2
        }
    };
    if let Ok(options) = parse(args.get(1..).unwrap_or_default()) {
        if let Some(id) = options.run_id.filter(|id| record_exit && valid_id(id)) {
            let _ = std::fs::write(options.out.join(id).join("exit-code"), format!("{code}\n"));
        }
    }
    std::process::exit(code);
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

struct Options {
    positional: Vec<String>,
    all: bool,
    no_build: bool,
    out: PathBuf,
    seconds: u64,
    zenoh: String,
    sovd: String,
    runtime_hook: Option<PathBuf>,
    run_id: Option<String>,
}

fn parse(args: &[String]) -> anyhow::Result<Options> {
    let mut options = Options {
        positional: Vec::new(),
        all: false,
        no_build: false,
        out: PathBuf::from("runs"),
        seconds: 60,
        zenoh: "tcp/127.0.0.1:7447".into(),
        sovd: "http://127.0.0.1:7690/sovd/v1".into(),
        runtime_hook: None,
        run_id: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().context(format!("{arg} needs a value"));
        match arg.as_str() {
            "--runtime-hook" => options.runtime_hook = Some(PathBuf::from(value()?)),
            "--run-id" => options.run_id = Some(value()?.clone()),
            "--all" => options.all = true,
            "--no-build" => options.no_build = true,
            "--out" => options.out = PathBuf::from(value()?),
            "--seconds" => options.seconds = value()?.parse()?,
            "--zenoh" => options.zenoh = value()?.clone(),
            "--sovd" => options.sovd = value()?.clone(),
            flag if flag.starts_with("--") => bail!("unknown option {flag}"),
            _ => options.positional.push(arg.clone()),
        }
    }
    Ok(options)
}

async fn dispatch(args: &[String]) -> anyhow::Result<bool> {
    let repo = std::env::current_dir()?;
    if !repo.join(CATALOG).is_file() {
        bail!("run from the repository root ({CATALOG} not found)");
    }
    let Some((command, rest)) = args.split_first() else {
        bail!("usage: campaign run|observe|evaluate …");
    };
    let options = parse(rest)?;
    let context = Context::load(&repo)?;
    match command.as_str() {
        "run" => run(&repo, &context, &options).await,
        "observe" => observe(&repo, &context, &options).await,
        "evaluate" => {
            let mut ok = true;
            for dir in &options.positional {
                let evaluation = judge(&repo, &context, Path::new(dir))?;
                println!("{}", report::console(&evaluation));
                ok &= passed(&evaluation);
            }
            Ok(ok)
        }
        other => bail!("unknown command {other}"),
    }
}

fn passed(evaluation: &Evaluation) -> bool {
    evaluation.status == ScenarioStatus::Planned || evaluation.verdict == Verdict::Pass
}

async fn run(repo: &Path, context: &Context, options: &Options) -> anyhow::Result<bool> {
    let capabilities = if let Some(hook) = &options.runtime_hook {
        let output = tokio::process::Command::new(hook)
            .arg("capabilities")
            .output()
            .await?;
        if !output.status.success() {
            bail!("runtime capabilities check failed");
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        vec!["network-isolation".to_owned(), "watchdog".to_owned()]
    };
    let supported = |s: &campaign::catalog::Scenario| match &s.stimulus {
        Stimulus::CanTrace {
            isolate, watchdog, ..
        } => {
            (!watchdog || capabilities.iter().any(|c| c == "watchdog"))
                && isolate.as_ref().is_none_or(|service| {
                    capabilities.iter().any(|c| {
                        c == if service == "can-link" {
                            "can-link-isolation"
                        } else {
                            "network-isolation"
                        }
                    })
                })
        }
        _ => true,
    };
    if options.all {
        for scenario in &context.catalog.scenarios {
            if !supported(scenario) {
                eprintln!(
                    "skipping {}: runtime lacks required isolation or watchdog capability",
                    scenario.id
                );
            }
        }
    }
    let ids: Vec<String> = if options.all {
        context
            .catalog
            .scenarios
            .iter()
            .filter(|s| !matches!(s.stimulus, Stimulus::External) && supported(s))
            .map(|s| s.id.clone())
            .collect()
    } else {
        options.positional.clone()
    };
    if ids.is_empty() {
        bail!("name a scenario or use --all");
    }
    for id in &ids {
        let scenario = context
            .catalog
            .scenario(id)
            .with_context(|| format!("unknown scenario {id}"))?;
        if !supported(scenario) {
            bail!("scenario {id} requires isolation or watchdog capability unavailable in this runtime");
        }
    }
    if !options.no_build && options.runtime_hook.is_none() {
        runner::build(repo).await?;
    }
    let campaign_id = options.run_id.clone().unwrap_or_else(timestamp_id);
    if !valid_id(&campaign_id) {
        bail!("invalid run ID");
    }
    let campaign_dir = options.out.join(&campaign_id);
    for id in &ids {
        if campaign_dir.join(id).exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "scenario evidence already exists; use a new run ID",
            )
            .into());
        }
    }
    // The plan lets a viewer such as the dashboard show the progress of a
    // running campaign: which scenarios are still to come.
    std::fs::create_dir_all(&campaign_dir)?;
    write_json(
        &campaign_dir.join("plan.json"),
        &serde_json::json!({
            "campaign_id": campaign_id,
            "started_at": humantime::format_rfc3339_seconds(SystemTime::now()).to_string(),
            "scenarios": ids,
        }),
    )?;
    let settings = Settings {
        repo: repo.to_path_buf(),
        fault_codes: context.fault_codes.clone(),
        entity: ENTITY.to_owned(),
        runtime_hook: options.runtime_hook.clone(),
        zenoh: options.zenoh.clone(),
        sovd: options.sovd.clone(),
    };
    let mut execution_failed = false;
    let mut evaluations = Vec::new();
    for id in &ids {
        let scenario = context.catalog.scenario(id).expect("checked above");
        let run_dir = campaign_dir.join(id);
        std::fs::create_dir_all(&campaign_dir)?;
        std::fs::create_dir(&run_dir)
            .context("scenario evidence already exists; use a new run ID")?;
        let manifest = manifest(
            repo,
            &format!("{campaign_id}/{id}"),
            id,
            "run",
            stimulus_json(&scenario.stimulus),
        )?;
        write_json(&run_dir.join("manifest.json"), &manifest)?;
        let recorder = Recorder::create(&run_dir.join("recording.jsonl"))?;
        eprintln!("running {id} …");
        let mut interrupted = false;
        if let Err(error) = runner::run(scenario, &settings, &run_dir, recorder).await {
            execution_failed = true;
            interrupted = error.is::<runner::Interrupted>();
            // The scenario is still judged: missing evidence makes it
            // INCONCLUSIVE, and the error stays next to the report.
            eprintln!("{id}: {error:#}");
            std::fs::write(run_dir.join("error.txt"), format!("{error:#}\n"))?;
        }
        let evaluation = judge(repo, context, &run_dir)?;
        println!("{}", report::console(&evaluation));
        evaluations.push((id.to_string(), evaluation));
        if interrupted {
            break;
        }
    }
    let summaries: Vec<_> = evaluations
        .iter()
        .map(|(dir, evaluation)| Summary {
            run_dir: dir.clone(),
            evaluation,
        })
        .collect();
    std::fs::write(
        campaign_dir.join("campaign.md"),
        report::campaign_markdown(&campaign_id, &summaries),
    )?;

    let judged: Vec<&Evaluation> = evaluations.iter().map(|(_, e)| e).collect();
    println!("{}", report::console_summary(&campaign_id, &judged));
    println!("evidence: {}", campaign_dir.join("campaign.md").display());
    Ok(!execution_failed && evaluations.iter().all(|(_, e)| passed(e)))
}

async fn observe(repo: &Path, context: &Context, options: &Options) -> anyhow::Result<bool> {
    let [id] = options.positional.as_slice() else {
        bail!("observe needs exactly one scenario");
    };
    let scenario = context
        .catalog
        .scenario(id)
        .with_context(|| format!("unknown scenario {id}"))?;
    let run_id = format!("{}-observe-{id}", timestamp_id());
    let run_dir = options.out.join(&run_id);
    std::fs::create_dir_all(&run_dir)?;
    let manifest = manifest(
        repo,
        &run_id,
        id,
        "observe",
        serde_json::json!({"type": "external", "zenoh": options.zenoh, "sovd": options.sovd}),
    )?;
    write_json(&run_dir.join("manifest.json"), &manifest)?;
    let recorder = Recorder::create(&run_dir.join("recording.jsonl"))?;
    let taps = record::start(
        Arc::clone(&recorder),
        &ZenohEndpoints {
            connect: vec![options.zenoh.clone()],
            mode: Some("client".to_owned()),
            ..Default::default()
        },
        Some(SovdTap {
            url: options.sovd.clone(),
            entity: ENTITY.to_owned(),
            codes: context.fault_codes.clone(),
        }),
    )
    .await?;
    eprintln!(
        "observing {id} for {} s; inject the fault now ({})",
        options.seconds, scenario.description
    );
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(options.seconds)) => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    drop(taps);
    let evaluation = judge(repo, context, &run_dir)?;
    println!("{}", report::console(&evaluation));
    println!("evidence: {}", run_dir.join("report.md").display());
    Ok(passed(&evaluation))
}

/// Evaluates a run directory and writes its reports.
fn judge(repo: &Path, context: &Context, run_dir: &Path) -> anyhow::Result<Evaluation> {
    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(run_dir.join("manifest.json")).context("manifest.json")?,
    )?;
    let scenario = context
        .catalog
        .scenario(&manifest.scenario)
        .with_context(|| format!("scenario {} not in the catalog", manifest.scenario))?;
    let observations = campaign::recording::read(&run_dir.join("recording.jsonl"))?;
    let mut evaluation = evaluate(
        scenario,
        &observations,
        &context.budgets,
        &context.onset,
        &context.classes,
    )
    .map_err(|error| anyhow::anyhow!(error))?;
    if evaluation.verdict == Verdict::Pass {
        if let Ok(error) = std::fs::read_to_string(run_dir.join("error.txt")) {
            evaluation.verdict = Verdict::Inconclusive;
            evaluation.reason = format!("execution did not complete: {}", error.trim());
        }
    }
    // Paired CAN evidence augments, rather than replaces, the shared safety
    // evaluator. Bad stimulus cannot be credited as a Guardian pass.
    if let Ok(bytes) = std::fs::read(run_dir.join("can-path.json")) {
        let path: serde_json::Value = serde_json::from_slice(&bytes)?;
        match path["classification"].as_str() {
            Some("source_failure") => {
                evaluation.verdict = Verdict::Inconclusive;
                evaluation.reason = "source integrity failed; inspect can-path.json".to_owned();
            }
            Some("transport_failure") => {
                evaluation.verdict = Verdict::Fail;
                evaluation.reason =
                    "measured CAN transport failed; inspect can-path.json".to_owned();
            }
            Some("evidence_incomplete") => {
                evaluation.verdict = Verdict::Inconclusive;
                evaluation.reason = "CAN evidence incomplete; inspect can-path.json".to_owned();
            }
            Some("verified") => {}
            _ => bail!("invalid can-path classification"),
        }
        let classification = if path["classification"] != "verified" {
            path["classification"].clone()
        } else if evaluation.verdict == Verdict::Fail {
            serde_json::json!("guardian_or_downstream_failure")
        } else if evaluation.verdict == Verdict::Inconclusive {
            serde_json::json!("application_evidence_incomplete")
        } else {
            serde_json::json!("verified")
        };
        write_json(
            &run_dir.join("failure-layer.json"),
            &serde_json::json!({
                "classification": classification,
                "can_path": path,
                "verdict": evaluation.verdict,
            }),
        )?;
    }
    let _ = repo;
    let report = Report {
        manifest: &manifest,
        hazard: scenario.hazard.as_deref(),
        safety_goal: scenario.safety_goal.as_deref(),
        description: &scenario.description,
        note: scenario.note.as_deref(),
        evaluation: &evaluation,
    };
    write_json(&run_dir.join("report.json"), &report)?;
    std::fs::write(
        run_dir.join("report.md"),
        report::markdown(&manifest, scenario, &evaluation),
    )?;
    Ok(evaluation)
}

fn manifest(
    repo: &Path,
    run_id: &str,
    scenario: &str,
    mode: &str,
    stimulus: serde_json::Value,
) -> anyhow::Result<Manifest> {
    let mut inputs = std::collections::BTreeMap::new();
    let mut files = vec![CATALOG.to_owned(), SAFETY_PARAMS.to_owned()];
    if let Some(trace) = stimulus["trace"].as_str() {
        files.push(trace.to_owned());
    }
    for file in files {
        let bytes = std::fs::read(repo.join(&file)).with_context(|| file.clone())?;
        inputs.insert(file, hex(&Sha256::digest(&bytes)));
    }
    Ok(Manifest {
        run_id: run_id.to_owned(),
        scenario: scenario.to_owned(),
        mode: mode.to_owned(),
        started_at: humantime::format_rfc3339_seconds(SystemTime::now()).to_string(),
        git_revision: git_revision(repo),
        stimulus,
        inputs,
    })
}

fn stimulus_json(stimulus: &Stimulus) -> serde_json::Value {
    serde_json::to_value(stimulus).unwrap_or(serde_json::Value::Null)
}

fn git_revision(repo: &Path) -> Option<String> {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let revision = match run(&["rev-parse", "--short", "HEAD"]) {
        Some(revision) => revision,
        None => {
            return std::fs::read_to_string(repo.join("git-revision.txt"))
                .ok()
                .map(|s| s.trim().to_owned())
        }
    };
    let dirty =
        run(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    Some(if dirty {
        format!("{revision}-dirty")
    } else {
        revision
    })
}

fn timestamp_id() -> String {
    let timestamp = humantime::format_rfc3339_nanos(SystemTime::now())
        .to_string()
        .replace([':', '-'], "")
        .replace('T', "-")
        .trim_end_matches('Z')
        .replace('.', "-");
    format!("{timestamp}-{}", std::process::id())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
