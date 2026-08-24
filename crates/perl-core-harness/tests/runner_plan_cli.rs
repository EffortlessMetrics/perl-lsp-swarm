//! CLI proof for the runner-plan binary: argument contracts, declared
//! scheduling parsing, receipt writing, and exact summary output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const USAGE_FRAGMENT: &str = "usage: perl-core-harness-runner-plan build <matrix>";

fn runner_bin() -> &'static str {
    env!("CARGO_BIN_EXE_perl-core-harness-runner-plan")
}

fn run(args: &[&str]) -> Output {
    Command::new(runner_bin()).args(args).output().expect("runner-plan binary must spawn")
}

fn bundle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".ci/perl-core-harness/upstream-targets-5.42.2.v1")
}

fn write_discovery(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("discovery fixture must write");
    path
}

fn discovery_paths(dir: &Path) -> String {
    let single = dir.join("single.txt");
    std::fs::write(&single, b"t/base/if.t\n").expect("discovery fixture must write");
    single.to_string_lossy().into_owned()
}

fn expect_failure(output: &Output, fragment: &str) {
    assert!(!output.status.success(), "expected CLI failure, but it succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(fragment), "unexpected stderr: {stderr}");
}

#[test]
fn missing_command_prints_usage_and_fails() {
    let output = run(&[]);
    expect_failure(&output, USAGE_FRAGMENT);
}

#[test]
fn unsupported_command_is_named_with_usage() {
    let output = run(&["teleport"]);
    expect_failure(&output, "unsupported command teleport;");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(USAGE_FRAGMENT), "unexpected stderr: {stderr}");
}

#[test]
fn build_with_too_few_arguments_prints_usage() {
    let output = run(&["build", "only-matrix-and-target"]);
    expect_failure(&output, USAGE_FRAGMENT);
}

#[test]
fn compare_argument_count_prints_usage() {
    let output = run(&["compare", "one", "two"]);
    expect_failure(&output, USAGE_FRAGMENT);
}

#[test]
fn check_plan_argument_count_prints_usage() {
    let output = run(&["check-plan", "only-matrix"]);
    expect_failure(&output, USAGE_FRAGMENT);
}

#[test]
fn check_parity_argument_count_prints_usage() {
    let output = run(&["check-parity"]);
    expect_failure(&output, USAGE_FRAGMENT);
}

#[test]
fn unsupported_build_option_is_named() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run(&[
        "build",
        bundle().to_string_lossy().as_ref(),
        "component_base",
        "test",
        discovery_paths(dir.path()).as_str(),
        dir.path().join("plan.json").to_string_lossy().as_ref(),
        "--bogus",
    ]);
    expect_failure(&output, "unsupported build option --bogus");
}

#[test]
fn build_requires_an_explicit_discovery_frame() {
    let dir = tempfile::tempdir().expect("tempdir");
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path());
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let missing = run(&["build", &matrix, "component_base", "test", &raw, &out]);
    expect_failure(
        &missing,
        "--frame is required; expected runner-t-directory-relative, repository-root-relative, or canonical-repository-path",
    );

    let unsupported =
        run(&["build", &matrix, "component_base", "test", &raw, &out, "--frame", "teleport"]);
    expect_failure(&unsupported, "unsupported discovery frame teleport");

    let duplicated = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        "--frame",
        "canonical-repository-path",
        "--frame",
        "repository-root-relative",
    ]);
    expect_failure(&duplicated, "duplicate --frame");
}

#[test]
fn jobs_requires_a_positive_integer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path());
    let out = dir.path().join("plan.json").to_string_lossy().into_owned();
    let zero = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        "--frame",
        "canonical-repository-path",
        "--jobs",
        "0",
    ]);
    expect_failure(&zero, "--jobs requires a positive integer");

    let nonnumeric = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &out,
        "--frame",
        "canonical-repository-path",
        "--jobs",
        "seven",
    ]);
    expect_failure(&nonnumeric, "--jobs requires a positive integer");
}

