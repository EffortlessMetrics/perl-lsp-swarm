#![deny(clippy::map_err_ignore)]

//! Frozen parity authority for the historical `perl_kwalitee.v1` evaluator.
//!
//! The expected side is committed data, not values derived from the live catalog.
//! The subject is behind [`LegacyParitySubject`] so the mechanical namespace move
//! can replay this exact harness by changing one adapter rather than rebuilding
//! the oracle from the moved implementation.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use perl_kwalitee::{
    EvidencePaths, IndicatorStatus, KwaliteeOptions, KwaliteeProfile, KwaliteeReceipt,
    indicator_ids,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

// This is the independent parity ledger.  It is intentionally not obtained from
// `indicator_ids()` or from the manifest: changing either side must be visible to
// this test before a moved evaluator can claim parity.
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
    profile: KwaliteeProfile,
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

trait LegacyParitySubject {
    fn evaluate(&self, options: &KwaliteeOptions) -> KwaliteeReceipt;
    fn read_legacy_receipt(&self, bytes: &[u8]) -> Result<KwaliteeReceipt, String>;
    fn render_migration_reference(&self) -> Result<String, String>;
}

struct CurrentLegacySubject;

impl LegacyParitySubject for CurrentLegacySubject {
    fn evaluate(&self, options: &KwaliteeOptions) -> KwaliteeReceipt {
        perl_kwalitee::evaluate(options)
    }

    fn read_legacy_receipt(&self, bytes: &[u8]) -> Result<KwaliteeReceipt, String> {
        perl_kwalitee::read_legacy_receipt(bytes).map_err(|error| error.to_string())
    }

    fn render_migration_reference(&self) -> Result<String, String> {
        perl_kwalitee::render_legacy_migration_markdown().map_err(|error| error.to_string())
    }
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.ci/fixtures/perl-kwalitee-legacy-parity")
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

fn catalog_matches_independent_authority(candidate: &[String]) -> bool {
    candidate == independent_frozen_catalog_ids()
}

fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_str(text).map_err(|error| error.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn checked_artifact_path(relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || !path.components().all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!("artifact path must stay inside the fixture root: {relative}"));
    }
    Ok(fixture_dir().join(path))
}

