//! Thin CLI for the transition classify + check command surface.
//!
//! `classify` loads V2 accepted baseline + current run-report JSON, optionally
//! binds `--series` identity and `--discovery` to that series, applies the
//! in-lib `classify_transition` core, and writes a non-authorizing
//! classification receipt (with input digests) to `--output`.
//!
//! `check` reloads the same evidence (and optional series/discovery),
//! recomputes classification + digests, and verifies an existing receipt
//! matches exactly.
//!
//! Windows hard-link identity remains a follow-up slice. Full #6880
//! validated-wrapper centralization is intentionally out of scope.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    DISCOVERY_SCHEMA_VERSION, DiscoveryReport, RUN_REPORT_SCHEMA_VERSION, RunReport,
    SERIES_MANIFEST_SCHEMA_VERSION, SeriesManifest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

include!("perl-core-harness-transition/cli.rs");

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-transition <classify|check> --accepted-baseline <path> --compile <path> (--output|--receipt) <path>"
        )
    })?;
    let options = Options::parse(args)?;
    match command.as_str() {
        "classify" => {
            let config = ClassifyConfig::from_options(options)?;
            run_classify(&config)
        }
        "check" => {
            let config = CheckConfig::from_options(options)?;
            run_check(&config)
        }
        _ => bail!("unknown perl-core-harness-transition command: {command}"),
    }
}

fn run_classify(config: &ClassifyConfig) -> Result<()> {
    reject_output_input_path_collision(config)?;
    let accepted_bytes = read_bytes(&config.accepted_baseline, "accepted baseline")?;
    let compile_bytes = read_bytes(&config.compile, "compile observation")?;
    let accepted = decode_accepted_v2(&accepted_bytes, &config.accepted_baseline)?;
    let current = decode_run_report(&compile_bytes, &config.compile)?;
    if let Some(series_path) = &config.series {
        let series = load_series_manifest(series_path)?;
        bind_series_identity(&series, &accepted)?;
        if let Some(discovery_path) = &config.discovery {
            let discovery = load_discovery_report(discovery_path)?;
            bind_discovery_to_series(&discovery, &series)?;
        }
    }
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    write_classification_receipt(
        &config.output,
        &classification,
        &sha256_digest_bytes(&accepted_bytes),
        &sha256_digest_bytes(&compile_bytes),
    )
}

fn run_check(config: &CheckConfig) -> Result<()> {
    let accepted_bytes = read_bytes(&config.accepted_baseline, "accepted baseline")?;
    let compile_bytes = read_bytes(&config.compile, "compile observation")?;
    let accepted = decode_accepted_v2(&accepted_bytes, &config.accepted_baseline)?;
    let current = decode_run_report(&compile_bytes, &config.compile)?;
    if let Some(series_path) = &config.series {
        let series = load_series_manifest(series_path)?;
        bind_series_identity(&series, &accepted)?;
        if let Some(discovery_path) = &config.discovery {
            let discovery = load_discovery_report(discovery_path)?;
            bind_discovery_to_series(&discovery, &series)?;
        }
    }
    let expected = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    let expected_accepted_digest = sha256_digest_bytes(&accepted_bytes);
    let expected_compile_digest = sha256_digest_bytes(&compile_bytes);
    let receipt = load_classify_receipt(&config.receipt)?;
    verify_classify_receipt(
        &receipt,
        &expected,
        &expected_accepted_digest,
        &expected_compile_digest,
    )
}

fn reject_output_input_path_collision(config: &ClassifyConfig) -> Result<()> {
    // Lean path-string identity only. Symlink/hard-link identity remains deferred.
    if paths_equal(&config.output, &config.accepted_baseline)
        || paths_equal(&config.output, &config.compile)
        || config.series.as_ref().is_some_and(|series| paths_equal(&config.output, series))
        || config.discovery.as_ref().is_some_and(|discovery| paths_equal(&config.output, discovery))
    {
        bail!(
            "output path must not alias --accepted-baseline, --compile, --series, or --discovery; refusing to overwrite evidence"
        );
    }
    Ok(())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn read_bytes(path: &Path, label: &str) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {label} {}", path.display()))
}