#[test]
fn property_requires_key_equals_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run(&[
        "build",
        bundle().to_string_lossy().as_ref(),
        "component_base",
        "test",
        discovery_paths(dir.path()).as_str(),
        dir.path().join("plan.json").to_string_lossy().as_ref(),
        "--property",
        "lane",
    ]);
    expect_failure(&output, "--property requires key=value");
}

#[test]
fn duplicate_scheduling_property_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run(&[
        "build",
        bundle().to_string_lossy().as_ref(),
        "component_base",
        "test",
        discovery_paths(dir.path()).as_str(),
        dir.path().join("plan.json").to_string_lossy().as_ref(),
        "--property",
        "lane=nightly",
        "--property",
        "lane=smoke",
    ]);
    expect_failure(&output, "duplicate scheduling property lane");
}

#[test]
fn build_writes_plan_and_prints_exact_summary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = write_discovery(dir.path(), "raw.txt", b"t/base/if.t\nt/base/cond.t\n");
    let plan_path = dir.path().join("plan.json");
    let output = run(&[
        "build",
        bundle().to_string_lossy().as_ref(),
        "component_base",
        "test",
        raw.to_string_lossy().as_ref(),
        plan_path.to_string_lossy().as_ref(),
        "--frame",
        "canonical-repository-path",
        "--asap",
        "--jobs",
        "3",
        "--state-ordering",
        "--property",
        "lane=nightly",
    ]);
    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout, "runner plan valid: target=component_base runner=Test files=2\n",
        "unexpected stdout: {stdout}"
    );

    let plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan_path).expect("plan receipt must exist"))
            .expect("plan receipt must be valid JSON");
    assert_eq!(plan["schema_version"], "perl_core_harness.runner_plan.v2");
    assert_eq!(plan["target_id"], "component_base");
    assert_eq!(plan["runner"], "test");
    assert_eq!(plan["source_items"][0]["discovery_frame"], "canonical_repository_path");
    assert_eq!(
        plan["source_items"][0]["normalization_version"],
        "perl_core_harness.runner_source_normalization.v2"
    );
    assert_eq!(plan["normalized_membership"], serde_json::json!(["t/base/cond.t", "t/base/if.t"]));
    assert_eq!(plan["scheduling"]["jobs"], 3);
    assert_eq!(plan["scheduling"]["asap"], true);
    assert_eq!(plan["scheduling"]["state_ordering"], true);
    assert_eq!(plan["scheduling"]["properties"]["lane"], "nightly");
    let limitations = plan["limitations"].as_array().expect("limitations array");
    assert!(
        limitations
            .iter()
            .any(|value| value
                == "raw_discovery_stream_is_declared_input_not_observed_runner_output")
    );
}

#[test]
fn compare_prints_exact_parity_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path());
    let left = dir.path().join("left.json").to_string_lossy().into_owned();
    let right = dir.path().join("right.json").to_string_lossy().into_owned();
    let built_test = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &left,
        "--frame",
        "canonical-repository-path",
    ]);
    assert!(built_test.status.success(), "left build failed");
    let built_harness = run(&[
        "build",
        &matrix,
        "component_base",
        "harness",
        &raw,
        &right,
        "--frame",
        "canonical-repository-path",
    ]);
    assert!(built_harness.status.success(), "right build failed");

    let report = dir.path().join("parity.json");
    let output =
        run(&["compare", &matrix, &left, &raw, &right, &raw, report.to_string_lossy().as_ref()]);
    assert!(output.status.success(), "compare failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "runner parity valid: target=component_base status=Parity\n");

    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).expect("parity receipt")).unwrap();
    assert_eq!(parsed["membership_status"], "parity");
    assert_eq!(parsed["left_runner"], "test");
    assert_eq!(parsed["right_runner"], "harness");
}

