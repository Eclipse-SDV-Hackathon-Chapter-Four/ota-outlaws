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

//! `campaign` command line. Run from the repository root.
//!
//! ```text
//! campaign run <scenario>... | --all [--no-build] [--out runs]
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
    match dispatch(&args).await {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(2);
        }
    }
}

struct Options {
    positional: Vec<String>,
    all: bool,
    no_build: bool,
    out: PathBuf,
    seconds: u64,
    zenoh: String,
    sovd: String,
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
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().context(format!("{arg} needs a value"));
        match arg.as_str() {
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
                println!(
                    "{}: {:?} — {}",
                    evaluation.scenario, evaluation.verdict, evaluation.reason
                );
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
    let ids: Vec<String> = if options.all {
        context
            .catalog
            .scenarios
            .iter()
            .filter(|s| !matches!(s.stimulus, Stimulus::External))
            .map(|s| s.id.clone())
            .collect()
    } else {
        options.positional.clone()
    };
    if ids.is_empty() {
        bail!("name a scenario or use --all");
    }
    for id in &ids {
        if context.catalog.scenario(id).is_none() {
            bail!("unknown scenario {id}");
        }
    }
    if !options.no_build {
        runner::build(repo).await?;
    }
    let campaign_id = timestamp_id();
    let campaign_dir = options.out.join(&campaign_id);
    let settings = Settings {
        repo: repo.to_path_buf(),
        fault_codes: context.fault_codes.clone(),
        entity: ENTITY.to_owned(),
    };
    let mut evaluations = Vec::new();
    for id in &ids {
        let scenario = context.catalog.scenario(id).expect("checked above");
        let run_dir = campaign_dir.join(id);
        std::fs::create_dir_all(&run_dir)?;
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
        if let Err(error) = runner::run(scenario, &settings, &run_dir, recorder).await {
            // The scenario is still judged: missing evidence makes it
            // INCONCLUSIVE, and the error stays next to the report.
            eprintln!("{id}: {error:#}");
            std::fs::write(run_dir.join("error.txt"), format!("{error:#}\n"))?;
        }
        let evaluation = judge(repo, context, &run_dir)?;
        println!("{id}: {:?} — {}", evaluation.verdict, evaluation.reason);
        evaluations.push((id.to_string(), evaluation));
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
    println!("evidence: {}", campaign_dir.display());
    Ok(evaluations.iter().all(|(_, e)| passed(e)))
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
    println!("{id}: {:?} — {}", evaluation.verdict, evaluation.reason);
    println!("evidence: {}", run_dir.display());
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
    let evaluation = evaluate(scenario, &observations, &context.budgets, &context.onset)
        .map_err(|error| anyhow::anyhow!(error))?;
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
    let revision = run(&["rev-parse", "--short", "HEAD"])?;
    let dirty =
        run(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    Some(if dirty {
        format!("{revision}-dirty")
    } else {
        revision
    })
}

fn timestamp_id() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now())
        .to_string()
        .replace([':', '-'], "")
        .replace('T', "-")
        .trim_end_matches('Z')
        .to_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
