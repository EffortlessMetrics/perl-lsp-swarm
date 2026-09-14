#![expect(
    clippy::print_stdout,
    reason = "The runner-plan CLI prints exact one-line summaries; tracing is not the user-facing contract."
)]

//! Build and compare target-driven upstream runner discovery plans.

#[path = "../runner_plan/build.rs"]
mod build;
#[path = "../runner_plan/compare.rs"]
mod compare;
// The target-contract modules are shared verbatim with `perl-core-harness-targets`,
// which owns the topology-drift surface. This binary consumes only the target matrix,
// so the drift items are legitimately unused here.
#[allow(dead_code)]
#[path = "../target_contracts/contract.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../target_contracts/io.rs"]
mod io;
#[allow(dead_code)]
#[path = "../target_contracts/matrix.rs"]
mod matrix;
#[allow(dead_code)]
#[path = "../target_contracts/model.rs"]
mod model;
#[path = "../runner_plan/normalize.rs"]
mod normalize;
#[path = "../runner_plan/model.rs"]
mod runner_model;

use build::{
    DeclaredPlanInputs, build_runner_plan_with_frame, validate_runner_plan,
    validate_runner_plan_against,
};
use color_eyre::eyre::{Context, ContextCompat, Result, bail};
use compare::{compare_runner_plans_against, validate_runner_parity_against};
use io::read_matrix;
use runner_model::{DiscoveryFrame, RunnerKind, RunnerParityReport, RunnerPlan, RunnerScheduling};
use serde::Serialize;
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
        "check-plan" => check_plan_command(args.collect()),
        "check-parity" => check_parity_command(args.collect()),
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
    let (discovery_frame, scheduling) = parse_declaration_options(&args[5..], "")?;
    let matrix = read_matrix(&matrix_path)?;
    let raw = read_bytes(&discovery_path)?;
    let plan = build_runner_plan_with_frame(
        &matrix,
        &target_id,
        runner,
        &raw,
        discovery_frame,
        scheduling,
    )
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
    let sides = read_two_sided_arguments(&args)?;
    let report = compare_runner_plans_against(
        &sides.matrix,
        &sides.left.declared,
        &sides.left.plan,
        &sides.left.raw,
        &sides.right.declared,
        &sides.right.plan,
        &sides.right.raw,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    write_json(&sides.tail, &report)?;
    println!(
        "runner parity valid: target={} status={:?}",
        report.target_id, report.membership_status
    );
    Ok(())
}

