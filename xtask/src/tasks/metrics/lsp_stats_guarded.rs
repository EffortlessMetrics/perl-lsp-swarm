//! Fail-closed boundary for the receipt-based editor UX scorecard.
//!
//! The historical scorecard implementation remains in `lsp_stats.rs`; this
//! module validates receipt and fixture identity before delegating to it. The
//! boundary prevents malformed or matrix-drifted UX run evidence from
//! disappearing from an otherwise green aggregation while preserving other
//! valid receipt families that share the same directory.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_ux_tests::recorder::UxScenarioRunReceipt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub use super::lsp_stats_impl::{
    LatencyMetric, MeasuredEditorUxScorecard, RateMetric, WorkflowResult,
};

const RECEIPT_SCHEMA_PATH: &str = ".ci/schemas/ux-scenario-run.schema.json";
const FIXTURE_MATRIX_PATH: &str = "crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json";

/// Fields that distinguish an editor-UX scenario run from companion receipts
/// such as Scenario 67's `golden_editor_workload` evidence. Generic receipt
/// fields (`schema_version`, timestamps, and `run_identity`) are deliberately
/// excluded because multiple receipt families share them.
const UX_RUN_SIGNATURE_FIELDS: &[&str] = &[
    "workflow_id",
    "scenario_file",
    "test_name",
    "ci_tier",
    "result",
    "duration_ms",
    "assertions",
    "canonical_repro",
    "friendly_repro",
];

const UX_RUN_IDENTITY_FIELDS: &[&str] = &["workflow_id", "scenario_file", "test_name"];
const MIN_UX_RUN_SIGNATURE_FIELDS: usize = 2;

#[derive(Debug, Deserialize)]
struct FixtureMatrix {
    workflows: Vec<FixtureWorkflow>,
}

#[derive(Debug, Deserialize)]
struct FixtureWorkflow {
    id: String,
    scenario_file: String,
}

#[derive(Debug)]
struct ReceiptCandidate {
    path: PathBuf,
    value: Value,
}

/// Run `cargo xtask metrics lsp-stats` with fail-closed receipt validation.
pub fn run_with_receipt_dir(json: bool, receipt_dir: Option<&Path>) -> Result<()> {
    if let Some(receipts_dir) = receipt_dir {
        let root = project_root()?;
        validate_scorecard_inputs(
            receipts_dir,
            &root.join(FIXTURE_MATRIX_PATH),
            &root.join(RECEIPT_SCHEMA_PATH),
        )?;
    }

    super::lsp_stats_impl::run_with_receipt_dir(json, receipt_dir)
}

/// Aggregate receipts after validating their schema and fixture identity.
pub fn aggregate_from_receipts(
    receipts_dir: &Path,
    fixture_matrix: &Path,
    flake_ledger: Option<&Path>,
) -> Result<MeasuredEditorUxScorecard> {
    let root = project_root()?;
    validate_scorecard_inputs(receipts_dir, fixture_matrix, &root.join(RECEIPT_SCHEMA_PATH))?;
    super::lsp_stats_impl::aggregate_from_receipts(receipts_dir, fixture_matrix, flake_ledger)
}

fn validate_scorecard_inputs(
    receipts_dir: &Path,
    fixture_matrix: &Path,
    receipt_schema: &Path,
) -> Result<()> {
    let workflows = load_workflows(fixture_matrix)?;
    let validator = load_receipt_validator(receipt_schema)?;

    for candidate in read_receipt_candidates(receipts_dir)? {
        let kind = candidate.value.get("kind").and_then(Value::as_str);
        if kind != Some("ux_scenario_run") {
            if looks_like_ux_scenario_run(&candidate.value) {
                bail!(
                    "editor UX run-shaped JSON {} has unsupported or malformed kind {:?}",
                    candidate.path.display(),
                    kind
                );
            }
            continue;
        }

        if let Err(error) = validator.validate(&candidate.value) {
            bail!("invalid UX scenario receipt {}: {error}", candidate.path.display());
        }

        let receipt: UxScenarioRunReceipt = serde_json::from_value(candidate.value)
            .with_context(|| format!("deserializing UX receipt: {}", candidate.path.display()))?;
        let workflow = workflows.get(receipt.workflow_id.as_str()).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "editor UX matrix drift: receipt {} test `{}` references unknown workflow `{}`",
                candidate.path.display(),
                receipt.test_name,
                receipt.workflow_id
            )
        })?;
        if receipt.scenario_file != workflow.scenario_file {
            bail!(
                "editor UX matrix drift: receipt {} workflow `{}` reports scenario `{}`, expected `{}`",
                candidate.path.display(),
                receipt.workflow_id,
                receipt.scenario_file,
                workflow.scenario_file
            );
        }
    }

    Ok(())
}

