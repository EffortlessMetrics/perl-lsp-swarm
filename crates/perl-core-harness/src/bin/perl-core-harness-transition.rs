//! Thin CLI for the transition classify + check command surface.
//!
//! `classify` loads V2 accepted baseline + current run-report JSON, applies the
//! in-lib `classify_transition` core, and writes a non-authorizing classification
//! receipt (with input digests) to `--output`.
//!
//! `check` reloads the same evidence, recomputes classification + digests, and
//! verifies an existing receipt matches exactly.
//!
//! Discovery/series binding and Windows hard-link identity remain follow-up
//! slices. Full #6880 validated-wrapper centralization is intentionally out of
//! scope.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    RUN_REPORT_SCHEMA_VERSION, RunReport,
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
    {
        bail!(
            "output path must not alias --accepted-baseline or --compile; refusing to overwrite evidence"
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
const CLASSIFY_CLAIM_BOUNDARY: &str = "loads V2 accepted baseline + run-report JSON, classifies via in-lib classify_transition, writes non-authorizing receipt with input digests; check recomputes digests+classification; does not accept ratchets, bind discovery/series, or claim hard-link identity";

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
                "--series".to_string(),
                "series.json".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("unrecognized options must fail")
        .to_string();
        assert_eq!(err, "unrecognized option(s): --series");
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
        })
        .expect_err("alias must fail")
        .to_string();
        assert_eq!(err.contains("output path must not alias"), true);
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