fn check_plan_command(args: Vec<OsString>) -> Result<()> {
    if args.len() < 5 {
        bail!(usage());
    }
    let matrix = read_matrix(Path::new(&args[0]))?;
    let target_id = args[1].to_string_lossy().into_owned();
    let runner = RunnerKind::parse(&args[2].to_string_lossy())
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let raw = read_bytes(Path::new(&args[3]))?;
    let plan_path = PathBuf::from(&args[4]);
    let plan = read_plan(&plan_path)?;
    let (discovery_frame, scheduling) = parse_declaration_options(&args[5..], "")?;
    let declared = DeclaredPlanInputs::new(target_id, runner, discovery_frame, scheduling);
    validate_runner_plan_against(&matrix, &raw, &declared, &plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    println!("runner plan authority valid: {}", plan_path.display());
    Ok(())
}

fn check_parity_command(args: Vec<OsString>) -> Result<()> {
    let sides = read_two_sided_arguments(&args)?;
    validate_runner_plan_against(
        &sides.matrix,
        &sides.left.raw,
        &sides.left.declared,
        &sides.left.plan,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    validate_runner_plan_against(
        &sides.matrix,
        &sides.right.raw,
        &sides.right.declared,
        &sides.right.plan,
    )
    .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let report = read_parity(&sides.tail)?;
    validate_runner_parity_against(&report, &sides.left.plan, &sides.right.plan)
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    println!("runner parity authority valid: {}", sides.tail.display());
    Ok(())
}

/// One side of a two-sided command: its candidate plan, the exact raw
/// discovery bytes it claims, and the caller's declared reconstruction inputs.
struct SideArguments {
    declared: DeclaredPlanInputs,
    plan: RunnerPlan,
    raw: Vec<u8>,
}

/// The complete argument set of `compare` and `check-parity`.
struct TwoSidedArguments {
    matrix: model::UpstreamTargetMatrix,
    left: SideArguments,
    right: SideArguments,
    /// Output receipt for `compare`; parity receipt under check for
    /// `check-parity`.
    tail: PathBuf,
}

fn read_two_sided_arguments(args: &[OsString]) -> Result<TwoSidedArguments> {
    if args.len() < 9 {
        bail!(usage());
    }
    let matrix = read_matrix(Path::new(&args[0]))?;
    let target_id = args[1].to_string_lossy().into_owned();
    let left_runner = RunnerKind::parse(&args[2].to_string_lossy())
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let left_plan = read_plan(Path::new(&args[3]))?;
    let left_raw = read_bytes(Path::new(&args[4]))?;
    let right_runner = RunnerKind::parse(&args[5].to_string_lossy())
        .map_err(|error| color_eyre::eyre::eyre!(error))?;
    let right_plan = read_plan(Path::new(&args[6]))?;
    let right_raw = read_bytes(Path::new(&args[7]))?;
    let tail = PathBuf::from(&args[8]);
    let (left_options, right_options) = split_side_options(&args[9..])?;
    let (left_frame, left_scheduling) = parse_declaration_options(&left_options, "left-")?;
    let (right_frame, right_scheduling) = parse_declaration_options(&right_options, "right-")?;
    Ok(TwoSidedArguments {
        matrix,
        left: SideArguments {
            declared: DeclaredPlanInputs::new(
                target_id.clone(),
                left_runner,
                left_frame,
                left_scheduling,
            ),
            plan: left_plan,
            raw: left_raw,
        },
        right: SideArguments {
            declared: DeclaredPlanInputs::new(
                target_id,
                right_runner,
                right_frame,
                right_scheduling,
            ),
            plan: right_plan,
            raw: right_raw,
        },
        tail,
    })
}

/// Route `--left-*` and `--right-*` options to their side, keeping each flag
/// with the value it owns. Both sides declare independently: neither may
/// inherit the other's frame or scheduling.
fn split_side_options(args: &[OsString]) -> Result<(Vec<OsString>, Vec<OsString>)> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy().into_owned();
        let (side, flag) = if let Some(rest) = arg.strip_prefix("--left-") {
            (&mut left, format!("--{rest}"))
        } else if let Some(rest) = arg.strip_prefix("--right-") {
            (&mut right, format!("--{rest}"))
        } else {
            bail!("unsupported option {arg}; declare each side with --left-* and --right-*");
        };
        side.push(OsString::from(&flag));
        if matches!(flag.as_str(), "--frame" | "--jobs" | "--property") {
            index += 1;
            let value = args.get(index).with_context(|| format!("{arg} requires a value"))?;
            side.push(value.clone());
        }
        index += 1;
    }
    Ok((left, right))
}

