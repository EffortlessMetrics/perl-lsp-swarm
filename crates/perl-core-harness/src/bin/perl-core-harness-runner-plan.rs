#![allow(clippy::print_stdout)]

//! Build and compare target-driven upstream runner discovery plans.

#[path = "../target_contracts/model.rs"]
mod model;
#[path = "../target_contracts/contract.rs"]
mod contract;
#[path = "../target_contracts/matrix.rs"]
mod matrix;
#[path = "../target_contracts/io.rs"]
mod io;
#[path = "../runner_plan/model.rs"]
mod runner_model;
#[path = "../runner_plan/normalize.rs"]
mod normalize;
#[path = "../runner_plan/build.rs"]
mod build;
#[path = "../runner_plan/compare.rs"]
mod compare;

use build::{build_runner_plan, validate_runner_plan};
use color_eyre::eyre::{Context, Result, bail};
use compare::{compare_runner_plans, validate_runner_parity};
use io::read_matrix;
use runner_model::{
    RUNNER_PARITY_SCHEMA_VERSION, RUNNER_PLAN_SCHEMA_VERSION, RunnerKind,
    RunnerParityReport, RunnerPlan, RunnerScheduling,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        bail!(usage());
    };
    match command.to_string_lossy().as_ref() {
        "build" => build_command(args.collect()),
        "compare" => compare_command(args.collect()),
        "check" => check_command(args.collect()),
        other => bail!("unsupported command {other}; {}", usage()),
    }
}

fn build_command(args: Vec<OsString>) -> Result<()> {
    if args.len() < 5 {
        bail!(usage());
    }
    let matrix_path = PathBuf::from(&args[0]);
    let target_id = args[1].to_string_lossy().into_owned();
    let runner = RunnerKind::parse(&args[2].to_string_lossy())
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let discovery_path = PathBuf::from(&args[3]);
    let output_path = PathBuf::from(&args[4]);
    let scheduling = parse_scheduling(&args[5..])?;
    let matrix = read_matrix(&matrix_path)?;
    let raw = fs::read(&discovery_path)
        .with_context(|| format!("reading {}", discovery_path.display()))?;
    let plan = build_runner_plan(&matrix, &target_id, runner, &raw, scheduling)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    write_json(&output_path, &plan)?;
    println!(
        "runner plan valid: target={} runner={:?} files={}",
        plan.target_id,
        plan.runner,
        plan.normalized_membership.len()
    );
    Ok(())
}

fn compare_command(args: Vec<OsString>) -> Result<()> {
    if args.len() != 3 {
        bail!(usage());
    }
    let left = read_plan(Path::new(&args[0]))?;
    let right = read_plan(Path::new(&args[1]))?;
    let output = PathBuf::from(&args[2]);
    let report = compare_runner_plans(&left, &right)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    write_json(&output, &report)?;
    println!(
        "runner parity valid: target={} status={:?}",
        report.target_id, report.membership_status
    );
    Ok(())
}

fn check_command(args: Vec<OsString>) -> Result<()> {
    if args.len() != 1 {
        bail!(usage());
    }
    let path = PathBuf::from(&args[0]);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding {}", path.display()))?;
    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .context("receipt has no string schema_version")?;
    match schema {
        RUNNER_PLAN_SCHEMA_VERSION => {
            let plan: RunnerPlan = serde_json::from_value(value)
                .with_context(|| format!("decoding runner plan {}", path.display()))?;
            validate_runner_plan(&plan).map_err(|error| color_eyre::eyre::eyre!(error))?;
        }
        RUNNER_PARITY_SCHEMA_VERSION => {
            let report: RunnerParityReport = serde_json::from_value(value)
                .with_context(|| format!("decoding runner parity {}", path.display()))?;
            validate_runner_parity(&report)
                .map_err(|error| color_eyre::eyre::eyre!(error))?;
        }
        other => bail!("unsupported runner receipt schema {other}"),
    }
    println!("runner receipt valid: {}", path.display());
    Ok(())
}

fn parse_scheduling(args: &[OsString]) -> Result<RunnerScheduling> {
    let mut scheduling = RunnerScheduling::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        match arg.as_ref() {
            "--asap" => scheduling.asap = true,
            "--state-ordering" => scheduling.state_ordering = true,
            "--jobs" => {
                index += 1;
                let value = args.get(index).context("--jobs requires a positive integer")?;
                let jobs = value
                    .to_string_lossy()
                    .parse::<u32>()
                    .context("--jobs requires a positive integer")?;
                if jobs == 0 {
                    bail!("--jobs requires a positive integer");
                }
                scheduling.jobs = Some(jobs);
            }
            "--property" => {
                index += 1;
                let value = args.get(index).context("--property requires key=value")?;
                let value = value.to_string_lossy();
                let (key, property) = value
                    .split_once('=')
                    .context("--property requires key=value")?;
                if key.trim().is_empty() || property.trim().is_empty() {
                    bail!("--property requires non-empty key=value");
                }
                if scheduling
                    .properties
                    .insert(key.to_string(), property.to_string())
                    .is_some()
                {
                    bail!("duplicate scheduling property {key}");
                }
            }
            other => bail!("unsupported scheduling option {other}"),
        }
        index += 1;
    }
    Ok(scheduling)
}

fn read_plan(path: &Path) -> Result<RunnerPlan> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let plan: RunnerPlan = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding runner plan {}", path.display()))?;
    validate_runner_plan(&plan).map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(plan)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).context("serializing runner receipt")?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn usage() -> &'static str {
    "usage: perl-core-harness-runner-plan build <matrix> <target-id> <test|harness|direct_fallback> <raw-discovery> <output> [--jobs N] [--asap] [--state-ordering] [--property key=value] | compare <left-plan> <right-plan> <output> | check <receipt>"
}

#[cfg(test)]
#[path = "../runner_plan/tests.rs"]
mod tests;
