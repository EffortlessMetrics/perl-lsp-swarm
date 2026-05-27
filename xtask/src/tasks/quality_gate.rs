//! Aggregated quality gate receipt for coverage and RIPR proof.
//!
//! This is the stable local/CI front door. Initial CI rollout can run it in
//! advisory mode while later PRs flip specific modes to blocking.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::quality_baseline::{command_arg, display_path, git_head, required_coverage_roots};

const QUALITY_GATE_SCHEMA_VERSION: u64 = 1;
const COVERAGE_TARGET: f64 = 95.0;
const CODECOV_CONFIG_PATH: &str = "codecov.yml";
const LOCAL_COMMAND_PREFIX: &str = "rtk";
const RIPR_SEAM_SUGGESTED_TEST: &str = "Add focused tests that reveal the predicate, return value, error variant, field construction, or observer behavior for this seam cluster.";
const NEW_RIPR_GAP_SUGGESTED_TEST: &str = "Add or update the focused test named by RIPR review guidance for the changed file, line, and seam.";
const COVERAGE_GAP_SUGGESTED_TEST: &str = "Prefer focused tests for error paths, boundary conditions, config parsing, serialization, cancellation, provider decisions, or output contracts named by the uncovered files.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityGateMode {
    Advisory,
    EnforceNewRipr,
    EnforcePatchCoverage,
    Enforce,
}

impl QualityGateMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::EnforceNewRipr => "enforce-new-ripr",
            Self::EnforcePatchCoverage => "enforce-patch-coverage",
            Self::Enforce => "enforce",
        }
    }

    fn is_enforcing(self) -> bool {
        !matches!(self, Self::Advisory)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchStatusSource {
    Codecov,
}

impl PatchStatusSource {
    fn receipt_source(self) -> &'static str {
        match self {
            Self::Codecov => "codecov_status",
        }
    }
}

pub struct QualityGateConfig<'a> {
    pub mode: QualityGateMode,
    pub ripr_receipt: &'a Path,
    pub ripr_pr_receipt: &'a Path,
    pub review_receipt: &'a Path,
    pub coverage_receipt: &'a Path,
    pub codecov: &'a Path,
    pub patch_coverage: Option<f64>,
    pub patch_status_source: Option<PatchStatusSource>,
    pub exceptions: &'a Path,
    pub receipt: &'a Path,
    pub summary: Option<&'a Path>,
    pub check: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct QualityGateReceipt {
    schema_version: u64,
    kind: String,
    mode: String,
    receipt: String,
    summary: Option<String>,
    head: Option<String>,
    ripr_plus: RiprGateState,
    ripr_pr: RiprPrGateState,
    review_guidance: ReviewGuidanceState,
    coverage: CoverageGateState,
    exceptions: QualityGateExceptionState,
    decision: String,
    next_actions: Vec<Value>,
    claim_boundary: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct RiprGateState {
    status: String,
    receipt: String,
    head: Option<String>,
    expected_head: Option<String>,
    unresolved: Option<u64>,
    new_unresolved: Option<u64>,
    top_files: Vec<Value>,
    top_actionable_files: Vec<Value>,
    deferred_files: Vec<Value>,
    receipt_next_actions: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct RiprPrGateState {
    status: String,
    receipt: String,
    expected_base_sha: Option<String>,
    expected_head_sha: Option<String>,
    new_unresolved: Option<u64>,
    changed_files: Option<u64>,
    weakly_exposed: Option<u64>,
    reachable_unrevealed: Option<u64>,
    no_static_path: Option<u64>,
    base: Option<String>,
    base_sha: Option<String>,
    head: Option<String>,
    head_sha: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ReviewGuidanceState {
    status: String,
    receipt: String,
    base: Option<String>,
    base_sha: Option<String>,
    expected_base_sha: Option<String>,
    head_sha: Option<String>,
    expected_head_sha: Option<String>,
    comments: Option<u64>,
    summary_only: Option<u64>,
    suppressed: Option<u64>,
    top_gaps: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CoverageGateState {
    status: String,
    receipt: String,
    head: Option<String>,
    expected_head: Option<String>,
    codecov_config: String,
    codecov_config_status: String,
    project: Option<f64>,
    patch: Option<f64>,
    patch_source: Option<String>,
    files_below_target: Vec<Value>,
    patch_policy: CodecovStatusPolicy,
    project_policy: CodecovStatusPolicy,
    codecov_comment: CodecovCommentPolicy,
    coverage_scope: Value,
    target: f64,
    lcov: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CodecovStatusPolicy {
    target: Option<String>,
    threshold: Option<String>,
    informational: Option<bool>,
    if_ci_failed: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CodecovCommentPolicy {
    layout: Vec<String>,
    behavior: Option<String>,
    require_head: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct QualityGateExceptionState {
    status: String,
    path: String,
    active: Vec<QualityGateException>,
    warnings: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct QualityGateException {
    id: String,
    applies_to: String,
    owner: String,
    reason: String,
    final_target: String,
    current_evidence: Vec<String>,
    removal_criteria: String,
    review_after: String,
    expires: String,
}

#[derive(Debug, Deserialize)]
struct QualityGateExceptionFile {
    schema_version: u64,
    policy: String,
    status: String,
    updated: String,
    #[serde(default, rename = "exception")]
    exceptions: Vec<QualityGateException>,
}

#[derive(Clone, Debug)]
struct QualityGateCommandState {
    ripr_receipt: String,
    ripr_pr_receipt: String,
    review_receipt: String,
    coverage_receipt: String,
    codecov: String,
    exceptions: String,
    receipt: String,
    summary: Option<String>,
}

struct RequiredQualityException {
    id: &'static str,
    applies_to: &'static str,
    final_target: &'static str,
    evidence: &'static [&'static str],
}

const REQUIRED_QUALITY_EXCEPTIONS: &[RequiredQualityException] = &[
    RequiredQualityException {
        id: "ripr-total-burndown",
        applies_to: "ripr_total_not_zero",
        final_target: "ripr_plus.unresolved == 0",
        evidence: &[
            "target/receipts/quality/ripr-plus.json",
            "target/receipts/quality/quality-gate.json",
            "target/receipts/quality/quality-gate.md",
        ],
    },
    RequiredQualityException {
        id: "project-coverage-burndown",
        applies_to: "project_coverage_below_target",
        final_target: "coverage.project >= 95.0",
        evidence: &[
            "target/receipts/quality/coverage-baseline.json",
            "target/receipts/quality/coverage-quality-gate.json",
            "target/receipts/quality/coverage-quality-gate.md",
            "codecov.yml",
        ],
    },
];

enum ReceiptInput {
    Present(Value),
    Missing,
    Invalid(String),
}

#[derive(Clone, Copy)]
enum ExpectedFieldValue {
    String(&'static str),
    U64(u64),
}

#[derive(Clone, Copy)]
struct ExpectedField {
    pointer: &'static str,
    value: ExpectedFieldValue,
}

const COVERAGE_BASELINE_CONTRACT: &[ExpectedField] = &[
    ExpectedField { pointer: "/schema_version", value: ExpectedFieldValue::U64(1) },
    ExpectedField { pointer: "/kind", value: ExpectedFieldValue::String("coverage_baseline") },
];

const RIPR_PLUS_CONTRACT: &[ExpectedField] = &[
    ExpectedField { pointer: "/schema_version", value: ExpectedFieldValue::U64(1) },
    ExpectedField { pointer: "/kind", value: ExpectedFieldValue::String("ripr_plus_baseline") },
];

const RIPR_PR_CONTRACT: &[ExpectedField] = &[
    ExpectedField { pointer: "/schema_version", value: ExpectedFieldValue::String("0.1") },
    ExpectedField { pointer: "/tool", value: ExpectedFieldValue::String("ripr") },
    ExpectedField { pointer: "/kind", value: ExpectedFieldValue::String("pr_evidence") },
    ExpectedField { pointer: "/scope", value: ExpectedFieldValue::String("diff") },
];

const RIPR_REVIEW_CONTRACT: &[ExpectedField] = &[
    ExpectedField { pointer: "/schema_version", value: ExpectedFieldValue::String("0.1") },
    ExpectedField { pointer: "/tool", value: ExpectedFieldValue::String("ripr") },
];

pub fn run(config: QualityGateConfig<'_>) -> Result<()> {
    validate_patch_inputs(config.patch_coverage, config.patch_status_source)?;
    let packet = quality_gate_receipt(&config);
    let failed = packet.decision == "fail";
    let gate_command = quality_gate_command_state(&config);
    let refresh_command =
        quality_gate_command(packet.mode.as_str(), &gate_command, Some(&packet.coverage), false);
    let verify_command =
        quality_gate_command(packet.mode.as_str(), &gate_command, Some(&packet.coverage), true);
    write_or_check_quality_gate_receipt(
        config.receipt,
        &packet,
        config.check,
        &refresh_command,
        &verify_command,
    )?;
    let summary = render_quality_gate_markdown(&packet);
    if let Some(path) = config.summary {
        write_or_check_text(path, &summary, config.check, &refresh_command, &verify_command)?;
    }
    println!("{summary}");
    if failed {
        if let Some(summary) = config.summary {
            bail!(
                "quality gate failed; see receipt {} and summary {}",
                config.receipt.display(),
                summary.display()
            );
        }
        bail!("quality gate failed; see receipt {}", config.receipt.display());
    }
    Ok(())
}

fn quality_gate_receipt(config: &QualityGateConfig<'_>) -> QualityGateReceipt {
    let head = git_head();
    let gate_command = quality_gate_command_state(config);
    let mut ripr = ripr_state(config.ripr_receipt, head.as_deref());
    let ripr_pr = ripr_pr_state(config.ripr_pr_receipt, head.as_deref());
    ripr.new_unresolved = ripr_pr.new_unresolved;
    let review_guidance = review_guidance_state(config.review_receipt, head.as_deref());
    let mut coverage = coverage_state(config.coverage_receipt, config.codecov, head.as_deref());
    if let Some(patch_coverage) = config.patch_coverage {
        coverage.patch = Some(patch_coverage);
        coverage.patch_source = Some("cli".to_string());
    } else if let Some(source) = config.patch_status_source {
        coverage.patch_source = Some(source.receipt_source().to_string());
    }
    let exceptions = exception_state(config.exceptions);
    let blockers = blockers(
        config.mode,
        &ripr,
        &ripr_pr,
        &review_guidance,
        &coverage,
        &exceptions,
        &gate_command,
    );
    let mut next_actions = Vec::new();
    next_actions.extend(advisory_actions(
        config.mode,
        &ripr,
        &ripr_pr,
        &review_guidance,
        &coverage,
        &gate_command,
    ));
    if !config.mode.is_enforcing() {
        next_actions.extend(exception_actions(&exceptions, &coverage, &gate_command));
    }
    next_actions.extend(blockers.iter().cloned());

    let decision = if !config.mode.is_enforcing() {
        "advisory"
    } else if blockers.is_empty() {
        "pass"
    } else {
        "fail"
    };
    QualityGateReceipt {
        schema_version: QUALITY_GATE_SCHEMA_VERSION,
        kind: "quality_gate".to_string(),
        mode: config.mode.as_str().to_string(),
        receipt: gate_command.receipt,
        summary: gate_command.summary,
        head,
        ripr_plus: ripr,
        ripr_pr,
        review_guidance,
        coverage,
        exceptions,
        decision: decision.to_string(),
        next_actions,
        claim_boundary: vec![
            "quality-gate is the local/CI aggregation surface for coverage and RIPR proof.".to_string(),
            "advisory mode reports missing or stale proof without blocking.".to_string(),
            "enforce-new-ripr blocks missing, stale, malformed, or non-zero RIPR receipt proof."
                .to_string(),
            "enforce-patch-coverage blocks missing or stale local coverage receipts and non-blocking Codecov patch policy.".to_string(),
            "full enforce mode is reserved for post-burn-down ripr+ zero and 95% project/patch coverage.".to_string(),
            "temporary exceptions document burn-down debt; they do not waive full enforce blockers.".to_string(),
        ],
    }
}

fn validate_patch_inputs(
    patch_coverage: Option<f64>,
    patch_status_source: Option<PatchStatusSource>,
) -> Result<()> {
    if let Some(value) = patch_coverage
        && !(0.0..=100.0).contains(&value)
    {
        bail!("patch coverage must be between 0 and 100, got {value}");
    }
    if patch_coverage.is_some() && patch_status_source.is_some() {
        bail!("use either --patch-coverage or --patch-status-source, not both");
    }
    Ok(())
}

fn ripr_state(path: &Path, current_head: Option<&str>) -> RiprGateState {
    match read_receipt(path) {
        ReceiptInput::Present(value) => {
            let status = ripr_plus_receipt_status(&value, current_head);
            RiprGateState {
                status,
                receipt: display_path(path),
                head: value.get("head").and_then(Value::as_str).map(ToOwned::to_owned),
                expected_head: current_head.map(ToOwned::to_owned),
                unresolved: value.get("unresolved").and_then(Value::as_u64),
                new_unresolved: value.get("new_unresolved").and_then(Value::as_u64),
                top_files: array_prefix(value.get("top_files"), 5),
                top_actionable_files: array_prefix(value.get("top_actionable_files"), 5),
                deferred_files: array_prefix(value.get("deferred_files"), 5),
                receipt_next_actions: array_prefix(value.get("next_actions"), 5),
            }
        }
        ReceiptInput::Missing => RiprGateState {
            status: "missing".to_string(),
            receipt: display_path(path),
            head: None,
            expected_head: current_head.map(ToOwned::to_owned),
            unresolved: None,
            new_unresolved: None,
            top_files: Vec::new(),
            top_actionable_files: Vec::new(),
            deferred_files: Vec::new(),
            receipt_next_actions: Vec::new(),
        },
        ReceiptInput::Invalid(message) => RiprGateState {
            status: format!("invalid: {message}"),
            receipt: display_path(path),
            head: None,
            expected_head: current_head.map(ToOwned::to_owned),
            unresolved: None,
            new_unresolved: None,
            top_files: Vec::new(),
            top_actionable_files: Vec::new(),
            deferred_files: Vec::new(),
            receipt_next_actions: Vec::new(),
        },
    }
}

fn ripr_pr_state(path: &Path, current_head: Option<&str>) -> RiprPrGateState {
    match read_receipt(path) {
        ReceiptInput::Present(value) => {
            let status = diff_receipt_status(&value, current_head, RIPR_PR_CONTRACT);
            RiprPrGateState {
                status,
                receipt: display_path(path),
                expected_base_sha: current_receipt_base_sha(&value),
                expected_head_sha: current_head.map(ToOwned::to_owned),
                new_unresolved: value.pointer("/summary/severe_gaps").and_then(Value::as_u64),
                changed_files: value.pointer("/summary/changed_files").and_then(Value::as_u64),
                weakly_exposed: value.pointer("/summary/weakly_exposed").and_then(Value::as_u64),
                reachable_unrevealed: value
                    .pointer("/summary/reachable_unrevealed")
                    .and_then(Value::as_u64),
                no_static_path: value.pointer("/summary/no_static_path").and_then(Value::as_u64),
                base: value.get("base").and_then(Value::as_str).map(ToOwned::to_owned),
                base_sha: value.get("base_sha").and_then(Value::as_str).map(ToOwned::to_owned),
                head: value.get("head").and_then(Value::as_str).map(ToOwned::to_owned),
                head_sha: value.get("head_sha").and_then(Value::as_str).map(ToOwned::to_owned),
            }
        }
        ReceiptInput::Missing => RiprPrGateState {
            status: "missing".to_string(),
            receipt: display_path(path),
            expected_base_sha: None,
            expected_head_sha: current_head.map(ToOwned::to_owned),
            new_unresolved: None,
            changed_files: None,
            weakly_exposed: None,
            reachable_unrevealed: None,
            no_static_path: None,
            base: None,
            base_sha: None,
            head: None,
            head_sha: None,
        },
        ReceiptInput::Invalid(message) => RiprPrGateState {
            status: format!("invalid: {message}"),
            receipt: display_path(path),
            expected_base_sha: None,
            expected_head_sha: current_head.map(ToOwned::to_owned),
            new_unresolved: None,
            changed_files: None,
            weakly_exposed: None,
            reachable_unrevealed: None,
            no_static_path: None,
            base: None,
            base_sha: None,
            head: None,
            head_sha: None,
        },
    }
}

fn review_guidance_state(path: &Path, current_head: Option<&str>) -> ReviewGuidanceState {
    match read_receipt(path) {
        ReceiptInput::Present(value) => {
            let packet_status = value.get("status").and_then(Value::as_str).unwrap_or("present");
            let freshness = diff_receipt_status(&value, current_head, RIPR_REVIEW_CONTRACT);
            let mut status = match (packet_status, freshness.as_str()) {
                ("advisory", "present") => "present".to_string(),
                ("advisory", other) => other.to_string(),
                (other, _) => other.to_string(),
            };
            let top_gaps =
                if status == "present" { review_guidance_items(&value, 3) } else { Vec::new() };
            if status == "present" && top_gaps.is_empty() && review_guidance_declares_items(&value)
            {
                status = "incomplete".to_string();
            }
            ReviewGuidanceState {
                status,
                receipt: display_path(path),
                base: value.get("base").and_then(Value::as_str).map(ToOwned::to_owned),
                base_sha: value.get("base_sha").and_then(Value::as_str).map(ToOwned::to_owned),
                expected_base_sha: current_receipt_base_sha(&value),
                head_sha: value.get("head_sha").and_then(Value::as_str).map(ToOwned::to_owned),
                expected_head_sha: current_head.map(ToOwned::to_owned),
                comments: value.pointer("/summary/comments").and_then(Value::as_u64),
                summary_only: value.pointer("/summary/summary_only").and_then(Value::as_u64),
                suppressed: value.pointer("/summary/suppressed").and_then(Value::as_u64),
                top_gaps,
            }
        }
        ReceiptInput::Missing => ReviewGuidanceState {
            status: "missing".to_string(),
            receipt: display_path(path),
            base: None,
            base_sha: None,
            expected_base_sha: None,
            head_sha: None,
            expected_head_sha: current_head.map(ToOwned::to_owned),
            comments: None,
            summary_only: None,
            suppressed: None,
            top_gaps: Vec::new(),
        },
        ReceiptInput::Invalid(message) => ReviewGuidanceState {
            status: format!("invalid: {message}"),
            receipt: display_path(path),
            base: None,
            base_sha: None,
            expected_base_sha: None,
            head_sha: None,
            expected_head_sha: current_head.map(ToOwned::to_owned),
            comments: None,
            summary_only: None,
            suppressed: None,
            top_gaps: Vec::new(),
        },
    }
}

fn coverage_state(path: &Path, codecov: &Path, current_head: Option<&str>) -> CoverageGateState {
    let mut state = coverage_receipt_state(path, current_head);
    let codecov = resolve_codecov_config_path(codecov);
    apply_codecov_config_fallback(&mut state, &codecov);
    state
}

fn resolve_codecov_config_path(path: &Path) -> PathBuf {
    if path.is_absolute() || path.exists() || path != Path::new(CODECOV_CONFIG_PATH) {
        return path.to_path_buf();
    }

    let Ok(current_dir) = std::env::current_dir() else {
        return path.to_path_buf();
    };

    for ancestor in current_dir.ancestors() {
        let candidate = ancestor.join(path);
        if candidate.exists() {
            return candidate;
        }
    }

    path.to_path_buf()
}

fn coverage_receipt_state(path: &Path, current_head: Option<&str>) -> CoverageGateState {
    match read_receipt(path) {
        ReceiptInput::Present(value) => {
            let status = coverage_receipt_status(&value, current_head);
            let patch = value.pointer("/coverage/patch").and_then(Value::as_f64);
            let patch_source = value
                .pointer("/coverage/patch_source")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| patch.map(|_| "coverage_receipt".to_string()));
            CoverageGateState {
                status,
                receipt: display_path(path),
                head: value.get("head").and_then(Value::as_str).map(ToOwned::to_owned),
                expected_head: current_head.map(ToOwned::to_owned),
                codecov_config: String::new(),
                codecov_config_status: "unknown".to_string(),
                project: value.pointer("/measured/line_coverage").and_then(Value::as_f64),
                patch,
                patch_source,
                files_below_target: coverage_file_guidance_prefix(
                    value.get("files_below_target"),
                    5,
                ),
                patch_policy: codecov_status_policy(&value, "patch"),
                project_policy: codecov_status_policy(&value, "project"),
                codecov_comment: codecov_comment_policy(&value),
                coverage_scope: coverage_scope_value(&value),
                target: COVERAGE_TARGET,
                lcov: value.get("lcov").and_then(Value::as_str).map(ToOwned::to_owned),
            }
        }
        ReceiptInput::Missing => CoverageGateState {
            status: "missing".to_string(),
            receipt: display_path(path),
            head: None,
            expected_head: current_head.map(ToOwned::to_owned),
            codecov_config: String::new(),
            codecov_config_status: "unknown".to_string(),
            project: None,
            patch: None,
            patch_source: None,
            files_below_target: Vec::new(),
            patch_policy: CodecovStatusPolicy::missing(),
            project_policy: CodecovStatusPolicy::missing(),
            codecov_comment: CodecovCommentPolicy::missing(),
            coverage_scope: unknown_coverage_scope(),
            target: COVERAGE_TARGET,
            lcov: None,
        },
        ReceiptInput::Invalid(message) => CoverageGateState {
            status: format!("invalid: {message}"),
            receipt: display_path(path),
            head: None,
            expected_head: current_head.map(ToOwned::to_owned),
            codecov_config: String::new(),
            codecov_config_status: "unknown".to_string(),
            project: None,
            patch: None,
            patch_source: None,
            files_below_target: Vec::new(),
            patch_policy: CodecovStatusPolicy::missing(),
            project_policy: CodecovStatusPolicy::missing(),
            codecov_comment: CodecovCommentPolicy::missing(),
            coverage_scope: unknown_coverage_scope(),
            target: COVERAGE_TARGET,
            lcov: None,
        },
    }
}

fn coverage_scope_value(value: &Value) -> Value {
    value.get("coverage_scope").cloned().unwrap_or_else(unknown_coverage_scope)
}

fn unknown_coverage_scope() -> Value {
    let required_roots = required_coverage_roots()
        .unwrap_or_else(|_| vec!["crates".to_string(), "xtask".to_string()]);
    json!({
        "kind": "unknown",
        "source_files": 0,
        "roots": [],
        "required_roots": required_roots.clone(),
        "missing_required_roots": required_roots
    })
}

fn apply_codecov_config_fallback(coverage: &mut CoverageGateState, path: &Path) {
    coverage.codecov_config = display_path(path);
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            coverage.codecov_config_status = "missing".to_string();
            return;
        }
        Err(error) => {
            coverage.codecov_config_status = format!("invalid: {error}");
            return;
        }
    };

    let value = match serde_yaml_ng::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            coverage.codecov_config_status = format!("invalid: {error}");
            return;
        }
    };

    coverage.codecov_config_status = "present".to_string();
    coverage.patch_policy = codecov_status_policy_from_config(&value, "patch");
    coverage.project_policy = codecov_status_policy_from_config(&value, "project");
    coverage.codecov_comment = codecov_comment_policy_from_config(&value);
}

impl CodecovStatusPolicy {
    fn missing() -> Self {
        Self { target: None, threshold: None, informational: None, if_ci_failed: None }
    }
}

impl CodecovCommentPolicy {
    fn missing() -> Self {
        Self { layout: Vec::new(), behavior: None, require_head: None }
    }
}

fn codecov_status_policy(value: &Value, status: &str) -> CodecovStatusPolicy {
    let base = format!("/codecov_status/{status}/default");
    codecov_status_policy_at(value, &base)
}

fn codecov_status_policy_from_config(value: &Value, status: &str) -> CodecovStatusPolicy {
    let base = format!("/coverage/status/{status}/default");
    codecov_status_policy_at(value, &base)
}

fn codecov_status_policy_at(value: &Value, base: &str) -> CodecovStatusPolicy {
    CodecovStatusPolicy {
        target: value
            .pointer(&format!("{base}/target"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        threshold: value
            .pointer(&format!("{base}/threshold"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        informational: value.pointer(&format!("{base}/informational")).and_then(Value::as_bool),
        if_ci_failed: value
            .pointer(&format!("{base}/if_ci_failed"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

fn codecov_comment_policy(value: &Value) -> CodecovCommentPolicy {
    codecov_comment_policy_at(value, "/codecov_comment")
}

fn codecov_comment_policy_from_config(value: &Value) -> CodecovCommentPolicy {
    codecov_comment_policy_at(value, "/comment")
}

fn codecov_comment_policy_at(value: &Value, base: &str) -> CodecovCommentPolicy {
    CodecovCommentPolicy {
        layout: split_codecov_comment_layout(
            value.pointer(&format!("{base}/layout")).and_then(Value::as_str),
        ),
        behavior: value
            .pointer(&format!("{base}/behavior"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        require_head: value.pointer(&format!("{base}/require_head")).and_then(Value::as_bool),
    }
}

fn split_codecov_comment_layout(layout: Option<&str>) -> Vec<String> {
    layout
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn exception_state(path: &Path) -> QualityGateExceptionState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return QualityGateExceptionState {
                status: "missing".to_string(),
                path: display_path(path),
                active: Vec::new(),
                warnings: vec![json!({
                    "kind": "quality_exception_policy_missing",
                    "repair": "Add policy/quality-gate-exceptions.toml with the temporary burn-down exceptions.",
                    "verify": "rtk cargo xtask quality-gate --mode advisory"
                })],
            };
        }
        Err(error) => {
            return QualityGateExceptionState {
                status: format!("invalid: {error}"),
                path: display_path(path),
                active: Vec::new(),
                warnings: vec![json!({
                    "kind": "quality_exception_policy_unreadable",
                    "reason": error.to_string()
                })],
            };
        }
    };

    let parsed = match toml::from_str::<QualityGateExceptionFile>(&raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            return QualityGateExceptionState {
                status: format!("invalid: {error}"),
                path: display_path(path),
                active: Vec::new(),
                warnings: vec![json!({
                    "kind": "quality_exception_policy_invalid",
                    "reason": error.to_string(),
                    "repair": "Fix the quality-gate exception ledger schema.",
                    "verify": "rtk cargo xtask quality-gate --mode advisory"
                })],
            };
        }
    };

    let warnings = exception_warnings(&parsed);
    let status = if warnings.is_empty() { "present" } else { "invalid" };
    QualityGateExceptionState {
        status: status.to_string(),
        path: display_path(path),
        active: parsed.exceptions,
        warnings,
    }
}

fn exception_warnings(file: &QualityGateExceptionFile) -> Vec<Value> {
    let mut warnings = Vec::new();
    if file.schema_version != QUALITY_GATE_SCHEMA_VERSION {
        warnings.push(json!({
            "kind": "quality_exception_schema_mismatch",
            "expected": QUALITY_GATE_SCHEMA_VERSION,
            "actual": file.schema_version
        }));
    }
    if file.policy != "quality-gate-exceptions" {
        warnings.push(json!({
            "kind": "quality_exception_policy_mismatch",
            "expected": "quality-gate-exceptions",
            "actual": file.policy
        }));
    }
    if file.status.trim().is_empty() {
        warnings.push(json!({
            "kind": "quality_exception_status_missing"
        }));
    } else if file.status != "active" {
        warnings.push(json!({
            "kind": "quality_exception_status_invalid",
            "expected": "active",
            "actual": file.status.as_str(),
            "repair": "Set the temporary exception policy status to active while entries remain, or remove the entries after the burn-down targets are met."
        }));
    }
    if file.updated.trim().is_empty() {
        warnings.push(json!({
            "kind": "quality_exception_updated_missing"
        }));
    } else if parse_policy_date(&file.updated).is_none() {
        warnings.push(json!({
            "kind": "quality_exception_date_invalid",
            "field": "updated",
            "value": file.updated.as_str(),
            "expected": "YYYY-MM-DD"
        }));
    }
    if file.exceptions.is_empty() {
        warnings.push(json!({
            "kind": "quality_exception_entries_missing",
            "repair": "List the active burn-down exceptions for total RIPR+ and project coverage."
        }));
    }
    for exception in &file.exceptions {
        warnings.extend(exception_entry_warnings(exception, &file.updated));
    }
    warnings.extend(required_exception_warnings(file));
    warnings
}

fn required_exception_warnings(file: &QualityGateExceptionFile) -> Vec<Value> {
    let mut warnings = Vec::new();
    for required in REQUIRED_QUALITY_EXCEPTIONS {
        let Some(exception) = file.exceptions.iter().find(|exception| exception.id == required.id)
        else {
            warnings.push(json!({
                "kind": "quality_exception_required_entry_missing",
                "id": required.id,
                "applies_to": required.applies_to,
                "repair": "Restore the required temporary burn-down exception entry instead of silently dropping transition debt."
            }));
            continue;
        };

        if exception.applies_to != required.applies_to {
            warnings.push(json!({
                "kind": "quality_exception_required_entry_mismatch",
                "id": required.id,
                "field": "applies_to",
                "expected": required.applies_to,
                "actual": exception.applies_to.as_str()
            }));
        }
        if exception.final_target != required.final_target {
            warnings.push(json!({
                "kind": "quality_exception_required_entry_mismatch",
                "id": required.id,
                "field": "final_target",
                "expected": required.final_target,
                "actual": exception.final_target.as_str()
            }));
        }
        for evidence in required.evidence {
            if !exception.current_evidence.iter().any(|item| item == evidence) {
                warnings.push(json!({
                    "kind": "quality_exception_required_evidence_missing",
                    "id": required.id,
                    "evidence": evidence,
                    "repair": "Keep the receipt evidence path in the temporary exception so the burn-down can be rechecked."
                }));
            }
        }
    }
    warnings
}

fn exception_entry_warnings(exception: &QualityGateException, updated: &str) -> Vec<Value> {
    let mut warnings = Vec::new();
    for (field, value) in [
        ("id", exception.id.as_str()),
        ("applies_to", exception.applies_to.as_str()),
        ("owner", exception.owner.as_str()),
        ("reason", exception.reason.as_str()),
        ("final_target", exception.final_target.as_str()),
        ("removal_criteria", exception.removal_criteria.as_str()),
        ("review_after", exception.review_after.as_str()),
        ("expires", exception.expires.as_str()),
    ] {
        if value.trim().is_empty() {
            warnings.push(json!({
                "kind": "quality_exception_required_field_missing",
                "id": exception.id.as_str(),
                "field": field
            }));
        }
    }
    if exception.current_evidence.is_empty() {
        warnings.push(json!({
            "kind": "quality_exception_evidence_missing",
            "id": exception.id.as_str()
        }));
    }
    let updated_date = parse_policy_date(updated);
    let review_after = exception_date_warning(exception, "review_after", &exception.review_after);
    let expires = exception_date_warning(exception, "expires", &exception.expires);
    if let Some(warning) = review_after.warning {
        warnings.push(warning);
    }
    if let Some(warning) = expires.warning {
        warnings.push(warning);
    }
    if let (Some(updated_date), Some(review_after_date)) = (updated_date, review_after.date)
        && review_after_date < updated_date
    {
        warnings.push(json!({
            "kind": "quality_exception_date_order_invalid",
            "id": exception.id.as_str(),
            "field": "review_after",
            "value": exception.review_after.as_str(),
            "must_be_on_or_after": updated
        }));
    }
    if let (Some(review_after_date), Some(expires_date)) = (review_after.date, expires.date)
        && expires_date < review_after_date
    {
        warnings.push(json!({
            "kind": "quality_exception_date_order_invalid",
            "id": exception.id.as_str(),
            "field": "expires",
            "value": exception.expires.as_str(),
            "must_be_on_or_after": exception.review_after.as_str()
        }));
    }
    if let Some(review_after_date) = review_after.date {
        let today = current_policy_date();
        if review_after_date < today {
            warnings.push(json!({
                "kind": "quality_exception_review_due",
                "id": exception.id.as_str(),
                "field": "review_after",
                "value": exception.review_after.as_str(),
                "today": format_policy_date(today),
                "repair": "Re-justify the temporary exception with fresh evidence and dates, or remove it after the burn-down target is met."
            }));
        }
    }
    if let Some(expires_date) = expires.date {
        let today = current_policy_date();
        if expires_date < today {
            warnings.push(json!({
                "kind": "quality_exception_expired",
                "id": exception.id.as_str(),
                "field": "expires",
                "value": exception.expires.as_str(),
                "today": format_policy_date(today),
                "repair": "Remove the expired exception or update it with a fresh review date, expiry date, evidence, and removal criteria."
            }));
        }
    }
    warnings
}

struct ExceptionDateWarning {
    date: Option<NaiveDate>,
    warning: Option<Value>,
}

fn exception_date_warning(
    exception: &QualityGateException,
    field: &str,
    value: &str,
) -> ExceptionDateWarning {
    if value.trim().is_empty() {
        return ExceptionDateWarning { date: None, warning: None };
    }
    let date = parse_policy_date(value);
    let warning = if date.is_none() {
        Some(json!({
            "kind": "quality_exception_date_invalid",
            "id": exception.id.as_str(),
            "field": field,
            "value": value,
            "expected": "YYYY-MM-DD"
        }))
    } else {
        None
    };
    ExceptionDateWarning { date, warning }
}

fn parse_policy_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

fn current_policy_date() -> NaiveDate {
    Utc::now().date_naive()
}

fn format_policy_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn read_receipt(path: &Path) -> ReceiptInput {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return ReceiptInput::Missing,
        Err(error) => return ReceiptInput::Invalid(error.to_string()),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(value) => ReceiptInput::Present(value),
        Err(error) => ReceiptInput::Invalid(error.to_string()),
    }
}

fn quality_gate_command_state(config: &QualityGateConfig<'_>) -> QualityGateCommandState {
    QualityGateCommandState {
        ripr_receipt: display_path(config.ripr_receipt),
        ripr_pr_receipt: display_path(config.ripr_pr_receipt),
        review_receipt: display_path(config.review_receipt),
        coverage_receipt: display_path(config.coverage_receipt),
        codecov: display_path(config.codecov),
        exceptions: display_path(config.exceptions),
        receipt: display_path(config.receipt),
        summary: config.summary.map(display_path),
    }
}

fn write_or_check_quality_gate_receipt(
    path: &Path,
    packet: &QualityGateReceipt,
    check: bool,
    refresh_command: &str,
    verify_command: &str,
) -> Result<()> {
    let expected = format!("{}\n", serde_json::to_string_pretty(packet)?);
    if check {
        let actual = match fs::read_to_string(path) {
            Ok(actual) => actual,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                bail!(
                    "missing quality-gate receipt {}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    path.display()
                );
            }
            Err(error) => {
                bail!(
                    "unreadable quality-gate receipt {}: {error}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    path.display()
                );
            }
        };
        if actual != expected {
            bail!(
                "{} is stale; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                path.display()
            );
        }
        return Ok(());
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, expected)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn write_or_check_text(
    path: &Path,
    expected: &str,
    check: bool,
    refresh_command: &str,
    verify_command: &str,
) -> Result<()> {
    if check {
        let actual = match fs::read_to_string(path) {
            Ok(actual) => actual,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                bail!(
                    "missing quality-gate summary {}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    path.display()
                );
            }
            Err(error) => {
                bail!(
                    "unreadable quality-gate summary {}: {error}; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                    path.display()
                );
            }
        };
        if actual != expected {
            bail!(
                "{} is stale; refresh with `{refresh_command}`, then verify with `{verify_command}`",
                path.display()
            );
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, expected)?;
    Ok(())
}

fn receipt_status(
    value: &Value,
    current_head: Option<&str>,
    freshness_pointer: &str,
    contract: &[ExpectedField],
) -> String {
    if let Some(violation) = receipt_contract_violation(value, contract) {
        return format!("invalid: {violation}");
    }
    present_status(value, current_head, freshness_pointer)
}

fn diff_receipt_status(
    value: &Value,
    current_head: Option<&str>,
    contract: &[ExpectedField],
) -> String {
    if let Some(violation) = receipt_contract_violation(value, contract) {
        return format!("invalid: {violation}");
    }
    if let Some(violation) = required_string_violation(value, &["/base", "/base_sha", "/head_sha"])
    {
        return format!("invalid: {violation}");
    }
    let head_status = present_status(value, current_head, "/head_sha");
    if head_status != "present" {
        return head_status;
    }
    base_status(value)
}

fn ripr_plus_receipt_status(value: &Value, current_head: Option<&str>) -> String {
    let status = receipt_status(value, current_head, "/head", RIPR_PLUS_CONTRACT);
    if status != "present" {
        return status;
    }
    if let Some(violation) = ripr_plus_measurement_violation(value) {
        return format!("invalid: {violation}");
    }
    status
}

fn ripr_plus_measurement_violation(value: &Value) -> Option<String> {
    let unresolved = match required_u64(value, "/unresolved") {
        Ok(value) => value,
        Err(violation) => return Some(violation),
    };
    if unresolved > 0 && !has_ripr_plus_actionable_or_deferred_guidance(value) {
        return Some(
            "/top_actionable_files or /next_actions expected actionable RIPR guidance when /unresolved is greater than 0"
                .to_string(),
        );
    }
    None
}

fn has_ripr_plus_actionable_or_deferred_guidance(value: &Value) -> bool {
    has_ripr_plus_actionable_guidance(value) || has_ripr_plus_missing_guidance_action(value)
}

fn has_ripr_plus_actionable_guidance(value: &Value) -> bool {
    ["top_actionable_files", "top_files"].iter().any(|field| {
        value
            .get(*field)
            .and_then(Value::as_array)
            .is_some_and(|files| files.iter().any(ripr_plus_file_guidance_is_actionable))
    })
}

fn has_ripr_plus_missing_guidance_action(value: &Value) -> bool {
    value
        .get("next_actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| actions.iter().any(ripr_plus_missing_guidance_action_is_actionable))
}

fn ripr_plus_missing_guidance_action_is_actionable(action: &Value) -> bool {
    let has_text = |field: &str| {
        action.get(field).and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty())
    };
    has_text("path")
        && action.get("unresolved").and_then(Value::as_u64).is_some_and(|count| count > 0)
        && action.get("kind").and_then(Value::as_str) == Some("ripr_receipt_gap_guidance_missing")
        && action.get("reason").and_then(Value::as_str) == Some("missing_actionable_sample")
        && has_text("repair")
        && has_text("suggested_test")
        && has_text("verify")
        && has_text("receipt")
}

fn ripr_plus_file_guidance_is_actionable(file: &Value) -> bool {
    let has_name =
        file.get("name").and_then(Value::as_str).is_some_and(|name| !name.trim().is_empty());
    let has_count = file.get("count").and_then(Value::as_u64).is_some_and(|count| count > 0);
    let has_sample_seam = file
        .get("sample_seams")
        .and_then(Value::as_array)
        .is_some_and(|samples| samples.iter().any(ripr_plus_sample_seam_is_actionable));
    has_name && has_count && has_sample_seam
}

fn ripr_plus_sample_seam_is_actionable(sample: &Value) -> bool {
    let has_text = |field: &str| {
        sample.get(field).and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty())
    };
    has_text("gap_id")
        && sample.get("line").and_then(Value::as_u64).is_some_and(|line| line > 0)
        && has_text("kind")
        && has_text("seam")
        && has_text("reason")
        && has_text("suggested_test")
}

fn coverage_receipt_status(value: &Value, current_head: Option<&str>) -> String {
    let status = receipt_status(value, current_head, "/head", COVERAGE_BASELINE_CONTRACT);
    if status != "present" {
        return status;
    }
    if let Some(violation) = coverage_measurement_violation(value) {
        return format!("invalid: {violation}");
    }
    status
}

fn coverage_measurement_violation(value: &Value) -> Option<String> {
    let line_found = match required_u64(value, "/measured/line_found") {
        Ok(value) => value,
        Err(violation) => return Some(violation),
    };
    if line_found == 0 {
        return Some("/measured/line_found expected positive line count, got 0".to_string());
    }
    let line_hit = match required_u64(value, "/measured/line_hit") {
        Ok(value) => value,
        Err(violation) => return Some(violation),
    };
    if line_hit > line_found {
        return Some(format!(
            "/measured/line_hit expected <= /measured/line_found, got {line_hit} > {line_found}"
        ));
    }
    let line_coverage = match required_f64(value, "/measured/line_coverage") {
        Ok(value) => value,
        Err(violation) => return Some(violation),
    };
    if !(0.0..=100.0).contains(&line_coverage) {
        return Some(format!("/measured/line_coverage expected 0..=100, got {line_coverage}"));
    }
    if line_coverage < COVERAGE_TARGET && !has_coverage_file_guidance(value) {
        return Some(
            "/files_below_target expected at least one below-target file guidance row with positive sample_uncovered_lines when /measured/line_coverage is below 95"
                .to_string(),
        );
    }
    None
}

fn has_coverage_file_guidance(value: &Value) -> bool {
    value
        .get("files_below_target")
        .and_then(Value::as_array)
        .is_some_and(|files| files.iter().any(coverage_file_guidance_is_valid))
}

fn coverage_file_guidance_is_valid(file: &Value) -> bool {
    let has_path =
        file.get("path").and_then(Value::as_str).is_some_and(|path| !path.trim().is_empty());
    let has_lines = file.get("line_found").and_then(Value::as_u64).is_some_and(|count| count > 0);
    let is_below_target = file
        .get("line_coverage")
        .and_then(Value::as_f64)
        .is_some_and(|coverage| coverage < COVERAGE_TARGET);
    let has_sample_uncovered_lines = file
        .get("sample_uncovered_lines")
        .and_then(Value::as_array)
        .is_some_and(|lines| lines.iter().filter_map(Value::as_u64).any(|line| line > 0));
    has_path && has_lines && is_below_target && has_sample_uncovered_lines
}

fn required_u64(value: &Value, pointer: &str) -> std::result::Result<u64, String> {
    match value.pointer(pointer).and_then(Value::as_u64) {
        Some(actual) => Ok(actual),
        None => Err(format!(
            "{pointer} expected unsigned integer, got {}",
            value_label(value.pointer(pointer))
        )),
    }
}

fn required_f64(value: &Value, pointer: &str) -> std::result::Result<f64, String> {
    match value.pointer(pointer).and_then(Value::as_f64) {
        Some(actual) => Ok(actual),
        None => {
            Err(format!("{pointer} expected number, got {}", value_label(value.pointer(pointer))))
        }
    }
}

fn required_string_violation(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| match value.pointer(pointer).and_then(Value::as_str) {
        Some(actual) if !actual.trim().is_empty() => None,
        _ => Some(format!(
            "{pointer} expected non-empty string, got {}",
            value_label(value.pointer(pointer))
        )),
    })
}

fn base_status(value: &Value) -> String {
    let Some(receipt_base_sha) = value.get("base_sha").and_then(Value::as_str) else {
        return "present".to_string();
    };
    match current_receipt_base_sha(value) {
        Some(current_base_sha) if current_base_sha == receipt_base_sha => "present".to_string(),
        Some(_) => "stale".to_string(),
        None => "present".to_string(),
    }
}

fn current_receipt_base_sha(value: &Value) -> Option<String> {
    let base = value.get("base").and_then(Value::as_str)?;
    git_rev_parse(base)
}

fn git_rev_parse(rev: &str) -> Option<String> {
    let output =
        Command::new("git").args(["rev-parse", &format!("{rev}^{{commit}}")]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn receipt_contract_violation(value: &Value, contract: &[ExpectedField]) -> Option<String> {
    contract.iter().find_map(|field| {
        if expected_field_matches(value.pointer(field.pointer), field.value) {
            None
        } else {
            Some(format!(
                "{} expected {}, got {}",
                field.pointer,
                expected_field_label(field.value),
                value_label(value.pointer(field.pointer))
            ))
        }
    })
}

fn expected_field_matches(actual: Option<&Value>, expected: ExpectedFieldValue) -> bool {
    match (actual, expected) {
        (Some(Value::String(actual)), ExpectedFieldValue::String(expected)) => actual == expected,
        (Some(Value::Number(actual)), ExpectedFieldValue::U64(expected)) => {
            actual.as_u64() == Some(expected)
        }
        _ => false,
    }
}

fn expected_field_label(expected: ExpectedFieldValue) -> String {
    match expected {
        ExpectedFieldValue::String(value) => format!("{value:?}"),
        ExpectedFieldValue::U64(value) => value.to_string(),
    }
}

fn value_label(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => format!("{value:?}"),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Array(_)) => "array".to_string(),
        Some(Value::Object(_)) => "object".to_string(),
        None => "missing".to_string(),
    }
}

fn present_status(value: &Value, current_head: Option<&str>, freshness_pointer: &str) -> String {
    let Some(current_head) = current_head else {
        return "present".to_string();
    };
    match value.pointer(freshness_pointer).and_then(Value::as_str) {
        Some(receipt_head) if receipt_head == current_head => "present".to_string(),
        _ => "stale".to_string(),
    }
}

fn array_prefix(value: Option<&Value>, limit: usize) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().take(limit).cloned().collect())
        .unwrap_or_default()
}

fn coverage_file_guidance_prefix(value: Option<&Value>, limit: usize) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().filter_map(actionable_coverage_file_guidance).take(limit).collect()
        })
        .unwrap_or_default()
}

fn actionable_coverage_file_guidance(file: &Value) -> Option<Value> {
    if !coverage_file_guidance_is_valid(file) {
        return None;
    }

    let mut cleaned = file.clone();
    if let Some(object) = cleaned.as_object_mut() {
        object.insert(
            "sample_uncovered_lines".to_string(),
            Value::Array(
                positive_sample_uncovered_lines(file).into_iter().map(Value::from).collect(),
            ),
        );
    }
    Some(cleaned)
}

fn positive_sample_uncovered_lines(file: &Value) -> Vec<u64> {
    file.get("sample_uncovered_lines")
        .and_then(Value::as_array)
        .map(|lines| lines.iter().filter_map(Value::as_u64).filter(|line| *line > 0).collect())
        .unwrap_or_default()
}

fn ripr_guidance_files(ripr: &RiprGateState) -> Vec<Value> {
    let actionable = actionable_ripr_guidance_prefix(&ripr.top_actionable_files, 5);
    if actionable.is_empty() {
        actionable_ripr_guidance_prefix(&ripr.top_files, 5)
    } else {
        actionable
    }
}

fn ripr_missing_guidance_actions(ripr: &RiprGateState) -> Vec<Value> {
    ripr.receipt_next_actions
        .iter()
        .filter(|action| ripr_plus_missing_guidance_action_is_actionable(action))
        .take(5)
        .cloned()
        .collect()
}

fn actionable_ripr_guidance_prefix(files: &[Value], limit: usize) -> Vec<Value> {
    files.iter().filter_map(actionable_ripr_file_guidance).take(limit).collect()
}

fn actionable_ripr_file_guidance(file: &Value) -> Option<Value> {
    if !ripr_plus_file_guidance_is_actionable(file) {
        return None;
    }

    let sample_seams = file
        .get("sample_seams")
        .and_then(Value::as_array)?
        .iter()
        .filter(|sample| ripr_plus_sample_seam_is_actionable(sample))
        .cloned()
        .collect::<Vec<_>>();
    if sample_seams.is_empty() {
        return None;
    }

    let mut cleaned = file.clone();
    if let Some(object) = cleaned.as_object_mut() {
        object.insert("sample_seams".to_string(), Value::Array(sample_seams));
    }
    Some(cleaned)
}

fn review_guidance_items(value: &Value, limit: usize) -> Vec<Value> {
    let mut gaps = Vec::new();
    for source in ["comments", "summary_only"] {
        let Some(items) = value.get(source).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if gaps.len() >= limit {
                return gaps;
            }
            let gap = review_guidance_item(source, item);
            if review_guidance_item_is_actionable(&gap) {
                gaps.push(gap);
            }
        }
    }
    gaps
}

fn review_guidance_declares_items(value: &Value) -> bool {
    ["comments", "summary_only"].iter().any(|field| {
        value.get(*field).and_then(Value::as_array).is_some_and(|items| !items.is_empty())
    }) || ["/summary/comments", "/summary/summary_only"].iter().any(|pointer| {
        value.pointer(pointer).and_then(Value::as_u64).is_some_and(|count| count > 0)
    })
}

fn review_guidance_item_is_actionable(item: &Value) -> bool {
    let has_text = |field: &str| {
        item.get(field).and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty())
    };
    has_text("gap_id")
        && has_text("path")
        && item.get("line").and_then(Value::as_u64).is_some_and(|line| line > 0)
        && has_text("seam")
        && has_text("reason")
        && has_text("suggested_test")
}

