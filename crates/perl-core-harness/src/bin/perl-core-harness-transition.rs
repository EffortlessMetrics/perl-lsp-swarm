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
    use perl_core_harness_types::{
        HarnessMode, HarnessProfile, HarnessRunner, RunFileResult, RunSummary, RunnerStatus,
    };
    use std::collections::BTreeMap;

    /// RIPR-named observer for unsupported accepted schema rejection.
    #[test]
    fn unsupported_accepted_schema_bail_is_observed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("accepted.json");
        fs::write(&path, r#"{"schema_version":"perl_core_harness.compile_baseline.v1"}"#)
            .expect("write");
        let err = load_accepted_v2(&path).expect_err("v1 must fail").to_string();
        assert!(err.contains("unsupported accepted baseline schema"));
        assert!(err.contains("compile_baseline.v1"));
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
        assert!(err.contains("output path must not alias"));
    }

    /// RIPR-named observer for classify I/O no-change receipt write.
    #[test]
    fn classify_io_exact_match_writes_no_change_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let accepted_path = dir.path().join("accepted.json");
        let compile_path = dir.path().join("compile.json");
        let output_path = dir.path().join("out.json");
        write_sample_pair(&accepted_path, &compile_path, 2, 2, 2, 2);
        run_classify(&ClassifyConfig {
            accepted_baseline: accepted_path,
            compile: compile_path,
            output: output_path.clone(),
        })
        .expect("classify");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output_path).expect("read")).expect("decode");
        assert_eq!(value["schema_version"], CLASSIFY_RECEIPT_SCHEMA_VERSION);
        assert_eq!(value["transition"], "no_change");
        assert_eq!(value["requires_candidate"], false);
    }

    fn write_sample_pair(
        accepted_path: &Path,
        compile_path: &Path,
        accepted_total: usize,
        accepted_passed: usize,
        current_total: usize,
        current_passed: usize,
    ) {
        let accepted = sample_v2_baseline(accepted_total, accepted_passed);
        let current = sample_report(current_total, current_passed);
        fs::write(accepted_path, serde_json::to_string_pretty(&accepted).expect("encode"))
            .expect("write accepted");
        fs::write(compile_path, serde_json::to_string_pretty(&current).expect("encode"))
            .expect("write compile");
    }

    fn sample_report(total: usize, passed: usize) -> RunReport {
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "a".repeat(40),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: "perl".into(),
            prepared_tree: "<prepared>".into(),
            run_tree: "<run>".into(),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            harness_status: Some(0),
            summary: RunSummary {
                files_total: total,
                files_passed: passed,
                files_failed: total - passed,
                tap_assertions_total: total,
                tap_assertions_passed: passed,
            },
            buckets: BTreeMap::new(),
            file_results: sample_results(total, passed),
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        }
    }

    fn sample_results(total: usize, passed: usize) -> Vec<RunFileResult> {
        (0..total)
            .map(|index| {
                let status = if index < passed { RunnerStatus::Pass } else { RunnerStatus::Fail };
                RunFileResult {
                    path: format!("base/{index}.t"),
                    status,
                    assertions_passed: usize::from(status == RunnerStatus::Pass),
                    assertions_total: 1,
                }
            })
            .collect()
    }

    fn sample_v2_baseline(total: usize, passed: usize) -> CompileBaselineV2 {
        let file_results = sample_results(total, passed);
        CompileBaselineV2 {
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
            accepted_transition_id: Some("transition".into()),
            evidence_bundle: Some("bundle".into()),
            mode: HarnessMode::Compile,
            profile: HarnessProfile::Base,
            runner: HarnessRunner::Test,
            file_membership: file_results.iter().map(|result| result.path.clone()).collect(),
            files_total: total,
            files_passed: passed,
            files_failed: total - passed,
            tap_assertions_total: total,
            tap_assertions_passed: passed,
            buckets: BTreeMap::new(),
            expected_failures: Vec::new(),
            file_results,
            semantic_boundaries: Vec::new(),
            boundary_retirements: Vec::new(),
        }
    }
}
