//! Discriminating proof for the lean transition classify CLI arg-parse slice.

use serde_json::Value;
use std::process::Command;

#[test]
fn classify_cli_rejects_missing_required_option() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args(["classify", "--accepted-baseline", "accepted.json", "--compile", "compile.json"])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required option --output was not supplied"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn classify_cli_rejects_unknown_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args(["check"])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown perl-core-harness-transition command"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn classify_cli_rejects_unrecognized_option() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            "accepted.json",
            "--compile",
            "compile.json",
            "--output",
            "out.json",
            "--series",
            "series.json",
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let marker = "unrecognized option(s): --series";
    let observed = stderr.contains(marker);
    assert_eq!(observed, true);
    assert_eq!(marker, "unrecognized option(s): --series");
}

#[test]
fn classify_cli_emits_parsed_args_receipt() {
    let output = Command::new(env!("CARGO_BIN_EXE_perl-core-harness-transition"))
        .args([
            "classify",
            "--accepted-baseline",
            "accepted.json",
            "--compile",
            "compile.json",
            "--output",
            "out.json",
        ])
        .output()
        .expect("spawn classify CLI");
    assert!(
        output.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("decode stdout JSON");
    assert_eq!(value["schema_version"], "perl_core_harness.transition_classify_args.v1");
    assert_eq!(value["command"], "classify");
    assert_eq!(value["accepted_baseline"], "accepted.json");
    assert_eq!(value["compile"], "compile.json");
    assert_eq!(value["output"], "out.json");
    assert!(
        value["claim_boundary"]
            .as_str()
            .expect("claim boundary")
            .contains("parses classify CLI arguments only")
    );
}
