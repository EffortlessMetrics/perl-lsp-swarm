//! End-to-end parity for the historical `cargo xtask perl-kwalitee` surface.
//!
//! The library fixture owns the expected artifacts. These tests prove that the
//! real command still emits them, preserves exit behavior, and explains every
//! frozen indicator before the namespace and command moves.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use assert_cmd::Command;
use perl_kwalitee::KwaliteeReceipt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    subject: String,
    generated_at: String,
    repo_token: String,
    input_files: BTreeMap<String, String>,
    catalog_ids: Vec<String>,
    cases: Vec<ParityCase>,
    explain: Artifact,
}

#[derive(Debug, Deserialize)]
struct ParityCase {
    id: String,
    profile: String,
    strict: bool,
    json: Artifact,
    markdown: Artifact,
    report: Option<ReportExpectation>,
    check: CheckExpectation,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct ReportExpectation {
    exit_code: i32,
    stdout_sha256: String,
}

#[derive(Debug, Deserialize)]
struct CheckExpectation {
    exit_code: i32,
    stdout_sha256: String,
    stderr_contains: Option<String>,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.ci/fixtures/perl-kwalitee-legacy-parity")
}

fn load_manifest() -> Manifest {
    let path = fixture_dir().join("manifest.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}

fn checked_input_path(relative: &str) -> &Path {
    let path = Path::new(relative);
    assert!(!path.is_absolute(), "fixture input path must be relative: {relative}");
    assert!(
        path.components().all(|component| matches!(component, Component::Normal(_))),
        "fixture input path must not escape its root: {relative}"
    );
    path
}

fn materialize_inputs(manifest: &Manifest) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create parity fixture root");
    for (relative, contents) in &manifest.input_files {
        let destination = temp.path().join(checked_input_path(relative));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        fs::write(&destination, contents)
            .unwrap_or_else(|error| panic!("write {}: {error}", destination.display()));
    }
    temp
}

fn normalize_slashes(value: &str) -> String {
    value.replace('\\', "/")
}

fn normalize_paths(value: &str, replacements: &[(&Path, &str)]) -> String {
    let mut normalized = normalize_slashes(value);
    for (path, token) in replacements {
        normalized = normalized.replace(&normalize_slashes(&path.to_string_lossy()), token);
    }
    normalized
}

fn normalize_generated_line(value: &str, generated_at: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for segment in value.split_inclusive('\n') {
        if segment.starts_with("Generated: ") {
            normalized.push_str("Generated: ");
            normalized.push_str(generated_at);
            if segment.ends_with('\n') {
                normalized.push('\n');
            }
        } else {
            normalized.push_str(segment);
        }
    }
    normalized
}

fn normalize_receipt(receipt: &mut KwaliteeReceipt, root: &Path, manifest: &Manifest) {
    receipt.generated_at = manifest.generated_at.clone();
    let normalized_root = normalize_slashes(&root.to_string_lossy());
    for indicator in &mut receipt.indicators {
        for evidence in &mut indicator.evidence {
            let normalized = normalize_slashes(&evidence.value);
            evidence.value = normalized.replace(&normalized_root, &manifest.repo_token);
        }
    }
}

fn sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("write digest");
    }
    encoded
}