fn load_workflows(fixture_matrix: &Path) -> Result<BTreeMap<String, FixtureWorkflow>> {
    let matrix_raw = fs::read_to_string(fixture_matrix)
        .with_context(|| format!("reading fixture matrix: {}", fixture_matrix.display()))?;
    let matrix: FixtureMatrix = serde_json::from_str(&matrix_raw)
        .with_context(|| format!("parsing fixture matrix: {}", fixture_matrix.display()))?;

    if matrix.workflows.is_empty() {
        bail!("editor UX fixture matrix has no workflows: {}", fixture_matrix.display());
    }

    let mut workflows = BTreeMap::new();
    for workflow in matrix.workflows {
        if workflow.id.trim().is_empty() {
            bail!("editor UX fixture matrix contains an empty workflow id");
        }
        if workflow.scenario_file.trim().is_empty() {
            bail!("editor UX fixture matrix workflow `{}` has an empty scenario_file", workflow.id);
        }
        let id = workflow.id.clone();
        if workflows.insert(id.clone(), workflow).is_some() {
            bail!("editor UX fixture matrix contains duplicate workflow id `{id}`");
        }
    }
    Ok(workflows)
}

fn load_receipt_validator(receipt_schema: &Path) -> Result<jsonschema::Validator> {
    let schema_raw = fs::read_to_string(receipt_schema)
        .with_context(|| format!("reading UX receipt schema: {}", receipt_schema.display()))?;
    let schema: Value = serde_json::from_str(&schema_raw)
        .with_context(|| format!("parsing UX receipt schema: {}", receipt_schema.display()))?;
    jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("compiling UX receipt schema: {error}"))
}

/// Identify malformed UX run candidates without claiming every JSON receipt in
/// the shared directory. A candidate must carry at least one UX-run identity
/// field and one additional UX-run signature field. This catches missing,
/// non-string, or wrong discriminators while allowing distinct companion
/// receipts with generic metadata to remain outside the scorecard denominator.
fn looks_like_ux_scenario_run(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    let has_identity = UX_RUN_IDENTITY_FIELDS.iter().any(|field| object.contains_key(*field));
    let signature_count =
        UX_RUN_SIGNATURE_FIELDS.iter().filter(|field| object.contains_key(**field)).count();
    has_identity && signature_count >= MIN_UX_RUN_SIGNATURE_FIELDS
}

