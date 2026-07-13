//! Scenario 67 - checked-in golden daily-driver workload baseline.
//!
//! The manifest is deliberately broader than the current exact-support proof.
//! This runner records the observed result class and provider receipt for each
//! journey without promoting a support tier or turning a fallback into an
//! exactness claim.

use anyhow::{Context, Result, anyhow, ensure};
use perl_lsp_ux_tests::{
    ProjectFixtureFile, ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available,
    create_fixture_harness, fixture_content, load_catalyst_fixture_files,
    load_dancer2_fixture_files, load_mojolicious_fixture_files, missing_binary_skip,
    open_all_fixture_files, run_ux_scenario,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCENARIO_FILE: &str = "ux_scenario_67_golden_editor_workload.rs";
const MANIFEST: &str = include_str!("../fixtures/golden_editor_workload.json");
const PLAIN_ACTIVE_FILE: &str = "lib/Plain/App.pm";
const PLAIN_SOURCE: &str = r#"package Plain::App;
use strict;
use warnings;

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub run {
    my ($self) = @_;
    my $value = $self->value;
    return $value;
}

sub value {
    return 1;
}

1;
"#;

#[derive(Debug, Deserialize)]
struct WorkloadManifest {
    kind: String,
    schema_version: u32,
    manifest_version: String,
    claim_boundary: String,
    projects: Vec<ProjectSpec>,
    journeys: Vec<JourneySpec>,
    zero_budget_metrics: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectSpec {
    name: String,
    fixture: String,
    active_file: String,
    completion_needle: String,
    definition_needle: String,
    reference_needle: String,
    safe_rename_needle: String,
}

#[derive(Debug, Deserialize, Clone)]
struct JourneySpec {
    id: String,
    provider: String,
    request_shape: String,
    expected_result_class: String,
}

#[derive(Debug, Serialize)]
struct WorkloadReceipt {
    kind: &'static str,
    schema_version: u32,
    measured_at_unix_ms: u128,
    manifest_version: String,
    claim_boundary: String,
    run_identity: RunIdentity,
    projects: Vec<ProjectReceipt>,
    rows: Vec<WorkloadRow>,
    rollup: Rollup,
}

#[derive(Debug, Serialize)]
struct RunIdentity {
    commit: Option<String>,
    run_id: Option<String>,
    ci: bool,
}

#[derive(Debug, Serialize)]
struct ProjectReceipt {
    project: String,
    fixture: String,
    source_snapshot: &'static str,
    file_count: usize,
    active_file: String,
    index_ready: bool,
}

#[derive(Debug, Serialize)]
struct WorkloadRow {
    project: String,
    source_snapshot: &'static str,
    file: String,
    cursor: CursorReceipt,
    request_shape: String,
    expected_result_class: String,
    actual_result_class: String,
    actual_locations: Value,
    actual_edits: Value,
    missing: Vec<String>,
    extra: Vec<String>,
    comparison: &'static str,
    answering_tier: String,
    fact_producer: String,
    proof_class: String,
    confidence: String,
    freshness: String,
    fallback_or_blocker: String,
    readiness_state: String,
    latency_ms: f64,
    unsafe_edit: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct CursorReceipt {
    line: u32,
    character: u32,
}

#[derive(Debug, Default, Serialize)]
struct Rollup {
    exact_success_count: usize,
    false_exact_count: usize,
    unsafe_edit_count: usize,
    stale_exact_count: usize,
    bounded_fallback_count: usize,
    unexplained_empty_count: usize,
    error_count: usize,
    measured_debt_count: usize,
    time_to_first_useful_answer_ms: Option<f64>,
    p50_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
}

#[test]
fn scenario_67_golden_editor_workload_receipt() {
    run_ux_scenario(
        "golden_daily_driver_workload",
        SCENARIO_FILE,
        "scenario_67_golden_editor_workload_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let manifest: WorkloadManifest =
                serde_json::from_str(MANIFEST).context("golden workload manifest must parse")?;
            validate_manifest(&manifest)?;

            let mut rows = Vec::new();
            let mut project_receipts = Vec::new();
            for project in &manifest.projects {
                let files = project_files(project)?;
                let harness = create_harness(project, &files)?;
                open_all_fixture_files(&harness, &files)?;
                let source = if project.fixture == "inline_plain_modern_oo" {
                    PLAIN_SOURCE.to_owned()
                } else {
                    fixture_content(&files, &project.active_file)?.to_owned()
                };
                let index_ready = harness.wait_for_index_ready(Duration::from_secs(10));
                project_receipts.push(ProjectReceipt {
                    project: project.name.clone(),
                    fixture: project.fixture.clone(),
                    source_snapshot: "checked_in_fixture",
                    file_count: files.len(),
                    active_file: project.active_file.clone(),
                    index_ready,
                });

                run_project_workload(
                    recorder,
                    &manifest.journeys,
                    project,
                    &source,
                    &harness,
                    index_ready,
                    &mut rows,
                )?;
                harness.assert_no_crash();
            }

            let rollup = build_rollup(&rows)?;
            let receipt = WorkloadReceipt {
                kind: "golden_editor_workload",
                schema_version: 1,
                measured_at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                manifest_version: manifest.manifest_version,
                claim_boundary: manifest.claim_boundary,
                run_identity: RunIdentity {
                    commit: std::env::var("GITHUB_SHA").ok(),
                    run_id: std::env::var("GITHUB_RUN_ID").ok(),
                    ci: std::env::var("CI").is_ok(),
                },
                projects: project_receipts,
                rows,
                rollup,
            };
            write_workload_receipt(&receipt)?;

            recorder.check("golden workload produced rows", !receipt.rows.is_empty())?;
            recorder
                .check("false exact count remains zero", receipt.rollup.false_exact_count == 0)?;
            recorder
                .check("unsafe edit count remains zero", receipt.rollup.unsafe_edit_count == 0)?;
            recorder
                .check("stale exact count remains zero", receipt.rollup.stale_exact_count == 0)?;
            recorder.check(
                "unexplained empty count remains zero",
                receipt.rollup.unexplained_empty_count == 0,
            )?;
            Ok(())
        },
    );
}

fn validate_manifest(manifest: &WorkloadManifest) -> Result<()> {
    ensure!(manifest.kind == "golden_editor_workload", "unexpected manifest kind");
    ensure!(manifest.schema_version == 1, "unsupported manifest schema");
    ensure!(manifest.projects.len() == 4, "workload must cover four projects");
    ensure!(manifest.journeys.len() >= 8, "workload must cover the daily-driver journeys");
    for metric in
        ["false_exact_count", "unsafe_edit_count", "stale_exact_count", "unexplained_empty_count"]
    {
        ensure!(
            manifest.zero_budget_metrics.iter().any(|candidate| candidate == metric),
            "manifest is missing zero-budget metric {metric}"
        );
    }
    Ok(())
}

fn project_files(project: &ProjectSpec) -> Result<Vec<ProjectFixtureFile>> {
    match project.fixture.as_str() {
        "mojolicious" => load_mojolicious_fixture_files(),
        "dancer2" => load_dancer2_fixture_files(),
        "catalyst" => load_catalyst_fixture_files(),
        "inline_plain_modern_oo" => Ok(Vec::new()),
        other => Err(anyhow!("unknown workload fixture {other}")),
    }
}

fn create_harness(project: &ProjectSpec, files: &[ProjectFixtureFile]) -> Result<UxHarness> {
    if project.fixture == "inline_plain_modern_oo" {
        return UxHarness::new(
            ScenarioConfig { timeout: Duration::from_secs(30), ..Default::default() }
                .env("PERL_LSP_WORKSPACE", "1")
                .with_file(PLAIN_ACTIVE_FILE, PLAIN_SOURCE),
        );
    }
    create_fixture_harness(files)
}

fn run_project_workload(
    recorder: &mut perl_lsp_ux_tests::UxRunRecorder,
    journeys: &[JourneySpec],
    project: &ProjectSpec,
    source: &str,
    harness: &UxHarness,
    index_ready: bool,
    rows: &mut Vec<WorkloadRow>,
) -> Result<()> {
    let completion_cursor = position_after(source, &project.completion_needle)?;
    let definition_cursor = position_at(source, &project.definition_needle)?;
    let reference_cursor = position_at(source, &project.reference_needle)?;
    let rename_cursor = position_at(source, &project.safe_rename_needle)?;

    for journey in journeys {
        let operation = operation_name(project, journey);
        recorder.mark_request_start(&operation);
        let request_started = Instant::now();
        let (cursor, response, actual_edits) = match journey.id.as_str() {
            "open_before_index" => {
                let start = Instant::now();
                let value = capture(harness.completion(
                    &project.active_file,
                    completion_cursor.line,
                    completion_cursor.character,
                ));
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let mut row = response_row(
                    project,
                    journey,
                    completion_cursor,
                    value,
                    Value::Null,
                    "active_document_open",
                    elapsed,
                    harness,
                )?;
                row.fallback_or_blocker = "baseline_pre_index_request".to_owned();
                rows.push(row);
                recorder.mark_first_useful_result(&operation);
                continue;
            }
            "completion_after_ready" => (
                completion_cursor,
                capture(harness.completion(
                    &project.active_file,
                    completion_cursor.line,
                    completion_cursor.character,
                )),
                Value::Null,
            ),
            "hover_after_ready" => (
                reference_cursor,
                capture(
                    harness
                        .hover(
                            &project.active_file,
                            reference_cursor.line,
                            reference_cursor.character,
                        )
                        .map(|value| value.unwrap_or(Value::Null)),
                ),
                Value::Null,
            ),
            "definition_local_or_imported" => (
                definition_cursor,
                capture(harness.definition(
                    &project.active_file,
                    definition_cursor.line,
                    definition_cursor.character,
                )),
                Value::Null,
            ),
            "references_lexical" => (
                reference_cursor,
                capture(harness.references(
                    &project.active_file,
                    reference_cursor.line,
                    reference_cursor.character,
                    true,
                )),
                Value::Null,
            ),
            "rename_safe_lexical" => {
                let result = capture(request_rename(harness, &project.active_file, rename_cursor));
                let edits = result.clone();
                (rename_cursor, result, edits)
            }
            "diagnostics_present_import" => {
                let diagnostics =
                    harness.wait_for_diagnostics(&project.active_file, Duration::from_secs(5));
                (CursorReceipt { line: 0, character: 0 }, json!(diagnostics), Value::Null)
            }
            "workspace_symbols_after_ready" => (
                CursorReceipt { line: 0, character: 0 },
                capture(harness.workspace_symbols("new")),
                Value::Null,
            ),
            "edit_burst_completion_hover" => {
                for edit in 0..20 {
                    harness.change_file_full(
                        &project.active_file,
                        &format!("{source}\n# golden editor burst {edit}"),
                    )?;
                }
                let _ = harness.wait_for_index_ready(Duration::from_secs(10));
                let completion = capture(harness.completion(
                    &project.active_file,
                    completion_cursor.line,
                    completion_cursor.character,
                ));
                let hover = capture(
                    harness
                        .hover(
                            &project.active_file,
                            reference_cursor.line,
                            reference_cursor.character,
                        )
                        .map(|value| value.unwrap_or(Value::Null)),
                );
                harness.change_file_full(&project.active_file, source)?;
                (completion_cursor, json!({"completion": completion, "hover": hover}), Value::Null)
            }
            "close_reopen_pending" => {
                let uri = harness.workspace.uri(&project.active_file);
                harness
                    .client
                    .notify("textDocument/didClose", json!({"textDocument": {"uri": uri}}))?;
                harness.open_file(&project.active_file, source)?;
                (CursorReceipt { line: 0, character: 0 }, json!({"reopened": true}), Value::Null)
            }
            other => return Err(anyhow!("unhandled golden workload journey {other}")),
        };
        let request_latency_ms = request_started.elapsed().as_secs_f64() * 1000.0;

        let provider_receipt = explain_provider(harness, &journey.provider);
        let row = response_row(
            project,
            journey,
            cursor,
            response,
            actual_edits,
            if index_ready { "workspace_index_ready" } else { "workspace_index_pending" },
            request_latency_ms,
            harness,
        )?;
        let row = apply_provider_receipt(row, provider_receipt);
        if row.actual_result_class != "empty" && row.actual_result_class != "error" {
            recorder.mark_first_useful_result(&operation);
        }
        rows.push(row);
    }
    Ok(())
}

fn request_rename(harness: &UxHarness, file: &str, cursor: CursorReceipt) -> Result<Value> {
    let response = harness.client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": harness.workspace.uri(file) },
            "position": { "line": cursor.line, "character": cursor.character },
            "newName": "golden_renamed_value"
        }),
        Duration::from_secs(5),
    )?;
    ensure!(response.get("error").is_none(), "rename returned a protocol error: {response}");
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn response_row(
    project: &ProjectSpec,
    journey: &JourneySpec,
    cursor: CursorReceipt,
    response: Value,
    actual_edits: Value,
    readiness_state: &str,
    latency_ms: f64,
    harness: &UxHarness,
) -> Result<WorkloadRow> {
    let actual_result_class = classify_result(&response, journey.provider.as_str());
    let is_empty = actual_result_class == "empty";
    let fallback_or_blocker =
        response.get("_golden_error").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(
            || {
                if is_empty {
                    "empty_result_requires_class_proof".to_owned()
                } else {
                    "not_observed".to_owned()
                }
            },
        );
    let actual_locations = if matches!(journey.provider.as_str(), "definition" | "references") {
        harness.normalize_response(&response)
    } else {
        Value::Array(Vec::new())
    };
    let normalized_edits = harness.normalize_response(&actual_edits);
    let unsafe_edit = journey.provider == "rename"
        && rename_edit_targets_other_file(&normalized_edits, &project.active_file);
    Ok(WorkloadRow {
        project: project.name.clone(),
        source_snapshot: "checked_in_fixture",
        file: project.active_file.clone(),
        cursor,
        request_shape: journey.request_shape.clone(),
        expected_result_class: journey.expected_result_class.clone(),
        actual_result_class,
        actual_locations,
        actual_edits: normalized_edits,
        missing: Vec::new(),
        extra: Vec::new(),
        comparison: "not_scored_until_class_promotion",
        answering_tier: "not_observed".to_owned(),
        fact_producer: "not_observed".to_owned(),
        proof_class: "baseline_only".to_owned(),
        confidence: "not_observed".to_owned(),
        freshness: "not_observed".to_owned(),
        fallback_or_blocker,
        readiness_state: readiness_state.to_owned(),
        latency_ms,
        unsafe_edit,
    })
}