fn review_guidance_item(source: &str, item: &Value) -> Value {
    json!({
        "source": source,
        "gap_id": first_string(
            item,
            &[
                "/canonical_gap_id",
                "/gap_id",
                "/identity/canonical_gap_id",
                "/evidence_record/canonical_gap_id",
                "/evidence_record/gap_id",
                "/id",
            ],
        ),
        "path": first_string(
            item,
            &[
                "/placement/path",
                "/path",
                "/file",
                "/location/path",
                "/span/path",
                "/evidence_record/path",
            ],
        ),
        "line": first_u64(
            item,
            &[
                "/placement/line",
                "/line",
                "/location/line",
                "/span/line",
                "/evidence_record/line",
            ],
        ),
        "seam": first_string(
            item,
            &["/seam", "/placement/mode", "/owner", "/evidence_record/seam"],
        ),
        "reason": first_string(item, &["/reason", "/why", "/message", "/kind"]),
        "suggested_test": first_string(
            item,
            &[
                "/suggested_test/intent",
                "/suggested_test/name",
                "/suggested_test",
                "/repair",
                "/recommended_repair",
            ],
        ),
    })
}

fn first_string(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        value
            .pointer(path)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(ToOwned::to_owned)
    })
}

fn first_u64(value: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| value.pointer(path).and_then(Value::as_u64))
}

