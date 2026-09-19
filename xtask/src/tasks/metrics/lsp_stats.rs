//! Semantic timing guard for compatibility editor-UX receipts.
//!
//! This is the public entry point of the `metrics lsp-stats` chain: timing
//! guard here, then receipt/fixture admission in `lsp_stats_guarded.rs`, then
//! the historical scorecard in `lsp_stats_impl.rs`.
//!
//! The existing admission module owns JSON/schema and fixture-matrix identity.
//! This boundary adds the producer's three-state operation-timing invariant
//! before any admitted receipt can contribute to the legacy scorecard. The
//! directory is intentionally still read by the compatibility layers more than
//! once; #13424 owns replacing those reads with one immutable receipt set.

use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_ux_tests::recorder::{OperationTiming, UxScenarioRunReceipt};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub use super::lsp_stats_guarded::{
    LatencyMetric, MeasuredEditorUxScorecard, RateMetric, WorkflowResult,
};

/// Run `cargo xtask metrics lsp-stats` after semantic timing validation.
pub fn run_with_receipt_dir(
    json: bool,
    receipt_dir: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    if let Some(receipts_dir) = receipt_dir {
        validate_timing_receipts(receipts_dir)?;
    }
    super::lsp_stats_guarded::run_with_receipt_dir(json, receipt_dir, output)
}

