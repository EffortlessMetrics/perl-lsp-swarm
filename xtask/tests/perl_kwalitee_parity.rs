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
use serde_json::Value;
use sha2::{Digest, Sha256};

// Independent frozen parity ledger. Keep this separate from both the live
// evaluator catalog and manifest.json so catalog drift cannot make both sides
// move together.
const FROZEN_CATALOG_IDS: [&str; 17] = [
    "manifest.workspace_member_declared",
    "manifest.publish_policy_clean",
    "license.declared",
    "product_surface.native_only",
    "dap.cli_native_only",
    "release.native_binaries_present",
    "release.no_external_tooling",
    "release.checksums_valid",
    "formatter.native_default",
    "critic.native_default",
    "critic.run_critic_registry_parity",
    "quality.no_new_severe_gaps",
    "docs.status_current",
    "formatter.corpus_idempotent",
    "critic.no_false_positives",
    "formatter.perltidy_compat_no_external_only",
    "critic.perlcritic_compat_no_external_only",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    subject: String,
    generated_at: String,
    repo_token: String,
    input_files: BTreeMap<String, String>,
    catalog_ids: Vec<String>,
    cases: Vec<ParityCase>,
    explain: Artifact,
    legacy_reader: LegacyReaderFixture,
    migration_reference: Artifact,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportExpectation {
    exit_code: i32,
    stdout_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckExpectation {
    exit_code: i32,
    stdout_sha256: String,
    stderr_contains: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReaderFixture {
    input: String,
    expected: Artifact,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.ci/fixtures/perl-kwalitee-legacy-parity")
}

fn load_manifest() -> Manifest {
    let path = fixture_dir().join("manifest.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    parse_manifest(&text).unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}

fn independent_frozen_catalog_ids() -> Vec<String> {
    FROZEN_CATALOG_IDS.iter().map(|id| (*id).to_string()).collect()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn checked_artifact_path(relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || !path.components().all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!("artifact path must stay inside fixture root: {relative}"));
    }
    Ok(fixture_dir().join(path))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("write digest");
    }
    encoded
}

fn validate_artifact(artifact: &Artifact, label: &str) -> Result<(), String> {
    if !valid_sha256(&artifact.sha256) {
        return Err(format!("{label} has an invalid SHA-256 digest"));
    }
    let path = checked_artifact_path(&artifact.path)?;
    let bytes =
        fs::read(&path).map_err(|error| format!("{label}: read {}: {error}", path.display()))?;
    if sha256_bytes(&bytes) != artifact.sha256 {
        return Err(format!("{label}: committed artifact digest drifted"));
    }
    Ok(())
}

fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_str(text).map_err(|error| error.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema_version != 1 || manifest.subject != "perl_kwalitee.v1" {
        return Err("unsupported parity manifest identity".to_string());
    }
    if manifest.generated_at.is_empty() || manifest.repo_token.is_empty() {
        return Err("parity manifest requires generated_at and repo_token".to_string());
    }
    if manifest.catalog_ids != independent_frozen_catalog_ids() {
        return Err("parity manifest catalog differs from independent frozen ledger".to_string());
    }
    let expected_cases = [
        ("pr", false),
        ("pr", true),
        ("release", false),
        ("release", true),
        ("nightly", false),
        ("nightly", true),
    ];
    if manifest.cases.len() != expected_cases.len() {
        return Err("parity manifest must contain exactly six profile/strictness cases".to_string());
    }
    for case in &manifest.cases {
        let expected_id = format!(
            "{}_{}",
            case.profile.as_str(),
            if case.strict { "strict" } else { "non_strict" }
        );
        if !expected_cases.contains(&(case.profile.as_str(), case.strict)) || case.id != expected_id
        {
            return Err(format!("unexpected or duplicate parity case {}", case.id));
        }
        validate_artifact(&case.json, &format!("{} JSON", case.id))?;
        validate_artifact(&case.markdown, &format!("{} Markdown", case.id))?;
        if case.strict {
            if case.report.is_some() {
                return Err(format!("{} strict case unexpectedly has report contract", case.id));
            }
        } else if case
            .report
            .as_ref()
            .is_none_or(|report| report.exit_code != 0 || !valid_sha256(&report.stdout_sha256))
        {
            return Err(format!("{} has an invalid report CLI contract", case.id));
        }
        let expected_exit =
            if case.id == "pr_non_strict" || case.id == "nightly_non_strict" { 0 } else { 1 };
        if case.check.exit_code != expected_exit
            || !valid_sha256(&case.check.stdout_sha256)
            || case.check.stderr_contains.is_some() != (expected_exit != 0)
        {
            return Err(format!("{} has an invalid check CLI contract", case.id));
        }
    }
    validate_artifact(&manifest.explain, "explain catalog")?;
    validate_artifact(&manifest.legacy_reader.expected, "legacy reader")?;
    validate_artifact(&manifest.migration_reference, "migration reference")?;
    checked_artifact_path(&manifest.legacy_reader.input).and_then(|path| {
        fs::metadata(&path).map_err(|error| format!("legacy reader input: {error}"))
    })?;
    Ok(())
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

fn json_escaped_fragment(value: &str) -> String {
    serde_json::to_string(value).expect("encode JSON path fragment").trim_matches('"').to_string()
}

fn normalize_raw_json(value: &[u8], root: &Path, manifest: &Manifest) -> String {
    let text = String::from_utf8(value.to_vec()).expect("CLI JSON must be UTF-8");
    let mut normalized = String::with_capacity(text.len());
    for segment in text.split_inclusive('\n') {
        if let Some(value_start) = segment.find("\"generated_at\": \"") {
            let value_start = value_start + "\"generated_at\": \"".len();
            let Some(value_end) = segment[value_start..].find('"') else {
                normalized.push_str(segment);
                continue;
            };
            let value_end = value_start + value_end;
            normalized.push_str(&segment[..value_start]);
            normalized.push_str(&manifest.generated_at);
            normalized.push_str(&segment[value_end..]);
        } else {
            normalized.push_str(segment);
        }
    }

    // Replace the encoded temporary root before canonicalizing the remaining
    // encoded separators. Doing this in the opposite order turns each JSON
    // backslash escape into two separators and leaves the temp root unstable.
    normalized =
        normalized.replace(&json_escaped_fragment(&root.to_string_lossy()), &manifest.repo_token);
    normalized = normalized.replace("\\\\", "/");
    let parsed: Value = serde_json::from_str(&normalized).expect("decode normalized report JSON");
    format!("{}\n", serde_json::to_string_pretty(&parsed).expect("format normalized report JSON"))
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
    assert_eq!(sha256(&expected), artifact.sha256, "{label}: committed artifact digest drifted");
    assert_eq!(sha256(actual), artifact.sha256, "{label}: command artifact digest drifted");
    assert_eq!(actual, expected, "{label}: command artifact bytes drifted");
}

fn exit_code(output: &std::process::Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn required_parity_wiring_is_present(workflow: &str) -> bool {
    // The checked-in workflow is CRLF on Windows, while the contract must
    // remain portable to the LF representation used by hosted CI.
    let Some(start) = workflow.find("\n  check-all-targets:") else {
        return false;
    };
    let section = &workflow[start..];
    let end = section
        .match_indices("\n  ")
        .skip(1)
        .find_map(|(offset, _)| {
            (section.as_bytes().get(offset + 3) != Some(&b' ')).then_some(offset)
        })
        .unwrap_or(section.len());
    let job = &section[..end];
    job.contains("name: Legacy parity library authority (required merge surface)")
        && job.contains("cargo test -p perl-kwalitee --test legacy_parity --locked -- --nocapture")
        && job.contains("name: Legacy parity CLI authority (required merge surface)")
        && job.contains("cargo test -p xtask --test perl_kwalitee_parity --locked -- --nocapture")
        && !job.contains("continue-on-error: true")
}

#[test]
fn manifest_schema_rejects_malformed_missing_and_unknown_fields() {
    let source = fs::read_to_string(fixture_dir().join("manifest.json")).expect("read manifest");
    assert!(parse_manifest("{not-json").is_err(), "malformed JSON must fail closed");

    let mut missing = serde_json::from_str::<Value>(&source).expect("decode manifest");
    missing["cases"][0]["check"].as_object_mut().expect("check object").remove("stdout_sha256");
    assert!(
        parse_manifest(&serde_json::to_string(&missing).expect("encode missing field")).is_err(),
        "missing required CLI field must fail closed"
    );

    let mut extra = serde_json::from_str::<Value>(&source).expect("decode manifest");
    extra["unexpected"] = Value::String("must be rejected".to_string());
    assert!(
        parse_manifest(&serde_json::to_string(&extra).expect("encode unknown field")).is_err(),
        "unknown manifest field must fail closed"
    );
}

#[test]
fn manifest_validation_rejects_extra_rows_digest_and_cli_drift() {
    let source = fs::read_to_string(fixture_dir().join("manifest.json")).expect("read manifest");

    let mut extra_row = serde_json::from_str::<Value>(&source).expect("decode manifest");
    let duplicate = extra_row["cases"][0].clone();
    extra_row["cases"].as_array_mut().expect("cases array").push(duplicate);
    assert!(
        parse_manifest(&serde_json::to_string(&extra_row).expect("encode extra row")).is_err(),
        "extra parity row must fail closed"
    );

    let mut bad_digest = serde_json::from_str::<Value>(&source).expect("decode manifest");
    bad_digest["cases"][0]["json"]["sha256"] = Value::String("0".repeat(64));
    assert!(
        parse_manifest(&serde_json::to_string(&bad_digest).expect("encode bad digest")).is_err(),
        "artifact digest drift must fail closed"
    );

    let mut bad_cli = serde_json::from_str::<Value>(&source).expect("decode manifest");
    bad_cli["cases"][0]["check"]["exit_code"] = Value::Number(1.into());
    assert!(
        parse_manifest(&serde_json::to_string(&bad_cli).expect("encode bad CLI contract")).is_err(),
        "CLI exit-contract drift must fail closed"
    );
}

#[test]
fn required_parity_wiring_rejects_missing_or_advisory_controls() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow_path = root.join("../.github/workflows/ci.yml");
    let workflow = fs::read_to_string(&workflow_path).expect("read CI workflow");
    assert!(
        required_parity_wiring_is_present(&workflow),
        "both parity suites must run in the required Compile All Targets job"
    );

    let missing_library = workflow.replace("cargo test -p perl-kwalitee --test legacy_parity", "");
    assert!(
        !required_parity_wiring_is_present(&missing_library),
        "missing library parity command must fail the wiring contract"
    );

    let advisory = workflow.replacen(
        "name: Legacy parity library authority (required merge surface)",
        "continue-on-error: true\n      - name: Legacy parity library authority (required merge surface)",
        1,
    );
    assert!(
        !required_parity_wiring_is_present(&advisory),
        "advisory parity wiring must fail closed"
    );
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
        assert_eq!(sha256(&normalized_stdout), report.stdout_sha256, "{} report stdout", case.id);

        let raw_json = fs::read(&json_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", json_path.display()));
        let normalized_json = normalize_raw_json(&raw_json, fixture.path(), &manifest);
        assert_artifact(&normalized_json, &case.json, &format!("{} report raw JSON", case.id));

        let mut receipt: KwaliteeReceipt = serde_json::from_slice(&raw_json)
            .unwrap_or_else(|error| panic!("decode {}: {error}", json_path.display()));
        normalize_receipt(&mut receipt, fixture.path(), &manifest);
        let normalized_json = receipt.to_json_pretty().expect("serialize report receipt");
        let expected_json = read_artifact(&case.json);
        assert_eq!(
            serde_json::from_str::<Value>(&normalized_json).expect("decode normalized receipt"),
            serde_json::from_str::<Value>(&expected_json).expect("decode expected report receipt"),
            "{} report JSON semantic values",
            case.id
        );

        let written_markdown = fs::read_to_string(&markdown_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", markdown_path.display()));
        let normalized_markdown = normalize_generated_line(
            &normalize_paths(&written_markdown, &[(fixture.path(), manifest.repo_token.as_str())]),
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
        assert!(
            output.status.success(),
            "explain {id} failed.\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "explain {id} wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        writeln!(&mut actual, "=== {id} ===").expect("write explain heading");
        actual.push_str(&String::from_utf8(output.stdout).expect("explain stdout utf-8"));
    }

    assert_artifact(&actual, &manifest.explain, "explain catalog");
}
