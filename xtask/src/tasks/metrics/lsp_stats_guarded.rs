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
    "time_to_first_useful_result_ms",
    "operation_timings",
    "assertions",
    "canonical_repro",
    "friendly_repro",
];

const UX_RUN_DISTINCTIVE_FIELDS: &[&str] =
    &["operation_timings", "time_to_first_useful_result_ms", "canonical_repro", "friendly_repro"];

const KNOWN_NON_UX_COMPANION_KINDS: &[&str] = &["golden_editor_workload"];

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
pub fn run_with_receipt_dir(
    json: bool,
    receipt_dir: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let root = project_root()?;
    validate_run_inputs(&root, receipt_dir)?;

    super::lsp_stats_impl::run_with_receipt_dir(json, receipt_dir, output)
}

fn validate_run_inputs(root: &Path, receipt_dir: Option<&Path>) -> Result<()> {
    let Some(receipts_dir) = receipt_dir else {
        // The no-argument command intentionally retains the legacy fixture-inventory
        // behavior. Receipt validation is opt-in through an explicit directory.
        return Ok(());
    };
    validate_scorecard_inputs(
        receipts_dir,
        &root.join(FIXTURE_MATRIX_PATH),
        &root.join(RECEIPT_SCHEMA_PATH),
    )
}

/// Aggregate receipts after validating their schema and fixture identity.
#[cfg(test)]
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
/// the shared directory. Malformed signature fields fail closed, while valid
/// shared timing/report fields remain compatible with unrelated companion receipts.
fn looks_like_ux_scenario_run(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    let signature_fields: Vec<&str> = UX_RUN_SIGNATURE_FIELDS
        .iter()
        .copied()
        .filter(|field| object.contains_key(*field))
        .collect();
    let has_distinctive = UX_RUN_DISTINCTIVE_FIELDS.iter().any(|field| object.contains_key(*field));
    let kind = object.get("kind").and_then(Value::as_str);
    if kind.is_some_and(|kind| KNOWN_NON_UX_COMPANION_KINDS.contains(&kind)) {
        return false;
    }

    let has_ux_identity = ["workflow_id", "scenario_file", "test_name", "ci_tier", "assertions"]
        .iter()
        .any(|field| object.contains_key(*field));
    let has_malformed_marker = signature_fields.iter().any(|field| malformed_marker(object, field));
    if has_malformed_marker {
        return true;
    }

    let is_unknown_non_ux_companion =
        kind.is_some_and(|kind| kind != "ux_scenario_run") && !has_ux_identity;
    let has_explicit_ux_identity = kind == Some("ux_scenario_run") || has_ux_identity;

    if !has_explicit_ux_identity {
        return false;
    }

    !is_unknown_non_ux_companion && signature_fields.len() >= 3 && has_distinctive
}

fn malformed_marker(object: &serde_json::Map<String, Value>, marker: &str) -> bool {
    let Some(value) = object.get(marker) else {
        return false;
    };
    match marker {
        "workflow_id" | "scenario_file" | "test_name" | "canonical_repro" | "friendly_repro" => {
            value.as_str().is_none_or(str::is_empty)
        }
        "ci_tier" => !matches!(value.as_str(), Some("pr" | "nightly" | "release")),
        "result" => !matches!(value.as_str(), Some("pass" | "fail" | "quarantined" | "skipped")),
        "duration_ms" => !value.as_f64().is_some_and(|number| number.is_finite() && number >= 0.0),
        "time_to_first_useful_result_ms" => {
            !value.is_null()
                && !value.as_f64().is_some_and(|number| number.is_finite() && number >= 0.0)
        }
        "assertions" => !value.is_object(),
        "operation_timings" => malformed_operation_timings(value),
        _ => false,
    }
}

