//! Process-level admission and threshold controls for native critic receipts.

use anyhow::Result;
use assert_cmd::{Command, cargo::cargo_bin_cmd};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

const SOURCE: &str = "sub run { my $unused = 1; return 1; }\n";

fn command(severity: u8, source: &Path, receipt: &Path, summary: &Path) -> Command {
    let mut command = cargo_bin_cmd!("xtask");
    command
        .timeout(Duration::from_secs(10))
        .args(["native-critic", "check", "--profile", "strict", "--severity"])
        .arg(severity.to_string())
        .arg("--root")
        .arg(source)
        .arg("--receipt")
        .arg(receipt)
        .arg("--summary")
        .arg(summary)
        .env("NO_COLOR", "1")
        .env("RUST_BACKTRACE", "0");
    command
}

fn assert_invalid_output(output: &Output, severity: u8) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.code().is_some(), "command terminated abnormally: {output:?}");
    assert!(!output.status.success(), "invalid severity {severity} succeeded");
    assert!(stderr.contains(&format!("severity {severity}")), "missing rejected value: {stderr}");
    assert!(stderr.contains("1..=5"), "missing supported range: {stderr}");
}

fn rejects_without_creating_outputs(severity: u8) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fixture.pl");
    fs::write(&source, SOURCE)?;
    let output_directory = directory.path().join("not-created");
    let receipt = output_directory.join("receipt.json");
    let summary = output_directory.join("summary.md");

    let output = command(severity, &source, &receipt, &summary).output()?;
    assert_invalid_output(&output, severity);
    assert!(!output_directory.exists(), "invalid input created output directories");
    Ok(())
}

#[test]
fn severity_zero_is_rejected() -> Result<()> {
    rejects_without_creating_outputs(0)
}

#[test]
fn severity_six_is_rejected() -> Result<()> {
    rejects_without_creating_outputs(6)
}

#[test]
fn severity_255_is_rejected() -> Result<()> {
    rejects_without_creating_outputs(255)
}

#[test]
fn invalid_severity_takes_precedence_over_missing_sources() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let output = command(
        6,
        &directory.path().join("missing.pl"),
        &directory.path().join("receipt.json"),
        &directory.path().join("summary.md"),
    )
    .output()?;
    assert_invalid_output(&output, 6);
    assert!(!directory.path().join("receipt.json").exists());
    assert!(!directory.path().join("summary.md").exists());
    Ok(())
}

#[test]
fn invalid_severity_preserves_existing_outputs() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fixture.pl");
    fs::write(&source, SOURCE)?;
    let receipt = directory.path().join("receipt.json");
    let summary = directory.path().join("summary.md");
    for severity in [0, 6, 255] {
        fs::write(&receipt, "existing receipt\n")?;
        fs::write(&summary, "existing summary\n")?;
        let output = command(severity, &source, &receipt, &summary).output()?;
        assert_invalid_output(&output, severity);
        assert_eq!(fs::read_to_string(&receipt)?, "existing receipt\n");
        assert_eq!(fs::read_to_string(&summary)?, "existing summary\n");
    }
    Ok(())
}

#[test]
fn valid_thresholds_preserve_findings_and_clean_control() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fixture.pl");
    fs::write(&source, SOURCE)?;
    for severity in 1..=5 {
        let receipt = directory.path().join(format!("receipt-{severity}.json"));
        let summary = directory.path().join(format!("summary-{severity}.md"));
        let output = command(severity, &source, &receipt, &summary)
            .args([
                "--include",
                "native.testing.require_use_strict",
                "--include",
                "native.variables.unused_lexical",
            ])
            .output()?;
        assert!(output.status.success(), "valid severity {severity}: {output:?}");
        let value: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
        assert_eq!(value["severity"], severity);
        let findings = value["files"][0]["findings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("receipt must contain findings"))?;
        let mut observed = findings
            .iter()
            .map(|finding| (finding["rule_id"].as_str(), finding["severity"].as_str()))
            .collect::<Vec<_>>();
        observed.sort_unstable();
        let mut expected = Vec::new();
        if severity <= 3 {
            expected.push((Some("native.testing.require_use_strict"), Some("Harsh")));
        }
        if severity <= 4 {
            expected.push((Some("native.variables.unused_lexical"), Some("Stern")));
        }
        expected.sort_unstable();
        assert_eq!(observed, expected, "threshold {severity}: {value}");
        assert!(summary.is_file(), "valid input must still write a summary");
    }
    Ok(())
}