fn advisory_actions(
    mode: QualityGateMode,
    ripr: &RiprGateState,
    ripr_pr: &RiprPrGateState,
    review_guidance: &ReviewGuidanceState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    let mut actions = Vec::new();
    if ripr.status != "present" && advisory_ripr_receipt_gap_is_useful(mode) {
        actions.push(json!({
            "kind": "ripr_receipt_gap",
            "path": ripr.receipt,
            "reason": ripr.status,
            "repair": "Regenerate the RIPR+ receipt for the current HEAD.",
            "verify": ripr_plus_command(ripr, true),
            "receipt": ripr_plus_command(ripr, false)
        }));
    }
    if ripr_pr.status != "present" && advisory_ripr_pr_receipt_gap_is_useful(mode) {
        actions.push(json!({
            "kind": "ripr_pr_receipt_gap",
            "path": ripr_pr.receipt,
            "reason": ripr_pr.status,
            "repair": "Regenerate the diff-scoped RIPR PR receipt for the current HEAD.",
            "verify": ripr_pr_command(ripr_pr, true),
            "receipt": ripr_pr_command(ripr_pr, false)
        }));
    }
    if review_guidance.status != "present" && advisory_ripr_review_guidance_gap_is_useful(mode) {
        actions.push(ripr_review_guidance_gap_action(
            mode,
            ripr_pr,
            review_guidance,
            coverage,
            gate_command,
        ));
    }
    if ripr_pr.status == "present"
        && ripr_pr.new_unresolved.is_none()
        && advisory_new_ripr_gap_unknown_is_useful(mode)
    {
        actions.push(json!({
            "kind": "ripr_new_gap_unknown",
            "path": ripr_pr.receipt,
            "reason": "diff-scoped severe_gaps is not measured yet",
            "repair": "Regenerate the RIPR PR evidence receipt so new-gap count comes from summary.severe_gaps.",
            "verify": ripr_pr_command(ripr_pr, true),
            "receipt": ripr_pr_command(ripr_pr, false)
        }));
    }
    if let Some(count) = ripr_pr.new_unresolved
        && count > 0
    {
        if advisory_new_ripr_gap_is_useful(mode) {
            actions.push(new_ripr_gap_action(
                count,
                ripr_pr,
                review_guidance,
                coverage,
                gate_command,
                QualityGateMode::EnforceNewRipr,
            ));
        }
        if (review_guidance.status == "present" && review_guidance.top_gaps.is_empty())
            || (review_guidance.status != "present"
                && !advisory_ripr_review_guidance_gap_is_useful(mode))
        {
            actions.push(ripr_review_guidance_gap_action(
                mode,
                ripr_pr,
                review_guidance,
                coverage,
                gate_command,
            ));
        }
    }
    if ripr.status == "present" && advisory_ripr_seam_clusters_are_useful(mode) {
        let guidance_files = ripr_guidance_files(ripr);
        for file in &guidance_files {
            let mut action = json!({
                "kind": "ripr_seam_cluster",
                "path": file.get("name").and_then(Value::as_str),
                "unresolved": file.get("count").and_then(Value::as_u64),
                "repair": "Add a focused behavior test for the seam cluster before changing production code.",
                "suggested_test": RIPR_SEAM_SUGGESTED_TEST,
                "verify": ripr_plus_command(ripr, true),
                "receipt": ripr_plus_command(ripr, false)
            });
            if let Some(sample_seams) = file.get("sample_seams").cloned()
                && let Some(object) = action.as_object_mut()
            {
                object.insert("sample_seams".to_string(), sample_seams);
            }
            actions.push(action);
        }
        if guidance_files.is_empty() {
            actions.extend(ripr_missing_guidance_actions(ripr));
        }
    }

    if coverage.status != "present" && advisory_coverage_receipt_gap_is_useful(mode) {
        actions.push(json!({
            "kind": "coverage_receipt_gap",
            "path": coverage.receipt,
            "reason": coverage.status,
            "repair": "Generate LCOV and refresh the coverage baseline receipt.",
            "verify": coverage_baseline_command(coverage, true),
            "receipt": coverage_baseline_command(coverage, false)
        }));
    }
    if coverage.codecov_config_status != "present" && advisory_codecov_config_gap_is_useful(mode) {
        actions.push(json!({
            "kind": "codecov_config_gap",
            "path": coverage.codecov_config,
            "reason": coverage.codecov_config_status,
            "repair": "Restore a parseable codecov.yml so quality-gate can validate PR coverage policy and failure guidance.",
            "verify": quality_gate_command("advisory", gate_command, Some(coverage), true),
            "receipt": quality_gate_command("advisory", gate_command, Some(coverage), false)
        }));
    }
    if coverage.status == "present"
        && coverage.project.is_some_and(|project| project < COVERAGE_TARGET)
        && advisory_project_coverage_gap_is_useful(mode)
    {
        actions.push(json!({
            "kind": "project_coverage_gap",
            "current": coverage.project,
            "target": COVERAGE_TARGET,
            "top_files": coverage.files_below_target.clone(),
            "repair": "Add behavior tests for uncovered public API, error paths, config, serialization, cancellation, or output contracts.",
            "suggested_test": COVERAGE_GAP_SUGGESTED_TEST,
            "verify": coverage_baseline_command(coverage, true),
            "receipt": coverage_baseline_command(coverage, false)
        }));
    }
    actions
}

fn advisory_ripr_receipt_gap_is_useful(mode: QualityGateMode) -> bool {
    !matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::Enforce)
}

fn advisory_ripr_pr_receipt_gap_is_useful(mode: QualityGateMode) -> bool {
    !matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::Enforce)
}

fn advisory_ripr_review_guidance_gap_is_useful(mode: QualityGateMode) -> bool {
    !matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::Enforce)
}

fn advisory_new_ripr_gap_unknown_is_useful(mode: QualityGateMode) -> bool {
    !matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::Enforce)
}

fn advisory_new_ripr_gap_is_useful(mode: QualityGateMode) -> bool {
    !mode.is_enforcing()
}

fn advisory_ripr_seam_clusters_are_useful(mode: QualityGateMode) -> bool {
    mode != QualityGateMode::Enforce
}

fn advisory_coverage_receipt_gap_is_useful(mode: QualityGateMode) -> bool {
    matches!(mode, QualityGateMode::Advisory)
}

fn advisory_codecov_config_gap_is_useful(mode: QualityGateMode) -> bool {
    !matches!(mode, QualityGateMode::EnforcePatchCoverage | QualityGateMode::Enforce)
}

fn advisory_project_coverage_gap_is_useful(mode: QualityGateMode) -> bool {
    mode != QualityGateMode::Enforce
}

fn exception_actions(
    exceptions: &QualityGateExceptionState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    if exceptions.status == "present" {
        return Vec::new();
    }

    let mut action = json!({
        "kind": "quality_exception_policy_gap",
        "path": exceptions.path.clone(),
        "reason": exceptions.status.clone(),
        "repair": "Add or fix policy/quality-gate-exceptions.toml so temporary burn-down debt is explicit and dated.",
        "verify": quality_gate_command("advisory", gate_command, Some(coverage), true),
        "receipt": quality_gate_command("advisory", gate_command, Some(coverage), false),
    });
    if !exceptions.warnings.is_empty()
        && let Some(object) = action.as_object_mut()
    {
        object.insert("warnings".to_string(), Value::Array(exceptions.warnings.clone()));
    }
    vec![action]
}

fn new_ripr_gap_action(
    count: u64,
    ripr_pr: &RiprPrGateState,
    review_guidance: &ReviewGuidanceState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: QualityGateMode,
) -> Value {
    let gate_mode = ripr_gate_verify_mode(mode);
    let coverage = if mode == QualityGateMode::Enforce { Some(coverage) } else { None };
    let mut action = json!({
        "kind": "new_ripr_gap",
        "path": ripr_pr.receipt,
        "unresolved": count,
        "weakly_exposed": ripr_pr.weakly_exposed,
        "reachable_unrevealed": ripr_pr.reachable_unrevealed,
        "no_static_path": ripr_pr.no_static_path,
        "repair": "Add focused tests that expose the new RIPR seam before merging.",
        "suggested_test": NEW_RIPR_GAP_SUGGESTED_TEST,
        "verify": quality_gate_command(gate_mode, gate_command, coverage, true),
        "receipt": quality_gate_command(gate_mode, gate_command, coverage, false)
    });
    if !review_guidance.top_gaps.is_empty()
        && let Some(object) = action.as_object_mut()
    {
        object.insert("top_gaps".to_string(), Value::Array(review_guidance.top_gaps.clone()));
    } else if let Some(object) = action.as_object_mut() {
        object.insert(
            "guidance_status".to_string(),
            Value::String(review_guidance_gap_reason(review_guidance).to_string()),
        );
        object.insert("guidance_path".to_string(), Value::String(review_guidance.receipt.clone()));
        object.insert(
            "guidance_repair".to_string(),
            Value::String(
                "Generate RIPR review guidance so the failure names file, line, seam, gap id, and suggested proof."
                    .to_string(),
            ),
        );
        object.insert(
            "guidance_verify".to_string(),
            Value::String(ripr_review_command(ripr_pr, true)),
        );
        object.insert(
            "guidance_receipt".to_string(),
            Value::String(ripr_review_command(ripr_pr, false)),
        );
    }
    action
}

fn ripr_pr_verify_command(
    mode: QualityGateMode,
    ripr_pr: &RiprPrGateState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> String {
    if mode == QualityGateMode::Enforce {
        quality_gate_command("enforce", gate_command, Some(coverage), true)
    } else {
        ripr_pr_command(ripr_pr, true)
    }
}

fn ripr_review_verify_command(
    mode: QualityGateMode,
    ripr_pr: &RiprPrGateState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> String {
    if mode == QualityGateMode::Enforce {
        quality_gate_command("enforce", gate_command, Some(coverage), true)
    } else {
        ripr_review_command(ripr_pr, true)
    }
}

fn ripr_review_guidance_gap_action(
    mode: QualityGateMode,
    ripr_pr: &RiprPrGateState,
    review_guidance: &ReviewGuidanceState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Value {
    json!({
        "kind": "ripr_review_guidance_gap",
        "path": review_guidance.receipt,
        "reason": review_guidance_gap_reason(review_guidance),
        "repair": "Generate RIPR review guidance so the failing gate names the changed file, line, seam, and suggested test.",
        "verify": ripr_review_guidance_verify_command(mode, ripr_pr, coverage, gate_command),
        "receipt": ripr_review_command(ripr_pr, false)
    })
}

fn ripr_review_guidance_verify_command(
    mode: QualityGateMode,
    ripr_pr: &RiprPrGateState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> String {
    if mode.is_enforcing() {
        let coverage = if mode == QualityGateMode::Enforce { Some(coverage) } else { None };
        quality_gate_command(ripr_gate_verify_mode(mode), gate_command, coverage, true)
    } else {
        ripr_review_command(ripr_pr, true)
    }
}

fn ripr_gate_verify_mode(mode: QualityGateMode) -> &'static str {
    if mode == QualityGateMode::Enforce { "enforce" } else { "enforce-new-ripr" }
}

fn review_guidance_gap_reason(review_guidance: &ReviewGuidanceState) -> &str {
    if review_guidance.status == "present" && review_guidance.top_gaps.is_empty() {
        "no_top_gaps"
    } else {
        &review_guidance.status
    }
}

fn blockers(
    mode: QualityGateMode,
    ripr: &RiprGateState,
    ripr_pr: &RiprPrGateState,
    review_guidance: &ReviewGuidanceState,
    coverage: &CoverageGateState,
    exceptions: &QualityGateExceptionState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    if !mode.is_enforcing() {
        return Vec::new();
    }

    let mut blockers = Vec::new();
    if matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::EnforcePatchCoverage) {
        blockers.extend(exception_policy_blockers(
            mode.as_str(),
            exceptions,
            coverage,
            gate_command,
        ));
    }

    if matches!(mode, QualityGateMode::EnforceNewRipr | QualityGateMode::Enforce) {
        if mode == QualityGateMode::EnforceNewRipr && ripr.status != "present" {
            blockers.push(json!({
                "kind": "ripr_receipt_not_current",
                "path": ripr.receipt,
                "reason": ripr.status,
                "receipt_head": ripr.head,
                "expected_head": ripr.expected_head,
                "repair": "Regenerate and check the repo-wide RIPR+ receipt for this HEAD. The transition gate does not require total RIPR+ zero yet, but it does require current total-debt proof.",
                "verify": ripr_plus_command(ripr, true),
                "receipt": ripr_plus_command(ripr, false)
            }));
        }

        if ripr_pr.status != "present" {
            blockers.push(json!({
                "kind": "ripr_pr_receipt_not_current",
                "path": ripr_pr.receipt,
                "reason": ripr_pr.status,
                "receipt_base": ripr_pr.base,
                "receipt_base_sha": ripr_pr.base_sha,
                "expected_base_sha": ripr_pr.expected_base_sha,
                "receipt_head_sha": ripr_pr.head_sha,
                "expected_head_sha": ripr_pr.expected_head_sha,
                "repair": "Regenerate and check the diff-scoped RIPR PR receipt for this HEAD.",
                "verify": ripr_pr_verify_command(mode, ripr_pr, coverage, gate_command),
                "receipt": ripr_pr_command(ripr_pr, false)
            }));
        }

        if review_guidance.status != "present" {
            blockers.push(json!({
                "kind": "ripr_review_receipt_not_current",
                "path": review_guidance.receipt,
                "reason": review_guidance.status,
                "receipt_base": review_guidance.base,
                "receipt_base_sha": review_guidance.base_sha,
                "expected_base_sha": review_guidance.expected_base_sha,
                "receipt_head_sha": review_guidance.head_sha,
                "expected_head_sha": review_guidance.expected_head_sha,
                "repair": "Regenerate and check the RIPR review-guidance receipt for this HEAD so failing gates can name the exact file, line, seam, and suggested proof.",
                "verify": ripr_review_verify_command(mode, ripr_pr, coverage, gate_command),
                "receipt": ripr_review_command(ripr_pr, false)
            }));
        }

        if ripr_pr.status == "present" {
            match ripr_pr.new_unresolved {
                Some(0) => {}
                Some(count) => blockers.push(new_ripr_gap_action(
                    count,
                    ripr_pr,
                    review_guidance,
                    coverage,
                    gate_command,
                    mode,
                )),
                None => blockers.push(json!({
                    "kind": "new_ripr_gap_unknown",
                    "path": ripr_pr.receipt,
                    "reason": "diff-scoped severe_gaps is not measured yet",
                    "repair": "Generate a diff-scoped RIPR PR receipt with summary.severe_gaps.",
                    "verify": ripr_pr_verify_command(mode, ripr_pr, coverage, gate_command),
                    "receipt": ripr_pr_command(ripr_pr, false)
                })),
            }
        }
    }

    if mode == QualityGateMode::EnforcePatchCoverage {
        blockers.extend(codecov_config_blockers(coverage, gate_command, "enforce-patch-coverage"));
        blockers.extend(patch_coverage_policy_blockers(
            coverage,
            gate_command,
            "enforce-patch-coverage",
        ));
        blockers.extend(codecov_comment_policy_blockers(
            coverage,
            gate_command,
            "enforce-patch-coverage",
        ));
        blockers.extend(patch_coverage_value_blockers(
            coverage,
            gate_command,
            "enforce-patch-coverage",
            true,
        ));
    } else if mode == QualityGateMode::Enforce {
        blockers.extend(codecov_config_blockers(coverage, gate_command, "enforce"));
        blockers.extend(patch_coverage_policy_blockers(coverage, gate_command, "enforce"));
        blockers.extend(project_coverage_policy_blockers(coverage, gate_command));
        blockers.extend(codecov_comment_policy_blockers(coverage, gate_command, "enforce"));
        blockers.extend(coverage_scope_blockers(coverage, gate_command));
        blockers.extend(patch_coverage_value_blockers(coverage, gate_command, "enforce", true));
    }

    if mode == QualityGateMode::Enforce {
        blockers.extend(final_exception_blockers(exceptions, coverage, gate_command));

        if ripr.status != "present" {
            blockers.push(json!({
                "kind": "ripr_receipt_not_current",
                "path": ripr.receipt,
                "reason": ripr.status,
                "receipt_head": ripr.head,
                "expected_head": ripr.expected_head,
                "repair": "Regenerate and check the repo-wide RIPR+ receipt for this HEAD.",
                "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
                "receipt": ripr_plus_command(ripr, false)
            }));
        }

        match ripr.unresolved {
            Some(0) => {}
            Some(count) => blockers.push(json!({
                "kind": "ripr_total_not_zero",
                "path": ripr.receipt,
                "unresolved": count,
                "top_files": ripr_guidance_files(ripr),
                "raw_top_files": ripr.top_files.clone(),
                "deferred_files": ripr.deferred_files.clone(),
                "receipt_guidance": ripr_missing_guidance_actions(ripr),
                "repair": "Burn down the named RIPR seam clusters with focused tests.",
                "suggested_test": RIPR_SEAM_SUGGESTED_TEST,
                "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
                "receipt": ripr_plus_command(ripr, false)
            })),
            None => blockers.push(json!({
                "kind": "ripr_total_unknown",
                "path": ripr.receipt,
                "repair": "Regenerate a RIPR+ receipt with an unresolved count.",
                "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
                "receipt": ripr_plus_command(ripr, false)
            })),
        }

        match coverage.project {
            Some(project) if project >= COVERAGE_TARGET => {}
            Some(project) => blockers.push(json!({
                "kind": "project_coverage_below_target",
                "path": coverage.receipt,
                "current": project,
                "target": COVERAGE_TARGET,
                "top_files": coverage.files_below_target.clone(),
                "repair": "Add behavior tests for high-risk uncovered code until project coverage reaches 95%.",
                "suggested_test": COVERAGE_GAP_SUGGESTED_TEST,
                "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
                "receipt": coverage_baseline_command(coverage, false)
            })),
            None => blockers.push(json!({
                "kind": "project_coverage_unknown",
                "path": coverage.receipt,
                "repair": "Generate coverage-baseline receipt from target/lcov.info.",
                "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
                "receipt": coverage_baseline_command(coverage, false)
            })),
        }
    }

    mark_blocking_actions(&mut blockers);
    blockers
}

fn mark_blocking_actions(actions: &mut [Value]) {
    for action in actions {
        if let Some(object) = action.as_object_mut() {
            object.insert("blocking".to_string(), Value::Bool(true));
        }
    }
}

fn final_exception_blockers(
    exceptions: &QualityGateExceptionState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    if exceptions.status == "missing" {
        return Vec::new();
    }
    if exceptions.status == "present" && exceptions.active.is_empty() {
        return Vec::new();
    }

    let mut blockers = Vec::new();
    if !exceptions.active.is_empty() {
        blockers.push(json!({
            "kind": "temporary_exceptions_still_active",
            "path": exceptions.path,
            "active": exceptions.active.iter().map(|exception| exception.id.as_str()).collect::<Vec<_>>(),
            "repair": "Remove temporary burn-down exceptions after RIPR+ zero and project coverage reach the final blocking targets.",
            "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
            "receipt": quality_gate_command("enforce", gate_command, Some(coverage), false)
        }));
    }
    if exceptions.status != "present" {
        let mut blocker = json!({
            "kind": "quality_exception_policy_not_current",
            "path": exceptions.path,
            "reason": exceptions.status,
            "repair": "Remove or fix the temporary exception policy before relying on final enforce mode.",
            "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
            "receipt": quality_gate_command("enforce", gate_command, Some(coverage), false)
        });
        if !exceptions.warnings.is_empty()
            && let Some(object) = blocker.as_object_mut()
        {
            object.insert("warnings".to_string(), Value::Array(exceptions.warnings.clone()));
        }
        blockers.push(blocker);
    }
    blockers
}

fn exception_policy_blockers(
    mode: &str,
    exceptions: &QualityGateExceptionState,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    if exceptions.status == "present" {
        return Vec::new();
    }

    let mut blocker = json!({
        "kind": "quality_exception_policy_not_current",
        "path": exceptions.path,
        "reason": exceptions.status,
        "repair": "Add or fix policy/quality-gate-exceptions.toml so transitional burn-down debt is explicit, dated, and removable.",
        "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
    });
    if !exceptions.warnings.is_empty()
        && let Some(object) = blocker.as_object_mut()
    {
        object.insert("warnings".to_string(), Value::Array(exceptions.warnings.clone()));
    }
    vec![blocker]
}

fn codecov_config_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
) -> Vec<Value> {
    if coverage.codecov_config_status == "present" {
        return Vec::new();
    }

    vec![json!({
        "kind": "codecov_config_not_current",
        "path": coverage.codecov_config,
        "reason": coverage.codecov_config_status,
        "repair": "Restore a parseable codecov.yml with coverage.status.patch, coverage.status.project, and actionable PR comment guidance.",
        "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
    })]
}

fn patch_coverage_policy_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
) -> Vec<Value> {
    let mut blockers = Vec::new();
    if coverage.status != "present" {
        blockers.push(json!({
            "kind": "coverage_receipt_not_current",
            "path": coverage.receipt,
            "reason": coverage.status,
            "receipt_head": coverage.head,
            "expected_head": coverage.expected_head,
            "repair": "Refresh the LCOV coverage receipt for this HEAD.",
            "verify": coverage_receipt_verify_command(mode, coverage, gate_command),
            "receipt": coverage_baseline_command(coverage, false)
        }));
    }

    if !live_codecov_policy_is_available(coverage) {
        return blockers;
    }

    let policy = &coverage.patch_policy;
    let target_ok = policy.target.as_deref() == Some("95%");
    let threshold_ok = policy.threshold.as_deref() == Some("0%");
    let blocking_ok = patch_policy_is_blocking(policy);
    let ci_failed_ok = policy.if_ci_failed.as_deref() == Some("error");
    if !(target_ok && threshold_ok && blocking_ok && ci_failed_ok) {
        blockers.push(json!({
            "kind": "patch_coverage_policy_not_enforcing",
            "path": coverage.codecov_config,
            "target": policy.target.clone(),
            "threshold": policy.threshold.clone(),
            "informational": policy.informational,
            "if_ci_failed": policy.if_ci_failed.clone(),
            "repair": "Set codecov.yml coverage.status.patch.default to target 95%, threshold 0%, if_ci_failed error, and do not mark it informational.",
            "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
            "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
        }));
    }
    blockers
}

fn coverage_receipt_verify_command(
    mode: &str,
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> String {
    if mode == "enforce" {
        quality_gate_command(mode, gate_command, Some(coverage), true)
    } else {
        coverage_baseline_command(coverage, true)
    }
}

fn patch_policy_is_blocking(policy: &CodecovStatusPolicy) -> bool {
    !matches!(policy.informational, Some(true))
}

fn patch_policy_is_enforcing(policy: &CodecovStatusPolicy) -> bool {
    policy.target.as_deref() == Some("95%")
        && policy.threshold.as_deref() == Some("0%")
        && patch_policy_is_blocking(policy)
        && policy.if_ci_failed.as_deref() == Some("error")
}

fn project_coverage_policy_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    if !live_codecov_policy_is_available(coverage)
        || project_policy_is_final(&coverage.project_policy)
    {
        return Vec::new();
    }

    let policy = &coverage.project_policy;
    vec![json!({
        "kind": "project_coverage_policy_not_enforcing",
        "path": coverage.codecov_config,
        "target": policy.target.clone(),
        "threshold": policy.threshold.clone(),
        "informational": policy.informational,
        "if_ci_failed": policy.if_ci_failed.clone(),
        "repair": "Promote codecov.yml coverage.status.project.default to target 95%, threshold 0.25%, if_ci_failed error, and remove informational mode before relying on full enforce.",
        "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
        "receipt": quality_gate_command("enforce", gate_command, Some(coverage), false)
    })]
}

fn project_policy_is_final(policy: &CodecovStatusPolicy) -> bool {
    policy.target.as_deref() == Some("95%")
        && policy.threshold.as_deref() == Some("0.25%")
        && patch_policy_is_blocking(policy)
        && policy.if_ci_failed.as_deref() == Some("error")
}

fn coverage_scope_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
) -> Vec<Value> {
    let contract = coverage_scope_contract(&coverage.coverage_scope);
    if coverage.status != "present" || contract.is_workspace {
        return Vec::new();
    }

    vec![json!({
        "kind": "coverage_scope_not_workspace",
        "path": coverage.receipt,
        "reason": contract.reason,
        "scope": coverage.coverage_scope.clone(),
        "receipt_required_roots": contract.receipt_required_roots,
        "current_required_roots": contract.current_required_roots,
        "missing_current_roots": contract.missing_current_roots,
        "extra_receipt_required_roots": contract.extra_receipt_required_roots,
        "repair": "Regenerate LCOV from the workspace coverage command so final project coverage includes production crates and the proof rail.",
        "suggested_test": "Run workspace-scoped coverage before promoting project coverage to blocking; parser-only LCOV cannot prove repo-wide coverage.",
        "verify": quality_gate_command("enforce", gate_command, Some(coverage), true),
        "receipt": coverage_baseline_command(coverage, false)
    })]
}

fn coverage_scope_is_workspace(scope: &Value) -> bool {
    coverage_scope_contract(scope).is_workspace
}

struct CoverageScopeContract {
    is_workspace: bool,
    reason: String,
    receipt_required_roots: Vec<String>,
    current_required_roots: Vec<String>,
    missing_current_roots: Vec<String>,
    extra_receipt_required_roots: Vec<String>,
}

fn coverage_scope_contract(scope: &Value) -> CoverageScopeContract {
    let Ok(required_roots) = required_coverage_roots() else {
        return CoverageScopeContract {
            is_workspace: false,
            reason: "current_required_roots_unavailable".to_string(),
            receipt_required_roots: string_array_value(scope.get("required_roots"))
                .unwrap_or_default(),
            current_required_roots: Vec::new(),
            missing_current_roots: Vec::new(),
            extra_receipt_required_roots: Vec::new(),
        };
    };
    let receipt_required_roots =
        string_array_value(scope.get("required_roots")).unwrap_or_default();
    let receipt_roots = string_array_value(scope.get("roots")).unwrap_or_default();
    let missing_current_roots = required_roots
        .iter()
        .filter(|required| !receipt_roots.iter().any(|root| root == *required))
        .cloned()
        .collect::<Vec<_>>();
    let extra_receipt_required_roots = receipt_required_roots
        .iter()
        .filter(|receipt| !required_roots.iter().any(|root| root == *receipt))
        .cloned()
        .collect::<Vec<_>>();
    let kind_is_workspace = scope.get("kind").and_then(Value::as_str) == Some("workspace");
    let receipt_missing_roots_empty =
        scope.get("missing_required_roots").and_then(Value::as_array).is_some_and(Vec::is_empty);
    let receipt_required_roots_current = receipt_required_roots == required_roots;
    let current_roots_present = missing_current_roots.is_empty();
    let is_workspace = kind_is_workspace
        && receipt_missing_roots_empty
        && receipt_required_roots_current
        && current_roots_present;
    let reason = if !kind_is_workspace {
        "scope_kind_not_workspace"
    } else if !receipt_missing_roots_empty {
        "receipt_missing_required_roots"
    } else if !receipt_required_roots_current {
        "stale_required_roots"
    } else if !current_roots_present {
        "missing_current_workspace_roots"
    } else {
        "present"
    };
    CoverageScopeContract {
        is_workspace,
        reason: reason.to_string(),
        receipt_required_roots,
        current_required_roots: required_roots,
        missing_current_roots,
        extra_receipt_required_roots,
    }
}

fn string_array_value(value: Option<&Value>) -> Option<Vec<String>> {
    let mut items = value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Option<Vec<_>>>()?;
    items.sort();
    items.dedup();
    Some(items)
}

fn codecov_comment_policy_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
) -> Vec<Value> {
    if !live_codecov_policy_is_available(coverage)
        || codecov_comment_is_actionable(&coverage.codecov_comment)
    {
        return Vec::new();
    }

    vec![json!({
        "kind": "codecov_comment_not_actionable",
        "path": coverage.codecov_config,
        "layout": coverage.codecov_comment.layout.clone(),
        "require_head": coverage.codecov_comment.require_head,
        "repair": "Set codecov.yml comment.layout to include diff and files, and keep require_head true so patch coverage failures include changed-line and file guidance.",
        "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
    })]
}