fn malformed_operation_timings(value: &Value) -> bool {
    let Some(entries) = value.as_array() else {
        return true;
    };

    entries.iter().any(|entry| {
        let Some(entry) = entry.as_object() else {
            return true;
        };
        if entry.keys().any(|key| {
            !matches!(
                key.as_str(),
                "operation" | "time_to_first_useful_result_ms" | "timing_status"
            )
        }) {
            return true;
        }
        if entry
            .get("operation")
            .and_then(Value::as_str)
            .is_none_or(|operation| operation.is_empty())
        {
            return true;
        }
        if let Some(timing) = entry.get("time_to_first_useful_result_ms")
            && !timing.is_null()
            && !timing.as_f64().is_some_and(|number| number.is_finite() && number >= 0.0)
        {
            return true;
        }
        if let Some(status) = entry.get("timing_status")
            && !matches!(status.as_str(), Some("missing_request_start"))
        {
            return true;
        }
        false
    })
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

    /// Legacy receipt directory used when the CLI receives no explicit
    /// `--receipt-dir`. Only tests reference it, so it lives here to keep
    /// non-test builds free of dead-code warnings.
    const DEFAULT_RECEIPT_DIR: &str = "target/receipts/editor-ux";

    fn validation_error(result: Result<()>, context: &str) -> Result<color_eyre::Report> {
        match result {
            Ok(()) => bail!("{context}"),
            Err(error) => Ok(error),
        }
    }

    fn checked_in_receipt_schema() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".ci/schemas/ux-scenario-run.schema.json")
    }

    #[test]
    fn default_receipt_dir_is_not_validated_when_argument_is_none() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join(DEFAULT_RECEIPT_DIR);
        fs::create_dir_all(&receipts)?;
        fs::write(receipts.join("broken.json"), "{")?;

        validate_run_inputs(temp.path(), None)?;
        Ok(())
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
        let schema = checked_in_receipt_schema();

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
        let schema = checked_in_receipt_schema();

        validate_scorecard_inputs(&receipts, &matrix, &schema)?;
        Ok(())
    }

    #[test]
    fn unrelated_json_with_one_receipt_like_field_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("test-report.json"),
            r#"{"test_name":"unrelated_report","status":"pass"}"#,
        )?;
        fs::write(
            receipts.join("scenario-metadata.json"),
            r#"{"scenario_file":"unrelated.json","status":"complete"}"#,
        )?;
        fs::write(
            receipts.join("timing-report.json"),
            r#"{"result":"pass","duration_ms":10.0,"status":"complete"}"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
        Ok(())
    }

    #[test]
    fn unrelated_workflow_metadata_with_multiple_identity_fields_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("workflow-report.json"),
            r#"{
                "workflow_id":"external_workflow",
                "scenario_file":"external_scenario.json",
                "result":"pass",
                "duration_ms":10.0,
                "status":"complete"
            }"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
        Ok(())
    }

    #[test]
    fn unrelated_report_with_two_ux_markers_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("external-report.json"),
            r#"{
                "result":"pass",
                "canonical_repro":"external-tool --check",
                "status":"complete"
            }"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
        Ok(())
    }

    #[test]
    fn unrelated_report_with_three_markers_and_no_ux_identity_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("external-report.json"),
            r#"{
                "result":"pass",
                "duration_ms":10,
                "operation_timings":[],
                "canonical_repro":"external-tool --check"
            }"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
        Ok(())
    }

    #[test]
    fn one_malformed_ux_marker_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("truncated.json"),
            r#"{"duration_ms":"not-a-number","assertions":{}}"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema()),
            "one-marker malformed UX receipt unexpectedly passed",
        )?;
        assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        Ok(())
    }

    #[test]
    fn malformed_duration_with_valid_result_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("malformed-duration.json"),
            r#"{"result":"pass","duration_ms":"not-a-number"}"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema()),
            "receipt-shaped JSON with a valid result and invalid duration unexpectedly passed",
        )?;
        assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        Ok(())
    }

    #[test]
    fn no_receipt_dir_preserves_legacy_validation_boundary() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let default_receipts = temp.path().join(DEFAULT_RECEIPT_DIR);
        fs::create_dir_all(&default_receipts)?;
        fs::write(default_receipts.join("broken.json"), "{")?;

        let schema = temp.path().join(RECEIPT_SCHEMA_PATH);
        if let Some(parent) = schema.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(checked_in_receipt_schema(), &schema)?;

        let matrix = temp.path().join(FIXTURE_MATRIX_PATH);
        if let Some(parent) = matrix.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&matrix, r#"{"workflows":[{"id":"known","scenario_file":"known.rs"}]}"#)?;

        validate_run_inputs(temp.path(), None)?;
        let error = validation_error(
            validate_run_inputs(temp.path(), Some(&default_receipts)),
            "explicit receipt directory unexpectedly bypassed validation",
        )?;
        assert!(format!("{error:#}").contains("broken.json"));
        Ok(())
    }

    #[test]
    fn null_duration_with_valid_result_fails_closed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let path = write_receipt(
            &receipts,
            "null-duration.json",
            "simple_file_smoke",
            "ux_scenario_01_simple_file.rs",
        )?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        value["duration_ms"] = Value::Null;
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        let fixture_matrix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json");

        let error = validation_error(
            aggregate_from_receipts(&receipts, &fixture_matrix, None).map(|_| ()),
            "passing receipt with null duration unexpectedly passed the guarded scorecard path",
        )?;
        assert!(format!("{error:#}").contains("invalid UX scenario receipt"));
        Ok(())
    }

    #[test]
    fn generic_companion_markers_remain_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        fs::write(
            receipts.join("companion.json"),
            r#"{"kind":"golden_editor_workload","result":"pass","duration_ms":10.0}"#,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
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
        let schema = checked_in_receipt_schema();

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
        let schema = checked_in_receipt_schema();

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "UX-run-shaped JSON with the wrong kind unexpectedly passed",
        )?;
        assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        Ok(())
    }

    #[test]
    fn malformed_kind_with_no_identity_fields_still_fails_closed() -> Result<()> {
        for kind in [None, Some(Value::Null)] {
            let temp = tempfile::tempdir()?;
            let receipts = temp.path().join("receipts");
            fs::create_dir_all(&receipts)?;
            let mut value = serde_json::json!({
                "result": "pass",
                "duration_ms": "not-a-number",
                "assertions": {},
                "canonical_repro": "cargo test -p perl-lsp-ux-tests scorecard_guard_test"
            });
            if let Some(kind) = kind {
                value["kind"] = kind;
            }
            fs::write(receipts.join("malformed-kind.json"), serde_json::to_string(&value)?)?;
            let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
            let schema = checked_in_receipt_schema();

            let error = validation_error(
                validate_scorecard_inputs(&receipts, &matrix, &schema),
                "UX-run-shaped JSON with no identity fields and a missing or malformed kind unexpectedly passed",
            )?;
            assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        }
        Ok(())
    }

    #[test]
    fn malformed_timing_without_identity_or_kind_still_fails_closed() -> Result<()> {
        for kind in [None, Some(Value::Null)] {
            let temp = tempfile::tempdir()?;
            let receipts = temp.path().join("receipts");
            fs::create_dir_all(&receipts)?;
            let mut value = serde_json::json!({
                "result": "pass",
                "duration_ms": 10.0,
                "time_to_first_useful_result_ms": "not-a-number"
            });
            if let Some(kind) = kind {
                value["kind"] = kind;
            }
            fs::write(receipts.join("malformed-timing.json"), serde_json::to_string(&value)?)?;
            let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

            let error = validation_error(
                validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema()),
                "identity-free malformed timing unexpectedly passed the scorecard boundary",
            )?;
            assert!(format!("{error:#}").contains("unsupported or malformed kind"));
        }
        Ok(())
    }

    #[test]
    fn malformed_nested_timing_without_identity_or_kind_still_fails_closed() -> Result<()> {
        let malformed_entries = [
            serde_json::json!({
                "operation": "hover",
                "time_to_first_useful_result_ms": "not-a-number"
            }),
            serde_json::json!({ "operation": "hover", "timing_status": "unexpected" }),
            serde_json::json!({ "operation": "" }),
            serde_json::json!({ "operation": "hover", "unexpected": true }),
            serde_json::json!("not-an-operation-object"),
        ];

        for (index, entry) in malformed_entries.into_iter().enumerate() {
            for kind in [None, Some(Value::Null), Some(Value::String("other_receipt".to_owned()))] {
                let temp = tempfile::tempdir()?;
                let receipts = temp.path().join("receipts");
                fs::create_dir_all(&receipts)?;
                let mut value = serde_json::json!({
                    "result": "pass",
                    "operation_timings": [entry]
                });
                if let Some(kind) = kind {
                    value["kind"] = kind;
                }
                fs::write(
                    receipts.join(format!("malformed-nested-timing-{index}.json")),
                    serde_json::to_string(&value)?,
                )?;
                let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

                let error = validation_error(
                    validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema()),
                    "identity-free malformed nested timing unexpectedly passed the scorecard boundary",
                )?;
                assert!(format!("{error:#}").contains("unsupported or malformed kind"));
            }
        }
        Ok(())
    }

    #[test]
    fn identity_free_and_wrong_kind_null_duration_fails_but_optional_timing_null_passes()
    -> Result<()> {
        for kind in [None, Some("other_receipt")] {
            let temp = tempfile::tempdir()?;
            let receipts = temp.path().join("receipts");
            fs::create_dir_all(&receipts)?;
            let mut value = serde_json::json!({ "result": "pass", "duration_ms": null });
            if let Some(kind) = kind {
                value["kind"] = Value::String(kind.to_owned());
            }
            fs::write(receipts.join("null-duration.json"), serde_json::to_string(&value)?)?;
            let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
            let error = validation_error(
                validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema()),
                "identity-free null duration unexpectedly passed the scorecard boundary",
            )?;
            assert!(format!("{error:#}").contains("unsupported or malformed kind"));
            fs::remove_file(receipts.join("null-duration.json"))?;

            for (name, optional_timing) in [
                (
                    "top-level-null-timing.json",
                    serde_json::json!({
                        "result": "pass",
                        "time_to_first_useful_result_ms": null
                    }),
                ),
                (
                    "nested-null-timing.json",
                    serde_json::json!({
                        "result": "pass",
                        "operation_timings": [{
                            "operation": "hover",
                            "time_to_first_useful_result_ms": null
                        }]
                    }),
                ),
            ] {
                fs::write(receipts.join(name), serde_json::to_string(&optional_timing)?)?;
                validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
                fs::remove_file(receipts.join(name))?;
            }
        }
        Ok(())
    }

    #[test]
    fn timing_bearing_non_ux_companion_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let companion = serde_json::json!({
            "kind": "golden_editor_workload",
            "result": "pass",
            "duration_ms": 10.0,
            "operation_timings": [{
                "operation": "hover",
                "time_to_first_useful_result_ms": null
            }]
        });
        fs::write(
            receipts.join("golden-editor-workload-timing.json"),
            serde_json::to_string_pretty(&companion)?,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
        Ok(())
    }

    #[test]
    fn unknown_timing_bearing_non_ux_companion_remains_ignored() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let companion = serde_json::json!({
            "kind": "other_companion",
            "result": "pass",
            "duration_ms": 10.0,
            "operation_timings": []
        });
        fs::write(
            receipts.join("other-companion-timing.json"),
            serde_json::to_string_pretty(&companion)?,
        )?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;

        validate_scorecard_inputs(&receipts, &matrix, &checked_in_receipt_schema())?;
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
        let schema = checked_in_receipt_schema();

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
        }, {
            "operation": "completion",
            "time_to_first_useful_result_ms": 5.0,
            "timing_status": "missing_request_start"
        }]);
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = checked_in_receipt_schema();

        validate_scorecard_inputs(&receipts, &matrix, &schema)?;
        Ok(())
    }

    #[test]
    fn explicit_null_failure_class_receipt_passes_guarded_scorecard_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let path = write_receipt(
            &receipts,
            "null-failure-class.json",
            "simple_file_smoke",
            "ux_scenario_01_simple_file.rs",
        )?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        value["failure_class"] = Value::Null;
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;

        let fixture_matrix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json");
        let scorecard = aggregate_from_receipts(&receipts, &fixture_matrix, None)?;
        let workflow = scorecard
            .workflows
            .iter()
            .find(|workflow| workflow.id == "simple_file_smoke")
            .ok_or_else(|| {
                color_eyre::eyre::eyre!("guarded scorecard omitted the receipt workflow")
            })?;

        assert_eq!(workflow.pass_rate.state, "measured");
        assert_eq!(workflow.pass_rate.value, Some(1.0));
        Ok(())
    }

    #[test]
    fn null_failure_class_is_rejected_for_non_passing_results() -> Result<()> {
        let fixture_matrix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json");

        for result in ["fail", "quarantined", "skipped"] {
            let temp = tempfile::tempdir()?;
            let receipts = temp.path().join("receipts");
            fs::create_dir_all(&receipts)?;
            let path = write_receipt(
                &receipts,
                &format!("null-failure-class-{result}.json"),
                "simple_file_smoke",
                "ux_scenario_01_simple_file.rs",
            )?;
            let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
            value["result"] = Value::String(result.to_owned());
            value["failure_class"] = Value::Null;
            if result == "skipped" {
                value["skip_reason"] = Value::String("test skip".to_owned());
            }
            fs::write(&path, serde_json::to_string_pretty(&value)?)?;

            let error = validation_error(
                aggregate_from_receipts(&receipts, &fixture_matrix, None).map(|_| ()),
                &format!("{result} receipt with null failure_class unexpectedly passed"),
            )?;
            assert!(format!("{error:#}").contains("invalid UX scenario receipt"));
        }

        Ok(())
    }

    #[test]
    fn non_null_failure_class_is_rejected_for_passing_results() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        let path = write_receipt(
            &receipts,
            "pass-with-failure-class.json",
            "simple_file_smoke",
            "ux_scenario_01_simple_file.rs",
        )?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        value["failure_class"] = Value::String("server_crash".to_owned());
        fs::write(&path, serde_json::to_string_pretty(&value)?)?;

        let fixture_matrix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates/perl-lsp-ux-tests/fixtures/editor_ux_fixture_matrix.json");
        let error = validation_error(
            aggregate_from_receipts(&receipts, &fixture_matrix, None).map(|_| ()),
            "passing receipt with a non-null failure_class unexpectedly passed the guarded scorecard path",
        )?;
        assert!(format!("{error:#}").contains("invalid UX scenario receipt"));
        Ok(())
    }

    #[test]
    fn unknown_workflow_fails_as_matrix_drift() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipts = temp.path().join("receipts");
        fs::create_dir_all(&receipts)?;
        write_receipt(&receipts, "unknown.json", "unknown", "unknown.rs")?;
        let matrix = write_matrix(temp.path(), &[("known", "known.rs")])?;
        let schema = checked_in_receipt_schema();

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
        let schema = checked_in_receipt_schema();

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
        let schema = checked_in_receipt_schema();

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
        let schema = checked_in_receipt_schema();

        let error = validation_error(
            validate_scorecard_inputs(&receipts, &matrix, &schema),
            "empty fixture matrix unexpectedly passed fixture validation",
        )?;
        assert!(format!("{error:#}").contains("has no workflows"));
        Ok(())
    }
}