fn validate_artifact(artifact: &Artifact, label: &str) -> Result<(), String> {
    if !valid_sha256(&artifact.sha256) {
        return Err(format!("{label} has an invalid SHA-256 digest"));
    }
    let path = checked_artifact_path(&artifact.path)?;
    let bytes =
        fs::read(&path).map_err(|error| format!("{label}: read {}: {error}", path.display()))?;
    let actual = sha256_bytes(&bytes);
    if actual != artifact.sha256 {
        return Err(format!("{label}: committed artifact digest drifted"));
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("write digest");
    }
    encoded
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema_version != 1 || manifest.subject != "perl_kwalitee.v1" {
        return Err("unsupported parity manifest identity".to_string());
    }
    if manifest.generated_at.is_empty() || manifest.repo_token.is_empty() {
        return Err("parity manifest requires generated_at and repo_token".to_string());
    }
    let frozen = independent_frozen_catalog_ids();
    if manifest.catalog_ids != frozen {
        return Err(
            "parity manifest catalog differs from the independent frozen ledger".to_string()
        );
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
        let profile = case.profile.as_str();
        let expected_id =
            format!("{profile}_{}", if case.strict { "strict" } else { "non_strict" });
        if !expected_cases.contains(&(profile, case.strict)) || case.id != expected_id {
            return Err(format!("unexpected or duplicate parity case {}", case.id));
        }
        validate_artifact(&case.json, &format!("{} JSON", case.id))?;
        validate_artifact(&case.markdown, &format!("{} Markdown", case.id))?;
        if case.strict {
            if case.report.is_some() {
                return Err(format!("{} strict case unexpectedly has a report contract", case.id));
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

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

fn options_for(root: &Path, manifest: &Manifest, case: &ParityCase) -> KwaliteeOptions {
    let mut options = KwaliteeOptions::new(root, case.profile);
    options.strict = case.strict;
    options.commit = "unknown".to_string();
    options.generated_at = manifest.generated_at.clone();
    options.evidence = EvidencePaths {
        native_tooling_readiness: existing(
            root.join("target/receipts/native-tooling/readiness.json"),
        ),
        quality_gate_receipt: existing(root.join("target/receipts/quality/quality-gate.json")),
        native_format_corpus: existing(
            root.join("target/receipts/format/native-format-corpus.json"),
        ),
        native_critic_false_positive: existing(
            root.join("target/receipts/native-tooling/native-critic-false-positive.json"),
        ),
        native_format_perltidy_compat: existing(
            root.join("target/receipts/format/native-format-perltidy-compat.json"),
        ),
        native_tooling_perlcritic_compat: existing(
            root.join("target/receipts/native-tooling/perlcritic-compat.json"),
        ),
    };
    options
}

fn normalize_slashes(value: &str) -> String {
    value.replace('\\', "/")
}

fn normalize_receipt(receipt: &mut KwaliteeReceipt, root: &Path, repo_token: &str) {
    let normalized_root = normalize_slashes(&root.to_string_lossy());
    for indicator in &mut receipt.indicators {
        for evidence in &mut indicator.evidence {
            let normalized = normalize_slashes(&evidence.value);
            evidence.value = normalized.replace(&normalized_root, repo_token);
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

fn canonical_json(value: &str) -> String {
    let parsed: Value = serde_json::from_str(value).expect("decode parity JSON");
    format!("{}\n", serde_json::to_string_pretty(&parsed).expect("format parity JSON"))
}

fn read_artifact(artifact: &Artifact) -> String {
    let path = fixture_dir().join(&artifact.path);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn artifact_matches(actual: &str, artifact: &Artifact) -> bool {
    let expected = read_artifact(artifact);
    let actual = canonical_json(actual);
    sha256(&expected) == artifact.sha256 && sha256(&actual) == artifact.sha256 && actual == expected
}

fn assert_artifact(actual: &str, artifact: &Artifact, label: &str) {
    let expected = read_artifact(artifact);
    assert_eq!(sha256(&expected), artifact.sha256, "{label}: committed artifact digest drifted");
    assert_eq!(sha256(actual), artifact.sha256, "{label}: evaluated artifact digest drifted");
    assert_eq!(actual, expected, "{label}: evaluated artifact bytes drifted");
}

#[test]
fn frozen_matrix_covers_every_row_profile_and_strictness() {
    let manifest = load_manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.subject, "perl_kwalitee.v1");

    let independent_ids = independent_frozen_catalog_ids();
    assert_eq!(
        manifest.catalog_ids, independent_ids,
        "parity manifest drifted from the independent frozen migration ledger"
    );
    let live_ids = indicator_ids().into_iter().map(ToOwned::to_owned).collect::<Vec<String>>();
    assert_eq!(
        live_ids, independent_ids,
        "catalog identity or order changed from the independent frozen authority"
    );

    let expected_cases = BTreeSet::from([
        ("nightly".to_string(), false),
        ("nightly".to_string(), true),
        ("pr".to_string(), false),
        ("pr".to_string(), true),
        ("release".to_string(), false),
        ("release".to_string(), true),
    ]);
    let actual_cases = manifest
        .cases
        .iter()
        .map(|case| (case.profile.as_str().to_string(), case.strict))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_cases, expected_cases, "profile/strictness matrix is incomplete");

    for case in &manifest.cases {
        assert_eq!(case.check.stdout_sha256.len(), 64, "{} check digest", case.id);
        if case.strict {
            assert!(case.report.is_none(), "{}: report is intentionally non-strict", case.id);
        } else {
            let report = case.report.as_ref().expect("non-strict case has report expectation");
            assert_eq!(report.exit_code, 0, "{} report exit", case.id);
            assert_eq!(report.stdout_sha256.len(), 64, "{} report digest", case.id);
        }
        let expected_exit =
            if case.id == "pr_non_strict" || case.id == "nightly_non_strict" { 0 } else { 1 };
        assert_eq!(case.check.exit_code, expected_exit);
        assert_eq!(case.check.stderr_contains.is_some(), case.check.exit_code != 0);
    }
    assert_eq!(manifest.explain.sha256.len(), 64);

    let fixture = materialize_inputs(&manifest);
    let subject = CurrentLegacySubject;
    let mut observations = BTreeSet::new();
    let mut statuses = BTreeSet::new();

    for case in &manifest.cases {
        let mut receipt = subject.evaluate(&options_for(fixture.path(), &manifest, case));
        assert_eq!(receipt.profile, case.profile, "{} profile", case.id);
        assert_eq!(
            receipt.indicators.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
            manifest.catalog_ids,
            "{} indicator identity/order",
            case.id
        );

        for row in &receipt.indicators {
            observations.insert((row.id.clone(), case.profile.as_str().to_string(), case.strict));
            statuses.insert(row.status.as_str().to_string());
        }

        normalize_receipt(&mut receipt, fixture.path(), &manifest.repo_token);
        let json = canonical_json(&receipt.to_json_pretty().expect("serialize parity receipt"));
        let markdown = receipt.to_markdown();
        assert_artifact(&json, &case.json, &format!("{} JSON", case.id));
        assert_artifact(&markdown, &case.markdown, &format!("{} Markdown", case.id));
    }

    assert_eq!(
        observations.len(),
        manifest.catalog_ids.len() * manifest.cases.len(),
        "every legacy row must be observed in every profile/strictness case"
    );
    assert_eq!(
        statuses,
        BTreeSet::from([
            "fail".to_string(),
            "not_applicable".to_string(),
            "pass".to_string(),
            "unverified".to_string(),
            "warn".to_string(),
        ]),
        "fixture must exercise all five historical statuses"
    );
}

#[test]
fn independent_catalog_authority_rejects_missing_row_drift() {
    let authority = independent_frozen_catalog_ids();
    let mut drifted = authority.clone();
    drifted.pop();

    assert!(
        !catalog_matches_independent_authority(&drifted),
        "a catalog missing a frozen legacy row must not match the independent authority"
    );
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
fn manifest_validation_rejects_extra_rows_and_digest_or_cli_drift() {
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
fn focused_semantic_or_order_drift_is_rejected() {
    let manifest = load_manifest();
    let case =
        manifest.cases.iter().find(|case| case.id == "pr_non_strict").expect("pr_non_strict case");
    let expected = read_artifact(&case.json);
    let baseline: KwaliteeReceipt =
        serde_json::from_str(&expected).expect("decode committed receipt");
    assert!(
        artifact_matches(&baseline.to_json_pretty().expect("serialize baseline"), &case.json),
        "baseline must match before mutation controls run"
    );

    let mut mutations = Vec::new();

    let mut missing = baseline.clone();
    missing.indicators.remove(0);
    mutations.push(("indicator removal", missing));

    let mut reordered = baseline.clone();
    reordered.indicators.swap(0, 1);
    mutations.push(("indicator order", reordered));

    let mut renamed = baseline.clone();
    renamed.indicators[0].id.push_str(".changed");
    mutations.push(("indicator id", renamed));

    let mut status = baseline.clone();
    status.indicators[0].status = IndicatorStatus::Fail;
    mutations.push(("indicator status", status));

    let mut remediation = baseline.clone();
    remediation
        .indicators
        .iter_mut()
        .find(|row| row.id == "critic.native_default")
        .expect("critic row")
        .remediation = Some("changed remediation".to_string());
    mutations.push(("indicator remediation", remediation));

    let mut score = baseline;
    score.score = score.score.saturating_sub(1);
    mutations.push(("aggregate score", score));

    for (label, mutated) in mutations {
        let actual = mutated.to_json_pretty().expect("serialize mutation");
        assert!(
            !artifact_matches(&actual, &case.json),
            "{label} mutation escaped the parity comparator"
        );
    }
}

#[test]
fn pinned_reader_and_migration_reference_are_part_of_the_authority() {
    let manifest = load_manifest();
    let subject = CurrentLegacySubject;

    let legacy_input_path = fixture_dir().join(&manifest.legacy_reader.input);
    let legacy_bytes = fs::read(&legacy_input_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", legacy_input_path.display()));
    let legacy = subject
        .read_legacy_receipt(&legacy_bytes)
        .unwrap_or_else(|error| panic!("decode legacy receipt: {error}"));
    assert_artifact(
        &legacy.to_json_pretty().expect("serialize pinned legacy receipt"),
        &manifest.legacy_reader.expected,
        "legacy reader",
    );

    let expected_reference = read_artifact(&manifest.migration_reference);
    assert_eq!(
        sha256(&expected_reference),
        manifest.migration_reference.sha256,
        "migration reference digest drifted"
    );
    let rendered = subject
        .render_migration_reference()
        .unwrap_or_else(|error| panic!("render migration reference: {error}"));
    assert_eq!(
        sha256(&rendered),
        manifest.migration_reference.sha256,
        "rendered migration reference digest drifted"
    );
    assert_eq!(rendered, expected_reference, "migration reference bytes drifted");
}
