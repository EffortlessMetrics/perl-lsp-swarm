//! CLI proof for the runner-plan binary: argument contracts, declared
//! scheduling parsing, receipt writing, and exact summary output.

use perl_tdd_support::{must, must_some};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const USAGE_FRAGMENT: &str = "usage: perl-core-harness-runner-plan build <matrix>";

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn runner_bin() -> &'static str {
    env!("CARGO_BIN_EXE_perl-core-harness-runner-plan")
}

fn run(args: &[&str]) -> TestResult<Output> {
    Ok(Command::new(runner_bin()).args(args).output()?)
}

fn bundle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".ci/perl-core-harness/upstream-targets-5.42.2.v1")
}

fn write_discovery(dir: &Path, name: &str, contents: &[u8]) -> TestResult<PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, contents)?;
    Ok(path)
}

fn discovery_paths(dir: &Path) -> TestResult<String> {
    let single = dir.join("single.txt");
    std::fs::write(&single, b"t/base/if.t\n")?;
    Ok(single.to_string_lossy().into_owned())
}

fn build_invocation<'a>(
    matrix: &'a str,
    target: &'a str,
    runner: &'a str,
    raw: &'a str,
    output: &'a str,
    extra: &[&'a str],
) -> Vec<&'a str> {
    let mut args = Vec::with_capacity(8 + extra.len());
    args.extend_from_slice(&["build", matrix, target, runner, raw, output]);
    args.extend_from_slice(&["--frame", "canonical_repository_path"]);
    args.extend_from_slice(extra);
    args
}

fn assert_cli_failure(output: &Output, fragment: &str) {
    assert!(!output.status.success(), "expected CLI failure, but it succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(fragment), "unexpected stderr: {stderr}");
}

#[test]
fn missing_command_prints_usage_and_fails() -> TestResult {
    let output = run(&[])?;
    assert_cli_failure(&output, USAGE_FRAGMENT);
    Ok(())
}

#[test]
fn unsupported_command_is_named_with_usage() -> TestResult {
    let output = run(&["teleport"])?;
    assert_cli_failure(&output, "unsupported command teleport;");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(USAGE_FRAGMENT), "unexpected stderr: {stderr}");
    Ok(())
}

#[test]
fn build_with_too_few_arguments_prints_usage() -> TestResult {
    let output = run(&["build", "only-matrix-and-target"])?;
    assert_cli_failure(&output, USAGE_FRAGMENT);
    Ok(())
}

#[test]
fn compare_argument_count_prints_usage() -> TestResult {
    let output = run(&["compare", "one", "two"])?;
    assert_cli_failure(&output, USAGE_FRAGMENT);
    Ok(())
}

#[test]
fn check_plan_argument_count_prints_usage() -> TestResult {
    let output = run(&["check-plan", "only-matrix"])?;
    assert_cli_failure(&output, USAGE_FRAGMENT);
    Ok(())
}

#[test]
fn check_parity_argument_count_prints_usage() -> TestResult {
    let output = run(&["check-parity"])?;
    assert_cli_failure(&output, USAGE_FRAGMENT);
    Ok(())
}

#[test]
fn build_requires_an_explicit_discovery_frame() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&["build", &matrix, "component_base", "test", &raw, &out])?;
    assert_cli_failure(&output, "--frame is required; declare the raw discovery path frame");
    Ok(())
}

#[test]
fn frame_requires_a_discovery_frame_value() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&["build", &matrix, "component_base", "test", &raw, &out, "--frame"])?;
    assert_cli_failure(&output, "--frame requires a discovery frame");
    Ok(())
}

#[test]
fn unsupported_discovery_frame_is_named() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        "--frame",
        "pre_12262_implicit_path",
    ])?;
    assert_cli_failure(&output, "unsupported discovery frame pre_12262_implicit_path");
    Ok(())
}

