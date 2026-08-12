//! Thin CLI for the transition classify command surface.
//!
//! This slice loads V2 accepted baseline + current run-report JSON, applies the
//! in-lib `classify_transition` core, and writes a non-authorizing classification
//! receipt to `--output`.
//!
//! Receipt digests / `check`, discovery/series binding, and Windows hard-link
//! identity remain follow-up slices. Full #6880 validated-wrapper centralization
//! is intentionally out of scope.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    RUN_REPORT_SCHEMA_VERSION, RunReport,
};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

include!("perl-core-harness-transition/cli.rs");

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-transition classify --accepted-baseline <path> --compile <path> --output <path>"
        )
    })?;
    let options = Options::parse(args)?;
    match command.as_str() {
        "classify" => {
            let config = ClassifyConfig::from_options(options)?;
            run_classify(&config)
        }
        _ => bail!("unknown perl-core-harness-transition command: {command}"),
    }
}

fn run_classify(config: &ClassifyConfig) -> Result<()> {
    reject_output_input_path_collision(config)?;
    let accepted = load_accepted_v2(&config.accepted_baseline)?;
    let current = load_run_report(&config.compile)?;
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    write_classification_receipt(&config.output, &classification)
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

fn load_accepted_v2(path: &Path) -> Result<CompileBaselineV2> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading accepted baseline {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
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

fn load_run_report(path: &Path) -> Result<RunReport> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading compile observation {}", path.display()))?;
    let report: RunReport = serde_json::from_str(&raw)
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

fn write_classification_receipt(path: &Path, classification: &Classification) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let receipt = ClassifyReceipt {
        schema_version: CLASSIFY_RECEIPT_SCHEMA_VERSION,
        command: "classify",
        transition: classification.transition,
        reason: classification.reason.clone(),
        requires_candidate: classification.requires_candidate,
        semantic_boundary_change: classification.semantic_boundary_change,
        claim_boundary: "loads V2 accepted baseline + run-report JSON, classifies via in-lib classify_transition, writes non-authorizing receipt; does not accept ratchets, bind discovery/series, or verify digests",
    };
    let encoded = serde_json::to_string_pretty(&receipt).context("serializing classify receipt")?;
    fs::write(path, format!("{encoded}\n"))
        .with_context(|| format!("writing classify receipt {}", path.display()))?;
    Ok(())
}

const CLASSIFY_RECEIPT_SCHEMA_VERSION: &str = "perl_core_harness.transition_classify_result.v1";

#[derive(Debug, Serialize)]
struct ClassifyReceipt {
    schema_version: &'static str,
    command: &'static str,
    transition: CompatibilityTransition,
    reason: String,
    requires_candidate: bool,
    semantic_boundary_change: bool,
    claim_boundary: &'static str,
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
        assert_eq!(load_accepted_v2(&path).is_err(), true);
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
        let report: RunReport =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("decode");
        assert_eq!(report.schema_version != RUN_REPORT_SCHEMA_VERSION, true);
        assert_eq!(load_run_report(&path).is_err(), true);
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
}