fn apply_provider_receipt(mut row: WorkloadRow, receipt: Result<Value>) -> WorkloadRow {
    let Ok(receipt) = receipt else {
        return row;
    };
    row.answering_tier = string_field(&receipt, "answering_tier");
    row.fact_producer = string_field(&receipt, "fact_source");
    row.confidence = string_field(&receipt, "confidence");
    row.freshness = string_field(&receipt, "freshness");
    row.proof_class = string_field(&receipt, "proof_class");
    if row.actual_result_class != "error" {
        row.fallback_or_blocker = receipt
            .get("fallback")
            .or_else(|| receipt.get("fallback_state"))
            .and_then(Value::as_str)
            .unwrap_or(&row.fallback_or_blocker)
            .to_owned();
    }
    row
}

fn explain_provider(harness: &UxHarness, provider: &str) -> Result<Value> {
    let response = harness.client.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{ "provider": provider }]
        }),
        Duration::from_secs(20),
    )?;
    ensure!(response.get("error").is_none(), "provider explanation returned an error");
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn classify_result(response: &Value, provider: &str) -> String {
    if response.is_null() {
        return if provider == "rename" { "refused" } else { "empty" }.to_owned();
    }
    if response.get("error").is_some() || response.get("_golden_error").is_some() {
        return "error".to_owned();
    }
    if response.as_array().is_some_and(Vec::is_empty) {
        return "empty".to_owned();
    }
    if provider == "rename" && response.as_object().is_some_and(|object| object.is_empty()) {
        return "refused".to_owned();
    }
    "partial".to_owned()
}