#[test]
fn unsupported_scheduling_option_is_named() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output =
        run(&build_invocation(&matrix, "component_base", "test", &raw, &out, &["--bogus"]))?;
    assert_cli_failure(&output, "unsupported scheduling option --bogus");
    Ok(())
}

#[test]
fn jobs_requires_a_positive_integer() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let zero =
        run(&build_invocation(&matrix, "component_base", "test", &raw, &out, &["--jobs", "0"]))?;
    assert_cli_failure(&zero, "--jobs requires a positive integer");

    let nonnumeric = run(&build_invocation(
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        &["--jobs", "seven"],
    ))?;
    assert_cli_failure(&nonnumeric, "--jobs requires a positive integer");
    Ok(())
}

#[test]
fn property_requires_key_equals_value() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&build_invocation(
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        &["--property", "lane"],
    ))?;
    assert_cli_failure(&output, "--property requires key=value");
    Ok(())
}

#[test]
fn duplicate_scheduling_property_is_rejected() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&build_invocation(
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        &["--property", "lane=nightly", "--property", "lane=smoke"],
    ))?;
    assert_cli_failure(&output, "duplicate scheduling property lane");
    Ok(())
}

#[test]
fn build_writes_plan_and_prints_exact_summary() -> TestResult {
    let dir = tempfile::tempdir()?;
    let raw = write_discovery(dir.path(), "raw.txt", b"t/base/if.t\nt/base/cond.t\n")?;
    let plan_path = dir.path().join("plan.json");
    let output = run(&[
        "build",
        bundle().to_string_lossy().as_ref(),
        "component_base",
        "test",
        raw.to_string_lossy().as_ref(),
        plan_path.to_string_lossy().as_ref(),
        "--asap",
        "--jobs",
        "3",
        "--state-ordering",
        "--property",
        "lane=nightly",
        "--frame",
        "canonical_repository_path",
    ])?;
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout, "runner plan valid: target=component_base runner=Test files=2\n",
        "unexpected stdout: {stdout}"
    );

    let plan: serde_json::Value = must(serde_json::from_slice(&std::fs::read(&plan_path)?));
    assert_eq!(plan["schema_version"], "perl_core_harness.runner_plan.v2");
    assert_eq!(plan["discovery_frame"], "canonical_repository_path");
    assert_eq!(plan["normalization_schema"], "perl_core_harness.source_identity.v2");
    assert_eq!(plan["target_id"], "component_base");
    assert_eq!(plan["runner"], "test");
    assert_eq!(plan["normalized_membership"], serde_json::json!(["t/base/cond.t", "t/base/if.t"]));
    assert_eq!(plan["scheduling"]["jobs"], 3);
    assert_eq!(plan["scheduling"]["asap"], true);
    assert_eq!(plan["scheduling"]["state_ordering"], true);
    assert_eq!(plan["scheduling"]["properties"]["lane"], "nightly");
    let limitations = must_some(plan["limitations"].as_array());
    assert!(
        limitations
            .iter()
            .any(|value| value
                == "raw_discovery_stream_is_declared_input_not_observed_runner_output")
    );
    Ok(())
}

#[test]
fn build_applies_runner_t_directory_relative_frame() -> TestResult {
    let dir = tempfile::tempdir()?;
    let raw = write_discovery(dir.path(), "raw.txt", b"base/if.t\n")?;
    let matrix = bundle().to_string_lossy().into_owned();
    let plan_path = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        raw.to_string_lossy().as_ref(),
        &plan_path,
        "--frame",
        "runner_t_directory_relative",
    ])?;
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
    let plan: serde_json::Value = must(serde_json::from_slice(&std::fs::read(&plan_path)?));
    assert_eq!(plan["discovery_frame"], "runner_t_directory_relative");
    assert_eq!(plan["normalized_membership"], serde_json::json!(["t/base/if.t"]));
    Ok(())
}

#[test]
fn mismatched_discovery_frame_is_rejected() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let output = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        "--frame",
        "runner_t_directory_relative",
    ])?;
    assert_cli_failure(&output, "is outside target component_base");
    Ok(())
}

