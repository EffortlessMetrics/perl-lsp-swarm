//! Behavioral proof that the two path-composition lints promoted by #14441 are
//! live on the selected toolchain.
//!
//! The ledger's zero-finding denominator is about production code, not about
//! whether Clippy still recognises the hazard shapes. These tests compile a
//! minimal fixture that exercises the POSIX absolute-join and absolute-push
//! overwrite forms and assert each lint reports them under `--force-warn`.

use color_eyre::eyre::{Result, bail};
use serde_json::Value as JsonValue;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const PUSH_LINT: &str = "clippy::path_buf_push_overwrite";
const JOIN_LINT: &str = "clippy::join_absolute_paths";

const PUSH_HAZARD: &str = "hazard_push_absolute_component";
const JOIN_HAZARD: &str = "hazard_join_absolute_child";

const MAX_DIAGNOSTIC_LINES: usize = 20;
const MAX_DIAGNOSTIC_CHARS_PER_LINE: usize = 500;

const FIXTURE_MANIFEST: &str = r#"[package]
name = "path-overwrite-falsifier-fixture"
version = "0.0.0"
edition = "2024"
rust-version = "1.95"
publish = false

[lints.clippy]
path_buf_push_overwrite = "deny"
join_absolute_paths = "deny"

[workspace]
"#;

const FIXTURE_SOURCE: &str = r#"use std::path::{Path, PathBuf};

/// POSIX absolute component passed to `join` replaces the base path.
pub fn join_absolute_overwrites_base(base: &Path) -> PathBuf {
    let hazard_join_absolute_child = base.join("/etc/passwd");
    hazard_join_absolute_child
}

/// `PathBuf::push` with an absolute component discards the existing path.
pub fn push_absolute_overwrites_base() -> PathBuf {
    let mut hazard_push_absolute_component = PathBuf::from("/var/base");
    hazard_push_absolute_component.push("/etc/passwd");
    hazard_push_absolute_component
}
"#;

#[derive(Clone)]
struct LintFinding {
    lint: String,
    source_line: String,
}

#[test]
fn path_buf_push_overwrite_reports_absolute_push_component() -> Result<()> {
    let findings = measure_fixture(&["-A", JOIN_LINT, "--force-warn", PUSH_LINT], PUSH_LINT)?;
    assert_lint_covers(&findings, PUSH_LINT, &[PUSH_HAZARD])?;
    Ok(())
}

#[test]
fn join_absolute_paths_reports_absolute_join_component() -> Result<()> {
    let findings = measure_fixture(&["-A", PUSH_LINT, "--force-warn", JOIN_LINT], JOIN_LINT)?;
    assert_lint_covers(&findings, JOIN_LINT, &[JOIN_HAZARD])?;
    Ok(())
}

fn measure_fixture(lint_flags: &[&str], expected_lint: &str) -> Result<Vec<LintFinding>> {
    let fixture = tempdir()?;
    write_fixture(fixture.path())?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command
        .current_dir(fixture.path())
        .args(["clippy", "--offline", "--quiet", "--lib", "--no-deps", "--message-format=json"])
        .arg("--");
    command.args(lint_flags);
    let output = command
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .output()?;

    if !output.status.success() {
        let stdout = bounded_diagnostic(&output.stdout);
        let stderr = bounded_diagnostic(&output.stderr);
        bail!(
            "path-overwrite fixture failed to build under {lint_flags:?}:\nstdout (bounded):\n{stdout}\nstderr (bounded):\n{stderr}"
        );
    }

    let findings = collect_findings(&output.stdout, expected_lint)?;
    if findings.is_empty() {
        bail!("{expected_lint} reported nothing; the falsifier fixture is not live");
    }
    for finding in &findings {
        if finding.lint != expected_lint {
            bail!(
                "expected only {expected_lint}, got {} on `{}`",
                finding.lint,
                finding.source_line
            );
        }
    }
    Ok(findings)
}

fn write_fixture(root: &Path) -> Result<()> {
    let source_dir = root.join("src");
    fs::create_dir(&source_dir)?;
    fs::write(root.join("Cargo.toml"), FIXTURE_MANIFEST)?;
    fs::write(source_dir.join("lib.rs"), FIXTURE_SOURCE)?;
    Ok(())
}

fn collect_findings(stdout: &[u8], expected_lint: &str) -> Result<Vec<LintFinding>> {
    let mut findings = Vec::new();
    for line in stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let event: JsonValue = serde_json::from_slice(line)?;
        let Some(lint) = event.pointer("/message/code/code").and_then(JsonValue::as_str) else {
            continue;
        };
        if lint != expected_lint {
            continue;
        }
        let Some(source_line) =
            event.pointer("/message/spans/0/text/0/text").and_then(JsonValue::as_str)
        else {
            bail!("{lint} finding carried no primary span text");
        };
        findings.push(LintFinding {
            lint: lint.to_owned(),
            source_line: source_line.trim().to_owned(),
        });
    }
    Ok(findings)
}

fn assert_lint_covers(findings: &[LintFinding], lint: &str, markers: &[&str]) -> Result<()> {
    for marker in markers {
        if !findings
            .iter()
            .any(|finding| finding.lint == lint && finding.source_line.contains(marker))
        {
            bail!("{lint} did not report a finding on `{marker}`");
        }
    }
    Ok(())
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut output = String::new();

    for (index, line) in lines.by_ref().take(MAX_DIAGNOSTIC_LINES).enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let mut chars = line.chars();
        output.extend(chars.by_ref().take(MAX_DIAGNOSTIC_CHARS_PER_LINE));
        if chars.next().is_some() {
            output.push('…');
        }
    }
    if lines.next().is_some() {
        output.push_str("\n… additional lines omitted");
    }
    if output.is_empty() {
        output.push_str("<empty>");
    }

    output
}