fn decode_accepted_v2(bytes: &[u8], path: &Path) -> Result<CompileBaselineV2> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .with_context(|| format!("decoding accepted baseline JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        bail!(
            "unsupported accepted baseline schema: {schema}; classify I/O accepts {} only",
            COMPILE_BASELINE_V2_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding accepted V2 baseline {}", path.display()))
}

fn decode_run_report(bytes: &[u8], path: &Path) -> Result<RunReport> {
    let report: RunReport = serde_json::from_slice(bytes)
        .with_context(|| format!("decoding compile observation JSON {}", path.display()))?;
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!(
            "unsupported compile observation schema: {}; expected {}",
            report.schema_version,
            RUN_REPORT_SCHEMA_VERSION
        );
    }
    Ok(report)
}

fn load_series_manifest(path: &Path) -> Result<SeriesManifest> {
    let bytes = read_bytes(path, "series manifest")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding series manifest JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != SERIES_MANIFEST_SCHEMA_VERSION {
        bail!(
            "unsupported series manifest schema: {schema}; classify I/O accepts {} only",
            SERIES_MANIFEST_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding series manifest {}", path.display()))
}

/// Lean series identity binding for classify/check.
///
/// Binds accepted V2 to an explicit `--series` manifest via `series_id`,
/// `manifest_hash`, exact `file_membership`/`normalized_manifest` equality, and
/// subject fields (`profile`, `runner`, `perl_resolved_ref`). Does not rehash
/// the series, claim hard-link identity, or centralize #6880 validated wrappers.
fn bind_series_identity(series: &SeriesManifest, accepted: &CompileBaselineV2) -> Result<()> {
    if accepted.series_id != series.series_id {
        bail!("accepted baseline is not bound to series {}: series_id mismatch", series.series_id);
    }
    if accepted.manifest_hash != series.manifest_hash {
        bail!(
            "accepted baseline is not bound to series {}: manifest_hash mismatch",
            series.series_id
        );
    }
    if accepted.file_membership != series.normalized_manifest {
        bail!(
            "accepted baseline is not bound to series {}: file_membership mismatch",
            series.series_id
        );
    }
    if accepted.profile != series.profile {
        bail!("accepted baseline is not bound to series {}: profile mismatch", series.series_id);
    }
    if accepted.runner != series.runner {
        bail!("accepted baseline is not bound to series {}: runner mismatch", series.series_id);
    }
    if accepted.perl_resolved_ref != series.perl_resolved_ref {
        bail!(
            "accepted baseline is not bound to series {}: perl_resolved_ref mismatch",
            series.series_id
        );
    }
    Ok(())
}

fn load_discovery_report(path: &Path) -> Result<DiscoveryReport> {
    let bytes = read_bytes(path, "discovery report")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding discovery report JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != DISCOVERY_SCHEMA_VERSION {
        bail!(
            "unsupported discovery report schema: {schema}; classify I/O accepts {} only",
            DISCOVERY_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding discovery report {}", path.display()))
}

/// Lean discovery→series binding for classify/check.
///
/// When `--discovery` is supplied (requires `--series`), binds discovery
/// `schema_version`/`profile`/`runner`/`perl_ref`/`commit` and sorted test
/// paths to the series harness schema, subject fields, repository commit, and
/// `normalized_manifest`. Does not re-normalize paths, rehash the series,
/// claim hard-link identity, or centralize #6880 validated wrappers.
fn bind_discovery_to_series(discovery: &DiscoveryReport, series: &SeriesManifest) -> Result<()> {
    if discovery.schema_version != series.harness_schema_version {
        bail!(
            "discovery report is not bound to series {}: harness_schema_version mismatch",
            series.series_id
        );
    }
    if discovery.profile != series.profile {
        bail!("discovery report is not bound to series {}: profile mismatch", series.series_id);
    }
    if discovery.runner != series.runner {
        bail!("discovery report is not bound to series {}: runner mismatch", series.series_id);
    }
    if discovery.perl_ref != series.perl_resolved_ref {
        bail!(
            "discovery report is not bound to series {}: perl_resolved_ref mismatch",
            series.series_id
        );
    }
    if discovery.commit != series.repository_commit {
        bail!(
            "discovery report is not bound to series {}: repository_commit mismatch",
            series.series_id
        );
    }
    let mut discovery_paths: Vec<String> =
        discovery.tests.iter().map(|test| test.path.clone()).collect();
    discovery_paths.sort();
    if discovery_paths != series.normalized_manifest {
        bail!(
            "discovery report is not bound to series {}: normalized_manifest mismatch",
            series.series_id
        );
    }
    Ok(())
}

fn write_classification_receipt(
    path: &Path,
    classification: &Classification,
    accepted_baseline_digest: &str,
    compile_digest: &str,
) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let receipt = ClassifyReceipt {
        schema_version: CLASSIFY_RECEIPT_SCHEMA_VERSION.to_string(),
        command: "classify".to_string(),
        transition: classification.transition,
        reason: classification.reason.clone(),
        requires_candidate: classification.requires_candidate,
        semantic_boundary_change: classification.semantic_boundary_change,
        accepted_baseline_digest: accepted_baseline_digest.to_string(),
        compile_digest: compile_digest.to_string(),
        claim_boundary: CLASSIFY_CLAIM_BOUNDARY.to_string(),
    };
    let encoded = serde_json::to_string_pretty(&receipt).context("serializing classify receipt")?;
    fs::write(path, format!("{encoded}\n"))
        .with_context(|| format!("writing classify receipt {}", path.display()))?;
    Ok(())
}

fn load_classify_receipt(path: &Path) -> Result<ClassifyReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading classify receipt {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding classify receipt {}", path.display()))
}

fn verify_classify_receipt(
    receipt: &ClassifyReceipt,
    expected: &Classification,
    expected_accepted_digest: &str,
    expected_compile_digest: &str,
) -> Result<()> {
    if receipt.schema_version != CLASSIFY_RECEIPT_SCHEMA_VERSION {
        bail!(
            "classify receipt schema mismatch: {}; expected {}",
            receipt.schema_version,
            CLASSIFY_RECEIPT_SCHEMA_VERSION
        );
    }
    if receipt.command != "classify" {
        bail!("classify receipt command mismatch: {}; expected classify", receipt.command);
    }
    if receipt.accepted_baseline_digest != expected_accepted_digest {
        bail!("classify receipt accepted_baseline_digest does not match current evidence bytes");
    }
    if receipt.compile_digest != expected_compile_digest {
        bail!("classify receipt compile_digest does not match current evidence bytes");
    }
    if receipt.transition != expected.transition {
        bail!(
            "classify receipt transition mismatch: {:?}; recomputed {:?}",
            receipt.transition,
            expected.transition
        );
    }
    if receipt.reason != expected.reason {
        bail!("classify receipt reason does not match recomputed classification");
    }
    if receipt.requires_candidate != expected.requires_candidate {
        bail!("classify receipt requires_candidate does not match recomputed classification");
    }
    if receipt.semantic_boundary_change != expected.semantic_boundary_change {
        bail!("classify receipt semantic_boundary_change does not match recomputed classification");
    }
    if receipt.claim_boundary != CLASSIFY_CLAIM_BOUNDARY {
        bail!("classify receipt claim_boundary does not match current command contract");
    }
    Ok(())
}

fn sha256_digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_lower(&Sha256::digest(bytes)))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(*byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(*byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

const CLASSIFY_RECEIPT_SCHEMA_VERSION: &str = "perl_core_harness.transition_classify_result.v1";
const CLASSIFY_CLAIM_BOUNDARY: &str = "loads V2 accepted baseline + run-report JSON, optionally binds --series via series_id/manifest_hash/file_membership/profile/runner/perl_resolved_ref and --discovery via harness_schema_version/profile/runner/perl_resolved_ref/repository_commit/normalized_manifest, classifies via in-lib classify_transition, writes non-authorizing receipt with input digests; check recomputes digests+classification; does not accept ratchets or claim hard-link identity";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ClassifyReceipt {
    schema_version: String,
    command: String,
    transition: CompatibilityTransition,
    reason: String,
    requires_candidate: bool,
    semantic_boundary_change: bool,
    accepted_baseline_digest: String,
    compile_digest: String,
    claim_boundary: String,
}

#[cfg(test)]
mod classify_config_observer {
    use super::*;

    /// RIPR-named observer for Options::parse unrecognized-option rejection.
    #[test]
    fn unrecognized_option_parse_bail_is_observed() {
        let err = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--unknown".to_string(),
                "x.json".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("unrecognized options must fail")
        .to_string();
        assert_eq!(err, "unrecognized option(s): --unknown");
    }

    #[test]
    fn series_option_is_accepted_by_parse() {
        let options = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--series".to_string(),
                "series.json".to_string(),
            ]
            .into_iter(),
        )
        .expect("series option must parse");
        let config = ClassifyConfig::from_options(options).expect("classify config");
        assert_eq!(config.series, Some(PathBuf::from("series.json")));
        assert_eq!(config.discovery, None);
    }

    #[test]
    fn discovery_option_requires_series() {
        let err = ClassifyConfig::from_options(
            Options::parse(
                [
                    "--accepted-baseline".to_string(),
                    "accepted.json".to_string(),
                    "--compile".to_string(),
                    "compile.json".to_string(),
                    "--output".to_string(),
                    "out.json".to_string(),
                    "--discovery".to_string(),
                    "discovery.json".to_string(),
                ]
                .into_iter(),
            )
            .expect("parse"),
        )
        .expect_err("discovery without series must fail")
        .to_string();
        assert_eq!(
            err,
            "--discovery requires --series; discovery binds to the comparison-series manifest"
        );
    }

    #[test]
    fn discovery_option_is_accepted_with_series() {
        let options = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--series".to_string(),
                "series.json".to_string(),
                "--discovery".to_string(),
                "discovery.json".to_string(),
            ]
            .into_iter(),
        )
        .expect("discovery+series options must parse");
        let config = ClassifyConfig::from_options(options).expect("classify config");
        assert_eq!(config.series, Some(PathBuf::from("series.json")));
        assert_eq!(config.discovery, Some(PathBuf::from("discovery.json")));
    }

    /// RIPR observer for `Options::optional` empty-recorded queue → absent.
    #[test]
    fn optional_empty_recorded_value_is_treated_as_absent() {
        let mut values = BTreeMap::new();
        values.insert("--series".to_string(), VecDeque::new());
        let mut options = Options { values };
        let value = options.optional("--series").expect("empty recorded optional is absent");
        assert_eq!(value, None);
    }

    /// RIPR observer for `Options::optional` duplicate-value rejection.
    #[test]
    fn optional_duplicate_value_error_is_observed() {
        let mut values = BTreeMap::new();
        let mut recorded = VecDeque::new();
        recorded.push_back("a.json".to_string());
        recorded.push_back("b.json".to_string());
        values.insert("--series".to_string(), recorded);
        let mut options = Options { values };
        let err =
            options.optional("--series").expect_err("duplicate optional must fail").to_string();
        assert_eq!(err, "option --series may be supplied only once");
    }

    #[test]
    fn duplicate_option_is_rejected() {
        let err = Options::parse(
            [
                "--output".to_string(),
                "a.json".to_string(),
                "--output".to_string(),
                "b.json".to_string(),
            ]
            .into_iter(),
        )
        .and_then(|mut options| options.required("--output").map(|_| ()))
        .expect_err("duplicate option must fail")
        .to_string();
        assert_eq!(err, "option --output may be supplied only once");
    }

    #[test]
    fn check_rejects_output_option() {
        let err = CheckConfig::from_options(
            Options::parse(
                [
                    "--accepted-baseline".to_string(),
                    "accepted.json".to_string(),
                    "--compile".to_string(),
                    "compile.json".to_string(),
                    "--receipt".to_string(),
                    "receipt.json".to_string(),
                    "--output".to_string(),
                    "out.json".to_string(),
                ]
                .into_iter(),
            )
            .expect("parse"),
        )
        .expect_err("check must reject --output")
        .to_string();
        assert_eq!(err, "unrecognized option(s) for command: --output");
    }
}