fn read_artifact(artifact: &Artifact) -> String {
    let path = fixture_dir().join(&artifact.path);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn assert_artifact(actual: &str, artifact: &Artifact, label: &str) {
    let expected = read_artifact(artifact);
    assert_eq!(
        sha256(&expected),
        artifact.sha256,
        "{label}: committed artifact digest drifted"
    );
    assert_eq!(
        sha256(actual),
        artifact.sha256,
        "{label}: command artifact digest drifted"
    );
    assert_eq!(actual, expected, "{label}: command artifact bytes drifted");
}

fn exit_code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

#[test]
fn report_replays_non_strict_json_markdown_and_summary() {
    let manifest = load_manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.subject, "perl_kwalitee.v1");
    let fixture = materialize_inputs(&manifest);

    for case in manifest.cases.iter().filter(|case| case.report.is_some()) {
        assert!(!case.strict, "{} report case must be non-strict", case.id);
        let report = case.report.as_ref().expect("report expectation");
        let output_dir = tempfile::tempdir().expect("report output directory");
        let json_path = output_dir.path().join(format!("{}.json", case.id));
        let markdown_path = output_dir.path().join(format!("{}.md", case.id));

        let output = Command::cargo_bin("xtask")
            .expect("xtask binary")
            .args(["perl-kwalitee", "report", "--profile", &case.profile, "--repo-root"])
            .arg(fixture.path())
            .arg("--json")
            .arg(&json_path)
            .arg("--markdown")
            .arg(&markdown_path)
            .output()
            .expect("run report");

        assert_eq!(exit_code(&output), report.exit_code, "{} report exit", case.id);
        assert!(
            output.stderr.is_empty(),
            "{} report stderr: {}",
            case.id,
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8(output.stdout).expect("report stdout utf-8");
        let normalized_stdout = normalize_paths(
            &stdout,
            &[(&json_path, "<json-output>"), (&markdown_path, "<markdown-output>")],
        );
        assert_eq!(
            sha256(&normalized_stdout),
            report.stdout_sha256,
            "{} report stdout",
            case.id
        );

        let mut receipt: KwaliteeReceipt = serde_json::from_slice(
            &fs::read(&json_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", json_path.display())),
        )
        .unwrap_or_else(|error| panic!("decode {}: {error}", json_path.display()));
        normalize_receipt(&mut receipt, fixture.path(), &manifest);
        let normalized_json = receipt.to_json_pretty().expect("serialize report receipt");
        assert_artifact(&normalized_json, &case.json, &format!("{} report JSON", case.id));

        let written_markdown = fs::read_to_string(&markdown_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", markdown_path.display()));
        let normalized_markdown = normalize_generated_line(
            &normalize_paths(
                &written_markdown,
                &[(fixture.path(), manifest.repo_token.as_str())],
            ),
            &manifest.generated_at,
        );
        assert_artifact(
            &normalized_markdown,
            &case.markdown,
            &format!("{} report Markdown", case.id),
        );
        assert_eq!(
            normalized_markdown,
            receipt.to_markdown(),
            "{} report JSON and Markdown disagree",
            case.id
        );
    }
}

#[test]
fn check_replays_all_profile_strictness_exit_and_stdout_cases() {
    let manifest = load_manifest();
    let fixture = materialize_inputs(&manifest);

    for case in &manifest.cases {
        let mut command = Command::cargo_bin("xtask").expect("xtask binary");
        command
            .args(["perl-kwalitee", "check", "--profile", &case.profile, "--repo-root"])
            .arg(fixture.path());
        if case.strict {
            command.arg("--strict");
        }
        let output = command.output().expect("run check");
        assert_eq!(exit_code(&output), case.check.exit_code, "{} check exit", case.id);

        let stdout = String::from_utf8(output.stdout).expect("check stdout utf-8");
        let normalized_stdout = normalize_generated_line(
            &normalize_paths(&stdout, &[(fixture.path(), manifest.repo_token.as_str())]),
            &manifest.generated_at,
        );
        assert_eq!(
            sha256(&normalized_stdout),
            case.check.stdout_sha256,
            "{} check stdout digest",
            case.id
        );

        let expected_markdown = read_artifact(&case.markdown);
        let expected_json = read_artifact(&case.json);
        let expected_receipt: KwaliteeReceipt =
            serde_json::from_str(&expected_json).expect("decode expected receipt");
        let expected_stdout = format!(
            "{}\nPerl Kwalitee: {} (score {}/100, profile {})\n",
            expected_markdown,
            expected_receipt.verdict.label(),
            expected_receipt.score,
            expected_receipt.profile
        );
        assert_eq!(normalized_stdout, expected_stdout, "{} check stdout bytes", case.id);

        let stderr = String::from_utf8_lossy(&output.stderr);
        match &case.check.stderr_contains {
            Some(expected) => assert!(
                stderr.contains(expected),
                "{} check stderr missing `{expected}`: {stderr}",
                case.id
            ),
            None => assert!(stderr.is_empty(), "{} unexpected check stderr: {stderr}", case.id),
        }
    }
}

#[test]
fn explain_replays_every_frozen_indicator() {
    let manifest = load_manifest();
    let mut actual = String::new();

    for id in &manifest.catalog_ids {
        let output = Command::cargo_bin("xtask")
            .expect("xtask binary")
            .args(["perl-kwalitee", "explain", id])
            .output()
            .expect("run explain");
        assert!(output.status.success(), "explain {id} failed");
        assert!(output.stderr.is_empty(), "explain {id} wrote stderr");
        writeln!(&mut actual, "=== {id} ===").expect("write explain heading");
        actual.push_str(&String::from_utf8(output.stdout).expect("explain stdout utf-8"));
    }

    assert_artifact(&actual, &manifest.explain, "explain catalog");
}