/// Parse declared scheduling inputs. `prefix` is the side marker the operator
/// actually typed (`""`, `"left-"`, `"right-"`), so every error names the
/// option as it appeared on the command line rather than its stripped form.
fn parse_scheduling(args: &[OsString], prefix: &str) -> Result<RunnerScheduling> {
    let mut scheduling = RunnerScheduling::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        match arg.as_ref() {
            "--asap" => scheduling.asap = true,
            "--state-ordering" => scheduling.state_ordering = true,
            "--jobs" => {
                index += 1;
                let jobs_requirement = format!("--{prefix}jobs requires a positive integer");
                let value = args.get(index).context(jobs_requirement.clone())?;
                let jobs =
                    value.to_string_lossy().parse::<u32>().context(jobs_requirement.clone())?;
                if jobs == 0 {
                    bail!(jobs_requirement);
                }
                scheduling.jobs = Some(jobs);
            }
            "--property" => {
                index += 1;
                let property_requirement = format!("--{prefix}property requires key=value");
                let value = args.get(index).context(property_requirement.clone())?;
                let value = value.to_string_lossy();
                let (key, property) =
                    value.split_once('=').context(property_requirement.clone())?;
                if key.trim().is_empty() || property.trim().is_empty() {
                    bail!("--{prefix}property requires non-empty key=value");
                }
                if scheduling.properties.insert(key.to_string(), property.to_string()).is_some() {
                    bail!("duplicate scheduling property {key}");
                }
            }
            other => {
                let other = other.strip_prefix("--").unwrap_or(other);
                bail!("unsupported scheduling option --{prefix}{other}");
            }
        }
        index += 1;
    }
    Ok(scheduling)
}

/// Parse one side's declared reconstruction options: the discovery frame the
/// raw bytes are spelled in plus the declared scheduling inputs. Both `build`
/// and the checking commands consume the same declaration, so a plan is
/// checked against what its operator declared rather than against itself.
fn parse_declaration_options(
    args: &[OsString],
    prefix: &str,
) -> Result<(DiscoveryFrame, RunnerScheduling)> {
    let mut frame = None;
    let mut scheduling_args = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index].to_string_lossy() == "--frame" {
            index += 1;
            let value = args
                .get(index)
                .with_context(|| format!("--{prefix}frame requires a discovery frame"))?;
            frame = Some(match value.to_string_lossy().as_ref() {
                "runner_t_directory_relative" => DiscoveryFrame::RunnerTDirectoryRelative,
                "repository_root_relative" => DiscoveryFrame::RepositoryRootRelative,
                "canonical_repository_path" => DiscoveryFrame::CanonicalRepositoryPath,
                other => bail!("unsupported discovery frame {other}"),
            });
        } else {
            scheduling_args.push(args[index].clone());
        }
        index += 1;
    }
    let frame = frame.with_context(|| {
        format!("--{prefix}frame is required; declare the raw discovery path frame")
    })?;
    Ok((frame, parse_scheduling(&scheduling_args, prefix)?))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {}", path.display()))
}

fn read_plan(path: &Path) -> Result<RunnerPlan> {
    let bytes = read_bytes(path)?;
    let plan: RunnerPlan = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding runner plan {}", path.display()))?;
    validate_runner_plan(&plan).map_err(|error| color_eyre::eyre::eyre!(error))?;
    Ok(plan)
}

fn read_parity(path: &Path) -> Result<RunnerParityReport> {
    let bytes = read_bytes(path)?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding runner parity {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).context("serializing runner receipt")?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn usage() -> &'static str {
    "usage: perl-core-harness-runner-plan build <matrix> <target-id> <test|harness|direct_fallback> <raw-discovery> <output> --frame runner_t_directory_relative|repository_root_relative|canonical_repository_path [--jobs N] [--asap] [--state-ordering] [--property key=value] | check-plan <matrix> <target-id> <test|harness|direct_fallback> <raw-discovery> <plan> --frame <frame> [--jobs N] [--asap] [--state-ordering] [--property key=value] | compare <matrix> <target-id> <left-runner> <left-plan> <left-raw-discovery> <right-runner> <right-plan> <right-raw-discovery> <output> --left-frame <frame> --right-frame <frame> [--left-jobs N] [--left-asap] [--left-state-ordering] [--left-property key=value] [--right-jobs N] [--right-asap] [--right-state-ordering] [--right-property key=value] | check-parity <matrix> <target-id> <left-runner> <left-plan> <left-raw-discovery> <right-runner> <right-plan> <right-raw-discovery> <parity-report> --left-frame <frame> --right-frame <frame> [side scheduling options]"
}

#[cfg(test)]
#[path = "../runner_plan/tests.rs"]
mod tests;