fn codecov_comment_is_actionable(policy: &CodecovCommentPolicy) -> bool {
    policy.layout.iter().any(|part| part == "diff")
        && policy.layout.iter().any(|part| part == "files")
        && policy.require_head == Some(true)
}

fn live_codecov_policy_is_available(coverage: &CoverageGateState) -> bool {
    coverage.codecov_config_status == "present"
}

fn patch_coverage_value_blockers(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
    require_patch_value: bool,
) -> Vec<Value> {
    let mut blockers = Vec::new();
    if coverage.status != "present" {
        return blockers;
    }
    match coverage.patch {
        Some(patch) if patch >= COVERAGE_TARGET => {}
        Some(patch) => {
            blockers.push(patch_coverage_below_target_action(coverage, gate_command, mode, patch))
        }
        None if patch_status_is_external(coverage) => {}
        None if require_patch_value => {
            blockers.push(patch_coverage_unknown_action(coverage, gate_command, mode))
        }
        None => {}
    }
    blockers
}

fn patch_status_is_external(coverage: &CoverageGateState) -> bool {
    coverage.patch_source.as_deref() == Some("codecov_status")
}

fn patch_coverage_unknown_action(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
) -> Value {
    json!({
        "kind": "patch_coverage_unknown",
        "path": coverage.receipt,
        "repair": "Provide the Codecov patch coverage percentage or explicitly name the external Codecov status source before relying on the patch gate.",
        "guidance_status": "patch_coverage_value_required",
        "guidance_path": coverage.codecov_config,
        "guidance_repair": "Read the Codecov patch status for this PR and rerun quality-gate with --patch-coverage <percent>, or rerun with --patch-status-source codecov when Codecov is the required external status.",
        "guidance_verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "guidance_receipt": quality_gate_command(mode, gate_command, Some(coverage), false),
        "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
    })
}

fn patch_coverage_below_target_action(
    coverage: &CoverageGateState,
    gate_command: &QualityGateCommandState,
    mode: &str,
    patch: f64,
) -> Value {
    let mut action = json!({
        "kind": "patch_coverage_below_target",
        "path": coverage.receipt,
        "current": patch,
        "target": COVERAGE_TARGET,
        "source": coverage.patch_source.clone(),
        "top_files": coverage.files_below_target.clone(),
        "repair": "Add behavior tests for the changed code until patch coverage is at least 95%.",
        "suggested_test": COVERAGE_GAP_SUGGESTED_TEST,
        "verify": quality_gate_command(mode, gate_command, Some(coverage), true),
        "receipt": quality_gate_command(mode, gate_command, Some(coverage), false)
    });

    if coverage.files_below_target.is_empty()
        && let Some(object) = action.as_object_mut()
    {
        object.insert(
            "guidance_status".to_string(),
            Value::String("codecov_diff_files_required".to_string()),
        );
        object.insert("guidance_path".to_string(), Value::String(coverage.codecov_config.clone()));
        object.insert(
            "guidance_repair".to_string(),
            Value::String(
                "Open the Codecov patch diff/files report for this PR and add focused behavior tests for the changed uncovered lines."
                    .to_string(),
            ),
        );
        object.insert(
            "guidance_verify".to_string(),
            Value::String(quality_gate_command(mode, gate_command, Some(coverage), true)),
        );
        object.insert(
            "guidance_receipt".to_string(),
            Value::String(quality_gate_command(mode, gate_command, Some(coverage), false)),
        );
    }

    action
}

fn render_quality_gate_markdown(packet: &QualityGateReceipt) -> String {
    let mut out = String::new();
    out.push_str("# Quality Gate Summary\n\n");
    out.push_str(&format!("- mode: `{}`\n", packet.mode));
    out.push_str(&format!("- decision: `{}`\n", packet.decision));
    out.push_str(&format!("- head: `{}`\n", packet.head.as_deref().unwrap_or("unknown")));
    out.push_str(&format!("- ripr+ unresolved: {}\n", format_u64(packet.ripr_plus.unresolved)));
    out.push_str(&format!("- new ripr+ gaps: {}\n", format_u64(packet.ripr_pr.new_unresolved)));
    out.push_str(&format!(
        "- Codecov config: {} `{}`\n",
        packet.coverage.codecov_config_status, packet.coverage.codecov_config
    ));
    out.push_str(&format!(
        "- Codecov patch coverage: {} / {:.1}%\n",
        format_percent(packet.coverage.patch),
        packet.coverage.target
    ));
    if let Some(source) = &packet.coverage.patch_source {
        out.push_str(&format!("- Codecov patch source: `{source}`\n"));
    }
    out.push_str(&format!(
        "- Codecov patch policy: target {}, threshold {}, {}\n",
        packet.coverage.patch_policy.target.as_deref().unwrap_or("unknown"),
        packet.coverage.patch_policy.threshold.as_deref().unwrap_or("unknown"),
        if matches!(packet.coverage.patch_policy.informational, Some(true)) {
            "informational"
        } else {
            "blocking"
        }
    ));
    out.push_str(&format!(
        "- Codecov project policy: target {}, threshold {}, {}\n",
        packet.coverage.project_policy.target.as_deref().unwrap_or("unknown"),
        packet.coverage.project_policy.threshold.as_deref().unwrap_or("unknown"),
        if matches!(packet.coverage.project_policy.informational, Some(true)) {
            "informational"
        } else {
            "blocking"
        }
    ));
    out.push_str(&format!(
        "- Codecov project coverage: {} / {:.1}%\n",
        format_percent(packet.coverage.project),
        packet.coverage.target
    ));
    out.push_str(&format!(
        "- Coverage scope: `{}`\n\n",
        coverage_scope_kind(&packet.coverage.coverage_scope)
    ));

    out.push_str(&render_quality_gate_matrix(packet));
    out.push_str(&render_claim_boundary(packet));
    out.push_str(&render_pr_summary_guidance(packet));

    out.push_str("## Temporary Exceptions\n\n");
    if packet.exceptions.status != "present" {
        out.push_str(&format!(
            "- exception ledger `{}` is `{}`\n\n",
            packet.exceptions.path, packet.exceptions.status
        ));
    } else if packet.exceptions.active.is_empty() {
        out.push_str("- none\n\n");
    } else {
        out.push_str("| ID | Applies to | Owner | Final target | Expires | Removal criteria |\n");
        out.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for exception in &packet.exceptions.active {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                md_cell(&exception.id),
                md_cell(&exception.applies_to),
                md_cell(&exception.owner),
                md_cell(&exception.final_target),
                md_cell(&exception.expires),
                md_cell(&exception.removal_criteria),
            ));
        }
        out.push_str("\nThese entries document transition debt only; they do not waive `quality-gate --mode enforce` blockers.\n\n");
    }

    out.push_str("## Blocking Failures\n\n");
    let blocking_actions: Vec<&Value> =
        packet.next_actions.iter().filter(|action| is_blocking_action(action)).collect();
    if packet.decision != "fail" || blocking_actions.is_empty() {
        out.push_str("- none\n\n");
    } else {
        for action in blocking_actions.iter().take(5) {
            out.push_str(&format_action(action));
        }
        out.push('\n');
    }

    out.push_str("## RIPR Review Guidance\n\n");
    if packet.review_guidance.top_gaps.is_empty() {
        out.push_str(&format!(
            "- no file/line guidance available\n- refresh review guidance receipt: `{}`\n- verify review guidance receipt: `{}`\n\n",
            ripr_review_command(&packet.ripr_pr, false),
            ripr_review_command(&packet.ripr_pr, true)
        ));
    } else {
        out.push_str("| Gap | File | Line | Seam | Reason | Suggested proof |\n");
        out.push_str("| --- | --- | ---: | --- | --- | --- |\n");
        for gap in &packet.review_guidance.top_gaps {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                md_cell(action_string(gap, "gap_id").as_deref().unwrap_or("unknown")),
                md_cell(action_string(gap, "path").as_deref().unwrap_or("unknown")),
                action_u64(gap, "line").map_or_else(|| "-".to_string(), |line| line.to_string()),
                md_cell(action_string(gap, "seam").as_deref().unwrap_or("unknown")),
                md_cell(action_string(gap, "reason").as_deref().unwrap_or("unspecified")),
                md_cell(
                    action_string(gap, "suggested_test")
                        .as_deref()
                        .unwrap_or("add a focused behavior test for this seam")
                ),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Next Actions\n\n");
    if packet.next_actions.is_empty() {
        out.push_str("- no action required by this gate\n");
    } else {
        for action in packet.next_actions.iter().take(6) {
            out.push_str(&format_action(action));
        }
    }
    out
}

fn render_pr_summary_guidance(packet: &QualityGateReceipt) -> String {
    let mut out = String::new();
    out.push_str("## PR Summary Guidance\n\n");
    out.push_str("Before requesting review, include a proof block in the PR body with:\n\n");
    out.push_str("- Objective: one sentence naming the proof target or burn-down cluster.\n");
    out.push_str("- Claim boundary: state what this proves and what it does not prove.\n");
    out.push_str(
        "- Non-goals: explicitly note no LSP 3.18 behavior work, unless the PR is in that lane.\n",
    );
    out.push_str("- RIPR/coverage effect: new-gap count, total count movement, patch/project value, uncovered file, or receipt path.\n");
    out.push_str("- Local proof commands: paste the commands run and their pass/fail result.\n");
    out.push_str("- Cleanup performed: state `rtk git status --short --branch`, `rtk git diff --check`, and `rtk bash scripts/storage-doctor` results, plus whether generated `target/receipts/quality/*` artifacts are uncommitted or uploaded only.\n");
    out.push_str("- What remains: name any advisory burn-down debt still covered by `policy/quality-gate-exceptions.toml`.\n\n");
    out.push_str("Suggested local proof commands for this gate:\n\n");
    for command in local_proof_commands(packet) {
        out.push_str(&format!("- `{command}`\n"));
    }
    out.push('\n');
    out
}

fn render_claim_boundary(packet: &QualityGateReceipt) -> String {
    let mut out = String::new();
    out.push_str("## Claim Boundary\n\n");
    for claim in &packet.claim_boundary {
        out.push_str(&format!("- {claim}\n"));
    }
    out.push('\n');
    out
}

fn render_quality_gate_matrix(packet: &QualityGateReceipt) -> String {
    let mut out = String::new();
    out.push_str("## Quality Gates\n\n");
    out.push_str("| Gate | Status | Current | Target | Blocking |\n");
    out.push_str("| --- | --- | ---: | ---: | --- |\n");
    out.push_str(&quality_gate_row(
        "ripr+ zero",
        zero_target_status(packet.ripr_plus.unresolved),
        &format_u64(packet.ripr_plus.unresolved),
        "0",
        final_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "new ripr+ gaps",
        zero_target_status(packet.ripr_pr.new_unresolved),
        &format_u64(packet.ripr_pr.new_unresolved),
        "0",
        new_ripr_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "RIPR+ receipt",
        receipt_presence_status(&packet.ripr_plus.status),
        &packet.ripr_plus.status,
        "present",
        new_ripr_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "RIPR PR receipt",
        receipt_presence_status(&packet.ripr_pr.status),
        &packet.ripr_pr.status,
        "present",
        new_ripr_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "RIPR review guidance",
        review_guidance_gate_status(packet),
        &review_guidance_current(packet),
        "present/actionable",
        new_ripr_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov patch coverage",
        patch_coverage_status(&packet.coverage),
        &patch_coverage_current(&packet.coverage),
        &format!("{:.1}%", packet.coverage.target),
        patch_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov config",
        if packet.coverage.codecov_config_status == "present" { "pass" } else { "fail" },
        &packet.coverage.codecov_config_status,
        "present",
        patch_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov patch policy",
        if patch_policy_is_enforcing(&packet.coverage.patch_policy) { "pass" } else { "fail" },
        &format!(
            "target {}, threshold {}",
            packet.coverage.patch_policy.target.as_deref().unwrap_or("unknown"),
            packet.coverage.patch_policy.threshold.as_deref().unwrap_or("unknown")
        ),
        "95% / 0%",
        patch_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov failure guidance",
        if codecov_comment_is_actionable(&packet.coverage.codecov_comment) {
            "pass"
        } else {
            "fail"
        },
        &format!(
            "layout {}",
            if packet.coverage.codecov_comment.layout.is_empty() {
                "unknown".to_string()
            } else {
                packet.coverage.codecov_comment.layout.join(",")
            }
        ),
        "diff + files",
        patch_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov project policy",
        if project_policy_is_final(&packet.coverage.project_policy) { "pass" } else { "fail" },
        &format!(
            "target {}, threshold {}",
            packet.coverage.project_policy.target.as_deref().unwrap_or("unknown"),
            packet.coverage.project_policy.threshold.as_deref().unwrap_or("unknown")
        ),
        "95% / 0.25%",
        final_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Codecov project coverage",
        percent_target_status(packet.coverage.project, packet.coverage.target),
        &format_percent(packet.coverage.project),
        &format!("{:.1}%", packet.coverage.target),
        final_blocking_posture(packet),
    ));
    out.push_str(&quality_gate_row(
        "Coverage scope",
        if coverage_scope_is_workspace(&packet.coverage.coverage_scope) { "pass" } else { "fail" },
        &coverage_scope_current(&packet.coverage.coverage_scope),
        "workspace",
        final_blocking_posture(packet),
    ));
    out.push('\n');
    out
}

fn coverage_scope_kind(scope: &Value) -> &str {
    scope.get("kind").and_then(Value::as_str).unwrap_or("unknown")
}

fn coverage_scope_current(scope: &Value) -> String {
    let contract = coverage_scope_contract(scope);
    let kind = coverage_scope_kind(scope);
    if contract.is_workspace {
        return kind.to_string();
    }

    let mut details = vec![contract.reason];
    if !contract.missing_current_roots.is_empty() {
        details.push(format!("missing {} current roots", contract.missing_current_roots.len()));
    }
    if !contract.extra_receipt_required_roots.is_empty() {
        details
            .push(format!("extra {} receipt roots", contract.extra_receipt_required_roots.len()));
    } else if let Some(missing) = string_array_value(scope.get("missing_required_roots"))
        && !missing.is_empty()
    {
        details.push(format!("receipt missing {} required roots", missing.len()));
    }

    format!("{kind} ({})", details.join("; "))
}

fn local_proof_commands(packet: &QualityGateReceipt) -> Vec<String> {
    let mut commands = Vec::new();
    let gate_command = QualityGateCommandState {
        ripr_receipt: packet.ripr_plus.receipt.clone(),
        ripr_pr_receipt: packet.ripr_pr.receipt.clone(),
        review_receipt: packet.review_guidance.receipt.clone(),
        coverage_receipt: packet.coverage.receipt.clone(),
        codecov: packet.coverage.codecov_config.clone(),
        exceptions: packet.exceptions.path.clone(),
        receipt: packet.receipt.clone(),
        summary: packet.summary.clone(),
    };
    match packet.mode.as_str() {
        "enforce-new-ripr" => {
            commands.push(ripr_plus_command(&packet.ripr_plus, true));
            commands.push(ripr_pr_command(&packet.ripr_pr, true));
            commands.push(ripr_review_command(&packet.ripr_pr, true));
            commands.push(quality_gate_command(
                "enforce-new-ripr",
                &gate_command,
                Some(&packet.coverage),
                true,
            ));
        }
        "enforce-patch-coverage" => {
            commands.push(coverage_baseline_command(&packet.coverage, true));
            commands.push(quality_gate_command(
                "enforce-patch-coverage",
                &gate_command,
                Some(&packet.coverage),
                true,
            ));
        }
        "enforce" => {
            commands.push(ripr_plus_command(&packet.ripr_plus, true));
            commands.push(ripr_pr_command(&packet.ripr_pr, true));
            commands.push(ripr_review_command(&packet.ripr_pr, true));
            commands.push(coverage_baseline_command(&packet.coverage, true));
            commands.push(quality_gate_command(
                "enforce",
                &gate_command,
                Some(&packet.coverage),
                true,
            ));
        }
        _ => {
            commands.push(coverage_baseline_command(&packet.coverage, false));
            commands.push(ripr_plus_command(&packet.ripr_plus, false));
            commands.push(ripr_pr_command(&packet.ripr_pr, false));
            commands.push(ripr_review_command(&packet.ripr_pr, false));
            commands.push(quality_gate_command(
                "advisory",
                &gate_command,
                Some(&packet.coverage),
                true,
            ));
        }
    }
    commands.push(local_command("git status --short --branch"));
    commands.push(local_command("git diff --check"));
    commands.push(local_command("bash scripts/storage-doctor"));
    commands
}

fn local_command(command: impl AsRef<str>) -> String {
    format!("{LOCAL_COMMAND_PREFIX} {}", command.as_ref())
}

fn quality_gate_command(
    mode: &str,
    gate_command: &QualityGateCommandState,
    coverage: Option<&CoverageGateState>,
    check: bool,
) -> String {
    let mut command = local_command(format!("cargo xtask quality-gate --mode {mode}"));
    command.push_str(&format!(" --ripr-receipt {}", command_arg(&gate_command.ripr_receipt)));
    command.push_str(&format!(" --ripr-pr-receipt {}", command_arg(&gate_command.ripr_pr_receipt)));
    command.push_str(&format!(" --review-receipt {}", command_arg(&gate_command.review_receipt)));
    command
        .push_str(&format!(" --coverage-receipt {}", command_arg(&gate_command.coverage_receipt)));
    if !gate_command.codecov.trim().is_empty() {
        command.push_str(&format!(" --codecov {}", command_arg(&gate_command.codecov)));
    }
    command.push_str(&format!(" --exceptions {}", command_arg(&gate_command.exceptions)));
    if let Some(coverage) = coverage
        && coverage.patch_source.as_deref() == Some("cli")
        && let Some(patch) = coverage.patch
    {
        command.push_str(&format!(" --patch-coverage {patch:.2}"));
    } else if let Some(coverage) = coverage
        && patch_status_is_external(coverage)
    {
        command.push_str(" --patch-status-source codecov");
    }
    command.push_str(&format!(" --receipt {}", command_arg(&gate_command.receipt)));
    if let Some(summary) = &gate_command.summary {
        command.push_str(&format!(" --summary {}", command_arg(summary)));
    }
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_plus_command(ripr: &RiprGateState, check: bool) -> String {
    let mut command =
        local_command(format!("cargo xtask ripr-plus --receipt {}", command_arg(&ripr.receipt)));
    if check {
        command.push_str(" --check");
    }
    command
}

fn coverage_baseline_command(coverage: &CoverageGateState, check: bool) -> String {
    let lcov = coverage.lcov.as_deref().unwrap_or("target/lcov.info");
    let mut command = local_command(format!(
        "cargo xtask coverage-baseline --lcov {} --receipt {}",
        command_arg(lcov),
        command_arg(&coverage.receipt)
    ));
    if !coverage.codecov_config.trim().is_empty() {
        command.push_str(&format!(" --codecov {}", command_arg(&coverage.codecov_config)));
    }
    if check {
        command.push_str(" --check");
    }
    command
}

fn quality_gate_row(
    gate: &str,
    status: &str,
    current: &str,
    target: &str,
    blocking: &str,
) -> String {
    format!(
        "| {} | {} | {} | {} | {} |\n",
        md_cell(gate),
        md_cell(status),
        md_cell(current),
        md_cell(target),
        md_cell(blocking)
    )
}

fn zero_target_status(value: Option<u64>) -> &'static str {
    match value {
        Some(0) => "pass",
        Some(_) => "fail",
        None => "unknown",
    }
}

fn percent_target_status(value: Option<f64>, target: f64) -> &'static str {
    match value {
        Some(value) if value >= target => "pass",
        Some(_) => "fail",
        None => "unknown",
    }
}

fn receipt_presence_status(status: &str) -> &'static str {
    if status == "present" { "pass" } else { "fail" }
}

fn review_guidance_gate_status(packet: &QualityGateReceipt) -> &'static str {
    if packet.review_guidance.status != "present" {
        return "fail";
    }
    if packet.ripr_pr.new_unresolved.is_some_and(|count| count > 0)
        && packet.review_guidance.top_gaps.is_empty()
    {
        "fail"
    } else {
        "pass"
    }
}

fn review_guidance_current(packet: &QualityGateReceipt) -> String {
    if packet.review_guidance.status == "present"
        && packet.ripr_pr.new_unresolved.is_some_and(|count| count > 0)
        && packet.review_guidance.top_gaps.is_empty()
    {
        "present/no_top_gaps".to_string()
    } else {
        packet.review_guidance.status.clone()
    }
}

fn final_blocking_posture(packet: &QualityGateReceipt) -> &'static str {
    if packet.mode == "enforce" { "yes" } else { "burn-down" }
}

fn patch_coverage_status(coverage: &CoverageGateState) -> &'static str {
    if patch_status_is_external(coverage) {
        "external"
    } else {
        percent_target_status(coverage.patch, coverage.target)
    }
}

fn patch_coverage_current(coverage: &CoverageGateState) -> String {
    if patch_status_is_external(coverage) {
        "Codecov status".to_string()
    } else {
        format_percent(coverage.patch)
    }
}

fn new_ripr_blocking_posture(packet: &QualityGateReceipt) -> &'static str {
    if matches!(packet.mode.as_str(), "enforce-new-ripr" | "enforce") { "yes" } else { "advisory" }
}

fn patch_blocking_posture(packet: &QualityGateReceipt) -> &'static str {
    if matches!(packet.mode.as_str(), "enforce-patch-coverage" | "enforce") {
        "yes"
    } else {
        "advisory"
    }
}

fn is_blocking_action(action: &Value) -> bool {
    action.get("blocking").and_then(Value::as_bool) == Some(true)
}

fn format_action(action: &Value) -> String {
    let kind = action_string(action, "kind").unwrap_or_else(|| "unknown".to_string());
    let mut out = format!("- `{kind}`");
    if let Some(path) = action_string(action, "path") {
        out.push_str(&format!(" path `{path}`"));
    }
    if let Some(unresolved) = action_u64(action, "unresolved") {
        out.push_str(&format!(" unresolved `{unresolved}`"));
    }
    if let Some(current) = action_f64(action, "current") {
        out.push_str(&format!(" current `{current:.2}`"));
    }
    if let Some(target) = action_f64(action, "target") {
        out.push_str(&format!(" target `{target:.2}`"));
    }
    if let Some(source) = action_string(action, "source") {
        out.push_str(&format!(" source `{source}`"));
    }
    if let Some(reason) = action_string(action, "reason") {
        out.push_str(&format!(" reason `{reason}`"));
    }
    if let Some(expected) = action_string(action, "expected_head") {
        out.push_str(&format!(" expected-head `{expected}`"));
    }
    if let Some(actual) = action_string(action, "receipt_head") {
        out.push_str(&format!(" receipt-head `{actual}`"));
    }
    if let Some(expected) = action_string(action, "expected_head_sha") {
        out.push_str(&format!(" expected-head `{expected}`"));
    }
    if let Some(actual) = action_string(action, "receipt_head_sha") {
        out.push_str(&format!(" receipt-head `{actual}`"));
    }
    if let Some(base) = action_string(action, "receipt_base") {
        out.push_str(&format!(" receipt-base `{base}`"));
    }
    if let Some(base_sha) = action_string(action, "receipt_base_sha") {
        out.push_str(&format!(" receipt-base-sha `{base_sha}`"));
    }
    if let Some(expected_base_sha) = action_string(action, "expected_base_sha") {
        out.push_str(&format!(" expected-base-sha `{expected_base_sha}`"));
    }
    out.push('\n');
    if let Some(scope) = action.get("scope")
        && let Some(summary) = format_scope(scope)
    {
        out.push_str(&format!("  scope: {summary}\n"));
    }
    if let Some(roots) = string_array_value(action.get("missing_current_roots"))
        && !roots.is_empty()
    {
        out.push_str(&format!("  missing current roots: `{}`\n", roots.join("`, `")));
    }
    if let Some(roots) = string_array_value(action.get("extra_receipt_required_roots"))
        && !roots.is_empty()
    {
        out.push_str(&format!("  extra receipt roots: `{}`\n", roots.join("`, `")));
    }
    if let Some(roots) = string_array_value(action.get("current_required_roots"))
        && !roots.is_empty()
    {
        out.push_str(&format!("  current required roots: `{}`\n", roots.join("`, `")));
    }
    if let Some(repair) = action_string(action, "repair") {
        out.push_str(&format!("  repair: {repair}\n"));
    }
    if let Some(suggested_test) = action_string(action, "suggested_test") {
        out.push_str(&format!("  suggested test: {suggested_test}\n"));
    }
    if let Some(verify) = action_string(action, "verify") {
        out.push_str(&format!("  verify: `{verify}`\n"));
    }
    if let Some(receipt) = action_string(action, "receipt") {
        out.push_str(&format!("  receipt: `{receipt}`\n"));
    }
    if let Some(guidance_status) = action_string(action, "guidance_status") {
        let guidance_path =
            action_string(action, "guidance_path").unwrap_or_else(|| "unknown".to_string());
        out.push_str(&format!("  guidance: `{guidance_status}` at `{guidance_path}`\n"));
    }
    if let Some(guidance_repair) = action_string(action, "guidance_repair") {
        out.push_str(&format!("  guidance repair: {guidance_repair}\n"));
    }
    if let Some(guidance_verify) = action_string(action, "guidance_verify") {
        out.push_str(&format!("  guidance verify: `{guidance_verify}`\n"));
    }
    if let Some(guidance_receipt) = action_string(action, "guidance_receipt") {
        out.push_str(&format!("  guidance receipt: `{guidance_receipt}`\n"));
    }
    if let Some(warnings) = action.get("warnings").and_then(Value::as_array)
        && !warnings.is_empty()
    {
        out.push_str("  warnings:\n");
        for warning in warnings.iter().take(3) {
            out.push_str(&format!("  - {}\n", format_warning(warning)));
        }
    }
    if let Some(gaps) = action.get("top_gaps").and_then(Value::as_array)
        && !gaps.is_empty()
    {
        out.push_str("  top gaps:\n");
        for gap in gaps.iter().take(3) {
            let gap_id = action_string(gap, "gap_id").unwrap_or_else(|| "unknown".to_string());
            let path = action_string(gap, "path").unwrap_or_else(|| "unknown".to_string());
            let line =
                action_u64(gap, "line").map_or_else(|| "-".to_string(), |line| line.to_string());
            let seam = action_string(gap, "seam").unwrap_or_else(|| "unknown".to_string());
            let reason = action_string(gap, "reason").unwrap_or_else(|| "unspecified".to_string());
            let suggested_test = action_string(gap, "suggested_test")
                .unwrap_or_else(|| "add a focused behavior test for this seam".to_string());
            out.push_str(&format!(
                "  - `{gap_id}` `{path}:{line}` seam `{seam}` {reason}; suggested test: {suggested_test}\n"
            ));
        }
    }
    if let Some(samples) = action.get("sample_seams").and_then(Value::as_array)
        && !samples.is_empty()
    {
        out.push_str("  sample seams:\n");
        format_sample_seams(&mut out, samples, "  ");
    }
    if let Some(files) = action.get("top_files").and_then(Value::as_array)
        && !files.is_empty()
    {
        out.push_str("  top files:\n");
        for file in files.iter().take(3) {
            let path = action_string(file, "path")
                .or_else(|| action_string(file, "name"))
                .unwrap_or_else(|| "unknown".to_string());
            if let Some(coverage) = action_f64(file, "line_coverage") {
                out.push_str(&format!("  - `{path}` line coverage {coverage:.2}%\n"));
            } else if let Some(unresolved) = action_u64(file, "count") {
                out.push_str(&format!("  - `{path}` unresolved {unresolved}\n"));
            } else {
                out.push_str(&format!("  - `{path}`\n"));
            }
            if let Some(lines) = format_sample_uncovered_lines(file) {
                out.push_str(&format!("    sample uncovered lines: {lines}\n"));
            }
            if let Some(samples) = file.get("sample_seams").and_then(Value::as_array)
                && !samples.is_empty()
            {
                format_sample_seams(&mut out, samples, "    ");
            }
        }
    }
    if let Some(files) = action.get("deferred_files").and_then(Value::as_array)
        && !files.is_empty()
    {
        out.push_str("  deferred files:\n");
        for file in files.iter().take(3) {
            let path = action_string(file, "name").unwrap_or_else(|| "unknown".to_string());
            let reason = action_string(file, "reason").unwrap_or_else(|| "deferred".to_string());
            if let Some(unresolved) = action_u64(file, "count") {
                out.push_str(&format!("  - `{path}` unresolved {unresolved} ({reason})\n"));
            } else {
                out.push_str(&format!("  - `{path}` ({reason})\n"));
            }
        }
    }
    if let Some(guidance) = action.get("receipt_guidance").and_then(Value::as_array)
        && !guidance.is_empty()
    {
        out.push_str("  receipt guidance:\n");
        for item in guidance.iter().take(3) {
            let kind = action_string(item, "kind").unwrap_or_else(|| "unknown".to_string());
            let path = action_string(item, "path").unwrap_or_else(|| "unknown".to_string());
            let reason = action_string(item, "reason").unwrap_or_else(|| "unspecified".to_string());
            if let Some(unresolved) = action_u64(item, "unresolved") {
                out.push_str(&format!(
                    "  - `{kind}` `{path}` unresolved {unresolved} ({reason})\n"
                ));
            } else {
                out.push_str(&format!("  - `{kind}` `{path}` ({reason})\n"));
            }
            if let Some(repair) = action_string(item, "repair") {
                out.push_str(&format!("    repair: {repair}\n"));
            }
            if let Some(verify) = action_string(item, "verify") {
                out.push_str(&format!("    verify: `{verify}`\n"));
            }
            if let Some(receipt) = action_string(item, "receipt") {
                out.push_str(&format!("    receipt: `{receipt}`\n"));
            }
        }
    }
    out
}