fn validate_timing_receipts(receipts_dir: &Path) -> Result<()> {
    if !receipts_dir.exists() {
        return Ok(());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(receipts_dir)
        .with_context(|| format!("reading receipts directory: {}", receipts_dir.display()))?
    {
        let path = entry.with_context(|| "reading receipt directory entry")?.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();

    for path in paths {
        validate_timing_candidate(&path)?;
    }
    Ok(())
}

fn validate_timing_candidate(path: &Path) -> Result<()> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading receipt candidate: {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parsing JSON receipt candidate: {}", path.display()))?;
    if value.get("kind").and_then(Value::as_str) != Some("ux_scenario_run") {
        return Ok(());
    }

    let receipt: UxScenarioRunReceipt = serde_json::from_value(value)
        .with_context(|| format!("deserializing UX receipt: {}", path.display()))?;
    validate_timing_semantics(&receipt, path)
}

fn validate_timing_semantics(receipt: &UxScenarioRunReceipt, path: &Path) -> Result<()> {
    validate_measurement("duration_ms", receipt.duration_ms, receipt.duration_ms, receipt, path)?;

    let mut operations = BTreeSet::new();
    // First operation row that carries a TTFR measurement; Started and
    // MissingRequestStart rows have none and are walked past until a
    // Completed row supplies the value.
    let mut first_completed_measurement = None;
    for timing in &receipt.operation_timings {
        if timing.operation.trim().is_empty() {
            bail!(
                "invalid UX timing receipt {} workflow `{}` test `{}`: operation name is empty",
                path.display(),
                receipt.workflow_id,
                receipt.test_name
            );
        }
        if !operations.insert(timing.operation.as_str()) {
            bail!(
                "invalid UX timing receipt {} workflow `{}` test `{}`: duplicate operation `{}`",
                path.display(),
                receipt.workflow_id,
                receipt.test_name,
                timing.operation
            );
        }

        validate_operation_timing(timing, receipt, path)?;
        if first_completed_measurement.is_none() {
            first_completed_measurement = timing.time_to_first_useful_result_ms;
        }
    }

    if let Some(top_level) = receipt.time_to_first_useful_result_ms {
        validate_measurement(
            "time_to_first_useful_result_ms",
            top_level,
            receipt.duration_ms,
            receipt,
            path,
        )?;
    }

    // Load-bearing admission boundary: legacy receipts carry only the
    // top-level TTFR with no operation rows, so the match below must be
    // skipped when no rows are present.
    if !receipt.operation_timings.is_empty() {
        match (first_completed_measurement, receipt.time_to_first_useful_result_ms) {
            (Some(first), Some(top_level)) if first != top_level => {
                bail!(
                    "invalid UX timing receipt {} workflow `{}` test `{}`: top-level TTFR {top_level} ms disagrees with first completed operation {first} ms",
                    path.display(),
                    receipt.workflow_id,
                    receipt.test_name
                );
            }
            (Some(first), None) => {
                bail!(
                    "invalid UX timing receipt {} workflow `{}` test `{}`: first completed operation has TTFR {first} ms but the top-level TTFR is absent",
                    path.display(),
                    receipt.workflow_id,
                    receipt.test_name
                );
            }
            (None, Some(top_level)) => {
                bail!(
                    "invalid UX timing receipt {} workflow `{}` test `{}`: top-level TTFR {top_level} ms exists but populated operation rows contain no completed measurement",
                    path.display(),
                    receipt.workflow_id,
                    receipt.test_name
                );
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_operation_timing(
    timing: &OperationTiming,
    receipt: &UxScenarioRunReceipt,
    path: &Path,
) -> Result<()> {
    match (timing.time_to_first_useful_result_ms, timing.timing_status.as_deref()) {
        (Some(measured), None) => validate_measurement(
            &format!("operation `{}` TTFR", timing.operation),
            measured,
            receipt.duration_ms,
            receipt,
            path,
        ),
        (None, None) | (None, Some("missing_request_start")) => Ok(()),
        (Some(measured), Some(status)) => bail!(
            "invalid UX timing receipt {} workflow `{}` test `{}`: operation `{}` has measured TTFR {measured} ms and status `{status}`",
            path.display(),
            receipt.workflow_id,
            receipt.test_name,
            timing.operation
        ),
        (None, Some(status)) => bail!(
            "invalid UX timing receipt {} workflow `{}` test `{}`: operation `{}` has unknown timing status `{status}`",
            path.display(),
            receipt.workflow_id,
            receipt.test_name,
            timing.operation
        ),
    }
}

fn validate_measurement(
    label: &str,
    value: f64,
    duration_ms: f64,
    receipt: &UxScenarioRunReceipt,
    path: &Path,
) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        bail!(
            "invalid UX timing receipt {} workflow `{}` test `{}`: {label} must be finite and nonnegative, got {value}",
            path.display(),
            receipt.workflow_id,
            receipt.test_name
        );
    }
    if label != "duration_ms" && value > duration_ms {
        bail!(
            "invalid UX timing receipt {} workflow `{}` test `{}`: {label} {value} ms exceeds duration_ms {duration_ms} ms",
            path.display(),
            receipt.workflow_id,
            receipt.test_name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn receipt(
        duration_ms: f64,
        top_level: Option<f64>,
        operation_timings: Value,
    ) -> Result<UxScenarioRunReceipt> {
        Ok(serde_json::from_value(json!({
            "kind": "ux_scenario_run",
            "schema_version": 1,
            "measured_at": "2026-08-29T00:00:00Z",
            "run_identity": { "sha": "abcdef12", "branch": "main" },
            "workflow_id": "known",
            "scenario_file": "known.rs",
            "test_name": "timing_semantics",
            "ci_tier": "pr",
            "result": "pass",
            "duration_ms": duration_ms,
            "time_to_first_useful_result_ms": top_level,
            "operation_timings": operation_timings,
            "assertions": {
                "passed": 1,
                "failed": 0,
                "basis": "instrumented"
            },
            "canonical_repro": "cargo test -p perl-lsp-ux-tests timing_semantics",
            "friendly_repro": "just ux-tests timing_semantics"
        }))?)
    }

    fn error_message(result: Result<()>, context: &str) -> Result<String> {
        match result {
            Ok(()) => bail!("{context}"),
            Err(error) => Ok(format!("{error:#}")),
        }
    }

    #[test]
    fn all_three_producer_timing_states_are_valid() -> Result<()> {
        let receipt = receipt(
            25.0,
            Some(12.0),
            json!([
                { "operation": "pending" },
                {
                    "operation": "completed",
                    "time_to_first_useful_result_ms": 12.0
                },
                {
                    "operation": "missing_start",
                    "time_to_first_useful_result_ms": null,
                    "timing_status": "missing_request_start"
                }
            ]),
        )?;

        validate_timing_semantics(&receipt, Path::new("valid.json"))?;
        Ok(())
    }

    #[test]
    fn standalone_legacy_top_level_timing_remains_valid() -> Result<()> {
        let receipt = receipt(25.0, Some(12.0), json!([]))?;
        validate_timing_semantics(&receipt, Path::new("legacy.json"))
    }

    #[test]
    fn measured_timing_and_status_fail_closed() -> Result<()> {
        let receipt = receipt(
            25.0,
            Some(12.0),
            json!([{
                "operation": "hover",
                "time_to_first_useful_result_ms": 12.0,
                "timing_status": "missing_request_start"
            }]),
        )?;
        let message = error_message(
            validate_timing_semantics(&receipt, Path::new("contradictory.json")),
            "measured timing plus status unexpectedly passed",
        )?;
        assert!(message.contains("has measured TTFR"));
        Ok(())
    }

    #[test]
    fn duplicate_operation_identity_fails_closed() -> Result<()> {
        let receipt = receipt(
            25.0,
            Some(12.0),
            json!([
                { "operation": "hover", "time_to_first_useful_result_ms": 12.0 },
                { "operation": "hover" }
            ]),
        )?;
        let message = error_message(
            validate_timing_semantics(&receipt, Path::new("duplicate.json")),
            "duplicate operation unexpectedly passed",
        )?;
        assert!(message.contains("duplicate operation `hover`"));
        Ok(())
    }

    #[test]
    fn operation_and_top_level_measurements_cannot_exceed_duration() -> Result<()> {
        for (name, receipt) in [
            (
                "operation",
                receipt(
                    10.0,
                    Some(11.0),
                    json!([{
                        "operation": "hover",
                        "time_to_first_useful_result_ms": 11.0
                    }]),
                )?,
            ),
            ("top-level", receipt(10.0, Some(11.0), json!([]))?),
        ] {
            let message = error_message(
                validate_timing_semantics(&receipt, Path::new("duration.json")),
                &format!("{name} timing greater than duration unexpectedly passed"),
            )?;
            assert!(message.contains("exceeds duration_ms"));
        }
        Ok(())
    }

    #[test]
    fn top_level_summary_must_match_first_completed_operation() -> Result<()> {
        let receipt = receipt(
            25.0,
            Some(15.0),
            json!([
                { "operation": "pending" },
                { "operation": "hover", "time_to_first_useful_result_ms": 12.0 },
                { "operation": "completion", "time_to_first_useful_result_ms": 15.0 }
            ]),
        )?;
        let message = error_message(
            validate_timing_semantics(&receipt, Path::new("summary.json")),
            "mismatched top-level timing unexpectedly passed",
        )?;
        assert!(message.contains("disagrees with first completed operation"));
        Ok(())
    }

    #[test]
    fn completed_operation_requires_top_level_summary() -> Result<()> {
        let receipt = receipt(
            25.0,
            None,
            json!([{
                "operation": "hover",
                "time_to_first_useful_result_ms": 12.0
            }]),
        )?;
        let message = error_message(
            validate_timing_semantics(&receipt, Path::new("missing-summary.json")),
            "completed operation without top-level timing unexpectedly passed",
        )?;
        assert!(message.contains("top-level TTFR is absent"));
        Ok(())
    }

    #[test]
    fn populated_rows_without_completed_measurement_forbid_top_level_timing() -> Result<()> {
        let receipt = receipt(
            25.0,
            Some(12.0),
            json!([
                { "operation": "pending" },
                {
                    "operation": "missing_start",
                    "timing_status": "missing_request_start"
                }
            ]),
        )?;
        let message = error_message(
            validate_timing_semantics(&receipt, Path::new("no-completed.json")),
            "top-level timing without completed operation unexpectedly passed",
        )?;
        assert!(message.contains("contain no completed measurement"));
        Ok(())
    }

    #[test]
    fn checked_schema_rejects_measured_missing_start_but_accepts_null_missing_start() -> Result<()>
    {
        let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".ci/schemas/ux-scenario-run.schema.json");
        let schema: Value = serde_json::from_str(&fs::read_to_string(schema_path)?)?;
        let validator = jsonschema::validator_for(&schema)
            .map_err(|error| color_eyre::eyre::eyre!("compiling receipt schema: {error}"))?;

        let invalid: Value = serde_json::to_value(receipt(
            25.0,
            Some(12.0),
            json!([{
                "operation": "hover",
                "time_to_first_useful_result_ms": 12.0,
                "timing_status": "missing_request_start"
            }]),
        )?)?;
        assert!(validator.validate(&invalid).is_err());

        let mut valid: Value = serde_json::to_value(receipt(25.0, None, json!([]))?)?;
        valid["operation_timings"] = json!([{
            "operation": "hover",
            "time_to_first_useful_result_ms": null,
            "timing_status": "missing_request_start"
        }]);
        validator.validate(&valid).map_err(|error| {
            color_eyre::eyre::eyre!("valid missing-start receipt rejected: {error}")
        })?;
        Ok(())
    }
}