fn read_receipt_candidates(receipts_dir: &Path) -> Result<Vec<ReceiptCandidate>> {
    if !receipts_dir.exists() {
        return Ok(Vec::new());
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

    paths
        .into_iter()
        .map(|path| {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("reading receipt candidate: {}", path.display()))?;
            let value = serde_json::from_str(&raw)
                .with_context(|| format!("parsing JSON receipt candidate: {}", path.display()))?;
            Ok(ReceiptCandidate { path, value })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_error(result: Result<()>, context: &str) -> Result<color_eyre::Report> {
        match result {
            Ok(()) => bail!("{context}"),
            Err(error) => Ok(error),
        }
    }

    fn write_schema(dir: &Path) -> Result<PathBuf> {
        let schema = serde_json::json!({
            "type": "object",
            "required": [
                "kind", "schema_version", "measured_at", "run_identity", "workflow_id",
                "scenario_file", "test_name", "ci_tier", "result", "duration_ms",
                "assertions", "canonical_repro", "friendly_repro"
            ],
            "properties": {
                "kind": { "const": "ux_scenario_run" },
                "schema_version": { "const": 1 },
                "measured_at": { "type": "string" },
                "run_identity": { "type": "object" },
                "workflow_id": { "type": "string", "minLength": 1 },
                "scenario_file": { "type": "string", "minLength": 1 },
                "test_name": { "type": "string", "minLength": 1 },
                "ci_tier": { "enum": ["pr", "nightly", "release"] },
                "result": { "enum": ["pass", "fail", "quarantined", "skipped"] },
                "duration_ms": { "type": "number", "minimum": 0 },
                "time_to_first_useful_result_ms": { "type": ["number", "null"], "minimum": 0 },
                "operation_timings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["operation"],
                        "properties": {
                            "operation": { "type": "string", "minLength": 1 },
                            "time_to_first_useful_result_ms": {
                                "type": ["number", "null"],
                                "minimum": 0
                            },
                            "timing_status": { "enum": ["missing_request_start"] }
                        },
                        "additionalProperties": false
                    }
                },
                "assertions": { "type": "object" },
                "canonical_repro": { "type": "string", "minLength": 1 },
                "friendly_repro": { "type": "string", "minLength": 1 }
            }
        });
        let path = dir.join("receipt.schema.json");
        fs::write(&path, serde_json::to_string_pretty(&schema)?)?;
        Ok(path)
    }

    fn write_matrix(dir: &Path, workflows: &[(&str, &str)]) -> Result<PathBuf> {
        let workflows: Vec<Value> = workflows
            .iter()
            .map(|(id, scenario_file)| {
                serde_json::json!({ "id": id, "scenario_file": scenario_file })
            })
            .collect();
        let path = dir.join("matrix.json");
        fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({ "workflows": workflows }))?,
        )?;
        Ok(path)
    }

    fn write_receipt(
        dir: &Path,
        filename: &str,
        workflow_id: &str,
        scenario_file: &str,
    ) -> Result<PathBuf> {
        let receipt = serde_json::json!({
            "kind": "ux_scenario_run",
            "schema_version": 1,
            "measured_at": "2026-08-27T00:00:00Z",
            "run_identity": { "sha": "abcdef12", "branch": "main" },
            "workflow_id": workflow_id,
            "scenario_file": scenario_file,
            "test_name": "scorecard_guard_test",
            "ci_tier": "pr",
            "result": "pass",
            "duration_ms": 10.0,
            "assertions": {
                "passed": 1,
                "failed": 0,
                "basis": "instrumented"
            },
            "canonical_repro": "cargo test -p perl-lsp-ux-tests scorecard_guard_test",
            "friendly_repro": "just ux-tests"
        });
        let path = dir.join(filename);
        fs::write(&path, serde_json::to_string_pretty(&receipt)?)?;
        Ok(path)
    }

    #[test]
    fn malformed_json_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(receipts.join("broken.json"), "{")?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "malformed JSON unexpectedly passed the scorecard boundary",
        )?;
        assert!(format!("{error:#}").contains("broken.json"));
        Ok(())
    }

    #[test]
    fn generic_non_receipt_json_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(receipts.join("schema.json"), r#"{"$schema":"https://json-schema.org"}"#)?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        validate_scorecard_inputs(&receipts, &matrix, &schema)?;
        Ok(())
    }

    #[test]
    fn golden_workload_companion_receipt_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let companion = serde_json::json!({
            "kind": "golden_editor_workload",
            "schema_version": 3,
            "measured_at_unix_ms": 1,
            "manifest_version": "test",
            "claim_boundary": "baseline_only",
            "run_identity": { "commit": "abcdef12", "run_id": "1", "ci": true },
            "projects": [],
            "rows": [],
            "rollup": {}
        });
        fs::write(
            receipts.join("golden-editor-workload-v3.json"),
            serde_json::to_string_pretty(&companion)?,
        )?;
        let matrix = write_matrix(temp.path(), &[("golden_editor_workload", "scenario.rs")])?;
        let schema = write_schema(temp.path())?;

        validate_scorecard_inputs(&receipts, &matrix, &schema)?;
        Ok(())
    }

    #[test]
    fn ux_run_shaped_json_with_wrong_kind_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let path = write_receipt(&receipts, "wrong-kind.json", "known", "known.rs")?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        value["kind"] = Value::String("other_receipt".to_string());
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "UX-run-shaped JSON with the wrong kind unexpectedly passed",
        )?;
        assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        Ok(())
    }

    #[test]
    fn malformed_kind_with_no_workflow_id_still_fails_closed() -> Result<()> {
        for kind in [None, Some(Value::Null)] {
            let temp = tempfile::tempdir()?;
            let receipts = temp.path().join("receipts");
            fs::create_dir_all(&receipts)?;
            let mut value = serde_json::json!({
                "scenario_file": "known.rs",
                "duration_ms": "not-a-number"
            });
            if let Some(kind) = kind {
                value["kind"] = kind;
            }
            fs::write(receipts.join("malformed-kind.json"), serde_json::to_string(&value)?)?;
            let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
            let schema = write_schema(temp.path())?;

            let error = validation_error(
                validate_scorecard_inputs(&receipts, &matrix, &schema),
                "UX-run-shaped JSON with a missing or malformed kind unexpectedly passed",
            )?;
            assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        }
        Ok(())
    }

    #[test]
    fn invalid_receipt_candidate_fails_schema_validation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("invalid.json"),
            r#"{"kind":"ux_scenario_run","schema_version":1}"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "invalid receipt unexpectedly passed schema validation",
        )?;
        assert!(format!("{error:#}").contains("invalid UX scenario receipt"));
        Ok(())
    }

    #[test]
    fn explicit_null_timing_fields_are_accepted() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let path = write_receipt(&receipts, "null-timing.json", "known", "known.rs")?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        value["time_to_first_useful_result_ms"] = Value::Null;
        value["operation_timings"] = serde_json::json!([{
            "operation": "hover",
            "time_to_first_useful_result_ms": null
        }]);
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        validate_scorecard_inputs(&receipts, &matrix, &schema)?;
        Ok(())
    }

    #[test]
    fn unknown_workflow_fails_as_matrix_drift() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        write_receipt(&receipts, "unknown.json", "unknown", "unknown.rs")?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "unknown workflow unexpectedly passed matrix validation",
        )?;
        assert!(format!("{error:#}").contains("unknown workflow `unknown`"));
        Ok(())
    }

    #[test]
    fn scenario_file_mismatch_fails_as_matrix_drift() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        write_receipt(&receipts, "mismatch.json", "known", "actual.rs")?;
        let matrix = write_matrix(temp.path(), &[("known", "expected.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "scenario identity mismatch unexpectedly passed matrix validation",
        )?;
        let message = format!("{error:#}");
        assert!(message.contains("reports scenario `actual.rs`"));
        assert!(message.contains("expected `expected.rs`"));
        Ok(())
    }

    #[test]
    fn duplicate_workflow_ids_fail_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let matrix =
            write_matrix(temp.path(), &[("duplicate", "first.rs"), ("duplicate", "second.rs")])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "duplicate workflow ids unexpectedly passed fixture validation",
        )?;
        assert!(format!("{error:#}").contains("duplicate workflow id `duplicate`"));
        Ok(())
    }

    #[test]
    fn empty_fixture_matrix_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let matrix = write_matrix(temp.path(), &[])?;
        let schema = write_schema(temp.path())?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "empty fixture matrix unexpectedly passed fixture validation",
        )?;
        assert!(format!("{error:#}").contains("has no workflows"));
        Ok(())
    }
}