#[test]
fn compare_prints_exact_parity_status() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let left = dir.path().join("left.json").to_string_lossy().into_owned();
    let right = dir.path().join("right.json").to_string_lossy().into_owned();
    let built_test = run(&build_invocation(&matrix, "component_base", "test", &raw, &left, &[]))?;
    assert!(built_test.status.success(), "left build failed");
    let built_harness =
        run(&build_invocation(&matrix, "component_base", "harness", &raw, &right, &[]))?;
    assert!(built_harness.status.success(), "right build failed");

    let report = dir.path().join("parity.json");
    let output =
        run(&["compare", &matrix, &left, &raw, &right, &raw, report.to_string_lossy().as_ref()])?;
    assert!(output.status.success(), "compare failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "runner parity valid: target=component_base status=Parity\n");

    let parsed: serde_json::Value = must(serde_json::from_slice(&std::fs::read(&report)?));
    assert_eq!(parsed["membership_status"], "parity");
    assert_eq!(parsed["left_runner"], "test");
    assert_eq!(parsed["right_runner"], "harness");
    Ok(())
}

#[test]
fn check_plan_revalidates_built_receipt_and_rejects_tampering() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let plan_path = dir.path().join("plan.json").to_string_lossy().into_owned();
    let built = run(&build_invocation(&matrix, "component_base", "test", &raw, &plan_path, &[]))?;
    assert!(built.status.success(), "build failed");

    let checked = run(&["check-plan", &matrix, &raw, &plan_path])?;
    assert!(
        checked.status.success(),
        "check-plan failed: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let stdout = String::from_utf8_lossy(&checked.stdout);
    assert_eq!(
        stdout,
        format!("runner plan authority valid: {}\n", dir.path().join("plan.json").display())
    );

    let mut forged: serde_json::Value =
        must(serde_json::from_slice(&std::fs::read(dir.path().join("plan.json"))?));
    forged["runner_entrypoint"] = "t/wrong".into();
    let forged_path = dir.path().join("forged.json");
    std::fs::write(&forged_path, serde_json::to_vec(&forged)?)?;
    let rejected = run(&["check-plan", &matrix, &raw, forged_path.to_string_lossy().as_ref()])?;
    assert_cli_failure(&rejected, "runner plan entrypoint t/wrong disagrees with");
    Ok(())
}

#[test]
fn check_parity_revalidates_report_and_rejects_tampering() -> TestResult {
    let dir = tempfile::tempdir()?;
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path())?;
    let left = dir.path().join("left.json").to_string_lossy().into_owned();
    let right = dir.path().join("right.json").to_string_lossy().into_owned();
    let report = dir.path().join("parity.json");
    let report_arg = report.to_string_lossy().into_owned();

    let built_left = run(&build_invocation(&matrix, "component_base", "test", &raw, &left, &[]))?;
    let built_right =
        run(&build_invocation(&matrix, "component_base", "harness", &raw, &right, &[]))?;
    assert!(built_left.status.success() && built_right.status.success(), "builds failed");

    let compared = run(&["compare", &matrix, &left, &raw, &right, &raw, &report_arg])?;
    assert!(compared.status.success(), "compare failed");

    let checked = run(&["check-parity", &matrix, &left, &raw, &right, &raw, &report_arg])?;
    assert!(
        checked.status.success(),
        "check-parity failed: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let stdout = String::from_utf8_lossy(&checked.stdout);
    assert_eq!(stdout, format!("runner parity authority valid: {}\n", report.display()));

    let mut forged: serde_json::Value = must(serde_json::from_slice(&std::fs::read(&report)?));
    forged["membership_status"] = "mismatch".into();
    std::fs::write(&report, serde_json::to_vec(&forged)?)?;
    let rejected = run(&["check-parity", &matrix, &left, &raw, &right, &raw, &report_arg])?;
    assert_cli_failure(&rejected, "mismatch");
    Ok(())
}