fn format_scope(scope: &Value) -> Option<String> {
    let kind = scope.get("kind").and_then(Value::as_str).unwrap_or("unknown");
    let mut parts = vec![format!("kind `{kind}`")];
    if let Some(source_files) = action_u64(scope, "source_files") {
        parts.push(format!("source files `{source_files}`"));
    }
    if let Some(missing) = string_array_value(scope.get("missing_required_roots"))
        && !missing.is_empty()
    {
        parts.push(format!("missing required roots `{}`", missing.join("`, `")));
    }
    if let Some(roots) = string_array_value(scope.get("roots"))
        && !roots.is_empty()
    {
        parts.push(format!("observed roots `{}`", roots.join("`, `")));
    }
    Some(parts.join("; "))
}

fn format_warning(warning: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(kind) = action_string(warning, "kind") {
        parts.push(format!("kind `{kind}`"));
    }
    for key in [
        "id",
        "field",
        "value",
        "today",
        "must_be_on_or_after",
        "evidence",
        "expected",
        "actual",
        "repair",
    ] {
        if let Some(value) = action_string(warning, key) {
            parts.push(format!("{key} `{value}`"));
        } else if let Some(value) = action_u64(warning, key) {
            parts.push(format!("{key} `{value}`"));
        }
    }
    if parts.is_empty() { warning.to_string() } else { parts.join(" ") }
}

fn format_sample_seams(out: &mut String, samples: &[Value], indent: &str) {
    for sample in samples.iter().take(3) {
        let gap_id = action_string(sample, "gap_id").unwrap_or_else(|| "unknown".to_string());
        let line =
            action_u64(sample, "line").map_or_else(|| "-".to_string(), |line| line.to_string());
        let kind = action_string(sample, "kind").unwrap_or_else(|| "unknown".to_string());
        let seam = action_string(sample, "seam");
        let reason = action_string(sample, "reason");
        let suggested_test = action_string(sample, "suggested_test");
        let mut details = format!("{indent}- `{gap_id}` line `{line}` kind `{kind}`");
        if let Some(seam) = seam {
            details.push_str(&format!(" seam `{seam}`"));
        }
        if let Some(reason) = reason {
            details.push_str(&format!(" reason {reason}"));
        }
        if let Some(suggested_test) = suggested_test {
            details.push_str(&format!("; suggested test: {suggested_test}"));
        }
        out.push_str(&details);
        out.push('\n');
    }
}

fn format_sample_uncovered_lines(file: &Value) -> Option<String> {
    let lines = positive_sample_uncovered_lines(file)
        .into_iter()
        .take(5)
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if lines.is_empty() { None } else { Some(lines.join(", ")) }
}

fn action_string(action: &Value, key: &str) -> Option<String> {
    action.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn action_u64(action: &Value, key: &str) -> Option<u64> {
    action.get(key).and_then(Value::as_u64)
}

fn action_f64(action: &Value, key: &str) -> Option<f64> {
    action.get(key).and_then(Value::as_f64)
}

fn format_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

fn format_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| format!("{value:.2}%"))
}