fn capture<T: serde::Serialize>(result: Result<T>) -> Value {
    match result {
        Ok(value) => json!(value),
        Err(error) => json!({ "_golden_error": error.to_string() }),
    }
}

fn string_field(value: &Value, field: &str) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or("not_observed").to_owned()
}

fn operation_name(project: &ProjectSpec, journey: &JourneySpec) -> String {
    format!("golden_{}_{}", project.name, journey.id)
}

fn build_rollup(rows: &[WorkloadRow]) -> Result<Rollup> {
    let mut rollup = Rollup::default();
    let mut latencies = Vec::with_capacity(rows.len());
    for row in rows {
        latencies.push(row.latency_ms);
        match row.actual_result_class.as_str() {
            "error" => rollup.error_count += 1,
            "empty" => {
                rollup.unexplained_empty_count +=
                    usize::from(row.fallback_or_blocker == "not_observed");
                rollup.measured_debt_count += 1;
            }
            "partial" => rollup.bounded_fallback_count += 1,
            "exact" => rollup.exact_success_count += 1,
            "refused" => rollup.measured_debt_count += 1,
            _ => {}
        }
        rollup.unsafe_edit_count += usize::from(row.unsafe_edit);
    }
    latencies.sort_by(f64::total_cmp);
    rollup.time_to_first_useful_answer_ms = rows
        .iter()
        .find(|row| {
            row.request_shape != "close_reopen_while_pending"
                && !matches!(row.actual_result_class.as_str(), "empty" | "error" | "refused")
        })
        .map(|row| row.latency_ms);
    rollup.p50_latency_ms = percentile(&latencies, 0.50);
    rollup.p95_latency_ms = percentile(&latencies, 0.95);
    Ok(rollup)
}