#[test]
fn check_plan_revalidates_built_receipt_and_rejects_tampering() {
    let dir = tempfile::tempdir().expect("tempdir");
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path());
    let plan_path = dir.path().join("plan.json").to_string_lossy().into_owned();
    let built = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        &plan_path,
        "--frame",
        "canonical-repository-path",
    ]);
    assert!(built.status.success(), "build failed");

    let checked = run(&["check-plan", &matrix, &raw, &plan_path]);
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

    let mut forged = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(dir.path().join("plan.json")).expect("plan receipt"),
    )
    .expect("valid JSON");
    forged["runner_entrypoint"] = "t/wrong".into();
    let forged_path = dir.path().join("forged.json");
    std::fs::write(&forged_path, serde_json::to_vec(&forged).unwrap()).expect("write forged");
    let rejected = run(&["check-plan", &matrix, &raw, forged_path.to_string_lossy().as_ref()]);
    expect_failure(&rejected, "runner plan entrypoint t/wrong disagrees with");

    // Forging one row's declared discovery frame must also fail closed: the
    // stored canonical identity no longer matches its frame re-derivation.
    let mut forged_frame = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(dir.path().join("plan.json")).expect("plan receipt"),
    )
    .expect("valid JSON");
    forged_frame["source_items"][0]["discovery_frame"] = "runner_t_directory_relative".into();
    let forged_frame_path = dir.path().join("forged-frame.json");
    std::fs::write(&forged_frame_path, serde_json::to_vec(&forged_frame).unwrap())
        .expect("write forged frame");
    let rejected_frame =
        run(&["check-plan", &matrix, &raw, forged_frame_path.to_string_lossy().as_ref()]);
    expect_failure(&rejected_frame, "discovery-frame normalization");
}

#[test]
fn check_parity_revalidates_report_and_rejects_tampering() {
    let dir = tempfile::tempdir().expect("tempdir");
    let matrix = bundle().to_string_lossy().into_owned();
    let raw = discovery_paths(dir.path());
    let left = dir.path().join("left.json");
    let right = dir.path().join("right.json");
    let report = dir.path().join("parity.json");

    let built_left = run(&[
        "build",
        &matrix,
        "component_base",
        "test",
        &raw,
        left.to_string_lossy().as_ref(),
        "--frame",
        "canonical-repository-path",
    ]);
    let built_right = run(&[
        "build",
        &matrix,
        "component_base",
        "harness",
        &raw,
        right.to_string_lossy().as_ref(),
        "--frame",
        "canonical-repository-path",
    ]);
    assert!(built_left.status.success() && built_right.status.success(), "builds failed");

    let compared = run(&[
        "compare",
        &matrix,
        left.to_string_lossy().as_ref(),
        &raw,
        right.to_string_lossy().as_ref(),
        &raw,
        report.to_string_lossy().as_ref(),
    ]);
    assert!(compared.status.success(), "compare failed");

    let left_arg = left.to_string_lossy().into_owned();
    let right_arg = right.to_string_lossy().into_owned();
    let checked = run(&[
        "check-parity",
        &matrix,
        &left_arg,
        &raw,
        &right_arg,
        &raw,
        report.to_string_lossy().as_ref(),
    ]);
    assert!(
        checked.status.success(),
        "check-parity failed: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let stdout = String::from_utf8_lossy(&checked.stdout);
    assert_eq!(stdout, format!("runner parity authority valid: {}\n", report.display()));

    let mut forged = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(&report).expect("parity receipt"),
    )
    .expect("valid JSON");
    forged["membership_status"] = "mismatch".into();
    std::fs::write(&report, serde_json::to_vec(&forged).unwrap()).expect("write forged");
    let rejected = run(&[
        "check-parity",
        &matrix,
        &left_arg,
        &raw,
        &right_arg,
        &raw,
        report.to_string_lossy().as_ref(),
    ]);
    expect_failure(&rejected, "mismatch");
}