fn md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn ripr_pr_command(ripr_pr: &RiprPrGateState, check: bool) -> String {
    let mut command = local_command(format!(
        "cargo xtask ripr-pr --base {} --head {}",
        ripr_base(ripr_pr),
        ripr_head(ripr_pr)
    ));
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_review_command(ripr_pr: &RiprPrGateState, check: bool) -> String {
    let mut command = local_command(format!(
        "cargo xtask ripr-review-comments --base {} --head {}",
        ripr_base(ripr_pr),
        ripr_head(ripr_pr)
    ));
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_base(ripr_pr: &RiprPrGateState) -> &str {
    ripr_pr.base.as_deref().unwrap_or("origin/HEAD")
}

fn ripr_head(ripr_pr: &RiprPrGateState) -> &str {
    ripr_pr.head.as_deref().unwrap_or("HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn advisory_action_predicates_match_enforcement_ownership() {
        assert!(advisory_ripr_receipt_gap_is_useful(QualityGateMode::Advisory));
        assert!(advisory_ripr_receipt_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_ripr_receipt_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_ripr_receipt_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_ripr_pr_receipt_gap_is_useful(QualityGateMode::Advisory));
        assert!(advisory_ripr_pr_receipt_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_ripr_pr_receipt_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_ripr_pr_receipt_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_ripr_review_guidance_gap_is_useful(QualityGateMode::Advisory));
        assert!(advisory_ripr_review_guidance_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_ripr_review_guidance_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_ripr_review_guidance_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_new_ripr_gap_unknown_is_useful(QualityGateMode::Advisory));
        assert!(advisory_new_ripr_gap_unknown_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_new_ripr_gap_unknown_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_new_ripr_gap_unknown_is_useful(QualityGateMode::Enforce));

        assert!(advisory_new_ripr_gap_is_useful(QualityGateMode::Advisory));
        assert!(!advisory_new_ripr_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_new_ripr_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_new_ripr_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_ripr_seam_clusters_are_useful(QualityGateMode::Advisory));
        assert!(advisory_ripr_seam_clusters_are_useful(QualityGateMode::EnforceNewRipr));
        assert!(advisory_ripr_seam_clusters_are_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_ripr_seam_clusters_are_useful(QualityGateMode::Enforce));

        assert!(advisory_coverage_receipt_gap_is_useful(QualityGateMode::Advisory));
        assert!(!advisory_coverage_receipt_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_coverage_receipt_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_coverage_receipt_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_codecov_config_gap_is_useful(QualityGateMode::Advisory));
        assert!(advisory_codecov_config_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(!advisory_codecov_config_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_codecov_config_gap_is_useful(QualityGateMode::Enforce));

        assert!(advisory_project_coverage_gap_is_useful(QualityGateMode::Advisory));
        assert!(advisory_project_coverage_gap_is_useful(QualityGateMode::EnforceNewRipr));
        assert!(advisory_project_coverage_gap_is_useful(QualityGateMode::EnforcePatchCoverage));
        assert!(!advisory_project_coverage_gap_is_useful(QualityGateMode::Enforce));
    }

    #[test]
    fn advisory_reports_missing_receipts_without_failing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        let config = QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        };

        run(config)?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("advisory"));
        assert_eq!(receipt.pointer("/ripr_plus/status").and_then(Value::as_str), Some("missing"));
        assert_eq!(receipt.pointer("/ripr_pr/status").and_then(Value::as_str), Some("missing"));
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("missing")
        );
        assert_eq!(receipt.pointer("/coverage/status").and_then(Value::as_str), Some("missing"));
        let review_action = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance repair action missing")?;
        assert_eq!(review_action.get("reason").and_then(Value::as_str), Some("missing"));
        assert!(review_action.get("verify").and_then(Value::as_str).is_some_and(|verify| {
            verify
                .starts_with("rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD")
                && verify.contains("--check")
        }));
        assert!(review_action.get("receipt").and_then(Value::as_str).is_some_and(|receipt| {
            receipt
                .starts_with("rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD")
                && !receipt.contains("--check")
        }));
        let markdown = fs::read_to_string(summary)?;
        let commands = local_proof_command_section(&markdown)?;
        for command in [
            "rtk cargo xtask coverage-baseline --lcov target/lcov.info",
            "rtk cargo xtask ripr-plus --receipt",
            "rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD",
            "rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD",
            "rtk cargo xtask quality-gate --mode advisory",
        ] {
            assert!(
                commands.contains(command),
                "advisory proof commands must include `{command}`: {commands}"
            );
        }
        assert_local_proof_commands_are_rtk_prefixed(&markdown)?;
        Ok(())
    }

    #[test]
    fn codecov_config_fallback_populates_policy_when_coverage_receipt_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let codecov = dir.path().join("codecov.yml");
        fs::write(
            &codecov,
            r#"coverage:
  status:
    project:
      default:
        target: 95%
        threshold: 2%
        if_ci_failed: error
        informational: true
    patch:
      default:
        target: 95%
        threshold: 0%
        if_ci_failed: error
comment:
  layout: "reach,diff,flags,files"
  behavior: default
  require_head: true
"#,
        )?;
        let mut coverage = coverage_receipt_state(&dir.path().join("missing-coverage.json"), None);

        apply_codecov_config_fallback(&mut coverage, &codecov);

        assert_eq!(coverage.status, "missing");
        assert_eq!(coverage.codecov_config_status, "present");
        assert_eq!(coverage.patch_policy.target.as_deref(), Some("95%"));
        assert_eq!(coverage.patch_policy.threshold.as_deref(), Some("0%"));
        assert_eq!(coverage.project_policy.threshold.as_deref(), Some("2%"));
        assert!(coverage.codecov_comment.layout.iter().any(|part| part == "diff"));
        assert!(coverage.codecov_comment.layout.iter().any(|part| part == "files"));
        assert_eq!(coverage.codecov_comment.require_head, Some(true));
        Ok(())
    }

    #[test]
    fn codecov_config_overrides_receipt_policy_snapshot() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage_path = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        write_coverage_receipt(&coverage_path, "95%", "0%", None)?;
        write_advisory_patch_codecov_config(&codecov)?;
        let mut coverage = coverage_receipt_state(&coverage_path, None);

        apply_codecov_config_fallback(&mut coverage, &codecov);

        assert_eq!(coverage.status, "present");
        assert_eq!(coverage.codecov_config_status, "present");
        assert_eq!(coverage.patch_policy.target.as_deref(), Some("75%"));
        assert_eq!(coverage.patch_policy.threshold.as_deref(), Some("2%"));
        assert_eq!(coverage.patch_policy.informational, Some(true));
        assert_eq!(coverage.codecov_comment.require_head, Some(true));
        Ok(())
    }

    #[test]
    fn codecov_config_blocker_names_missing_policy_file() -> TestResult {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("missing-codecov.yml");
        let mut coverage = coverage_receipt_state(&dir.path().join("missing-coverage.json"), None);

        apply_codecov_config_fallback(&mut coverage, &missing);

        let ripr = dir.path().join("custom-ripr-plus.json");
        let ripr_pr = dir.path().join("custom-ripr-pr.json");
        let review = dir.path().join("custom-review.json");
        let coverage_receipt = dir.path().join("missing-coverage.json");
        let exceptions = dir.path().join("custom-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        let gate_command = QualityGateCommandState {
            ripr_receipt: display_path(&ripr),
            ripr_pr_receipt: display_path(&ripr_pr),
            review_receipt: display_path(&review),
            coverage_receipt: display_path(&coverage_receipt),
            codecov: display_path(&missing),
            exceptions: display_path(&exceptions),
            receipt: display_path(&output),
            summary: Some(display_path(&summary)),
        };
        let blockers = codecov_config_blockers(&coverage, &gate_command, "enforce-patch-coverage");
        assert_eq!(blockers.len(), 1);
        assert_eq!(
            blockers[0].get("kind").and_then(Value::as_str),
            Some("codecov_config_not_current")
        );
        assert_eq!(blockers[0].get("reason").and_then(Value::as_str), Some("missing"));
        let verify = blockers[0].get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("missing-codecov.yml"));
        assert!(verify.contains(&format!("--ripr-receipt {}", display_path(&ripr))));
        assert!(verify.contains(&format!("--ripr-pr-receipt {}", display_path(&ripr_pr))));
        assert!(verify.contains(&format!("--review-receipt {}", display_path(&review))));
        assert!(
            verify.contains(&format!("--coverage-receipt {}", display_path(&coverage_receipt)))
        );
        assert!(verify.contains(&format!("--exceptions {}", display_path(&exceptions))));
        assert!(verify.contains("--receipt"));
        assert!(verify.contains("quality-gate.json"));
        assert!(verify.contains("--summary"));
        assert!(verify.contains("quality-gate.md"));
        assert!(verify.contains("--check"));
        Ok(())
    }

    #[test]
    fn quality_gate_command_quotes_configured_paths_with_spaces() {
        let gate_command = QualityGateCommandState {
            ripr_receipt: "target/quality receipts/ripr plus.json".to_string(),
            ripr_pr_receipt: "target/ripr/pr evidence/repo exposure.json".to_string(),
            review_receipt: "target/ripr/review comments/comments.json".to_string(),
            coverage_receipt: "target/quality receipts/coverage baseline.json".to_string(),
            codecov: "policy files/codecov policy.yml".to_string(),
            exceptions: "policy files/quality exceptions.toml".to_string(),
            receipt: "target/quality receipts/quality gate.json".to_string(),
            summary: Some("target/quality receipts/quality gate.md".to_string()),
        };
        let mut coverage = coverage_receipt_state(Path::new("missing coverage.json"), None);
        coverage.patch = Some(97.25);
        coverage.patch_source = Some("cli".to_string());

        let command =
            quality_gate_command("enforce-patch-coverage", &gate_command, Some(&coverage), true);

        assert!(command.contains("--ripr-receipt 'target/quality receipts/ripr plus.json'"));
        assert!(command.contains("--ripr-pr-receipt 'target/ripr/pr evidence/repo exposure.json'"));
        assert!(command.contains("--review-receipt 'target/ripr/review comments/comments.json'"));
        assert!(
            command.contains("--coverage-receipt 'target/quality receipts/coverage baseline.json'")
        );
        assert!(command.contains("--codecov 'policy files/codecov policy.yml'"));
        assert!(command.contains("--exceptions 'policy files/quality exceptions.toml'"));
        assert!(command.contains("--patch-coverage 97.25"));
        assert!(command.contains("--receipt 'target/quality receipts/quality gate.json'"));
        assert!(command.contains("--summary 'target/quality receipts/quality gate.md'"));
        assert!(command.ends_with(" --check"));
    }

    #[test]
    fn advisory_records_explicit_codecov_policy_input() -> TestResult {
        let dir = tempfile::tempdir()?;
        let codecov = dir.path().join("custom-codecov.yml");
        let output = dir.path().join("quality-gate.json");
        fs::write(
            &codecov,
            r#"coverage:
  status:
    patch:
      default:
        target: 91%
        threshold: 3%
        if_ci_failed: error
comment:
  layout: "reach,files"
  require_head: false
"#,
        )?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        assert_eq!(
            receipt.pointer("/coverage/codecov_config_status").and_then(Value::as_str),
            Some("present")
        );
        assert_eq!(
            receipt.pointer("/coverage/codecov_config").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_eq!(
            receipt.pointer("/coverage/patch_policy/target").and_then(Value::as_str),
            Some("91%")
        );
        assert_eq!(
            receipt.pointer("/coverage/patch_policy/threshold").and_then(Value::as_str),
            Some("3%")
        );
        assert_eq!(
            receipt.pointer("/coverage/codecov_comment/require_head").and_then(Value::as_bool),
            Some(false)
        );
        Ok(())
    }

    #[test]
    fn advisory_includes_temporary_exception_policy() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.pointer("/exceptions/status").and_then(Value::as_str), Some("present"));
        assert_eq!(
            receipt.pointer("/exceptions/active/0/id").and_then(Value::as_str),
            Some("ripr-total-burndown")
        );
        assert_eq!(
            receipt.pointer("/exceptions/active/1/applies_to").and_then(Value::as_str),
            Some("project_coverage_below_target")
        );
        assert!(!next_actions_contain(&receipt, "quality_exception_policy_gap"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("## Temporary Exceptions"));
        assert!(
            markdown.contains(
                "| ID | Applies to | Owner | Final target | Expires | Removal criteria |"
            ),
            "{markdown}"
        );
        assert!(markdown.contains("ripr-total-burndown"));
        assert!(markdown.contains("project-coverage-burndown"));
        assert!(markdown.contains("coverage-proof-lane"));
        assert!(markdown.contains("Remove after total RIPR+ is zero on main."));
        assert!(markdown.contains("Remove after project coverage reaches 95% and is blocking."));
        assert!(markdown.contains("do not waive `quality-gate --mode enforce` blockers"));
        Ok(())
    }

    #[test]
    fn advisory_reports_missing_exception_policy_as_repair_action() -> TestResult {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("quality-gate.json");

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        assert_eq!(receipt.pointer("/exceptions/status").and_then(Value::as_str), Some("missing"));
        let action = next_action(&receipt, "quality_exception_policy_gap")
            .ok_or("quality exception action missing")?;
        let verify = action.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode advisory"));
        assert!(verify.contains(&format!("--receipt {}", display_path(&output))));
        assert!(verify.contains("--check"));
        let receipt_command =
            action.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains("cargo xtask quality-gate --mode advisory"));
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&output))));
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn advisory_includes_below_target_coverage_files() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_low_coverage_receipt(&coverage)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let action = receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .and_then(|actions| {
                actions.iter().find(|action| {
                    action.get("kind").and_then(Value::as_str) == Some("project_coverage_gap")
                })
            })
            .ok_or("project coverage action missing")?;
        assert_eq!(
            action.pointer("/top_files/0/path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(
            action.get("suggested_test").and_then(Value::as_str),
            Some(COVERAGE_GAP_SUGGESTED_TEST)
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-parser/src/lib.rs"));
        assert!(markdown.contains("line coverage 40.00%"));
        assert!(markdown.contains("sample uncovered lines: 12, 13, 17"));
        assert!(markdown.contains(COVERAGE_GAP_SUGGESTED_TEST));
        Ok(())
    }

    #[test]
    fn advisory_rejects_coverage_receipt_without_measured_line_count() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        fs::write(
            &coverage,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "measured": {
                    "line_coverage": 100.0
                }
            }))?,
        )?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/coverage/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/measured/line_found"))
        );
        let action = next_action(&receipt, "coverage_receipt_gap")
            .ok_or("coverage receipt gap action missing")?;
        assert!(
            action
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("/measured/line_found"))
        );
        Ok(())
    }

    #[test]
    fn advisory_rejects_below_target_coverage_without_file_guidance() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        fs::write(
            &coverage,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "measured": measured_coverage(88, 100, 88.0),
                "files_below_target": []
            }))?,
        )?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(receipt.pointer("/coverage/status").and_then(Value::as_str).is_some_and(
            |status| status.contains(
                "/files_below_target expected at least one below-target file guidance row"
            )
        ));
        let action = next_action(&receipt, "coverage_receipt_gap")
            .ok_or("coverage receipt gap action missing")?;
        assert!(action.get("reason").and_then(Value::as_str).is_some_and(|reason| {
            reason.contains(
                "/files_below_target expected at least one below-target file guidance row",
            )
        }));
        Ok(())
    }

    #[test]
    fn advisory_rejects_below_target_coverage_without_uncovered_line_samples() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        fs::write(
            &coverage,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "measured": measured_coverage(88, 100, 88.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0
                    }
                ]
            }))?,
        )?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/coverage/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("sample_uncovered_lines"))
        );
        let action = next_action(&receipt, "coverage_receipt_gap")
            .ok_or("coverage receipt gap action missing")?;
        assert!(
            action
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| { reason.contains("sample_uncovered_lines") })
        );
        assert!(!next_actions_contain(&receipt, "project_coverage_gap"));
        Ok(())
    }

    #[test]
    fn advisory_rejects_below_target_coverage_with_zero_uncovered_line_sample() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        fs::write(
            &coverage,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "measured": measured_coverage(88, 100, 88.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [0]
                    }
                ]
            }))?,
        )?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/coverage/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("positive sample_uncovered_lines"))
        );
        let action = next_action(&receipt, "coverage_receipt_gap")
            .ok_or("coverage receipt gap action missing")?;
        assert!(
            action
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| { reason.contains("positive sample_uncovered_lines") })
        );
        assert!(!next_actions_contain(&receipt, "project_coverage_gap"));
        Ok(())
    }

    #[test]
    fn advisory_includes_ripr_seam_suggested_test() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let action =
            next_action(&receipt, "ripr_seam_cluster").ok_or("RIPR seam action missing")?;
        assert_eq!(
            action.get("path").and_then(Value::as_str),
            Some("crates/perl-lexer/src/lib.rs")
        );
        assert_eq!(
            action.get("suggested_test").and_then(Value::as_str),
            Some(RIPR_SEAM_SUGGESTED_TEST)
        );
        assert_eq!(
            action.pointer("/sample_seams/0/gap_id").and_then(Value::as_str),
            Some("RIPR-SPEC-0007")
        );
        assert_eq!(action.pointer("/sample_seams/0/line").and_then(Value::as_u64), Some(42));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-lexer/src/lib.rs"));
        assert!(markdown.contains(RIPR_SEAM_SUGGESTED_TEST));
        assert!(markdown.contains(
            "- `RIPR-SPEC-0007` line `42` kind `predicate_boundary` seam `lex_segment` reason lexer boundary branch is unobserved; suggested test: prove lexer boundary branch"
        ));
        Ok(())
    }

    #[test]
    fn summary_includes_quality_gate_matrix() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt_with_patch(&coverage, 97.1)?;
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("## Quality Gates"));
        assert!(
            markdown.contains("- Codecov project policy: target 95%, threshold 2%, informational")
        );
        assert!(markdown.contains("- Codecov project coverage: 96.00% / 95.0%"));
        assert!(markdown.contains("| Codecov patch coverage | pass | 97.10% | 95.0% | yes |"));
        assert!(markdown.contains(
            "| Codecov patch policy | pass | target 95%, threshold 0% | 95% / 0% | yes |"
        ));
        assert!(markdown.contains(
            "| Codecov failure guidance | pass | layout reach,diff,flags,files | diff + files | yes |"
        ));
        assert!(markdown.contains(
            "| Codecov project policy | fail | target 95%, threshold 2% | 95% / 0.25% | burn-down |"
        ));
        assert!(
            markdown.contains("| Codecov project coverage | pass | 96.00% | 95.0% | burn-down |")
        );
        assert!(markdown.contains("| ripr+ zero | unknown | unknown | 0 | burn-down |"));
        assert!(markdown.contains("| RIPR+ receipt | fail | missing | present | advisory |"));
        assert!(markdown.contains("| RIPR PR receipt | fail | missing | present | advisory |"));
        assert!(
            markdown.contains(
                "| RIPR review guidance | fail | missing | present/actionable | advisory |"
            )
        );
        assert!(markdown.contains("## Claim Boundary"));
        assert!(markdown.contains(
            "quality-gate is the local/CI aggregation surface for coverage and RIPR proof."
        ));
        assert!(markdown.contains(
            "full enforce mode is reserved for post-burn-down ripr+ zero and 95% project/patch coverage."
        ));
        assert!(markdown.contains(
            "temporary exceptions document burn-down debt; they do not waive full enforce blockers."
        ));
        assert!(markdown.contains("## PR Summary Guidance"));
        assert!(markdown.contains("Objective: one sentence naming the proof target"));
        assert!(markdown.contains("Claim boundary: state what this proves"));
        assert!(markdown.contains("no LSP 3.18 behavior work"));
        assert!(markdown.contains("RIPR/coverage effect: new-gap count"));
        assert!(
            markdown.contains(
                "Local proof commands: paste the commands run and their pass/fail result"
            )
        );
        assert!(markdown.contains("Cleanup performed: state `rtk git status --short --branch`, `rtk git diff --check`, and `rtk bash scripts/storage-doctor` results"));
        assert!(markdown.contains("What remains: name any advisory burn-down debt"));
        assert!(markdown.contains(&format!(
            "rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt {}",
            display_path(&coverage)
        )));
        assert!(markdown.contains("rtk cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(markdown.contains("--codecov"));
        assert!(markdown.contains("--check"));
        assert!(markdown.contains("rtk git status --short --branch"));
        assert!(markdown.contains("rtk git diff --check"));
        assert!(markdown.contains("rtk bash scripts/storage-doctor"));
        assert_local_proof_commands_are_rtk_prefixed(&markdown)?;
        Ok(())
    }

    #[test]
    fn quality_gate_check_fails_when_receipt_is_stale() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;
        fs::write(&output, "stale receipt\n")?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: true,
        });

        let error = result.err().ok_or("stale receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("quality-gate.json is stale"), "{message}");
        assert_quality_gate_summary_error_names_commands(&message, &output, &summary);
        Ok(())
    }

    #[test]
    fn quality_gate_check_fails_when_receipt_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: true,
        });

        let error = result.err().ok_or("missing receipt check should fail")?;
        let message = error.to_string();
        assert!(message.contains("missing quality-gate receipt"), "{message}");
        assert!(message.contains("quality-gate.json"), "{message}");
        assert_quality_gate_summary_error_names_commands(&message, &output, &summary);
        Ok(())
    }

    #[test]
    fn quality_gate_check_fails_when_summary_is_stale() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;
        fs::write(&summary, "stale summary\n")?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: true,
        });

        let error = result.err().ok_or("stale summary check should fail")?;
        let message = error.to_string();
        assert!(message.contains("quality-gate.md is stale"), "{message}");
        assert_quality_gate_summary_error_names_commands(&message, &output, &summary);
        Ok(())
    }

    #[test]
    fn quality_gate_check_fails_when_summary_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;
        fs::remove_file(&summary)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Advisory,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: true,
        });

        let error = result.err().ok_or("missing summary check should fail")?;
        let message = error.to_string();
        assert!(message.contains("missing quality-gate summary"), "{message}");
        assert!(message.contains("quality-gate.md"), "{message}");
        assert_quality_gate_summary_error_names_commands(&message, &output, &summary);
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("missing-ripr.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("fail"));
        let blocker = next_action(&receipt, "ripr_pr_receipt_not_current")
            .ok_or("RIPR PR receipt blocker missing")?;
        assert_eq!(
            blocker.get("verify").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check")
        );
        assert_eq!(
            blocker.get("receipt").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD")
        );
        assert!(!next_actions_contain(&receipt, "ripr_pr_receipt_gap"));
        assert!(!next_actions_contain(&receipt, "new_ripr_gap_unknown"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains(&format!(
            "rtk cargo xtask ripr-plus --receipt {} --check",
            display_path(&ripr)
        )));
        assert!(
            markdown.contains("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check")
        );
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_review_receipt_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("fail"));
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("missing")
        );
        let blocker = next_action(&receipt, "ripr_review_receipt_not_current")
            .ok_or("RIPR review receipt blocker missing")?;
        assert_eq!(blocker.get("reason").and_then(Value::as_str), Some("missing"));
        assert_eq!(
            blocker.get("verify").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-review-comments --base origin/main --head HEAD --check")
        );
        assert_eq!(
            blocker.get("receipt").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-review-comments --base origin/main --head HEAD")
        );
        assert!(!next_actions_contain(&receipt, "new_ripr_gap"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("ripr_review_receipt_not_current"));
        assert!(markdown.contains("rtk cargo xtask ripr-review-comments"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_has_new_gaps() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 2)?;
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("fail"));
        assert_eq!(receipt.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(2));
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(
            new_gap.get("path").and_then(Value::as_str),
            Some(display_path(&ripr_pr).as_str())
        );
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("missing"));
        assert_eq!(
            new_gap.get("suggested_test").and_then(Value::as_str),
            Some(NEW_RIPR_GAP_SUGGESTED_TEST)
        );
        assert_eq!(
            new_gap.get("guidance_receipt").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-review-comments --base origin/main --head HEAD")
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `missing`"));
        assert!(markdown.contains(
            "guidance receipt: `rtk cargo xtask ripr-review-comments --base origin/main --head HEAD`"
        ));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_has_wrong_kind() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_wrong_kind_pr_receipt(&ripr_pr)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_pr/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/kind expected \"pr_evidence\""))
        );
        let blocker = next_action(&receipt, "ripr_pr_receipt_not_current")
            .ok_or("RIPR PR receipt blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("not_pr_evidence"))
        );
        assert!(!next_actions_contain(&receipt, "new_ripr_gap_unknown"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_lacks_new_gap_metric() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt_without_severe_gaps(&ripr_pr)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker =
            next_action(&receipt, "new_ripr_gap_unknown").ok_or("new-gap blocker missing")?;
        assert_eq!(
            blocker.get("reason").and_then(Value::as_str),
            Some("diff-scoped severe_gaps is not measured yet")
        );
        assert_eq!(
            blocker.get("verify").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-pr --base origin/main --head HEAD --check")
        );
        assert_eq!(
            blocker.get("receipt").and_then(Value::as_str),
            Some("rtk cargo xtask ripr-pr --base origin/main --head HEAD")
        );
        assert!(!next_actions_contain(&receipt, "ripr_new_gap_unknown"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_lacks_base_sha() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt_without_base_sha(&ripr_pr)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_pr/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/base_sha expected non-empty string"))
        );
        let blocker = next_action(&receipt, "ripr_pr_receipt_not_current")
            .ok_or("RIPR PR receipt blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("base_sha"))
        );
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_when_pr_receipt_base_is_stale() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt_with_base(&ripr_pr, "HEAD", "stale-base")?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.pointer("/ripr_pr/status").and_then(Value::as_str), Some("stale"));
        let blocker = next_action(&receipt, "ripr_pr_receipt_not_current")
            .ok_or("RIPR PR receipt blocker missing")?;
        assert_eq!(blocker.get("receipt_base").and_then(Value::as_str), Some("HEAD"));
        assert_eq!(blocker.get("receipt_base_sha").and_then(Value::as_str), Some("stale-base"));
        assert_eq!(
            blocker.get("expected_base_sha").and_then(Value::as_str),
            Some(current_base_for_test("HEAD").as_str())
        );
        assert_eq!(
            blocker.get("receipt_head_sha").and_then(Value::as_str),
            Some(current_head_for_test().as_str())
        );
        assert_eq!(
            blocker.get("expected_head_sha").and_then(Value::as_str),
            Some(current_head_for_test().as_str())
        );
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_passes_when_pr_receipt_is_current_and_zero() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_exception_policy(&exceptions)?;
        let output = dir.path().join("quality-gate.json");

        run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("pass"));
        assert!(!next_actions_contain(&receipt, "coverage_receipt_gap"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_requires_current_ripr_plus_receipt() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("missing-ripr.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker =
            next_action(&receipt, "ripr_receipt_not_current").ok_or("RIPR blocker missing")?;
        assert_eq!(blocker.get("reason").and_then(Value::as_str), Some("missing"));
        assert_eq!(
            blocker.get("repair").and_then(Value::as_str),
            Some(
                "Regenerate and check the repo-wide RIPR+ receipt for this HEAD. The transition gate does not require total RIPR+ zero yet, but it does require current total-debt proof."
            )
        );
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert_eq!(
            verify,
            format!("rtk cargo xtask ripr-plus --receipt {} --check", display_path(&ripr))
        );
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert_eq!(
            receipt_command,
            format!("rtk cargo xtask ripr-plus --receipt {}", display_path(&ripr))
        );
        assert!(!next_actions_contain(&receipt, "ripr_receipt_gap"));
        assert!(!next_actions_contain(&receipt, "ripr_total_not_zero"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_ripr_plus_without_unresolved_count() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt_without_unresolved(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_plus/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/unresolved expected unsigned integer"))
        );
        let blocker =
            next_action(&receipt, "ripr_receipt_not_current").ok_or("RIPR blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("/unresolved expected unsigned integer"))
        );
        assert!(!next_actions_contain(&receipt, "new_ripr_gap"));
        Ok(())
    }

    #[test]
    fn ripr_plus_receipt_accepts_deferred_missing_guidance_action() -> TestResult {
        let receipt = json!({
            "schema_version": 1,
            "kind": "ripr_plus_baseline",
            "head": current_head_for_test(),
            "unresolved": 7,
            "top_files": [
                {
                    "name": "crates/perl-parser/src/lib.rs",
                    "count": 7,
                    "sample_seams": [
                        {
                            "kind": "predicate_boundary",
                            "line": 42
                        }
                    ]
                }
            ],
            "top_actionable_files": [],
            "deferred_files": [
                {
                    "name": "crates/perl-parser/src/lib.rs",
                    "count": 7,
                    "reason": "missing_actionable_sample"
                }
            ],
            "next_actions": [
                {
                    "kind": "ripr_receipt_gap_guidance_missing",
                    "path": "crates/perl-parser/src/lib.rs",
                    "unresolved": 7,
                    "reason": "missing_actionable_sample",
                    "repair": "Regenerate RIPR+ receipt with gap id, positive line, seam, reason, and suggested test.",
                    "suggested_test": "Add receipt fixture coverage for actionable sample seam details.",
                    "verify": "rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check",
                    "receipt": "rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json"
                }
            ]
        });

        assert_eq!(ripr_plus_measurement_violation(&receipt), None);
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_nonzero_ripr_plus_without_file_guidance() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt_without_file_guidance(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_plus/status")
                .and_then(Value::as_str)
                .is_some_and(|status| { status.contains("expected actionable RIPR guidance") })
        );
        let blocker =
            next_action(&receipt, "ripr_receipt_not_current").ok_or("RIPR blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| { reason.contains("expected actionable RIPR guidance") })
        );
        assert_eq!(receipt.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_nonzero_ripr_plus_without_sample_seams() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt_without_sample_seams(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_plus/status")
                .and_then(Value::as_str)
                .is_some_and(|status| { status.contains("expected actionable RIPR guidance") })
        );
        let blocker =
            next_action(&receipt, "ripr_receipt_not_current").ok_or("RIPR blocker missing")?;
        assert_eq!(
            blocker.get("repair").and_then(Value::as_str),
            Some(
                "Regenerate and check the repo-wide RIPR+ receipt for this HEAD. The transition gate does not require total RIPR+ zero yet, but it does require current total-debt proof."
            )
        );
        assert!(blocker.get("verify").and_then(Value::as_str).is_some_and(|command| {
            command.starts_with("rtk cargo xtask ripr-plus") && command.contains("--check")
        }));
        assert!(!next_actions_contain(&receipt, "ripr_total_not_zero"));
        assert!(!next_actions_contain(&receipt, "ripr_seam_cluster"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_fails_without_exception_policy() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 0)?;
        let output = dir.path().join("quality-gate.json");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("fail"));
        let blocker = next_action(&receipt, "quality_exception_policy_not_current")
            .ok_or("quality exception blocker missing")?;
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask quality-gate --mode enforce-new-ripr"));
        assert!(verify.contains(&format!("--receipt {}", display_path(&output))));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(
            receipt_command.starts_with("rtk cargo xtask quality-gate --mode enforce-new-ripr")
        );
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&output))));
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn exception_policy_rejects_non_active_status_with_transition_entries() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        write_exception_policy(&exceptions)?;
        let policy =
            fs::read_to_string(&exceptions)?.replace("status = \"active\"", "status = \"retired\"");
        fs::write(&exceptions, policy)?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_status_invalid")
                && warning.get("expected").and_then(Value::as_str) == Some("active")
                && warning.get("actual").and_then(Value::as_str) == Some("retired")
                && warning.get("repair").and_then(Value::as_str).is_some()
        }));
        Ok(())
    }

    #[test]
    fn exception_policy_rejects_bad_date_shapes_and_order() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        fs::write(
            &exceptions,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "2026-05-26"

[[exception]]
id = "bad-review-date"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Test bad review date."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after proof is green."
review_after = "2026/06/07"
expires = "2026-08-07"

[[exception]]
id = "bad-expiry-order"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Test bad expiry order."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after proof is green."
review_after = "2026-08-07"
expires = "2026-06-07"
"#,
        )?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_date_invalid")
                && warning.get("id").and_then(Value::as_str) == Some("bad-review-date")
                && warning.get("field").and_then(Value::as_str) == Some("review_after")
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_date_order_invalid")
                && warning.get("id").and_then(Value::as_str) == Some("bad-expiry-order")
                && warning.get("field").and_then(Value::as_str) == Some("expires")
        }));
        Ok(())
    }

    #[test]
    fn exception_policy_rejects_review_due_transition_entries() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        fs::write(
            &exceptions,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "1999-12-01"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Review-due transition debt must not stay valid without re-justification."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after proof is green."
review_after = "2000-01-01"
expires = "2099-01-01"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Review-due transition debt must not stay valid without re-justification."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after proof is green."
review_after = "2000-01-01"
expires = "2099-01-01"
"#,
        )?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_review_due")
                && warning.get("id").and_then(Value::as_str) == Some("ripr-total-burndown")
                && warning.get("field").and_then(Value::as_str) == Some("review_after")
                && warning.get("repair").and_then(Value::as_str).is_some()
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_review_due")
                && warning.get("id").and_then(Value::as_str) == Some("project-coverage-burndown")
                && warning.get("today").and_then(Value::as_str).is_some()
        }));
        assert!(!state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_expired")
        }));
        Ok(())
    }

    #[test]
    fn exception_policy_rejects_expired_transition_entries() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        fs::write(
            &exceptions,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "1999-12-01"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Expired transition debt must not stay valid."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after proof is green."
review_after = "1999-12-15"
expires = "2000-01-01"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Expired transition debt must not stay valid."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after proof is green."
review_after = "1999-12-15"
expires = "2000-01-01"
"#,
        )?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_expired")
                && warning.get("id").and_then(Value::as_str) == Some("ripr-total-burndown")
                && warning.get("value").and_then(Value::as_str) == Some("2000-01-01")
                && warning.get("repair").and_then(Value::as_str).is_some()
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str) == Some("quality_exception_expired")
                && warning.get("id").and_then(Value::as_str) == Some("project-coverage-burndown")
                && warning.get("today").and_then(Value::as_str).is_some()
        }));
        Ok(())
    }

    #[test]
    fn exception_policy_requires_named_transition_entries() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        fs::write(
            &exceptions,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "2026-05-26"

[[exception]]
id = "unrelated-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Looks like transition debt but is not the required named contract."
final_target = "ripr_plus.unresolved == 0"
current_evidence = ["target/receipts/quality/ripr-plus.json"]
removal_criteria = "Remove after proof is green."
review_after = "2026-06-07"
expires = "2026-08-07"
"#,
        )?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_required_entry_missing")
                && warning.get("id").and_then(Value::as_str) == Some("ripr-total-burndown")
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_required_entry_missing")
                && warning.get("id").and_then(Value::as_str) == Some("project-coverage-burndown")
        }));
        Ok(())
    }

    #[test]
    fn exception_policy_requires_transition_receipt_evidence() -> TestResult {
        let dir = tempfile::tempdir()?;
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        write_exception_policy_missing_receipt_evidence(&exceptions)?;

        let state = exception_state(&exceptions);

        assert_eq!(state.status, "invalid");
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_required_evidence_missing")
                && warning.get("id").and_then(Value::as_str) == Some("ripr-total-burndown")
                && warning.get("evidence").and_then(Value::as_str)
                    == Some("target/receipts/quality/ripr-plus.json")
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_required_evidence_missing")
                && warning.get("id").and_then(Value::as_str) == Some("project-coverage-burndown")
                && warning.get("evidence").and_then(Value::as_str)
                    == Some("target/receipts/quality/coverage-baseline.json")
        }));
        assert!(state.warnings.iter().any(|warning| {
            warning.get("kind").and_then(Value::as_str)
                == Some("quality_exception_required_evidence_missing")
                && warning.get("id").and_then(Value::as_str) == Some("project-coverage-burndown")
                && warning.get("evidence").and_then(Value::as_str) == Some("codecov.yml")
        }));
        Ok(())
    }

    #[test]
    fn quality_gate_summary_prints_exception_policy_warnings() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_exception_policy_missing_receipt_evidence(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "quality_exception_policy_not_current")
            .ok_or("quality exception blocker missing")?;
        assert!(blocker.get("warnings").and_then(Value::as_array).is_some_and(|warnings| {
            warnings.iter().any(|warning| {
                warning.get("kind").and_then(Value::as_str)
                    == Some("quality_exception_required_evidence_missing")
            })
        }));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("warnings:"));
        assert!(markdown.contains("quality_exception_required_evidence_missing"));
        assert!(markdown.contains("target/receipts/quality/ripr-plus.json"));
        Ok(())
    }

    #[test]
    fn quality_gate_summary_prints_exception_warning_values() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 0)?;
        fs::write(
            &exceptions,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "1999-12-01"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Review-due transition debt must name the exact stale date."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after proof is green."
review_after = "2000-01-01"
expires = "2099-01-01"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Expiry-order transition debt must name both relevant dates."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after proof is green."
review_after = "2099-01-01"
expires = "2098-01-01"
"#,
        )?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("quality_exception_review_due"));
        assert!(markdown.contains("value `2000-01-01`"));
        assert!(markdown.contains("today `"));
        assert!(markdown.contains("quality_exception_date_order_invalid"));
        assert!(markdown.contains("value `2098-01-01`"));
        assert!(markdown.contains("must_be_on_or_after `2099-01-01`"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_includes_review_guidance_in_receipt_and_summary() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_review_guidance(&review)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/top_gaps/0/path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(
            receipt.pointer("/review_guidance/top_gaps/0/line").and_then(Value::as_u64),
            Some(42)
        );
        let new_gap = receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .and_then(|actions| {
                actions.iter().find(|action| {
                    action.get("kind").and_then(Value::as_str) == Some("new_ripr_gap")
                })
            })
            .ok_or("new_ripr_gap action missing")?;
        assert_eq!(
            new_gap.pointer("/top_gaps/0/gap_id").and_then(Value::as_str),
            Some("RIPR-SPEC-0042")
        );
        assert_eq!(
            new_gap.get("suggested_test").and_then(Value::as_str),
            Some(NEW_RIPR_GAP_SUGGESTED_TEST)
        );
        let verify = new_gap.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains(&format!("--receipt {}", display_path(&output))));
        assert!(verify.contains(&format!("--summary {}", display_path(&summary))));
        assert!(verify.contains("--check"));
        let receipt_command =
            new_gap.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&output))));
        assert!(receipt_command.contains(&format!("--summary {}", display_path(&summary))));
        assert!(!receipt_command.contains("--check"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-parser/src/lib.rs"));
        assert!(markdown.contains("42"));
        assert!(markdown.contains("| Gap | File | Line | Seam | Reason | Suggested proof |"));
        assert!(markdown.contains(
            "| RIPR-SPEC-0042 | crates/perl-parser/src/lib.rs | 42 | exact_seam_line | changed parser branch has only weak proof | prove parser branch recovery |"
        ));
        assert!(markdown.contains("prove parser branch recovery"));
        assert!(markdown.contains(
            "- `RIPR-SPEC-0042` `crates/perl-parser/src/lib.rs:42` seam `exact_seam_line` changed parser branch has only weak proof; suggested test: prove parser branch recovery"
        ));
        assert!(markdown.contains(&format!(
            "rtk cargo xtask ripr-plus --receipt {} --check",
            display_path(&ripr)
        )));
        assert!(markdown.contains("rtk cargo xtask quality-gate --mode enforce-new-ripr"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_ignores_stale_review_guidance() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_review_guidance_with_head(&review, "stale-head")?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/review_guidance/top_gaps")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("stale"));
        assert!(new_gap.get("top_gaps").is_none());
        let guidance_blocker = next_action(&receipt, "ripr_review_receipt_not_current")
            .ok_or("review guidance blocker missing")?;
        assert_eq!(
            guidance_blocker.get("receipt_head_sha").and_then(Value::as_str),
            Some("stale-head")
        );
        assert_eq!(
            guidance_blocker.get("expected_head_sha").and_then(Value::as_str),
            Some(current_head_for_test().as_str())
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `stale`"));
        assert!(markdown.contains("receipt-head `stale-head`"));
        assert!(!markdown.contains("| RIPR-SPEC-0042 |"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_names_empty_review_guidance_as_no_top_gaps() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_empty_review_guidance(&review)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("present")
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("no_top_gaps"));
        let guidance_gap = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance gap missing")?;
        assert_eq!(guidance_gap.get("reason").and_then(Value::as_str), Some("no_top_gaps"));
        assert_enforce_new_ripr_guidance_gap_commands(guidance_gap)?;

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `no_top_gaps`"));
        assert!(markdown.contains("reason `no_top_gaps`"));
        assert!(markdown.contains("- no file/line guidance available"));
        assert!(markdown.contains(
            "- refresh review guidance receipt: `rtk cargo xtask ripr-review-comments --base origin/main --head HEAD`"
        ));
        assert!(markdown.contains(
            "- verify review guidance receipt: `rtk cargo xtask ripr-review-comments --base origin/main --head HEAD --check`"
        ));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_incomplete_review_guidance() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_incomplete_review_guidance(&review)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("incomplete")
        );
        assert!(
            receipt
                .pointer("/review_guidance/top_gaps")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("incomplete"));
        assert!(new_gap.get("top_gaps").is_none());
        assert!(
            new_gap
                .get("guidance_verify")
                .and_then(Value::as_str)
                .is_some_and(|command| command.starts_with("rtk cargo xtask ripr-review-comments"))
        );
        let guidance_gap = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance gap missing")?;
        assert_eq!(guidance_gap.get("reason").and_then(Value::as_str), Some("incomplete"));
        assert_enforce_new_ripr_guidance_gap_commands(guidance_gap)?;

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `incomplete`"));
        assert!(markdown.contains("reason `incomplete`"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_review_guidance_without_seam() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_review_guidance_without_seam(&review)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("incomplete")
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("incomplete"));
        assert!(new_gap.get("top_gaps").is_none());
        let guidance_gap = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance gap missing")?;
        assert_eq!(guidance_gap.get("reason").and_then(Value::as_str), Some("incomplete"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_rejects_review_guidance_with_zero_line() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_review_guidance_with_zero_line(&review)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("incomplete")
        );
        assert!(
            receipt
                .pointer("/review_guidance/top_gaps")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("incomplete"));
        let guidance_gap = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance gap missing")?;
        assert_eq!(guidance_gap.get("reason").and_then(Value::as_str), Some("incomplete"));
        Ok(())
    }

    #[test]
    fn enforce_new_ripr_ignores_review_guidance_with_stale_base() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 1)?;
        write_review_guidance_with_base(&review, "HEAD", "stale-base")?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforceNewRipr,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(
            receipt.pointer("/review_guidance/status").and_then(Value::as_str),
            Some("stale")
        );
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap action missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("stale"));
        assert!(new_gap.get("top_gaps").is_none());
        let guidance_blocker = next_action(&receipt, "ripr_review_receipt_not_current")
            .ok_or("review guidance blocker missing")?;
        assert_eq!(guidance_blocker.get("receipt_base").and_then(Value::as_str), Some("HEAD"));
        assert_eq!(
            guidance_blocker.get("receipt_base_sha").and_then(Value::as_str),
            Some("stale-base")
        );
        assert_eq!(
            guidance_blocker.get("expected_base_sha").and_then(Value::as_str),
            Some(current_base_for_test("HEAD").as_str())
        );
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("receipt-base `HEAD`"));
        assert!(markdown.contains("receipt-base-sha `stale-base`"));
        assert!(markdown.contains("expected-base-sha `"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_passes_with_current_receipt_and_blocking_policy() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            receipt.pointer("/coverage/patch_policy/target").and_then(Value::as_str),
            Some("95%")
        );
        assert_eq!(receipt.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_exception_policy_blocker_uses_patch_gate_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        write_coverage_receipt_with_patch(&coverage, 96.0)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "quality_exception_policy_not_current")
            .ok_or("quality exception blocker missing")?;
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(
            receipt_command
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
        );
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_patch_value_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        let blocker = next_action(&receipt, "patch_coverage_unknown")
            .ok_or("missing patch coverage blocker")?;
        assert_eq!(
            blocker.get("guidance_status").and_then(Value::as_str),
            Some("patch_coverage_value_required")
        );
        assert!(
            blocker
                .get("guidance_repair")
                .and_then(Value::as_str)
                .is_some_and(|repair| repair.contains("--patch-status-source codecov"))
        );
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--check"));
        assert_quality_gate_guidance_commands(blocker, "enforce-patch-coverage")?;
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("patch_coverage_value_required"));
        assert!(markdown.contains("--patch-status-source codecov"));
        assert!(
            markdown.contains(
                "guidance verify: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
            ),
            "{markdown}"
        );
        assert!(
            markdown.contains(
                "guidance receipt: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
            ),
            "{markdown}"
        );
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_passes_with_explicit_codecov_status_source() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_exception_policy(&exceptions)?;

        run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: Some(PatchStatusSource::Codecov),
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            receipt.pointer("/coverage/patch_source").and_then(Value::as_str),
            Some("codecov_status")
        );
        assert!(!next_actions_contain(&receipt, "patch_coverage_unknown"));
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("- Codecov patch source: `codecov_status`"));
        assert!(
            markdown
                .contains("| Codecov patch coverage | external | Codecov status | 95.0% | yes |")
        );
        assert!(markdown.contains("--patch-status-source codecov"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_receipt_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("missing-coverage.json");
        let output = dir.path().join("quality-gate.json");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("fail"));
        let blocker = next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains(&format!("--receipt {}", display_path(&coverage))));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&coverage))));
        assert!(!receipt_command.contains("--check"));
        assert!(!next_actions_contain(&receipt, "coverage_receipt_gap"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_still_reports_live_policy_gap_when_receipt_is_missing() -> TestResult
    {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("missing-coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_advisory_patch_codecov_config(&codecov)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        let policy = next_action(&receipt, "patch_coverage_policy_not_enforcing")
            .ok_or("patch policy blocker missing")?;
        assert_eq!(
            policy.get("path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_eq!(policy.get("target").and_then(Value::as_str), Some("75%"));
        assert_eq!(policy.get("informational").and_then(Value::as_bool), Some(true));
        let verify = policy.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains(&format!("--codecov {}", display_path(&codecov))));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("coverage_receipt_not_current"));
        assert!(markdown.contains("patch_coverage_policy_not_enforcing"));
        assert!(markdown.contains(&format!("path `{}`", display_path(&codecov))));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_still_reports_live_comment_gap_when_receipt_is_missing() -> TestResult
    {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("missing-coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_non_actionable_codecov_config(&codecov)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        let comment = next_action(&receipt, "codecov_comment_not_actionable")
            .ok_or("Codecov comment blocker missing")?;
        assert_eq!(comment.pointer("/layout/0").and_then(Value::as_str), Some("reach"));
        assert_eq!(comment.pointer("/layout/1").and_then(Value::as_str), Some("flags"));
        assert_eq!(comment.get("require_head").and_then(Value::as_bool), Some(false));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_coverage_receipt_is_stale() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt_with_head(&coverage, "95%", "0%", None, "stale-head")?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        assert_eq!(receipt.pointer("/coverage/status").and_then(Value::as_str), Some("stale"));
        let blocker = next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        assert_eq!(blocker.get("reason").and_then(Value::as_str), Some("stale"));
        assert_eq!(blocker.get("receipt_head").and_then(Value::as_str), Some("stale-head"));
        assert_eq!(
            blocker.get("expected_head").and_then(Value::as_str),
            Some(current_head_for_test().as_str())
        );
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("receipt-head `stale-head`"));
        assert!(markdown.contains("expected-head `"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_coverage_receipt_has_wrong_kind() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_wrong_kind_coverage_receipt(&coverage)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/coverage/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/kind expected \"coverage_baseline\""))
        );
        let blocker = next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("not_coverage_baseline"))
        );
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_policy_is_advisory() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_advisory_patch_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .and_then(|actions| {
                actions.iter().find(|action| {
                    action.get("kind").and_then(Value::as_str)
                        == Some("patch_coverage_policy_not_enforcing")
                })
            })
            .ok_or("patch policy blocker missing")?;
        assert_eq!(
            blocker.get("path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_eq!(blocker.get("target").and_then(Value::as_str), Some("75%"));
        assert_eq!(blocker.get("threshold").and_then(Value::as_str), Some("2%"));
        assert_eq!(blocker.get("informational").and_then(Value::as_bool), Some(true));
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("--check"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_without_actionable_codecov_comment() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_non_actionable_codecov_config(&codecov)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "codecov_comment_not_actionable")
            .ok_or("Codecov comment blocker missing")?;
        assert_eq!(
            blocker.get("path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_eq!(blocker.pointer("/layout/0").and_then(Value::as_str), Some("reach"));
        assert_eq!(blocker.pointer("/layout/1").and_then(Value::as_str), Some("flags"));
        assert_eq!(blocker.get("require_head").and_then(Value::as_bool), Some(false));
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("--check"));
        assert_eq!(
            receipt.pointer("/coverage/codecov_comment/layout/0").and_then(Value::as_str),
            Some("reach")
        );
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_patch_value_is_below_target() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt_with_patch(&coverage, 94.9)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .and_then(|actions| {
                actions.iter().find(|action| {
                    action.get("kind").and_then(Value::as_str)
                        == Some("patch_coverage_below_target")
                })
            })
            .ok_or("patch coverage blocker missing")?;
        assert_eq!(blocker.get("current").and_then(Value::as_f64), Some(94.9));
        assert_eq!(blocker.get("target").and_then(Value::as_f64), Some(95.0));
        assert_eq!(blocker.get("source").and_then(Value::as_str), Some("coverage_receipt"));
        assert_eq!(
            blocker.pointer("/top_files/0/path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(
            blocker.pointer("/top_files/0/sample_uncovered_lines/0").and_then(Value::as_u64),
            Some(12)
        );
        assert_eq!(
            blocker.get("suggested_test").and_then(Value::as_str),
            Some(
                "Prefer focused tests for error paths, boundary conditions, config parsing, serialization, cancellation, provider decisions, or output contracts named by the uncovered files."
            )
        );
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-parser/src/lib.rs"));
        assert!(markdown.contains("sample uncovered lines: 12, 13, 17"));
        assert!(markdown.contains("suggested test: Prefer focused tests for error paths"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_removes_non_positive_uncovered_line_samples() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt_with_patch_and_mixed_line_samples(&coverage, 94.9)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let lines = receipt
            .pointer("/coverage/files_below_target/0/sample_uncovered_lines")
            .and_then(Value::as_array)
            .ok_or("sample uncovered lines missing")?;
        assert_eq!(lines.iter().filter_map(Value::as_u64).collect::<Vec<_>>(), vec![12, 13]);

        let blocker = next_action(&receipt, "patch_coverage_below_target")
            .ok_or("patch coverage blocker missing")?;
        let blocker_lines = blocker
            .pointer("/top_files/0/sample_uncovered_lines")
            .and_then(Value::as_array)
            .ok_or("blocker sample uncovered lines missing")?;
        assert_eq!(
            blocker_lines.iter().filter_map(Value::as_u64).collect::<Vec<_>>(),
            vec![12, 13]
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("sample uncovered lines: 12, 13"));
        assert!(!markdown.contains("sample uncovered lines: 0"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_falls_back_when_file_guidance_is_not_actionable() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt_with_patch_and_non_actionable_files(&coverage, 94.9)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/coverage/files_below_target")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        );
        let blocker = next_action(&receipt, "patch_coverage_below_target")
            .ok_or("patch coverage blocker missing")?;
        assert_eq!(
            blocker.get("guidance_status").and_then(Value::as_str),
            Some("codecov_diff_files_required")
        );
        assert!(blocker.get("top_files").and_then(Value::as_array).is_some_and(Vec::is_empty));
        let guidance_verify = blocker
            .get("guidance_verify")
            .and_then(Value::as_str)
            .ok_or("guidance verify missing")?;
        assert!(
            guidance_verify
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
        );
        assert!(guidance_verify.contains("--check"));
        let guidance_receipt = blocker
            .get("guidance_receipt")
            .and_then(Value::as_str)
            .ok_or("guidance receipt missing")?;
        assert!(
            guidance_receipt
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
        );
        assert!(!guidance_receipt.contains("--check"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `codecov_diff_files_required`"));
        assert!(markdown.contains(
            "guidance verify: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
        ));
        assert!(markdown.contains(
            "guidance receipt: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
        ));
        assert!(!markdown.contains("sample uncovered lines: 0"));
        Ok(())
    }

    #[test]
    fn enforce_patch_coverage_fails_when_cli_patch_value_is_below_target() -> TestResult {
        let dir = tempfile::tempdir()?;
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: Some(94.9),
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        assert_eq!(receipt.pointer("/coverage/patch").and_then(Value::as_f64), Some(94.9));
        assert_eq!(receipt.pointer("/coverage/patch_source").and_then(Value::as_str), Some("cli"));
        let blocker = next_action(&receipt, "patch_coverage_below_target")
            .ok_or("patch coverage blocker missing")?;
        assert_eq!(
            blocker.get("path").and_then(Value::as_str),
            Some(display_path(&coverage).as_str())
        );
        assert_eq!(blocker.get("source").and_then(Value::as_str), Some("cli"));
        assert_eq!(
            blocker.get("guidance_status").and_then(Value::as_str),
            Some("codecov_diff_files_required")
        );
        assert_eq!(
            blocker.get("guidance_repair").and_then(Value::as_str),
            Some(
                "Open the Codecov patch diff/files report for this PR and add focused behavior tests for the changed uncovered lines."
            )
        );
        let guidance_verify = blocker
            .get("guidance_verify")
            .and_then(Value::as_str)
            .ok_or("guidance verify missing")?;
        assert!(
            guidance_verify
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
        );
        assert!(guidance_verify.contains("--check"));
        let guidance_receipt = blocker
            .get("guidance_receipt")
            .and_then(Value::as_str)
            .ok_or("guidance receipt missing")?;
        assert!(
            guidance_receipt
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
        );
        assert!(!guidance_receipt.contains("--check"));
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("--patch-coverage 94.90"));
        assert!(verify.contains("--check"));

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("cargo xtask quality-gate --mode enforce-patch-coverage"));
        assert!(markdown.contains("guidance: `codecov_diff_files_required`"));
        assert!(markdown.contains("guidance repair: Open the Codecov patch diff/files report"));
        assert!(markdown.contains(
            "guidance verify: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
        ));
        assert!(markdown.contains(
            "guidance receipt: `rtk cargo xtask quality-gate --mode enforce-patch-coverage"
        ));
        assert!(markdown.contains("--codecov"));
        assert!(markdown.contains("--patch-coverage 94.90"));
        assert!(markdown.contains("--check"));
        Ok(())
    }

    #[test]
    fn quality_gate_rejects_invalid_cli_patch_coverage() -> TestResult {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("quality-gate.json");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: Some(120.0),
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        assert!(!output.exists());
        Ok(())
    }

    #[test]
    fn quality_gate_rejects_ambiguous_patch_coverage_inputs() -> TestResult {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("quality-gate.json");

        let result = run(QualityGateConfig {
            mode: QualityGateMode::EnforcePatchCoverage,
            ripr_receipt: &dir.path().join("missing-ripr.json"),
            ripr_pr_receipt: &dir.path().join("missing-ripr-pr.json"),
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &dir.path().join("missing-coverage.json"),
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: Some(96.0),
            patch_status_source: Some(PatchStatusSource::Codecov),
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        let error = result.expect_err("ambiguous patch inputs should fail before writing receipt");
        assert!(error.to_string().contains("--patch-coverage or --patch-status-source"));
        assert!(!output.exists());
        Ok(())
    }

    #[test]
    fn final_enforce_names_missing_patch_coverage_proof() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt(&coverage, "95%", "0%", None)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        let blocker =
            next_action(&receipt, "patch_coverage_unknown").ok_or("patch blocker missing")?;
        assert_eq!(
            blocker.get("guidance_status").and_then(Value::as_str),
            Some("patch_coverage_value_required")
        );
        assert_eq!(
            blocker.get("guidance_path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert!(
            blocker
                .get("guidance_repair")
                .and_then(Value::as_str)
                .is_some_and(|repair| repair.contains("--patch-coverage <percent>"))
        );
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("rtk cargo xtask quality-gate --mode enforce"));
        assert!(verify.contains("--check"));
        assert_quality_gate_guidance_commands(blocker, "enforce")?;

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("guidance: `patch_coverage_value_required`"));
        assert!(markdown.contains("--patch-coverage <percent>"));
        assert!(
            markdown.contains("guidance verify: `rtk cargo xtask quality-gate --mode enforce"),
            "{markdown}"
        );
        assert!(
            markdown.contains("guidance receipt: `rtk cargo xtask quality-gate --mode enforce"),
            "{markdown}"
        );
        Ok(())
    }

    #[test]
    fn final_enforce_ripr_total_unknown_uses_final_gate_verify_command() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt_without_unresolved(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker =
            next_action(&receipt, "ripr_total_unknown").ok_or("RIPR unknown blocker missing")?;
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(verify.contains(&format!("--ripr-receipt {}", display_path(&ripr))));
        assert!(verify.contains(&format!("--coverage-receipt {}", display_path(&coverage))));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert_eq!(
            receipt_command,
            format!("rtk cargo xtask ripr-plus --receipt {}", display_path(&ripr))
        );
        Ok(())
    }

    #[test]
    fn final_enforce_project_unknown_uses_final_gate_verify_command() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_without_line_coverage(&coverage)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        for kind in ["coverage_receipt_not_current", "project_coverage_unknown"] {
            let blocker = next_action(&receipt, kind).ok_or("coverage blocker missing")?;
            let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
            assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
            assert!(verify.contains(&format!("--coverage-receipt {}", display_path(&coverage))));
            assert!(verify.contains("--check"));
            let receipt_command =
                blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
            assert!(receipt_command.starts_with("rtk cargo xtask coverage-baseline"));
            assert!(receipt_command.contains(&format!("--receipt {}", display_path(&coverage))));
            assert!(!receipt_command.contains("--check"));
        }
        Ok(())
    }

    #[test]
    fn final_enforce_still_reports_project_policy_gap_when_coverage_receipt_is_missing()
    -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("missing-coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_advisory_patch_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        next_action(&receipt, "coverage_receipt_not_current")
            .ok_or("coverage receipt blocker missing")?;
        next_action(&receipt, "patch_coverage_policy_not_enforcing")
            .ok_or("patch policy blocker missing")?;
        let project_policy = next_action(&receipt, "project_coverage_policy_not_enforcing")
            .ok_or("project policy blocker missing")?;
        assert_eq!(project_policy.get("threshold").and_then(Value::as_str), Some("2%"));
        assert_eq!(project_policy.get("informational").and_then(Value::as_bool), Some(true));
        assert_final_gate_commands(project_policy)?;
        Ok(())
    }

    #[test]
    fn final_enforce_ripr_pr_receipt_blockers_use_final_gate_verify_command() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("missing-ripr-pr.json");
        let review = dir.path().join("missing-review.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        for (kind, direct_receipt) in [
            (
                "ripr_pr_receipt_not_current",
                "rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD",
            ),
            (
                "ripr_review_receipt_not_current",
                "rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD",
            ),
        ] {
            let blocker = next_action(&receipt, kind).ok_or("RIPR receipt blocker missing")?;
            let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
            assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
            assert!(verify.contains(&format!("--ripr-pr-receipt {}", display_path(&ripr_pr))));
            assert!(verify.contains(&format!("--review-receipt {}", display_path(&review))));
            assert!(verify.contains(&format!("--coverage-receipt {}", display_path(&coverage))));
            assert!(verify.contains("--check"));
            assert_eq!(blocker.get("receipt").and_then(Value::as_str), Some(direct_receipt));
        }
        Ok(())
    }

    #[test]
    fn final_enforce_new_ripr_gap_uses_final_gate_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 2)?;
        write_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(next_action_count(&receipt, "new_ripr_gap"), 1);
        let blocker = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap missing")?;
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(!verify.contains("--mode enforce-new-ripr"));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains("cargo xtask quality-gate --mode enforce"));
        assert!(!receipt_command.contains("--mode enforce-new-ripr"));
        assert!(!receipt_command.contains("--check"));
        assert_eq!(
            blocker.pointer("/top_gaps/0/seam").and_then(Value::as_str),
            Some("exact_seam_line")
        );
        Ok(())
    }

    #[test]
    fn final_enforce_review_guidance_gap_uses_final_gate_verify_command() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 2)?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let new_gap = next_action(&receipt, "new_ripr_gap").ok_or("new RIPR gap missing")?;
        assert_eq!(new_gap.get("guidance_status").and_then(Value::as_str), Some("no_top_gaps"));
        let guidance_gap = next_action(&receipt, "ripr_review_guidance_gap")
            .ok_or("review guidance gap missing")?;
        let verify = guidance_gap.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(!verify.contains("--mode enforce-new-ripr"));
        assert!(verify.contains("--check"));
        let receipt_command =
            guidance_gap.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains("cargo xtask ripr-review-comments"));
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn final_enforce_fails_when_temporary_exceptions_are_still_active() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let exceptions = dir.path().join("quality-gate-exceptions.toml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_exception_policy(&exceptions)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &exceptions,
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
        let blocker = next_action(&receipt, "temporary_exceptions_still_active")
            .ok_or("temporary exception blocker missing")?;
        assert_eq!(
            blocker.pointer("/active/0").and_then(Value::as_str),
            Some("ripr-total-burndown")
        );
        assert_eq!(
            blocker.pointer("/active/1").and_then(Value::as_str),
            Some("project-coverage-burndown")
        );
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains(&format!("--receipt {}", display_path(&output))));
        assert!(verify.contains("--check"));
        let receipt_command =
            blocker.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt_command.contains("cargo xtask quality-gate --mode enforce"));
        assert!(receipt_command.contains("--codecov"));
        assert!(receipt_command.contains(&format!("--receipt {}", display_path(&output))));
        assert!(!receipt_command.contains("--check"));
        Ok(())
    }

    #[test]
    fn final_enforce_passes_when_targets_are_met_and_exception_policy_is_removed() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        })?;

        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("pass"));
        assert!(!next_actions_contain(&receipt, "temporary_exceptions_still_active"));
        Ok(())
    }

    #[test]
    fn final_enforce_summary_lists_review_guidance_check_command() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt_with_base(&ripr_pr, "origin/main", &current_base_for_test("origin/main"))?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        })?;

        let markdown = fs::read_to_string(summary)?;
        let commands = markdown
            .split("Suggested local proof commands for this gate:")
            .nth(1)
            .ok_or("summary missing suggested local proof commands")?;
        assert!(
            commands.contains(
                "rtk cargo xtask ripr-review-comments --base origin/main --head HEAD --check"
            ),
            "final enforce proof commands must include the review-guidance freshness check"
        );
        assert!(
            commands.contains("rtk cargo xtask quality-gate --mode enforce")
                && commands.contains("--check"),
            "final enforce proof commands must still end with the aggregate check"
        );
        Ok(())
    }

    #[test]
    fn final_enforce_fails_when_coverage_scope_is_partial() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_parser_only_coverage_receipt(&coverage)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        let blocker = next_action(&receipt, "coverage_scope_not_workspace")
            .ok_or("coverage scope blocker missing")?;
        assert_eq!(blocker.pointer("/scope/kind").and_then(Value::as_str), Some("partial"));
        assert!(
            blocker.pointer("/scope/missing_required_roots").and_then(Value::as_array).is_some_and(
                |roots| roots.iter().any(|root| { root.as_str() == Some("crates/perl-lsp-rs") })
            )
        );
        assert!(
            blocker
                .get("repair")
                .and_then(Value::as_str)
                .is_some_and(|repair| repair.contains("workspace coverage command"))
        );
        assert_eq!(
            receipt.pointer("/coverage/coverage_scope/kind").and_then(Value::as_str),
            Some("partial")
        );
        let markdown = fs::read_to_string(summary)?;
        assert!(
            markdown
                .contains("| Coverage scope | fail | partial (scope_kind_not_workspace; missing ")
        );
        assert!(markdown.contains("scope: kind `partial`"));
        assert!(markdown.contains("missing required roots `crates/perl-lsp-rs"));
        assert!(markdown.contains("observed roots `crates/perl-parser`"));
        Ok(())
    }

    #[test]
    fn final_enforce_names_stale_workspace_member_scope_roots() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;
        let mut receipt: Value = serde_json::from_str(&fs::read_to_string(&coverage)?)?;
        receipt["coverage_scope"] = stale_workspace_coverage_scope();
        fs::write(&coverage, serde_json::to_string_pretty(&receipt)?)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "coverage_scope_not_workspace")
            .ok_or("coverage scope blocker missing")?;
        assert_eq!(blocker.get("reason").and_then(Value::as_str), Some("stale_required_roots"));
        assert!(blocker.get("current_required_roots").and_then(Value::as_array).is_some_and(
            |roots| roots.iter().any(|root| root.as_str() == Some("crates/perl-lsp-rs"))
        ));
        assert!(blocker.get("missing_current_roots").and_then(Value::as_array).is_some_and(
            |roots| { roots.iter().any(|root| root.as_str() == Some("crates/perl-lsp-rs")) }
        ));
        let markdown = fs::read_to_string(summary)?;
        assert!(
            markdown
                .contains("| Coverage scope | fail | workspace (stale_required_roots; missing ")
        );
        assert!(markdown.contains("reason `stale_required_roots`"));
        assert!(markdown.contains("scope: kind `workspace`"));
        assert!(markdown.contains("missing current roots: `"));
        assert!(markdown.contains("crates/perl-lsp-rs"));
        assert!(markdown.contains("current required roots: `crates/perl-ast"));
        Ok(())
    }

    #[test]
    fn unknown_coverage_scope_names_current_workspace_roots() -> TestResult {
        let required_roots = required_coverage_roots()?;
        let scope = unknown_coverage_scope();

        assert_eq!(string_array_value(scope.get("required_roots")), Some(required_roots.clone()));
        assert_eq!(string_array_value(scope.get("missing_required_roots")), Some(required_roots));
        Ok(())
    }

    #[test]
    fn final_enforce_names_extra_stale_receipt_scope_roots() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;
        let mut receipt: Value = serde_json::from_str(&fs::read_to_string(&coverage)?)?;
        receipt["coverage_scope"] = extra_stale_workspace_coverage_scope();
        fs::write(&coverage, serde_json::to_string_pretty(&receipt)?)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "coverage_scope_not_workspace")
            .ok_or("coverage scope blocker missing")?;
        assert_eq!(blocker.get("reason").and_then(Value::as_str), Some("stale_required_roots"));
        assert!(
            blocker
                .get("missing_current_roots")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        );
        assert!(blocker.get("extra_receipt_required_roots").and_then(Value::as_array).is_some_and(
            |roots| {
                roots.iter().any(|root| root.as_str() == Some("crates/removed-proof-crate"))
            }
        ));
        assert!(blocker.get("receipt_required_roots").and_then(Value::as_array).is_some_and(
            |roots| {
                roots.iter().any(|root| root.as_str() == Some("crates/removed-proof-crate"))
            }
        ));
        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains(
            "| Coverage scope | fail | workspace (stale_required_roots; extra 1 receipt roots) |"
        ));
        assert!(markdown.contains("extra receipt roots: `crates/removed-proof-crate`"));
        Ok(())
    }

    #[test]
    fn final_enforce_fails_when_codecov_project_policy_is_still_advisory() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_advisory_project_policy(&coverage)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "project_coverage_policy_not_enforcing")
            .ok_or("project policy blocker missing")?;
        let expected_codecov =
            display_path(&resolve_codecov_config_path(Path::new(CODECOV_CONFIG_PATH)));
        assert_eq!(blocker.get("path").and_then(Value::as_str), Some(expected_codecov.as_str()));
        assert_eq!(blocker.get("target").and_then(Value::as_str), Some("95%"));
        assert_eq!(blocker.get("threshold").and_then(Value::as_str), Some("2%"));
        assert_eq!(blocker.get("informational").and_then(Value::as_bool), Some(true));
        let verify = blocker.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("--check"));
        assert_eq!(
            receipt.pointer("/coverage/project_policy/threshold").and_then(Value::as_str),
            Some("2%")
        );
        Ok(())
    }

    #[test]
    fn final_enforce_codecov_config_blocker_uses_final_gate_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let missing_codecov = dir.path().join("missing-codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &missing_codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "codecov_config_not_current")
            .ok_or("codecov config blocker missing")?;
        assert_final_gate_commands(blocker)?;
        Ok(())
    }

    #[test]
    fn final_enforce_patch_policy_blocker_uses_final_gate_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_advisory_patch_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "patch_coverage_policy_not_enforcing")
            .ok_or("patch policy blocker missing")?;
        assert_eq!(
            blocker.get("path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_final_gate_commands(blocker)?;
        Ok(())
    }

    #[test]
    fn final_enforce_codecov_comment_blocker_uses_final_gate_commands() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let review = dir.path().join("review-comments.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_ripr_plus_receipt(&ripr, 0)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_empty_review_guidance(&review)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_non_actionable_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &review,
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker = next_action(&receipt, "codecov_comment_not_actionable")
            .ok_or("Codecov comment blocker missing")?;
        assert_eq!(
            blocker.get("path").and_then(Value::as_str),
            Some(display_path(&codecov).as_str())
        );
        assert_final_gate_commands(blocker)?;
        Ok(())
    }

    #[test]
    fn final_enforce_blockers_include_receipts_and_top_file_guidance() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_ripr_plus_receipt(&ripr, 7)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_project_coverage_receipt(&coverage)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert_blocking_actions_have_repair_contract(&receipt)?;
        assert!(!next_actions_contain(&receipt, "ripr_seam_cluster"));
        assert!(!next_actions_contain(&receipt, "project_coverage_gap"));
        let ripr_blocker =
            next_action(&receipt, "ripr_total_not_zero").ok_or("RIPR total blocker missing")?;
        assert_eq!(
            ripr_blocker.get("path").and_then(Value::as_str),
            Some(display_path(&ripr).as_str())
        );
        let expected_ripr_receipt =
            format!("rtk cargo xtask ripr-plus --receipt {}", display_path(&ripr));
        assert_eq!(
            ripr_blocker.get("receipt").and_then(Value::as_str),
            Some(expected_ripr_receipt.as_str())
        );
        let ripr_verify =
            ripr_blocker.get("verify").and_then(Value::as_str).ok_or("RIPR verify missing")?;
        assert!(ripr_verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(ripr_verify.contains("--codecov"));
        assert!(ripr_verify.contains("--check"));
        assert_eq!(
            ripr_blocker.pointer("/top_files/0/name").and_then(Value::as_str),
            Some("crates/perl-lexer/src/lib.rs")
        );
        assert_eq!(
            ripr_blocker.get("suggested_test").and_then(Value::as_str),
            Some(RIPR_SEAM_SUGGESTED_TEST)
        );
        assert_eq!(
            ripr_blocker.pointer("/top_files/0/sample_seams/0/gap_id").and_then(Value::as_str),
            Some("RIPR-SPEC-0007")
        );

        let coverage_blocker = next_action(&receipt, "project_coverage_below_target")
            .ok_or("project coverage blocker missing")?;
        assert_eq!(
            coverage_blocker.get("path").and_then(Value::as_str),
            Some(display_path(&coverage).as_str())
        );
        let codecov_config = receipt
            .pointer("/coverage/codecov_config")
            .and_then(Value::as_str)
            .unwrap_or(CODECOV_CONFIG_PATH);
        let expected_coverage_receipt = format!(
            "rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt {} --codecov {}",
            display_path(&coverage),
            codecov_config
        );
        assert_eq!(
            coverage_blocker.get("receipt").and_then(Value::as_str),
            Some(expected_coverage_receipt.as_str())
        );
        let coverage_verify = coverage_blocker
            .get("verify")
            .and_then(Value::as_str)
            .ok_or("coverage verify missing")?;
        assert!(coverage_verify.contains("cargo xtask quality-gate --mode enforce"));
        assert!(coverage_verify.contains("--codecov"));
        assert!(coverage_verify.contains("--check"));
        assert_eq!(
            coverage_blocker.pointer("/top_files/0/path").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(
            coverage_blocker.get("suggested_test").and_then(Value::as_str),
            Some(COVERAGE_GAP_SUGGESTED_TEST)
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-lexer/src/lib.rs"));
        assert!(markdown.contains("unresolved 7"));
        assert!(markdown.contains(
            "- `RIPR-SPEC-0007` line `42` kind `predicate_boundary` seam `lex_segment` reason lexer boundary branch is unobserved; suggested test: prove lexer boundary branch"
        ));
        assert!(markdown.contains("crates/perl-parser/src/lib.rs"));
        assert!(markdown.contains("line coverage 40.00%"));
        Ok(())
    }

    #[test]
    fn final_enforce_filters_ripr_guidance_to_actionable_sample_seams() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let codecov = dir.path().join("codecov.yml");
        let output = dir.path().join("quality-gate.json");
        write_mixed_ripr_plus_receipt(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_coverage_receipt_with_patch(&coverage, 96.0)?;
        write_final_codecov_config(&codecov)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: &codecov,
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let blocker =
            next_action(&receipt, "ripr_total_not_zero").ok_or("RIPR total blocker missing")?;
        assert_eq!(
            blocker.pointer("/top_files/0/name").and_then(Value::as_str),
            Some("crates/perl-lexer/src/lib.rs")
        );
        assert_eq!(
            blocker.pointer("/top_files/0/sample_seams/0/gap_id").and_then(Value::as_str),
            Some("RIPR-SPEC-0007")
        );
        assert_eq!(
            blocker.pointer("/top_files/0/sample_seams/0/line").and_then(Value::as_u64),
            Some(42)
        );
        assert!(
            blocker.pointer("/top_files/0/sample_seams/1").is_none(),
            "blocking guidance should not render incomplete sample seams as repair targets"
        );
        assert!(
            blocker.pointer("/raw_top_files/0/sample_seams/1/line").is_some(),
            "raw receipt evidence should remain available separately"
        );
        Ok(())
    }

    #[test]
    fn final_enforce_fails_when_ripr_plus_receipt_has_wrong_schema() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        write_wrong_schema_ripr_plus_receipt(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_project_coverage_receipt(&coverage)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: None,
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        assert!(
            receipt
                .pointer("/ripr_plus/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.contains("/schema_version expected 1"))
        );
        let blocker =
            next_action(&receipt, "ripr_receipt_not_current").ok_or("RIPR blocker missing")?;
        assert!(
            blocker
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("schema_version"))
        );
        Ok(())
    }

    #[test]
    fn final_enforce_prefers_actionable_ripr_files_over_deferred_raw_files() -> TestResult {
        let dir = tempfile::tempdir()?;
        let ripr = dir.path().join("ripr-plus.json");
        let ripr_pr = dir.path().join("ripr-pr.json");
        let coverage = dir.path().join("coverage.json");
        let output = dir.path().join("quality-gate.json");
        let summary = dir.path().join("quality-gate.md");
        write_classified_ripr_plus_receipt(&ripr)?;
        write_pr_receipt(&ripr_pr, 0)?;
        write_project_coverage_receipt(&coverage)?;

        let result = run(QualityGateConfig {
            mode: QualityGateMode::Enforce,
            ripr_receipt: &ripr,
            ripr_pr_receipt: &ripr_pr,
            review_receipt: &dir.path().join("missing-review.json"),
            coverage_receipt: &coverage,
            codecov: Path::new(CODECOV_CONFIG_PATH),
            patch_coverage: None,
            patch_status_source: None,
            exceptions: &dir.path().join("missing-exceptions.toml"),
            receipt: &output,
            summary: Some(&summary),
            check: false,
        });

        assert!(result.is_err());
        let receipt: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
        let ripr_blocker =
            next_action(&receipt, "ripr_total_not_zero").ok_or("RIPR total blocker missing")?;
        assert_eq!(
            ripr_blocker.pointer("/top_files/0/name").and_then(Value::as_str),
            Some("crates/perl-parser/src/lib.rs")
        );
        assert_eq!(
            ripr_blocker.pointer("/raw_top_files/0/name").and_then(Value::as_str),
            Some("archive/crates/old-parser/src/lib.rs")
        );
        assert_eq!(
            ripr_blocker.pointer("/deferred_files/0/reason").and_then(Value::as_str),
            Some("archive")
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("crates/perl-parser/src/lib.rs"));
        assert!(markdown.contains("archive/crates/old-parser/src/lib.rs"));
        assert!(markdown.contains("(archive)"));
        Ok(())
    }

    fn next_action<'a>(receipt: &'a Value, kind: &str) -> Option<&'a Value> {
        receipt.get("next_actions").and_then(Value::as_array).and_then(|actions| {
            actions.iter().find(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
        })
    }

    fn next_actions_contain(receipt: &Value, kind: &str) -> bool {
        receipt.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
            actions.iter().any(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
        })
    }

    fn next_action_count(receipt: &Value, kind: &str) -> usize {
        receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .map(|actions| {
                actions
                    .iter()
                    .filter(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
                    .count()
            })
            .unwrap_or(0)
    }

    fn assert_blocking_actions_have_repair_contract(receipt: &Value) -> TestResult {
        let actions =
            receipt.get("next_actions").and_then(Value::as_array).ok_or("missing next_actions")?;
        let blocking =
            actions.iter().filter(|action| is_blocking_action(action)).collect::<Vec<_>>();
        if blocking.is_empty() {
            return Err("receipt must contain at least one blocking action".into());
        }
        for action in blocking {
            let kind = action.get("kind").and_then(Value::as_str).unwrap_or("unknown");
            let path = action
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("blocking action {kind} missing path"))?;
            if path.trim().is_empty() {
                return Err(format!("blocking action {kind} has empty path").into());
            }
            for field in ["repair", "verify", "receipt"] {
                let value = action
                    .get(field)
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("blocking action {kind} missing {field}"))?;
                if value.trim().is_empty() {
                    return Err(format!("blocking action {kind} has empty {field}").into());
                }
                if matches!(field, "verify" | "receipt") && !value.starts_with("rtk ") {
                    return Err(
                        format!("blocking action {kind} {field} must use rtk: {value}").into()
                    );
                }
            }
        }
        Ok(())
    }

    fn assert_final_gate_commands(action: &Value) -> TestResult {
        let verify = action.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask quality-gate --mode enforce"));
        assert!(!verify.contains("--mode enforce-patch-coverage"));
        assert!(verify.contains("--codecov"));
        assert!(verify.contains("--check"));
        let receipt = action.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt.starts_with("rtk cargo xtask quality-gate --mode enforce"));
        assert!(!receipt.contains("--mode enforce-patch-coverage"));
        assert!(!receipt.contains("--check"));
        Ok(())
    }

    fn assert_enforce_new_ripr_guidance_gap_commands(action: &Value) -> TestResult {
        let verify = action.get("verify").and_then(Value::as_str).ok_or("verify missing")?;
        assert!(verify.starts_with("rtk cargo xtask quality-gate --mode enforce-new-ripr"));
        assert!(verify.contains("--check"));
        let receipt = action.get("receipt").and_then(Value::as_str).ok_or("receipt missing")?;
        assert!(receipt.starts_with("rtk cargo xtask ripr-review-comments"));
        assert!(!receipt.contains("--check"));
        Ok(())
    }

    fn assert_quality_gate_guidance_commands(action: &Value, mode: &str) -> TestResult {
        let guidance_verify = action
            .get("guidance_verify")
            .and_then(Value::as_str)
            .ok_or("guidance verify missing")?;
        assert!(
            guidance_verify.starts_with(&format!("rtk cargo xtask quality-gate --mode {mode}"))
        );
        assert!(guidance_verify.contains("--check"));
        let guidance_receipt = action
            .get("guidance_receipt")
            .and_then(Value::as_str)
            .ok_or("guidance receipt missing")?;
        assert!(
            guidance_receipt.starts_with(&format!("rtk cargo xtask quality-gate --mode {mode}"))
        );
        assert!(!guidance_receipt.contains("--check"));
        Ok(())
    }

    fn assert_local_proof_commands_are_rtk_prefixed(markdown: &str) -> TestResult {
        let section = local_proof_command_section(markdown)?;
        let command_lines =
            section.lines().filter(|line| line.trim_start().starts_with("- `")).collect::<Vec<_>>();
        if command_lines.is_empty() {
            return Err("suggested local proof commands section must list commands".into());
        }
        for line in command_lines {
            assert!(line.starts_with("- `rtk "), "local proof command must use rtk: {line}");
        }
        Ok(())
    }

    fn local_proof_command_section(markdown: &str) -> TestResult<&str> {
        markdown
            .split("Suggested local proof commands for this gate:")
            .nth(1)
            .ok_or("missing suggested local proof commands section")?
            .split("## Temporary Exceptions")
            .next()
            .ok_or_else(|| "suggested local proof commands section is unterminated".into())
    }

    fn assert_quality_gate_summary_error_names_commands(
        message: &str,
        output: &Path,
        summary: &Path,
    ) {
        assert!(
            message.contains("refresh with `rtk cargo xtask quality-gate --mode advisory"),
            "{message}"
        );
        assert!(message.contains(&format!("--receipt {}", display_path(output))), "{message}");
        assert!(message.contains(&format!("--summary {}", display_path(summary))), "{message}");
        assert!(
            message.contains("then verify with `rtk cargo xtask quality-gate --mode advisory"),
            "{message}"
        );
        assert!(message.contains(" --check`"), "{message}");
    }

    fn write_pr_receipt(path: &Path, severe_gaps: u64) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "kind": "pr_evidence",
                "scope": "diff",
                "head_sha": current_head_for_test(),
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "summary": {
                    "changed_files": 1,
                    "weakly_exposed": severe_gaps,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0,
                    "severe_gaps": severe_gaps
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_pr_receipt_without_severe_gaps(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "kind": "pr_evidence",
                "scope": "diff",
                "head_sha": current_head_for_test(),
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "summary": {
                    "changed_files": 1,
                    "weakly_exposed": 1,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_wrong_kind_pr_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "kind": "not_pr_evidence",
                "scope": "diff",
                "head_sha": current_head_for_test(),
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "summary": {
                    "changed_files": 1,
                    "weakly_exposed": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0,
                    "severe_gaps": 0
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_pr_receipt_without_base_sha(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "kind": "pr_evidence",
                "scope": "diff",
                "head_sha": current_head_for_test(),
                "base": "origin/main",
                "head": "HEAD",
                "summary": {
                    "changed_files": 1,
                    "weakly_exposed": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0,
                    "severe_gaps": 0
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_pr_receipt_with_base(path: &Path, base: &str, base_sha: &str) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "kind": "pr_evidence",
                "scope": "diff",
                "head_sha": current_head_for_test(),
                "base": base,
                "base_sha": base_sha,
                "head": "HEAD",
                "summary": {
                    "changed_files": 1,
                    "weakly_exposed": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0,
                    "severe_gaps": 0
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_ripr_plus_receipt(path: &Path, unresolved: u64) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": unresolved,
                "top_files": [
                    {
                        "name": "crates/perl-lexer/src/lib.rs",
                        "count": unresolved,
                        "sample_seams": [
                            {
                                "gap_id": "RIPR-SPEC-0007",
                                "kind": "predicate_boundary",
                                "line": 42,
                                "seam": "lex_segment",
                                "reason": "lexer boundary branch is unobserved",
                                "suggested_test": "prove lexer boundary branch"
                            }
                        ]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_mixed_ripr_plus_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": 2,
                "top_files": [
                    {
                        "name": "crates/perl-lexer/src/lib.rs",
                        "count": 2,
                        "sample_seams": [
                            {
                                "gap_id": "RIPR-SPEC-0007",
                                "kind": "predicate_boundary",
                                "line": 42,
                                "seam": "lex_segment",
                                "reason": "lexer boundary branch is unobserved",
                                "suggested_test": "prove lexer boundary branch"
                            },
                            {
                                "kind": "predicate_boundary",
                                "line": 0
                            }
                        ]
                    }
                ],
                "top_actionable_files": [
                    {
                        "name": "crates/perl-parser/src/lib.rs",
                        "count": 1,
                        "sample_seams": [
                            {
                                "kind": "return_value",
                                "line": 0
                            }
                        ]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_ripr_plus_receipt_without_unresolved(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "top_files": []
            }))?,
        )?;
        Ok(())
    }

    fn write_ripr_plus_receipt_without_file_guidance(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": 7,
                "top_files": []
            }))?,
        )?;
        Ok(())
    }

    fn write_ripr_plus_receipt_without_sample_seams(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": 7,
                "top_files": [
                    {
                        "name": "crates/perl-lexer/src/lib.rs",
                        "count": 7
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_wrong_schema_ripr_plus_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 99,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": 0,
                "top_files": []
            }))?,
        )?;
        Ok(())
    }

    fn write_classified_ripr_plus_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "ripr_plus_baseline",
                "head": current_head_for_test(),
                "unresolved": 8,
                "top_files": [
                    {
                        "name": "archive/crates/old-parser/src/lib.rs",
                        "count": 5
                    }
                ],
                "top_actionable_files": [
                    {
                        "name": "crates/perl-parser/src/lib.rs",
                        "count": 3,
                        "sample_seams": [
                            {
                                "gap_id": "RIPR-SPEC-0044",
                                "kind": "return_value",
                                "line": 77,
                                "seam": "parse_stmt",
                                "reason": "parser return branch is weakly exposed",
                                "suggested_test": "prove parser return branch"
                            }
                        ]
                    }
                ],
                "deferred_files": [
                    {
                        "name": "archive/crates/old-parser/src/lib.rs",
                        "count": 5,
                        "reason": "archive"
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt(
        path: &Path,
        target: &str,
        threshold: &str,
        informational: Option<bool>,
    ) -> TestResult {
        write_coverage_receipt_with_head(
            path,
            target,
            threshold,
            informational,
            &current_head_for_test(),
        )
    }

    fn write_coverage_receipt_with_head(
        path: &Path,
        target: &str,
        threshold: &str,
        informational: Option<bool>,
        head: &str,
    ) -> TestResult {
        let mut patch = json!({
            "target": target,
            "threshold": threshold,
            "if_ci_failed": "error"
        });
        if let Some(informational) = informational
            && let Some(object) = patch.as_object_mut()
        {
            object.insert("informational".to_string(), json!(informational));
        }
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": head,
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": patch
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0)
            }))?,
        )?;
        Ok(())
    }

    fn workspace_coverage_scope() -> Value {
        let roots = required_coverage_roots().unwrap_or_else(|_| {
            vec![
                "crates/perl-parser".to_string(),
                "crates/perl-lsp-rs".to_string(),
                "xtask".to_string(),
            ]
        });
        let source_files = u64::try_from(roots.len()).unwrap_or(0);
        json!({
            "kind": "workspace",
            "source_files": source_files,
            "roots": roots.clone(),
            "required_roots": roots,
            "missing_required_roots": []
        })
    }

    fn parser_only_coverage_scope() -> Value {
        json!({
            "kind": "partial",
            "source_files": 1,
            "roots": ["crates/perl-parser"],
            "required_roots": ["crates/perl-parser", "crates/perl-lsp-rs", "xtask"],
            "missing_required_roots": ["crates/perl-lsp-rs", "xtask"]
        })
    }

    fn stale_workspace_coverage_scope() -> Value {
        json!({
            "kind": "workspace",
            "source_files": 2,
            "roots": ["crates/perl-parser", "xtask"],
            "required_roots": ["crates/perl-parser", "xtask"],
            "missing_required_roots": []
        })
    }

    fn extra_stale_workspace_coverage_scope() -> Value {
        let roots = required_coverage_roots().unwrap_or_else(|_| {
            vec![
                "crates/perl-parser".to_string(),
                "crates/perl-lsp-rs".to_string(),
                "xtask".to_string(),
            ]
        });
        let mut receipt_required_roots = roots.clone();
        receipt_required_roots.push("crates/removed-proof-crate".to_string());
        receipt_required_roots.sort();
        receipt_required_roots.dedup();
        let source_files = u64::try_from(roots.len()).unwrap_or(0);
        json!({
            "kind": "workspace",
            "source_files": source_files,
            "roots": roots,
            "required_roots": receipt_required_roots,
            "missing_required_roots": []
        })
    }

    fn write_advisory_patch_codecov_config(path: &Path) -> TestResult {
        fs::write(
            path,
            r#"coverage:
  status:
    patch:
      default:
        target: 75%
        threshold: 2%
        if_ci_failed: error
        informational: true
    project:
      default:
        target: 95%
        threshold: 2%
        if_ci_failed: error
        informational: true
comment:
  layout: "reach,diff,flags,files"
  behavior: default
  require_head: true
"#,
        )?;
        Ok(())
    }

    fn write_non_actionable_codecov_config(path: &Path) -> TestResult {
        fs::write(
            path,
            r#"coverage:
  status:
    patch:
      default:
        target: 95%
        threshold: 0%
        if_ci_failed: error
    project:
      default:
        target: 95%
        threshold: 2%
        if_ci_failed: error
        informational: true
comment:
  layout: "reach,flags"
  behavior: default
  require_head: false
"#,
        )?;
        Ok(())
    }

    fn write_final_codecov_config(path: &Path) -> TestResult {
        fs::write(
            path,
            r#"coverage:
  status:
    patch:
      default:
        target: 95%
        threshold: 0%
        if_ci_failed: error
    project:
      default:
        target: 95%
        threshold: 0.25%
        if_ci_failed: error
comment:
  layout: "reach,diff,flags,files"
  behavior: default
  require_head: true
"#,
        )?;
        Ok(())
    }

    fn write_wrong_kind_coverage_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "not_coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0)
            }))?,
        )?;
        Ok(())
    }

    fn write_project_coverage_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": 96.0
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(88, 100, 88.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [12, 13, 17]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt_without_line_coverage(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": 96.0
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": {
                    "line_hit": 96,
                    "line_found": 100
                }
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt_with_patch(path: &Path, patch: f64) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": patch
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [12, 13, 17]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt_with_patch_and_non_actionable_files(
        path: &Path,
        patch: f64,
    ) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": patch
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [0]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt_with_patch_and_mixed_line_samples(
        path: &Path,
        patch: f64,
    ) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": patch
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [0, 12, 13]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_coverage_receipt_with_advisory_project_policy(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": 96.0
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": {
                            "target": "95%",
                            "threshold": "2%",
                            "if_ci_failed": "error",
                            "informational": true
                        }
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0)
            }))?,
        )?;
        Ok(())
    }

    fn write_parser_only_coverage_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "coverage": {
                    "patch": 96.0
                },
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": parser_only_coverage_scope(),
                "measured": measured_coverage(96, 100, 96.0)
            }))?,
        )?;
        Ok(())
    }

    fn write_low_coverage_receipt(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "kind": "coverage_baseline",
                "head": current_head_for_test(),
                "lcov": "target/lcov.info",
                "codecov_status": {
                    "patch": {
                        "default": {
                            "target": "95%",
                            "threshold": "0%",
                            "if_ci_failed": "error"
                        }
                    },
                    "project": {
                        "default": final_codecov_project_status()
                    }
                },
                "codecov_comment": actionable_codecov_comment(),
                "coverage_scope": workspace_coverage_scope(),
                "measured": measured_coverage(88, 100, 88.0),
                "files_below_target": [
                    {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line_hit": 4,
                        "line_found": 10,
                        "line_coverage": 40.0,
                        "sample_uncovered_lines": [12, 13, 17]
                    }
                ]
            }))?,
        )?;
        Ok(())
    }

    fn write_review_guidance(path: &Path) -> TestResult {
        write_review_guidance_with_head(path, &current_head_for_test())
    }

    fn write_empty_review_guidance(path: &Path) -> TestResult {
        write_empty_review_guidance_with_head(path, &current_head_for_test())
    }

    fn write_incomplete_review_guidance(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "head_sha": current_head_for_test(),
                "summary": {
                    "comments": 1,
                    "summary_only": 0,
                    "suppressed": 0
                },
                "comments": [
                    {
                        "canonical_gap_id": "RIPR-SPEC-0042",
                        "kind": "focused_test",
                        "reason": "changed parser branch has only weak proof"
                    }
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        Ok(())
    }

    fn write_review_guidance_without_seam(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "head_sha": current_head_for_test(),
                "summary": {
                    "comments": 1,
                    "summary_only": 0,
                    "suppressed": 0
                },
                "comments": [
                    {
                        "canonical_gap_id": "RIPR-SPEC-0042",
                        "kind": "focused_test",
                        "reason": "changed parser branch has only weak proof",
                        "placement": {
                            "path": "crates/perl-parser/src/lib.rs",
                            "line": 42
                        },
                        "suggested_test": {
                            "intent": "prove parser branch recovery"
                        }
                    }
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        Ok(())
    }

    fn write_review_guidance_with_zero_line(path: &Path) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "head_sha": current_head_for_test(),
                "summary": {
                    "comments": 1,
                    "summary_only": 0,
                    "suppressed": 0
                },
                "comments": [
                    {
                        "canonical_gap_id": "RIPR-SPEC-0042",
                        "kind": "focused_test",
                        "reason": "changed parser branch has only weak proof",
                        "placement": {
                            "path": "crates/perl-parser/src/lib.rs",
                            "line": 0,
                            "mode": "exact_seam_line"
                        },
                        "suggested_test": {
                            "intent": "prove parser branch recovery"
                        }
                    }
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        Ok(())
    }

    fn write_empty_review_guidance_with_head(path: &Path, head_sha: &str) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "base": "origin/main",
                "base_sha": current_base_for_test("origin/main"),
                "head": "HEAD",
                "head_sha": head_sha,
                "summary": {
                    "comments": 0,
                    "summary_only": 0,
                    "suppressed": 0
                },
                "comments": [],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        Ok(())
    }

    fn write_review_guidance_with_head(path: &Path, head_sha: &str) -> TestResult {
        write_review_guidance_with_base_and_head(
            path,
            "origin/main",
            &current_base_for_test("origin/main"),
            head_sha,
        )
    }

    fn write_review_guidance_with_base(path: &Path, base: &str, base_sha: &str) -> TestResult {
        write_review_guidance_with_base_and_head(path, base, base_sha, &current_head_for_test())
    }

    fn write_review_guidance_with_base_and_head(
        path: &Path,
        base: &str,
        base_sha: &str,
        head_sha: &str,
    ) -> TestResult {
        fs::write(
            path,
            serde_json::to_string_pretty(&json!({
                "schema_version": "0.1",
                "tool": "ripr",
                "status": "advisory",
                "base": base,
                "base_sha": base_sha,
                "head": "HEAD",
                "head_sha": head_sha,
                "summary": {
                    "comments": 1,
                    "summary_only": 0,
                    "suppressed": 0
                },
                "comments": [
                    {
                        "canonical_gap_id": "RIPR-SPEC-0042",
                        "kind": "focused_test",
                        "severity": "severe",
                        "reason": "changed parser branch has only weak proof",
                        "placement": {
                            "path": "crates/perl-parser/src/lib.rs",
                            "line": 42,
                            "mode": "exact_seam_line"
                        },
                        "suggested_test": {
                            "intent": "prove parser branch recovery"
                        }
                    }
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            }))?,
        )?;
        Ok(())
    }

    fn write_exception_policy(path: &Path) -> TestResult {
        fs::write(
            path,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "2026-05-26"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Existing RIPR+ debt is being burned down while new gaps are blocking."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after total RIPR+ is zero on main."
review_after = "2026-06-07"
expires = "2026-08-07"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Project coverage is below target during the burn-down."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after project coverage reaches 95% and is blocking."
review_after = "2026-06-07"
expires = "2026-08-07"
"#,
        )?;
        Ok(())
    }

    fn write_exception_policy_missing_receipt_evidence(path: &Path) -> TestResult {
        fs::write(
            path,
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "2026-05-26"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Existing RIPR+ debt is being burned down."
final_target = "ripr_plus.unresolved == 0"
current_evidence = ["target/receipts/quality/quality-gate.json"]
removal_criteria = "Remove after proof is green."
review_after = "2026-06-07"
expires = "2026-08-07"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Project coverage is below target during burn-down."
final_target = "coverage.project >= 95.0"
current_evidence = ["target/receipts/quality/coverage-quality-gate.json"]
removal_criteria = "Remove after proof is green."
review_after = "2026-06-07"
expires = "2026-08-07"
"#,
        )?;
        Ok(())
    }

    fn actionable_codecov_comment() -> Value {
        json!({
            "layout": "reach,diff,flags,files",
            "behavior": "default",
            "require_head": true
        })
    }

    fn measured_coverage(line_hit: u64, line_found: u64, line_coverage: f64) -> Value {
        json!({
            "line_hit": line_hit,
            "line_found": line_found,
            "line_coverage": line_coverage
        })
    }

    fn final_codecov_project_status() -> Value {
        json!({
            "target": "95%",
            "threshold": "0.25%",
            "if_ci_failed": "error"
        })
    }

    fn current_head_for_test() -> String {
        git_head().unwrap_or_else(|| "test-head".to_string())
    }

    fn current_base_for_test(base: &str) -> String {
        git_rev_parse(base).unwrap_or_else(|| "test-base".to_string())
    }
}