#[cfg(test)]
mod classify_io_observer {
    use super::*;

    /// RIPR boundary discriminator for `paths_equal` (`left == right`).
    #[test]
    fn paths_equal_boundary_discriminator() {
        assert_eq!(paths_equal(Path::new("accepted.json"), Path::new("accepted.json")), true);
        assert_eq!(paths_equal(Path::new("accepted.json"), Path::new("compile.json")), false);
    }

    /// RIPR boundary discriminator for accepted schema inequality.
    #[test]
    fn load_accepted_v2_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("accepted.json");
        let schema = "perl_core_harness.compile_baseline.v1";
        assert_eq!(schema != COMPILE_BASELINE_V2_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let bytes = fs::read(&path).expect("read");
        assert_eq!(decode_accepted_v2(&bytes, &path).is_err(), true);
    }

    /// RIPR boundary discriminator for run-report schema inequality.
    #[test]
    fn load_run_report_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("compile.json");
        // Minimal JSON that deserializes as RunReport but fails the schema gate.
        let raw = format!(
            r#"{{
              "schema_version":"perl_core_harness.run_report.not_v1",
              "commit":"{commit}",
              "timestamp":"2026-08-11T00:00:00Z",
              "perl_ref":"perl",
              "prepared_tree":"<prepared>",
              "run_tree":"<run>",
              "host_perl":"perl",
              "runner":"test",
              "mode":"compile",
              "profile":"base",
              "harness_status":0,
              "summary":{{
                "files_total":0,
                "files_passed":0,
                "files_failed":0,
                "tap_assertions_total":0,
                "tap_assertions_passed":0
              }},
              "buckets":{{}},
              "file_results":[],
              "failures":[],
              "semantic_boundaries":[]
            }}"#,
            commit = "a".repeat(40)
        );
        fs::write(&path, raw).expect("write");
        let bytes = fs::read(&path).expect("read");
        let report: RunReport = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(report.schema_version != RUN_REPORT_SCHEMA_VERSION, true);
        assert_eq!(decode_run_report(&bytes, &path).is_err(), true);
    }

    /// RIPR-named observer for output/input path-string collision rejection.
    #[test]
    fn output_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("accepted.json"),
            series: None,
            discovery: None,
        })
        .expect_err("alias must fail")
        .to_string();
        assert_eq!(err.contains("output path must not alias"), true);
    }

    /// RIPR-named observer for output/--series path-string collision rejection.
    #[test]
    fn output_series_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("series.json"),
            series: Some(PathBuf::from("series.json")),
            discovery: None,
        })
        .expect_err("series alias must fail")
        .to_string();
        assert_eq!(err.contains("--series"), true);
    }

    /// RIPR-named observer for output/--discovery path-string collision rejection.
    #[test]
    fn output_discovery_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("discovery.json"),
            series: Some(PathBuf::from("series.json")),
            discovery: Some(PathBuf::from("discovery.json")),
        })
        .expect_err("discovery alias must fail")
        .to_string();
        assert_eq!(err.contains("--discovery"), true);
    }

    /// RIPR boundary discriminator for unsupported series schema rejection.
    #[test]
    fn load_series_manifest_schema_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("series.json");
        let schema = "perl_core_harness.comparison_series.not_v1";
        assert_eq!(schema != SERIES_MANIFEST_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let err = load_series_manifest(&path)
            .expect_err("unsupported series schema must fail")
            .to_string();
        assert_eq!(
            err,
            format!(
                "unsupported series manifest schema: {schema}; classify I/O accepts {} only",
                SERIES_MANIFEST_SCHEMA_VERSION
            )
        );
    }

    /// RIPR boundary discriminator for accepted.series_id != series.series_id.
    #[test]
    fn bind_series_series_id_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.series_id = "other".into();
        assert_eq!(accepted.series_id != series.series_id, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("series_id mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: series_id mismatch");
    }

    /// RIPR boundary discriminator for accepted.manifest_hash != series.manifest_hash.
    #[test]
    fn bind_series_manifest_hash_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.manifest_hash = "other-hash".into();
        assert_eq!(accepted.manifest_hash != series.manifest_hash, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("manifest_hash mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: manifest_hash mismatch");
    }

    /// RIPR boundary discriminator for accepted.file_membership != series.normalized_manifest.
    #[test]
    fn bind_series_file_membership_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.file_membership = vec!["base/9.t".into()];
        assert_eq!(accepted.file_membership != series.normalized_manifest, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("file_membership mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "accepted baseline is not bound to series series: file_membership mismatch"
        );
    }

    /// RIPR boundary discriminator for accepted.profile != series.profile.
    #[test]
    fn bind_series_profile_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.profile = perl_core_harness_types::HarnessProfile::Full;
        assert_eq!(accepted.profile != series.profile, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("profile mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: profile mismatch");
    }

    /// RIPR boundary discriminator for accepted.runner != series.runner.
    #[test]
    fn bind_series_runner_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.runner = perl_core_harness_types::HarnessRunner::Harness;
        assert_eq!(accepted.runner != series.runner, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("runner mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: runner mismatch");
    }

    /// RIPR boundary discriminator for accepted.perl_resolved_ref != series.perl_resolved_ref.
    #[test]
    fn bind_series_perl_resolved_ref_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.perl_resolved_ref = "other-perl".into();
        assert_eq!(accepted.perl_resolved_ref != series.perl_resolved_ref, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("perl_resolved_ref mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "accepted baseline is not bound to series series: perl_resolved_ref mismatch"
        );
    }

    /// RIPR boundary discriminator for unsupported discovery schema rejection.
    #[test]
    fn load_discovery_report_schema_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("discovery.json");
        let schema = "perl_core_harness.discovery.not_v1";
        assert_eq!(schema != DISCOVERY_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let err = load_discovery_report(&path)
            .expect_err("unsupported discovery schema must fail")
            .to_string();
        assert_eq!(
            err,
            format!(
                "unsupported discovery report schema: {schema}; classify I/O accepts {} only",
                DISCOVERY_SCHEMA_VERSION
            )
        );
    }

    /// RIPR boundary discriminator for discovery.schema_version != series.harness_schema_version.
    #[test]
    fn bind_discovery_schema_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.schema_version = "perl_core_harness.discovery.not_v1".into();
        assert_eq!(discovery.schema_version != series.harness_schema_version, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("schema mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: harness_schema_version mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery.profile != series.profile.
    #[test]
    fn bind_discovery_profile_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.profile = perl_core_harness_types::HarnessProfile::Full;
        assert_eq!(discovery.profile != series.profile, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("profile mismatch must fail")
            .to_string();
        assert_eq!(err, "discovery report is not bound to series series: profile mismatch");
    }

    /// RIPR boundary discriminator for discovery.runner != series.runner.
    #[test]
    fn bind_discovery_runner_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.runner = perl_core_harness_types::HarnessRunner::Harness;
        assert_eq!(discovery.runner != series.runner, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("runner mismatch must fail")
            .to_string();
        assert_eq!(err, "discovery report is not bound to series series: runner mismatch");
    }

    /// RIPR boundary discriminator for discovery.perl_ref != series.perl_resolved_ref.
    #[test]
    fn bind_discovery_perl_ref_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.perl_ref = "other-perl".into();
        assert_eq!(discovery.perl_ref != series.perl_resolved_ref, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("perl_ref mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: perl_resolved_ref mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery.commit != series.repository_commit.
    #[test]
    fn bind_discovery_commit_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.commit = "b".repeat(40);
        assert_eq!(discovery.commit != series.repository_commit, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("commit mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: repository_commit mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery paths != series.normalized_manifest.
    #[test]
    fn bind_discovery_membership_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.tests = vec![perl_core_harness_types::DiscoveredTest {
            path: "base/9.t".into(),
            root: "base".into(),
        }];
        let mut paths: Vec<String> = discovery.tests.iter().map(|t| t.path.clone()).collect();
        paths.sort();
        assert_eq!(paths != series.normalized_manifest, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("membership mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: normalized_manifest mismatch"
        );
    }

    fn discovery_bind_fixture() -> (SeriesManifest, DiscoveryReport) {
        let (series, _) = series_bind_fixture();
        let discovery = DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.into(),
            commit: series.repository_commit.clone(),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: series.perl_resolved_ref.clone(),
            prepared_tree: "<prepared>".into(),
            host_perl: "perl".into(),
            runner: series.runner,
            profile: series.profile,
            tests: series
                .normalized_manifest
                .iter()
                .map(|path| perl_core_harness_types::DiscoveredTest {
                    path: path.clone(),
                    root: "base".into(),
                })
                .collect(),
        };
        (series, discovery)
    }

    fn series_bind_fixture() -> (SeriesManifest, CompileBaselineV2) {
        let series = SeriesManifest {
            schema_version: SERIES_MANIFEST_SCHEMA_VERSION.to_string(),
            series_id: "series".into(),
            profile: perl_core_harness_types::HarnessProfile::Base,
            profile_roots: vec!["base".into()],
            repository_commit: "a".repeat(40),
            perl_requested_ref: "perl".into(),
            perl_resolved_ref: "perl".into(),
            runner: perl_core_harness_types::HarnessRunner::Test,
            normalized_manifest: vec!["base/0.t".into()],
            manifest_hash: "manifest".into(),
            preparation_receipt_id: "prepare".into(),
            preparation_receipt_digest: "sha256:prep".into(),
            harness_schema_version: "perl_core_harness.discovery.v1".into(),
            compiler_subject_identity: "compiler".into(),
            invocation_identity: "invocation".into(),
            capability_identity: "capability".into(),
            environment_identity: "environment".into(),
            normalization_version: "path-normalization.v1".into(),
            created_at: "2026-08-11T00:00:00Z".into(),
            replaces_series_id: None,
            change_reason: None,
        };
        let accepted = CompileBaselineV2 {
            schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.into(),
            report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            series_id: "series".into(),
            manifest_hash: "manifest".into(),
            repository_commit: "a".repeat(40),
            perl_resolved_ref: "perl".into(),
            preparation_receipt_id: "prepare".into(),
            compiler_subject_identity: "compiler".into(),
            invocation_identity: "invocation".into(),
            capability_identity: "capability".into(),
            environment_identity: "environment".into(),
            source_report_digest: "digest".into(),
            accepted_transition_id: None,
            evidence_bundle: None,
            mode: perl_core_harness_types::HarnessMode::Compile,
            profile: perl_core_harness_types::HarnessProfile::Base,
            runner: perl_core_harness_types::HarnessRunner::Test,
            file_membership: vec!["base/0.t".into()],
            files_total: 1,
            files_passed: 1,
            files_failed: 0,
            tap_assertions_total: 1,
            tap_assertions_passed: 1,
            buckets: BTreeMap::new(),
            expected_failures: Vec::new(),
            file_results: vec![perl_core_harness_types::RunFileResult {
                path: "base/0.t".into(),
                status: perl_core_harness_types::RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
            }],
            semantic_boundaries: Vec::new(),
            boundary_retirements: Vec::new(),
        };
        (series, accepted)
    }

    /// RIPR boundary discriminator for digest mismatch rejection.
    #[test]
    fn verify_classify_receipt_digest_mismatch_is_observed() {
        let expected = Classification {
            transition: CompatibilityTransition::NoChange,
            reason: "complete observation exactly matches the accepted v2 ratchet".into(),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
        let receipt = ClassifyReceipt {
            schema_version: CLASSIFY_RECEIPT_SCHEMA_VERSION.to_string(),
            command: "classify".to_string(),
            transition: expected.transition,
            reason: expected.reason.clone(),
            requires_candidate: expected.requires_candidate,
            semantic_boundary_change: expected.semantic_boundary_change,
            accepted_baseline_digest: "sha256:aaaa".to_string(),
            compile_digest: "sha256:bbbb".to_string(),
            claim_boundary: CLASSIFY_CLAIM_BOUNDARY.to_string(),
        };
        let err = verify_classify_receipt(&receipt, &expected, "sha256:cccc", "sha256:bbbb")
            .expect_err("digest mismatch must fail")
            .to_string();
        assert_eq!(
            err.contains("accepted_baseline_digest does not match current evidence bytes"),
            true
        );
    }
}