fn rename_edit_targets_other_file(edit: &Value, active_file: &str) -> bool {
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        return changes.keys().any(|uri| !uri.ends_with(active_file));
    }
    edit.get("documentChanges").and_then(Value::as_array).is_some_and(|changes| {
        changes.iter().any(|change| {
            change
                .get("textDocument")
                .and_then(|document| document.get("uri"))
                .and_then(Value::as_str)
                .is_some_and(|uri| !uri.ends_with(active_file))
        })
    })
}

fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values.get(index).copied()
}

fn write_workload_receipt(receipt: &WorkloadReceipt) -> Result<PathBuf> {
    let directory = std::env::var_os("PERL_LSP_UX_RECEIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/receipts/editor-ux"));
    fs::create_dir_all(&directory)
        .with_context(|| format!("creating receipt directory {}", directory.display()))?;
    let path = directory.join("golden-editor-workload-v1.json");
    let content = serde_json::to_string_pretty(receipt).context("serializing workload receipt")?;
    fs::write(&path, format!("{content}\n"))
        .with_context(|| format!("writing workload receipt {}", path.display()))?;
    Ok(path)
}

fn position_at(source: &str, needle: &str) -> Result<CursorReceipt> {
    let offset = source.find(needle).with_context(|| format!("missing `{needle}`"))?;
    position_from_offset(source, offset)
}

fn position_after(source: &str, needle: &str) -> Result<CursorReceipt> {
    let offset = source
        .find(needle)
        .with_context(|| format!("missing `{needle}`"))?
        .checked_add(needle.len())
        .context("cursor offset overflow")?;
    position_from_offset(source, offset)
}

fn position_from_offset(source: &str, offset: usize) -> Result<CursorReceipt> {
    let prefix = source.get(..offset).context("cursor is not a UTF-8 boundary")?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let character = prefix.rsplit('\n').next().map(str::chars).map(Iterator::count).unwrap_or(0);
    Ok(CursorReceipt { line: u32::try_from(line)?, character: u32::try_from(character)? })
}
