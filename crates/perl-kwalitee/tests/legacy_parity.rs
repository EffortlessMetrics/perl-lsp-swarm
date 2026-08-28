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
use sha2::{Digest, Sha256};

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
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}

fn independent_frozen_catalog_ids() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("legacy_indicator_migrations.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read independent catalog {}: {error}", path.display()));
    let document: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|error| panic!("decode independent catalog {}: {error}", path.display()));
    document
        .get("indicator")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("independent catalog {} has no indicator array", path.display()))
        .iter()
        .map(|row| {
            row.get("legacy_id")
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| panic!("independent catalog row has no legacy_id"))
                .to_string()
        })
        .collect()
}

fn catalog_matches_independent_authority(candidate: &[String]) -> bool {
    candidate == independent_frozen_catalog_ids()
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

fn read_artifact(artifact: &Artifact) -> String {
    let path = fixture_dir().join(&artifact.path);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn artifact_matches(actual: &str, artifact: &Artifact) -> bool {
    let expected = read_artifact(artifact);
    sha256(&expected) == artifact.sha256 && sha256(actual) == artifact.sha256 && actual == expected
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
        let json = receipt.to_json_pretty().expect("serialize parity receipt");
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
