//! `cargo xtask agent ledgers validate` — orchestration ledger validator.
//!
//! Validates `docs/agents/ledgers/*.jsonl`. The directory is **heterogeneous**: it
//! holds several kinds of append-only ledger, each with its own row contract. Every
//! ledger file therefore declares which contract it uses, on a header directive:
//!
//! ```text
//! #!ledger-schema: workflow-outcome.v1
//! ```
//!
//! The directive must be the first non-blank line and may appear once. A `.jsonl`
//! file with no directive, or one naming an unregistered schema, is an error: a new
//! ledger of an unanticipated shape fails closed rather than being silently judged by
//! another ledger's contract.
//!
//! Rows are decoded into typed `#[serde(deny_unknown_fields)]` structs, so unknown
//! fields and wrong types are rejected structurally, then checked for the semantic
//! rules their contract adds (enum vocabularies, non-empty strings, ISO dates, and
//! the conditional `close_proof` requirement).
//!
//! # Registered schemas
//!
//! | Id | Rows | Contract |
//! |---|---|---|
//! | `pr-triage.v1` | PR reconciliation worklist | the shape emitted by [`crate::tasks::pr_ledger`] |
//! | `workflow-outcome.v1` | one workflow execution | `docs/agents/workflow-outcome.schema.json` |
//! | `ub-review-calibration.v1` | one UB-review calibration datum | `docs/agents/WORKFLOW_TEMPLATES.md` Workflow 6 + `docs/ci/ub-review-adoption-notes.md` |
//!
//! # Exit codes
//! - `0` — all lines valid.
//! - `1` — at least one validation error.
//!
//! # Output formats
//! - Human (default): per-error lines to stderr + summary to stdout.
//! - JSON (`--format json`): `{"ok": bool, "errors": [...]}` to stdout.

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public config
// ---------------------------------------------------------------------------

/// Format for `agent-ledgers validate` output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidateFormat {
    Human,
    Json,
}

/// Configuration for `agent-ledgers validate`.
pub struct ValidateConfig {
    /// Directory containing `*.jsonl` ledger files. Defaults to
    /// `docs/agents/ledgers/` relative to the project root.
    pub ledger_dir: Option<PathBuf>,
    /// Output format.
    pub format: ValidateFormat,
    /// When set, every ledger file must declare this schema id. Callers use this to
    /// pin the contract they expect instead of accepting whatever each file declares.
    pub expected_schema: Option<String>,
}

// ---------------------------------------------------------------------------
// Schema registry
// ---------------------------------------------------------------------------

/// Header directive naming the row contract a ledger file uses.
const SCHEMA_DIRECTIVE: &str = "#!ledger-schema:";

/// A registered ledger row contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerSchemaId {
    /// PR reconciliation worklist rows.
    PrTriageV1,
    /// Workflow outcome rows, governed by `docs/agents/workflow-outcome.schema.json`.
    WorkflowOutcomeV1,
    /// UB-review calibration datums.
    UbReviewCalibrationV1,
}

impl LedgerSchemaId {
    /// Every registered schema, in the order reported to operators.
    const ALL: &'static [LedgerSchemaId] =
        &[Self::PrTriageV1, Self::WorkflowOutcomeV1, Self::UbReviewCalibrationV1];

    /// The directive text for this schema.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrTriageV1 => "pr-triage.v1",
            Self::WorkflowOutcomeV1 => "workflow-outcome.v1",
            Self::UbReviewCalibrationV1 => "ub-review-calibration.v1",
        }
    }

    /// Parse a directive value into a registered schema.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.as_str() == value)
    }

    /// Comma-separated list of registered ids, for error messages.
    fn registered() -> String {
        Self::ALL.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", ")
    }
}

impl fmt::Display for LedgerSchemaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Row contracts
// ---------------------------------------------------------------------------

/// Valid values for the pr-triage `classification` field.
const VALID_CLASSIFICATIONS: &[&str] = &[
    "unclassified",
    "builder-ready",
    "in-build",
    "in-review",
    "merge-ready",
    "close-superseded",
    "duplicate-of-merged",
    "already-fixed",
    "deferred",
    "needs-plan-review",
    "needs-builder-fix",
    "needs-ci-fix",
    "needs-diff-fix",
];

/// Classifications that require a `close_proof` field.
///
/// This rule prevents the 29-PR false-close class recorded in
/// `docs/agents/ledgers/workflow-outcomes.jsonl` and governed by
/// `docs/agents/CLOSE_PROOF_POLICY.md`. Do not relax it.
const CLOSE_PROOF_REQUIRED: &[&str] = &["close-superseded", "duplicate-of-merged"];

/// Valid values for the pr-triage `confidence` field.
const VALID_CONFIDENCES: &[&str] = &["high", "medium", "low"];

/// Valid values for the ub-review `classification` field.
///
/// Sourced from `docs/agents/WORKFLOW_TEMPLATES.md` Workflow 6 (TP / FP / quiet /
/// infra) in the spelling the committed ledger uses.
const VALID_UB_CLASSIFICATIONS: &[&str] =
    &["true-positive", "false-positive", "expected-quiet", "infra-excluded"];

/// Valid values for the ub-review `value` field.
const VALID_UB_VALUES: &[&str] = &["high", "medium", "low", "n/a"];

/// Accept an absent field, but reject an explicit JSON `null`.
///
/// `serde` resolves both a missing key and an explicit `null` to `None` for
/// `Option<T>`. These contracts permit omission but not null, so whenever the key is
/// present the value is decoded as `T` and a `null` fails with a type error.
fn absent_or<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// The structured `close_proof` contract governed by `docs/agents/CLOSE_PROOF_POLICY.md`.
///
/// Governed by `docs/agents/CLOSE_PROOF_POLICY.md`: landing ancestry alone never
/// authorizes a close, so the receipt and the separate semantic-completion evidence
/// are both required.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredCloseProof {
    command: String,
    receipt: LandingProofReceipt,
    semantic_completion_evidence: String,
    verified_date: String,
}

/// The `landing_proof.v1` receipt carried inside a structured close proof.
///
/// `content_survives` is optional in the schema and carries no rule of its own;
/// declaring it is what makes it *accepted* under `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct LandingProofReceipt {
    schema_version: String,
    commit_reachable: bool,
    commit: String,
    canonical_main: String,
    #[serde(default, deserialize_with = "absent_or")]
    content_survives: Option<bool>,
    semantic_completion: String,
}

/// One PR reconciliation worklist row.
///
/// Mirrors the shape [`crate::tasks::pr_ledger`] emits; the generator-provided
/// descriptive fields are optional so a hand-written worklist need not carry them.
///
/// Those descriptive fields carry no rule beyond their type: under
/// `deny_unknown_fields`, declaring them is what makes a generated worklist row
/// acceptable at all.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct PrTriageRow {
    pr: String,
    title: String,
    classification: String,
    confidence: String,
    evidence: Vec<String>,
    cleanup_done: bool,
    known_gaps: Vec<String>,
    /// Kept as a raw `Value` so absent, `null`, prose, and structured close-proof
    /// forms are distinguishable; see [`check_close_proof`].
    /// `absent_or` is required because `Option<Value>` alone would fold an explicit
    /// `null` back into `None`, losing the missing-versus-null distinction.
    #[serde(default, deserialize_with = "absent_or")]
    close_proof: Option<Value>,
    #[serde(default, deserialize_with = "absent_or")]
    surface_guess: Option<String>,
    #[serde(default, deserialize_with = "absent_or")]
    is_draft: Option<bool>,
    #[serde(default, deserialize_with = "absent_or")]
    mergeable: Option<String>,
    #[serde(default, deserialize_with = "absent_or")]
    head_ref: Option<String>,
    #[serde(default, deserialize_with = "absent_or")]
    author: Option<String>,
}

/// One workflow outcome row.
///
/// Conforms to `docs/agents/workflow-outcome.schema.json`: the 15 required
/// properties plus optional `notes`, with `additionalProperties: false`. Counters are
/// unsigned so the schema's `minimum: 0` holds by construction — their declared type
/// *is* their rule, so they need no further check.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WorkflowOutcomeRow {
    date: String,
    workflow_type: String,
    repo: String,
    agents_used: u64,
    model_mix: BTreeMap<String, u64>,
    items_processed: u64,
    merged: u64,
    closed_with_proof: u64,
    deferred: u64,
    false_closes_prevented: u64,
    ci_failures_diagnosed: u64,
    upstream_gaps_filed: u64,
    builders_dispatched: u64,
    known_gaps: Vec<String>,
    cleanup_done: bool,
    #[serde(default, deserialize_with = "absent_or")]
    notes: Option<String>,
}

/// One UB-review calibration datum.
///
/// `pr` is nullable: infra-excluded and expected-quiet datums are not bound to a
/// single PR. `category` is a free-form string rather than a closed vocabulary — the
/// taxonomy in `docs/ci/ub-review-adoption-notes.md` is a recording guide, and the
/// committed ledger already carries the compound value `proof-gap/docs-drift`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UbReviewCalibrationRow {
    date: String,
    /// Required but nullable. Typed as `Value` rather than `Option<u64>` because
    /// serde resolves a *missing* `Option` field to `None`, which would let the key
    /// be omitted entirely; the null-vs-absent distinction is checked below.
    pr: Value,
    runner: String,
    profile: String,
    classification: String,
    category: String,
    value: String,
    evidence: String,
    action_taken: String,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// A single per-line validation error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerError {
    /// Ledger file path (relative to project root).
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// Error description.
    pub message: String,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

/// Output structure for `--format json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateOutput {
    pub ok: bool,
    pub files_checked: usize,
    pub lines_checked: usize,
    pub errors: Vec<LedgerError>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn validate(config: ValidateConfig) -> Result<()> {
    let ledger_dir = resolve_ledger_dir(config.ledger_dir)?;

    let expected_schema = match config.expected_schema.as_deref() {
        None => None,
        Some(raw) => Some(LedgerSchemaId::parse(raw).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "unknown --expected-schema `{raw}`; registered schemas: {}",
                LedgerSchemaId::registered()
            )
        })?),
    };

    let jsonl_files = collect_jsonl_files(&ledger_dir)?;

    let mut all_errors: Vec<LedgerError> = Vec::new();
    let mut total_lines: usize = 0;

    for path in &jsonl_files {
        let rel = relative_display(path);
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading ledger file {}", path.display()))?;
        let (lines_checked, errors) = validate_file(&content, &rel, expected_schema);
        total_lines += lines_checked;
        all_errors.extend(errors);
    }

    let output = ValidateOutput {
        ok: all_errors.is_empty(),
        files_checked: jsonl_files.len(),
        lines_checked: total_lines,
        errors: all_errors,
    };

    match config.format {
        ValidateFormat::Json => {
            let json = serde_json::to_string_pretty(&output).context("serializing JSON output")?;
            println!("{json}");
        }
        ValidateFormat::Human => {
            for e in &output.errors {
                eprintln!("ERROR  {e}");
            }
            if output.ok {
                println!(
                    "OK  {} file(s), {} line(s) valid",
                    output.files_checked, output.lines_checked
                );
            } else {
                println!(
                    "FAIL  {} error(s) in {} file(s), {} line(s) checked",
                    output.errors.len(),
                    output.files_checked,
                    output.lines_checked
                );
            }
        }
    }

    if !output.ok {
        // Use std::process::exit only from bin/ normally; here we propagate via
        // an eyre error so the caller (main.rs) returns Err.
        color_eyre::eyre::bail!("ledger validation failed with {} error(s)", output.errors.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File-level validation
// ---------------------------------------------------------------------------

/// Validate one ledger file's contents. Returns the number of rows checked and any
/// errors. A file whose schema cannot be resolved reports that and checks no rows —
/// there is no contract to check them against.
fn validate_file(
    content: &str,
    file: &str,
    expected_schema: Option<LedgerSchemaId>,
) -> (usize, Vec<LedgerError>) {
    // A tool that writes the file with a UTF-8 BOM would otherwise hide the directive
    // behind a non-whitespace character that `str::trim` does not remove.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    let (schema, directive_line) = match resolve_schema(content, file) {
        Ok(resolved) => resolved,
        Err(errors) => return (0, errors),
    };

    if let Some(expected) = expected_schema
        && schema != expected
    {
        return (
            0,
            vec![LedgerError {
                file: file.to_string(),
                line: directive_line,
                message: format!("declares schema `{schema}` but `{expected}` was required"),
            }],
        );
    }

    let mut errors = Vec::new();
    let mut rows = 0;
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue; // blank lines, comments, and the schema directive
        }
        rows += 1;
        errors.extend(validate_line(trimmed, file, idx + 1, schema));
    }

    // A declared ledger carrying no rows is far more likely a truncated write than a
    // deliberate empty file, and silently passing it is the one shape that would
    // still validate vacuously.
    if rows == 0 {
        errors.push(LedgerError {
            file: file.to_string(),
            line: directive_line,
            message: format!("ledger declares schema `{schema}` but contains no rows"),
        });
    }

    (rows, errors)
}

/// Read the `#!ledger-schema:` header directive. Returns the schema and the 1-based
/// line it was declared on.
fn resolve_schema(content: &str, file: &str) -> Result<(LedgerSchemaId, usize), Vec<LedgerError>> {
    let err =
        |line: usize, message: String| vec![LedgerError { file: file.to_string(), line, message }];

    let mut first_content_line: Option<usize> = None;
    let mut directives: Vec<(usize, String)> = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if first_content_line.is_none() {
            first_content_line = Some(idx + 1);
        }
        if let Some(value) = trimmed.strip_prefix(SCHEMA_DIRECTIVE) {
            directives.push((idx + 1, value.trim().to_string()));
        }
    }

    let (line, value) = match directives.as_slice() {
        [] => {
            return Err(err(
                first_content_line.unwrap_or(1),
                format!(
                    "no `{SCHEMA_DIRECTIVE} <id>` header directive; \
                     every ledger file must declare its row contract. \
                     Registered schemas: {}",
                    LedgerSchemaId::registered()
                ),
            ));
        }
        [only] => only.clone(),
        [_, second, ..] => {
            return Err(err(
                second.0,
                format!(
                    "duplicate `{SCHEMA_DIRECTIVE}` directive; a ledger file declares one schema"
                ),
            ));
        }
    };

    if first_content_line != Some(line) {
        return Err(err(
            line,
            format!(
                "`{SCHEMA_DIRECTIVE}` must be the first non-blank line of the file, \
                 not line {line}"
            ),
        ));
    }

    match LedgerSchemaId::parse(&value) {
        Some(schema) => Ok((schema, line)),
        None => Err(err(
            line,
            format!(
                "unregistered ledger schema `{value}`; registered schemas: {}",
                LedgerSchemaId::registered()
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Line-level validation
// ---------------------------------------------------------------------------

fn validate_line(
    line: &str,
    file: &str,
    lineno: usize,
    schema: LedgerSchemaId,
) -> Vec<LedgerError> {
    let into_errors = |messages: Vec<String>| -> Vec<LedgerError> {
        messages
            .into_iter()
            .map(|message| LedgerError { file: file.to_string(), line: lineno, message })
            .collect()
    };

    let parsed: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return into_errors(vec![format!("invalid JSON: {e}")]),
    };

    if !parsed.is_object() {
        return into_errors(vec!["line must be a JSON object".to_string()]);
    }

    let messages = match schema {
        LedgerSchemaId::PrTriageV1 => decode_and_check::<PrTriageRow>(parsed, check_pr_triage),
        LedgerSchemaId::WorkflowOutcomeV1 => {
            decode_and_check::<WorkflowOutcomeRow>(parsed, check_workflow_outcome)
        }
        LedgerSchemaId::UbReviewCalibrationV1 => {
            decode_and_check::<UbReviewCalibrationRow>(parsed, check_ub_review_calibration)
        }
    };

    into_errors(messages)
}

/// Validate one row against the `pr-triage.v1` contract.
///
/// Crate seam for the generator ([`crate::tasks::pr_ledger`]): the reconciliation
/// worklist is written under `target/` and never committed, so the batch validator
/// that globs `docs/agents/ledgers/` never sees it (#15557). The generator calls
/// this at the only seam that always executes: row emission.
pub(crate) fn validate_pr_triage_row(line: &str) -> Vec<String> {
    validate_line(line, "<generated>", 0, LedgerSchemaId::PrTriageV1)
        .into_iter()
        .map(|error| error.message)
        .collect()
}

/// Decode one row into its typed contract, then apply that contract's semantic rules.
///
/// Structural failures (unknown field, wrong type, missing field) come from serde and
/// stop the row; semantic rules only run against a well-formed row.
fn decode_and_check<T>(value: Value, check: fn(&T) -> Vec<String>) -> Vec<String>
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_value::<T>(value) {
        Ok(row) => check(&row),
        Err(e) => vec![e.to_string()],
    }
}

fn check_pr_triage(row: &PrTriageRow) -> Vec<String> {
    let mut errors = Vec::new();

    for (field, value) in [("pr", &row.pr), ("title", &row.title)] {
        if value.trim().is_empty() {
            errors.push(format!("field `{field}` must not be empty"));
        }
    }

    if !VALID_CLASSIFICATIONS.contains(&row.classification.as_str()) {
        errors.push(format!(
            "unknown classification `{}`; valid values: {}",
            row.classification,
            VALID_CLASSIFICATIONS.join(", ")
        ));
    }

    check_close_proof(row.close_proof.as_ref(), &row.classification, &mut errors);

    if !VALID_CONFIDENCES.contains(&row.confidence.as_str()) {
        errors.push(format!(
            "unknown confidence `{}`; valid values: {}",
            row.confidence,
            VALID_CONFIDENCES.join(", ")
        ));
    }

    errors
}

/// Validate `close_proof` for a pr-triage row.
///
/// Which forms are accepted depends on whether the classification **gates a close**:
///
/// - `close-superseded` and `duplicate-of-merged` accept **only** the structured
///   close-proof contract, checked field by field. A prose string is
///   rejected: `CLOSE_PROOF_POLICY.md` requires landing proof *and* separate
///   semantic-completion evidence, and a free string carries neither in
///   machine-checkable form.
/// - Every other classification may also carry a prose string, where `close_proof` is
///   an incidental note rather than close authorization.
fn check_close_proof(proof: Option<&Value>, classification: &str, errors: &mut Vec<String>) {
    let required = CLOSE_PROOF_REQUIRED.contains(&classification);

    match proof {
        None => {
            if required {
                errors.push(format!(
                    "classification `{classification}` requires `close_proof` field"
                ));
            }
        }
        Some(Value::Null) => {
            if required {
                errors.push(format!(
                    "classification `{classification}` requires non-null `close_proof`"
                ));
            } else {
                errors.push("`close_proof` must not be null".to_string());
            }
        }
        Some(Value::String(prose)) => {
            if prose.trim().is_empty() {
                errors.push("`close_proof` must not be empty".to_string());
            } else if required {
                // CLOSE_PROOF_POLICY.md requires landing proof *and* separate
                // semantic-completion evidence. A prose string carries neither in
                // machine-checkable form, so it cannot authorize a gated close.
                errors.push(format!(
                    "classification `{classification}` requires the structured `close_proof` \
                     object; a prose string carries no landing receipt or \
                     semantic-completion evidence (CLOSE_PROOF_POLICY.md)"
                ));
            }
        }
        Some(Value::Object(_)) => {
            let structured: StructuredCloseProof = match serde_json::from_value(
                proof.cloned().unwrap_or(Value::Null),
            ) {
                Ok(parsed) => parsed,
                Err(e) => {
                    errors.push(format!("`close_proof` object does not match the structured close-proof contract: {e}"));
                    return;
                }
            };
            errors.extend(check_structured_close_proof(&structured));
        }
        Some(_) => errors
            .push("`close_proof` must be a string or a structured close-proof object".to_string()),
    }
}

/// Enforce the constants and non-empty fields the close-proof contract declares.
fn check_structured_close_proof(proof: &StructuredCloseProof) -> Vec<String> {
    let mut errors = Vec::new();

    for (field, value) in [
        ("close_proof.command", &proof.command),
        ("close_proof.semantic_completion_evidence", &proof.semantic_completion_evidence),
        ("close_proof.receipt.canonical_main", &proof.receipt.canonical_main),
    ] {
        if value.trim().is_empty() {
            errors.push(format!("field `{field}` must not be empty"));
        }
    }

    if !is_iso_date(&proof.verified_date) {
        errors.push(format!(
            "field `close_proof.verified_date` must be an ISO 8601 date (YYYY-MM-DD), got `{}`",
            proof.verified_date
        ));
    }

    if proof.receipt.schema_version != "landing_proof.v1" {
        errors.push(format!(
            "field `close_proof.receipt.schema_version` must be `landing_proof.v1`, got `{}`",
            proof.receipt.schema_version
        ));
    }

    if !proof.receipt.commit_reachable {
        errors.push(
            "field `close_proof.receipt.commit_reachable` must be true; an unreachable commit is not landing proof"
                .to_string(),
        );
    }

    if proof.receipt.commit.trim().len() < 7 {
        errors.push(format!(
            "field `close_proof.receipt.commit` must be at least 7 characters, got `{}`",
            proof.receipt.commit
        ));
    }

    // Per CLOSE_PROOF_POLICY.md the receipt is evidence, never close authorization:
    // semantic completion is always carried separately and is never self-asserted.
    if proof.receipt.semantic_completion != "not_evaluated" {
        errors.push(format!(
            "field `close_proof.receipt.semantic_completion` must be `not_evaluated`, got `{}`",
            proof.receipt.semantic_completion
        ));
    }

    errors
}

fn check_workflow_outcome(row: &WorkflowOutcomeRow) -> Vec<String> {
    let mut errors = Vec::new();

    if !is_iso_date(&row.date) {
        errors.push(format!(
            "field `date` must be an ISO 8601 date (YYYY-MM-DD), got `{}`",
            row.date
        ));
    }

    for (field, value) in [("workflow_type", &row.workflow_type), ("repo", &row.repo)] {
        if value.trim().is_empty() {
            errors.push(format!("field `{field}` must not be empty"));
        }
    }

    if row.known_gaps.iter().any(|gap| gap.trim().is_empty()) {
        errors.push("`known_gaps` entries must not be empty".to_string());
    }

    errors
}

fn check_ub_review_calibration(row: &UbReviewCalibrationRow) -> Vec<String> {
    let mut errors = Vec::new();

    match &row.pr {
        Value::Null => {}
        Value::Number(n) if n.as_u64().is_some_and(|v| v > 0) => {}
        other => {
            errors.push(format!("field `pr` must be a positive integer or null, got `{other}`"))
        }
    }

    if !is_iso_date(&row.date) {
        errors.push(format!(
            "field `date` must be an ISO 8601 date (YYYY-MM-DD), got `{}`",
            row.date
        ));
    }

    if !VALID_UB_CLASSIFICATIONS.contains(&row.classification.as_str()) {
        errors.push(format!(
            "unknown classification `{}`; valid values: {}",
            row.classification,
            VALID_UB_CLASSIFICATIONS.join(", ")
        ));
    }

    if !VALID_UB_VALUES.contains(&row.value.as_str()) {
        errors.push(format!(
            "unknown value `{}`; valid values: {}",
            row.value,
            VALID_UB_VALUES.join(", ")
        ));
    }

    for (field, value) in [
        ("runner", &row.runner),
        ("profile", &row.profile),
        ("category", &row.category),
        ("evidence", &row.evidence),
        ("action_taken", &row.action_taken),
    ] {
        if value.trim().is_empty() {
            errors.push(format!("field `{field}` must not be empty"));
        }
    }

    errors
}

/// `YYYY-MM-DD`, matching the `pattern` both ledger JSON Schemas declare.
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| bytes[i].is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_ledger_dir(override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p);
    }
    let root = crate::utils::project_root()?;
    Ok(root.join("docs").join("agents").join("ledgers"))
}

fn collect_jsonl_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        // No ledger directory yet — not an error; just nothing to validate.
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    files.sort(); // deterministic order
    Ok(files)
}

fn relative_display(path: &Path) -> String {
    // Best-effort: strip CWD prefix for human-readable display.
    let cwd = std::env::current_dir().unwrap_or_default();
    path.strip_prefix(&cwd).unwrap_or(path).to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Context, ensure, eyre};

    // ----- helpers ----------------------------------------------------------

    /// A conforming structured close proof, per the close-proof contract (CLOSE_PROOF_POLICY.md).
    const STRUCTURED_CLOSE_PROOF: &str = r#"{"command":"cargo xtask landing-proof --commit abc1234 --canonical-main origin/main --format json","receipt":{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"},"semantic_completion_evidence":"semantic-close packet #1100","verified_date":"2026-06-07"}"#;

    const PR_TRIAGE_HEADER: &str = "#!ledger-schema: pr-triage.v1";
    const WORKFLOW_HEADER: &str = "#!ledger-schema: workflow-outcome.v1";

    fn valid_row() -> &'static str {
        r#"{"pr":"1234","title":"fix: thing","classification":"unclassified","confidence":"medium","evidence":[],"cleanup_done":false,"known_gaps":[]}"#
    }

    fn valid_workflow_row() -> &'static str {
        r#"{"date":"2026-06-05","workflow_type":"issue-triage","repo":"perl-lsp-swarm","agents_used":3,"model_mix":{"haiku":3},"items_processed":9,"merged":0,"closed_with_proof":1,"deferred":2,"false_closes_prevented":0,"ci_failures_diagnosed":0,"upstream_gaps_filed":0,"builders_dispatched":0,"known_gaps":[],"cleanup_done":true}"#
    }

    fn valid_ub_row() -> &'static str {
        r#"{"date":"2026-06-05","pr":1243,"runner":"gh-hosted","profile":"bun-ub-v0","classification":"true-positive","category":"proof-gap/docs-drift","value":"high","evidence":"sensor caught a fabricated breakdown","action_taken":"author corrected before merge"}"#
    }

    /// Validate one in-memory ledger file under `pr-triage.v1`.
    fn line_errors(line: &str) -> Vec<LedgerError> {
        validate_line(line, "test.jsonl", 1, LedgerSchemaId::PrTriageV1)
    }

    fn first_msg(line: &str) -> String {
        line_errors(line).into_iter().next().map(|e| e.message).unwrap_or_default()
    }

    /// Validate a whole in-memory ledger file (header + rows).
    fn file_errors(contents: &str) -> Vec<LedgerError> {
        validate_file(contents, "test.jsonl", None).1
    }

    fn first_file_msg(contents: &str) -> String {
        file_errors(contents).into_iter().next().map(|e| e.message).unwrap_or_default()
    }

    fn write_ledger(path: &Path, contents: &str) -> Result<()> {
        let parent =
            path.parent().ok_or_else(|| eyre!("ledger path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("creating ledger directory {}", parent.display()))?;
        fs::write(path, contents)
            .with_context(|| format!("writing ledger fixture {}", path.display()))?;
        Ok(())
    }

    // ----- A1: the validator is true of the committed ledgers ---------------

    /// The committed `docs/agents/ledgers/` tree must validate clean.
    ///
    /// This is the regression control for #15379: before per-schema dispatch the
    /// validator judged all 11 committed rows by the pr-triage contract and reported
    /// 63 errors. If the validator and the artifacts it governs ever drift apart
    /// again, this test goes red.
    #[test]
    fn test_committed_ledgers_validate_clean() -> Result<()> {
        let ledger_dir = resolve_ledger_dir(None)?;
        ensure!(
            ledger_dir.is_dir(),
            "committed ledger directory missing: {}",
            ledger_dir.display()
        );

        let files = collect_jsonl_files(&ledger_dir)?;
        ensure!(!files.is_empty(), "no committed ledgers found in {}", ledger_dir.display());

        let mut checked_rows = 0;
        for path in &files {
            let content = fs::read_to_string(path)?;
            let (rows, errors) = validate_file(&content, &relative_display(path), None);
            ensure!(errors.is_empty(), "committed ledger {} failed: {errors:?}", path.display());
            ensure!(rows > 0, "committed ledger {} declared no rows", path.display());
            checked_rows += rows;
        }

        ensure!(checked_rows >= 11, "expected at least 11 committed rows, checked {checked_rows}");
        Ok(())
    }

    /// The committed pr-triage worklist is the live artifact of the #15557
    /// consolidation: `pr-triage.v1` follows the [`crate::tasks::pr_ledger`]
    /// generator row (the sole row authority; the competing
    /// `docs/agents/pr-ledger.schema.json` was retired) and owns
    /// `docs/agents/ledgers/pr-triage.jsonl`. Pin its exact rows so the
    /// registration can never be silently inert again.
    #[test]
    fn test_committed_pr_triage_ledger_exact_rows() -> Result<()> {
        let path = resolve_ledger_dir(None)?.join("pr-triage.jsonl");
        ensure!(path.is_file(), "committed pr-triage ledger missing: {}", path.display());
        let content = fs::read_to_string(&path)?;
        let (rows, errors) = validate_file(&content, "docs/agents/ledgers/pr-triage.jsonl", None);
        ensure!(errors.is_empty(), "committed pr-triage ledger invalid: {errors:?}");
        ensure!(rows == 1, "expected exactly 1 committed pr-triage row, got {rows}");
        ensure!(content.contains("\"pr\":\"15554\""), "pr-triage row for #15554 missing");
        ensure!(
            content.contains("\"classification\":\"merge-ready\""),
            "#15554 merge-ready disposition missing"
        );
        Ok(())
    }

    /// Every committed ledger declares a registered schema.
    #[test]
    fn test_every_committed_ledger_declares_a_registered_schema() -> Result<()> {
        let ledger_dir = resolve_ledger_dir(None)?;
        for path in collect_jsonl_files(&ledger_dir)? {
            let content = fs::read_to_string(&path)?;
            let resolved = resolve_schema(&content, &relative_display(&path));
            ensure!(resolved.is_ok(), "{} has no registered schema: {resolved:?}", path.display());
        }
        Ok(())
    }

    // ----- A2: schema selection is explicit and fails closed ----------------

    #[test]
    fn test_missing_directive_is_rejected() -> Result<()> {
        let msg = first_file_msg(&format!("{}\n", valid_row()));
        ensure!(msg.contains("header directive"), "got: {msg}");
        ensure!(msg.contains("pr-triage.v1"), "error should list registered schemas, got: {msg}");
        Ok(())
    }

    /// A file of an unanticipated shape must fail closed, never be silently judged by
    /// another ledger's contract. This is the F3 negative control.
    #[test]
    fn test_unregistered_schema_is_rejected() -> Result<()> {
        let msg = first_file_msg("#!ledger-schema: some-future-ledger.v1\n{\"anything\":1}\n");
        ensure!(msg.contains("unregistered ledger schema"), "got: {msg}");
        ensure!(msg.contains("some-future-ledger.v1"), "got: {msg}");
        Ok(())
    }

    #[test]
    fn test_directive_must_be_first_non_blank_line() -> Result<()> {
        let contents = format!("# a note\n{PR_TRIAGE_HEADER}\n{}\n", valid_row());
        let msg = first_file_msg(&contents);
        ensure!(msg.contains("must be the first non-blank line"), "got: {msg}");
        Ok(())
    }

    #[test]
    fn test_duplicate_directive_is_rejected() -> Result<()> {
        let contents = format!("{PR_TRIAGE_HEADER}\n{WORKFLOW_HEADER}\n{}\n", valid_row());
        let msg = first_file_msg(&contents);
        ensure!(msg.contains("duplicate"), "got: {msg}");
        Ok(())
    }

    /// A file whose schema cannot be resolved reports exactly one error and checks no
    /// rows — there is no contract to check them against.
    #[test]
    fn test_unresolved_schema_checks_no_rows() -> Result<()> {
        let contents = format!("{}\n{}\n{}\n", valid_row(), valid_row(), valid_row());
        let (rows, errors) = validate_file(&contents, "test.jsonl", None);
        ensure!(rows == 0, "expected no rows checked, got {rows}");
        ensure!(errors.len() == 1, "expected one error, got {errors:?}");
        Ok(())
    }

    /// Leading blank lines are allowed before the directive.
    #[test]
    fn test_blank_lines_before_directive_are_allowed() -> Result<()> {
        let contents = format!("\n\n{PR_TRIAGE_HEADER}\n{}\n", valid_row());
        ensure!(
            file_errors(&contents).is_empty(),
            "expected clean, got {:?}",
            file_errors(&contents)
        );
        Ok(())
    }

    // ----- A3: rows are typed, not probed -----------------------------------

    #[test]
    fn test_unknown_field_is_rejected() -> Result<()> {
        let line = r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[],"smuggled":"value"}"#;
        let msg = first_msg(line);
        ensure!(msg.contains("unknown field"), "got: {msg}");
        ensure!(msg.contains("smuggled"), "got: {msg}");
        Ok(())
    }

    /// The F8 defect: a wrong-typed value used to pass the "present and non-empty"
    /// check through a catch-all arm.
    #[test]
    fn test_non_string_scalar_fields_are_rejected() -> Result<()> {
        for (case, line) in [
            (
                "integer pr",
                r#"{"pr":42,"title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "array title",
                r#"{"pr":"1","title":[],"classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "null pr",
                r#"{"pr":null,"title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "non-string classification",
                r#"{"pr":"1","title":"t","classification":7,"confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "non-string confidence",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":false,"evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "non-array evidence",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":"string","cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "non-bool cleanup_done",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":"yes","known_gaps":[]}"#,
            ),
            (
                "non-array known_gaps",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":"none"}"#,
            ),
            (
                "non-string evidence entry",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[7],"cleanup_done":false,"known_gaps":[]}"#,
            ),
        ] {
            let errs = line_errors(line);
            ensure!(!errs.is_empty(), "{case}: wrong-typed value was accepted");
        }
        Ok(())
    }

    #[test]
    fn test_missing_required_fields_are_named() -> Result<()> {
        for (missing, line) in [
            (
                "pr",
                r#"{"title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
            ),
            (
                "cleanup_done",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"known_gaps":[]}"#,
            ),
            (
                "known_gaps",
                r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false}"#,
            ),
        ] {
            let msg = first_msg(line);
            ensure!(msg.contains(missing), "expected `{missing}` named, got: {msg}");
        }
        Ok(())
    }

    /// The optional generator-provided fields `pr_ledger` emits must still decode.
    #[test]
    fn test_pr_ledger_generator_row_is_accepted() -> Result<()> {
        let line = r#"{"pr":"1234","title":"fix: thing","surface_guess":"xtask","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[],"is_draft":false,"mergeable":"MERGEABLE","head_ref":"feat/1234-thing","author":"EffortlessSteven"}"#;
        let errs = line_errors(line);
        ensure!(errs.is_empty(), "generator row rejected: {errs:?}");
        Ok(())
    }

    #[test]
    fn test_workflow_outcome_rejects_unknown_field_and_wrong_types() -> Result<()> {
        let unknown = r#"{"date":"2026-06-05","workflow_type":"t","repo":"r","agents_used":1,"model_mix":{},"items_processed":1,"merged":0,"closed_with_proof":0,"deferred":0,"false_closes_prevented":0,"ci_failures_diagnosed":0,"upstream_gaps_filed":0,"builders_dispatched":0,"known_gaps":[],"cleanup_done":true,"extra":1}"#;
        let errs = validate_line(unknown, "t.jsonl", 1, LedgerSchemaId::WorkflowOutcomeV1);
        ensure!(
            errs.first().is_some_and(|e| e.message.contains("unknown field")),
            "expected unknown-field rejection, got {errs:?}"
        );

        let negative = r#"{"date":"2026-06-05","workflow_type":"t","repo":"r","agents_used":-1,"model_mix":{},"items_processed":1,"merged":0,"closed_with_proof":0,"deferred":0,"false_closes_prevented":0,"ci_failures_diagnosed":0,"upstream_gaps_filed":0,"builders_dispatched":0,"known_gaps":[],"cleanup_done":true}"#;
        ensure!(
            !validate_line(negative, "t.jsonl", 1, LedgerSchemaId::WorkflowOutcomeV1).is_empty(),
            "negative counter accepted"
        );

        let bad_date = valid_workflow_row().replace("2026-06-05", "05/06/2026");
        let errs = validate_line(&bad_date, "t.jsonl", 1, LedgerSchemaId::WorkflowOutcomeV1);
        ensure!(
            errs.first().is_some_and(|e| e.message.contains("ISO 8601")),
            "expected date rejection, got {errs:?}"
        );
        Ok(())
    }

    #[test]
    fn test_ub_review_row_rules() -> Result<()> {
        // A null `pr` is legitimate: infra-excluded datums bind to no single PR.
        let null_pr = valid_ub_row().replace("\"pr\":1243", "\"pr\":null");
        ensure!(
            validate_line(&null_pr, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1).is_empty(),
            "null pr rejected"
        );

        // `pr` must still be present: null and absent are different claims.
        let absent_pr = valid_ub_row().replace("\"pr\":1243,", "");
        ensure!(
            !validate_line(&absent_pr, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1)
                .is_empty(),
            "absent pr accepted"
        );

        // ...and it must be a positive integer when it is not null.
        for bad in ["\"pr\":\"1243\"", "\"pr\":0", "\"pr\":-3", "\"pr\":1.5"] {
            let line = valid_ub_row().replace("\"pr\":1243", bad);
            ensure!(
                !validate_line(&line, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1)
                    .is_empty(),
                "{bad} accepted"
            );
        }

        let bad_class = valid_ub_row().replace("true-positive", "sort-of-positive");
        let errs = validate_line(&bad_class, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1);
        ensure!(
            errs.first().is_some_and(|e| e.message.contains("unknown classification")),
            "got {errs:?}"
        );

        let bad_value = valid_ub_row().replace("\"value\":\"high\"", "\"value\":\"enormous\"");
        let errs = validate_line(&bad_value, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1);
        ensure!(errs.first().is_some_and(|e| e.message.contains("unknown value")), "got {errs:?}");

        // `evidence` is a string here, not an array as in pr-triage.
        let array_evidence = valid_ub_row()
            .replace("\"evidence\":\"sensor caught a fabricated breakdown\"", "\"evidence\":[]");
        ensure!(
            !validate_line(&array_evidence, "t.jsonl", 1, LedgerSchemaId::UbReviewCalibrationV1)
                .is_empty(),
            "array evidence accepted for ub-review schema"
        );
        Ok(())
    }

    /// Each schema rejects the other schemas' rows — dispatch actually discriminates.
    #[test]
    fn test_schemas_reject_each_others_rows() -> Result<()> {
        let cases = [
            (LedgerSchemaId::PrTriageV1, valid_workflow_row()),
            (LedgerSchemaId::PrTriageV1, valid_ub_row()),
            (LedgerSchemaId::WorkflowOutcomeV1, valid_row()),
            (LedgerSchemaId::WorkflowOutcomeV1, valid_ub_row()),
            (LedgerSchemaId::UbReviewCalibrationV1, valid_row()),
            (LedgerSchemaId::UbReviewCalibrationV1, valid_workflow_row()),
        ];
        for (schema, row) in cases {
            let errs = validate_line(row, "t.jsonl", 1, schema);
            ensure!(!errs.is_empty(), "{schema} accepted a foreign row: {row}");
        }
        Ok(())
    }

    // ----- A4: preserved support contracts ----------------------------------

    #[test]
    fn test_valid_row_produces_no_errors() -> Result<()> {
        let errs = line_errors(valid_row());
        ensure!(errs.is_empty(), "expected no errors, got: {errs:?}");
        Ok(())
    }

    /// A structured close proof satisfies both gated classifications.
    #[test]
    fn test_gated_classifications_accept_a_structured_close_proof() -> Result<()> {
        for classification in CLOSE_PROOF_REQUIRED {
            let line = format!(
                r#"{{"pr":"42","title":"chore: drop","classification":"{classification}","confidence":"high","evidence":["landing proof"],"cleanup_done":true,"known_gaps":[],"close_proof":{STRUCTURED_CLOSE_PROOF}}}"#
            );
            let errs = line_errors(&line);
            ensure!(errs.is_empty(), "{classification}: structured proof rejected: {errs:?}");
        }
        Ok(())
    }

    /// Prose is not machine-checkable evidence, so it cannot authorize a gated close.
    ///
    /// `CLOSE_PROOF_POLICY.md` requires landing proof *and* separate
    /// semantic-completion evidence; a free string carries neither. This is the
    /// control that keeps the prose escape hatch shut for the two classifications
    /// that close work.
    #[test]
    fn test_gated_classifications_reject_prose_close_proof() -> Result<()> {
        for classification in CLOSE_PROOF_REQUIRED {
            let line = format!(
                r#"{{"pr":"42","title":"chore: drop","classification":"{classification}","confidence":"high","evidence":[],"cleanup_done":true,"known_gaps":[],"close_proof":"abc1234 is ancestor of main"}}"#
            );
            let msg = first_msg(&line);
            ensure!(
                msg.contains("requires the structured `close_proof` object"),
                "{classification}: prose close proof accepted, got `{msg}`"
            );
        }
        Ok(())
    }

    /// An ungated row may still carry a prose note; only closes are gated.
    #[test]
    fn test_ungated_classification_accepts_prose_close_proof() -> Result<()> {
        let line = r#"{"pr":"7","title":"t","classification":"deferred","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":"context for a later wave"}"#;
        ensure!(line_errors(line).is_empty(), "got: {:?}", line_errors(line));
        Ok(())
    }

    /// The false-close guard must keep failing closed for both classifications.
    #[test]
    fn test_close_proof_is_required_and_non_empty() -> Result<()> {
        for classification in CLOSE_PROOF_REQUIRED {
            let missing = format!(
                r#"{{"pr":"1","title":"t","classification":"{classification}","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[]}}"#
            );
            ensure!(
                first_msg(&missing).contains("requires `close_proof`"),
                "{classification}: missing close_proof accepted"
            );

            let empty = format!(
                r#"{{"pr":"1","title":"t","classification":"{classification}","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":"  "}}"#
            );
            ensure!(
                first_msg(&empty).contains("`close_proof` must not be empty"),
                "{classification}: empty close_proof accepted"
            );

            let null = format!(
                r#"{{"pr":"1","title":"t","classification":"{classification}","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":null}}"#
            );
            ensure!(!line_errors(&null).is_empty(), "{classification}: null close_proof accepted");
        }
        Ok(())
    }

    #[test]
    fn test_invalid_json_returns_error() -> Result<()> {
        let errs = line_errors("not json at all {");
        ensure!(errs.first().is_some_and(|e| e.message.contains("invalid JSON")), "got {errs:?}");
        Ok(())
    }

    #[test]
    fn test_non_object_returns_error() -> Result<()> {
        let errs = line_errors(r#"["array"]"#);
        ensure!(errs.first().is_some_and(|e| e.message.contains("JSON object")), "got {errs:?}");
        Ok(())
    }

    #[test]
    fn test_empty_title_field() -> Result<()> {
        let msg = first_msg(
            r#"{"pr":"1","title":"  ","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
        );
        ensure!(msg.contains("must not be empty"), "got: {msg}");
        Ok(())
    }

    #[test]
    fn test_unknown_classification() -> Result<()> {
        let msg = first_msg(
            r#"{"pr":"1","title":"t","classification":"not-a-thing","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
        );
        ensure!(msg.contains("unknown classification"), "got: {msg}");
        Ok(())
    }

    #[test]
    fn test_unknown_confidence() -> Result<()> {
        let msg = first_msg(
            r#"{"pr":"1","title":"t","classification":"unclassified","confidence":"super-high","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
        );
        ensure!(msg.contains("unknown confidence"), "got: {msg}");
        Ok(())
    }

    #[test]
    fn test_multiple_errors_on_same_line() -> Result<()> {
        let line = r#"{"pr":"1","title":"t","classification":"bogus","confidence":"nope","evidence":[],"cleanup_done":false,"known_gaps":[]}"#;
        ensure!(
            line_errors(line).len() >= 2,
            "expected multiple errors, got: {:?}",
            line_errors(line)
        );
        Ok(())
    }

    #[test]
    fn test_validate_accepts_ledgers_with_blank_and_comment_lines() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let ledger_dir = temp.path().join("ledgers");
        write_ledger(
            &ledger_dir.join("b.jsonl"),
            &format!("{PR_TRIAGE_HEADER}\n# already reviewed\n\n{}\n", valid_row()),
        )?;
        write_ledger(
            &ledger_dir.join("a.jsonl"),
            &format!("{WORKFLOW_HEADER}\n{}\n", valid_workflow_row()),
        )?;
        write_ledger(&ledger_dir.join("notes.txt"), "{not json, and not a ledger}\n")?;

        validate(ValidateConfig {
            ledger_dir: Some(ledger_dir),
            format: ValidateFormat::Json,
            expected_schema: None,
        })?;
        Ok(())
    }

    #[test]
    fn test_validate_reports_invalid_rows_from_ledger_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let ledger_dir = temp.path().join("ledgers");
        write_ledger(
            &ledger_dir.join("ledger.jsonl"),
            &format!("{PR_TRIAGE_HEADER}\n# comment\n\n{}\n{{\"bad\":\"row\"}}\n", valid_row()),
        )?;

        let err = validate(ValidateConfig {
            ledger_dir: Some(ledger_dir),
            format: ValidateFormat::Human,
            expected_schema: None,
        })
        .err()
        .ok_or_else(|| eyre!("invalid ledger row unexpectedly passed validation"))?;

        ensure!(
            err.to_string().contains("ledger validation failed with"),
            "unexpected validation error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_sorts_and_ignores_non_ledgers() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let ledger_dir = temp.path().join("ledgers");
        write_ledger(&ledger_dir.join("b.jsonl"), "{}\n")?;
        write_ledger(&ledger_dir.join("a.jsonl"), "{}\n")?;
        write_ledger(&ledger_dir.join("notes.txt"), "{}\n")?;

        let files = collect_jsonl_files(&ledger_dir)?;
        let names = files
            .iter()
            .map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| eyre!("non-utf8 ledger path: {}", path.display()))
                    .map(ToOwned::to_owned)
            })
            .collect::<Result<Vec<_>>>()?;

        ensure!(names == ["a.jsonl", "b.jsonl"], "expected sorted jsonl files only, got {names:?}");
        Ok(())
    }

    #[test]
    fn test_missing_ledger_directory_is_empty_success() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let missing_dir = temp.path().join("missing-ledgers");

        ensure!(
            collect_jsonl_files(&missing_dir)?.is_empty(),
            "missing ledger directory should collect no files"
        );
        validate(ValidateConfig {
            ledger_dir: Some(missing_dir),
            format: ValidateFormat::Human,
            expected_schema: None,
        })?;
        Ok(())
    }

    #[test]
    fn test_default_ledger_directory_resolves_to_project_ledgers() -> Result<()> {
        let suffix = PathBuf::from("docs").join("agents").join("ledgers");
        let resolved = resolve_ledger_dir(None)?;
        ensure!(
            resolved.ends_with(&suffix),
            "default ledger directory should end with {}, got {}",
            suffix.display(),
            resolved.display()
        );
        Ok(())
    }

    #[test]
    fn test_error_carries_file_and_line_metadata() -> Result<()> {
        let errs = validate_line(
            r#"{"bad":"row"}"#,
            "docs/agents/ledgers/foo.jsonl",
            42,
            LedgerSchemaId::PrTriageV1,
        );
        let first = errs.first().ok_or_else(|| eyre!("expected an error"))?;
        ensure!(first.file == "docs/agents/ledgers/foo.jsonl", "got {}", first.file);
        ensure!(first.line == 42, "got {}", first.line);
        Ok(())
    }

    /// Row line numbers are the file's real line numbers, not row ordinals.
    #[test]
    fn test_row_errors_report_real_file_line_numbers() -> Result<()> {
        let contents = format!("{PR_TRIAGE_HEADER}\n\n# note\n{{\"bad\":\"row\"}}\n");
        let errs = file_errors(&contents);
        let first = errs.first().ok_or_else(|| eyre!("expected an error"))?;
        ensure!(first.line == 4, "expected line 4, got {}", first.line);
        Ok(())
    }

    // ----- A5: conformance to the published JSON Schema ---------------------

    /// The Rust type and `workflow-outcome.schema.json` must not drift apart: the
    /// schema's own example has to decode and validate clean.
    #[test]
    fn test_workflow_outcome_schema_example_validates() -> Result<()> {
        let schema_path =
            crate::utils::project_root()?.join("docs/agents/workflow-outcome.schema.json");
        let schema: Value = serde_json::from_str(&fs::read_to_string(&schema_path)?)?;

        let example = schema
            .get("examples")
            .and_then(|e| e.get(0))
            .ok_or_else(|| eyre!("workflow-outcome.schema.json has no examples[0]"))?;
        let line = serde_json::to_string(example)?;

        let errs = validate_line(&line, "schema-example", 1, LedgerSchemaId::WorkflowOutcomeV1);
        ensure!(errs.is_empty(), "schema example rejected by the Rust contract: {errs:?}");

        // Every property the schema marks required must be required by the struct too.
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("schema has no required list"))?;
        for field in required {
            let name = field.as_str().ok_or_else(|| eyre!("non-string required entry"))?;
            let mut trimmed = example.clone();
            trimmed.as_object_mut().ok_or_else(|| eyre!("example is not an object"))?.remove(name);
            let line = serde_json::to_string(&trimmed)?;
            let errs = validate_line(&line, "schema-example", 1, LedgerSchemaId::WorkflowOutcomeV1);
            ensure!(!errs.is_empty(), "required field `{name}` was not enforced");
        }
        Ok(())
    }

    // ----- A6: caller-side schema pin ---------------------------------------

    #[test]
    fn test_expected_schema_match_and_mismatch() -> Result<()> {
        let contents = format!("{WORKFLOW_HEADER}\n{}\n", valid_workflow_row());

        let (_, errs) =
            validate_file(&contents, "t.jsonl", Some(LedgerSchemaId::WorkflowOutcomeV1));
        ensure!(errs.is_empty(), "matching pin rejected: {errs:?}");

        let (rows, errs) = validate_file(&contents, "t.jsonl", Some(LedgerSchemaId::PrTriageV1));
        ensure!(rows == 0, "mismatched pin should check no rows, got {rows}");
        let first = errs.first().ok_or_else(|| eyre!("expected a mismatch error"))?;
        ensure!(first.message.contains("but `pr-triage.v1` was required"), "got {}", first.message);
        Ok(())
    }

    #[test]
    fn test_unknown_expected_schema_is_an_error() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let err = validate(ValidateConfig {
            ledger_dir: Some(temp.path().to_path_buf()),
            format: ValidateFormat::Human,
            expected_schema: Some("not-a-schema.v9".to_string()),
        })
        .err()
        .ok_or_else(|| eyre!("unknown --expected-schema unexpectedly accepted"))?;
        ensure!(err.to_string().contains("unknown --expected-schema"), "got {err:?}");
        Ok(())
    }

    // ----- registry ---------------------------------------------------------

    // ----- close-proof contract (structured form) --------------------------

    /// The structured `close_proof` contract must be accepted.
    ///
    /// Exercises every constant the contract pins: the `landing_proof.v1` receipt,
    /// `commit_reachable: true`, `semantic_completion: not_evaluated`, and the ISO
    /// verified date.
    #[test]
    fn test_structured_close_proof_is_accepted() -> Result<()> {
        let close_proof: Value = serde_json::from_str(
            r#"{"command":"cargo xtask landing-proof --commit abc1234 --canonical-main origin/main --format json","receipt":{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"},"semantic_completion_evidence":"semantic-close packet #1100-row-evidence (see CLOSE_PROOF_POLICY.md Three Distinct Proof Layers)","verified_date":"2026-06-07"}"#,
        )?;

        let mut row: Value = serde_json::from_str(valid_row())?;
        let obj = row.as_object_mut().ok_or_else(|| eyre!("row is not an object"))?;
        obj.insert("classification".to_string(), Value::String("close-superseded".to_string()));
        obj.insert("close_proof".to_string(), close_proof.clone());

        let errs = line_errors(&serde_json::to_string(&row)?);
        ensure!(errs.is_empty(), "structured close_proof rejected: {errs:?}");
        Ok(())
    }

    /// Each constant the close-proof contract pins must actually be enforced.
    #[test]
    fn test_structured_close_proof_constants_are_enforced() -> Result<()> {
        let base = r#"{"command":"cargo xtask landing-proof --commit abc1234 --canonical-main origin/main --format json","receipt":{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"},"semantic_completion_evidence":"semantic-close packet #1100","verified_date":"2026-06-07"}"#;

        let row_with = |proof: &str| {
            format!(
                r#"{{"pr":"1","title":"t","classification":"close-superseded","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":{proof}}}"#
            )
        };

        ensure!(line_errors(&row_with(base)).is_empty(), "valid structured close proof rejected");

        for (case, mutated, expected) in [
            (
                "wrong receipt schema_version",
                base.replace("landing_proof.v1", "landing_proof.v2"),
                "must be `landing_proof.v1`",
            ),
            (
                "unreachable commit",
                base.replace(r#""commit_reachable":true"#, r#""commit_reachable":false"#),
                "must be true",
            ),
            (
                "self-asserted semantic completion",
                base.replace("not_evaluated", "complete"),
                "must be `not_evaluated`",
            ),
            (
                "short commit",
                base.replace(r#""commit":"abc1234""#, r#""commit":"abc""#),
                "at least 7 characters",
            ),
            ("bad verified_date", base.replace("2026-06-07", "07/06/2026"), "ISO 8601"),
            (
                "empty semantic evidence",
                base.replace("semantic-close packet #1100", "   "),
                "must not be empty",
            ),
        ] {
            let msg = first_msg(&row_with(&mutated));
            ensure!(msg.contains(expected), "{case}: expected `{expected}`, got `{msg}`");
        }

        // An unknown key inside the receipt is structural drift, not extra detail.
        let extra = base.replace(
            r#""semantic_completion":"not_evaluated""#,
            r#""semantic_completion":"not_evaluated","smuggled":1"#,
        );
        ensure!(!line_errors(&row_with(&extra)).is_empty(), "unknown receipt field accepted");

        // A missing receipt is not a close proof at all.
        let no_receipt =
            r#"{"command":"c","semantic_completion_evidence":"e","verified_date":"2026-06-07"}"#;
        ensure!(
            !line_errors(&row_with(no_receipt)).is_empty(),
            "close proof without receipt accepted"
        );
        Ok(())
    }

    #[test]
    fn test_close_proof_of_wrong_json_type_is_rejected() -> Result<()> {
        let line = r#"{"pr":"1","title":"t","classification":"close-superseded","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":42}"#;
        let msg = first_msg(line);
        ensure!(msg.contains("must be a string or a structured close-proof object"), "got: {msg}");
        Ok(())
    }

    // ----- explicit null is not absence --------------------------------------

    /// These contracts permit omission but not an explicit `null`.
    #[test]
    fn test_explicit_null_optional_fields_are_rejected() -> Result<()> {
        let workflow_null_notes =
            valid_workflow_row().trim_end_matches('}').to_string() + r#","notes":null}"#;
        ensure!(
            !validate_line(&workflow_null_notes, "t.jsonl", 1, LedgerSchemaId::WorkflowOutcomeV1)
                .is_empty(),
            "workflow-outcome `notes: null` accepted"
        );
        // ...while omitting it entirely stays valid.
        ensure!(
            validate_line(valid_workflow_row(), "t.jsonl", 1, LedgerSchemaId::WorkflowOutcomeV1)
                .is_empty(),
            "workflow-outcome row without notes rejected"
        );

        for field in ["surface_guess", "mergeable", "head_ref", "author", "is_draft"] {
            let line = format!(
                r#"{{"pr":"1","title":"t","classification":"unclassified","confidence":"low","evidence":[],"cleanup_done":false,"known_gaps":[],"{field}":null}}"#
            );
            ensure!(!line_errors(&line).is_empty(), "`{field}: null` accepted");
        }

        // An explicit null close_proof is reported as null, not as absence.
        let null_proof = r#"{"pr":"1","title":"t","classification":"close-superseded","confidence":"high","evidence":[],"cleanup_done":false,"known_gaps":[],"close_proof":null}"#;
        ensure!(first_msg(null_proof).contains("non-null"), "got: {}", first_msg(null_proof));
        Ok(())
    }

    // ----- file-level shapes -------------------------------------------------

    /// A declared ledger with a valid header but no rows must not pass vacuously.
    #[test]
    fn test_declared_ledger_with_no_rows_is_rejected() -> Result<()> {
        let contents = format!(
            "{WORKFLOW_HEADER}
# header only, no data

"
        );
        let (rows, errs) = validate_file(&contents, "t.jsonl", None);
        ensure!(rows == 0, "expected zero rows, got {rows}");
        let first = errs.first().ok_or_else(|| eyre!("empty ledger accepted"))?;
        ensure!(first.message.contains("contains no rows"), "got: {}", first.message);
        Ok(())
    }

    /// A UTF-8 BOM must not hide the directive: `str::trim` does not remove U+FEFF.
    #[test]
    fn test_leading_bom_does_not_hide_the_directive() -> Result<()> {
        let contents = format!("\u{feff}{PR_TRIAGE_HEADER}\n{}\n", valid_row());
        let errs = file_errors(&contents);
        ensure!(errs.is_empty(), "BOM-prefixed ledger rejected: {errs:?}");
        Ok(())
    }

    // ----- generator drift ---------------------------------------------------

    /// `pr-triage.v1` must keep accepting exactly what `pr_ledger` emits.
    ///
    /// Serializing the generator's own row type means a field added, removed, or
    /// renamed in `pr_ledger::LedgerRow` turns this red via `deny_unknown_fields`,
    /// rather than silently making every generated worklist invalid.
    #[test]
    fn test_pr_triage_accepts_the_generator_row_type() -> Result<()> {
        let generated = crate::tasks::pr_ledger::LedgerRow {
            pr: "1234".to_string(),
            title: "fix: thing (#1234)".to_string(),
            surface_guess: "xtask".to_string(),
            classification: "unclassified".to_string(),
            confidence: "low".to_string(),
            evidence: Vec::new(),
            cleanup_done: false,
            known_gaps: Vec::new(),
            is_draft: false,
            mergeable: "MERGEABLE".to_string(),
            head_ref: "feat/1234-thing".to_string(),
            author: "EffortlessSteven".to_string(),
        };
        let line = serde_json::to_string(&generated)?;
        let errs = line_errors(&line);
        ensure!(errs.is_empty(), "pr_ledger::LedgerRow rejected by pr-triage.v1: {errs:?}");
        Ok(())
    }

    #[test]
    fn test_schema_ids_round_trip_and_are_unique() -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for id in LedgerSchemaId::ALL {
            ensure!(seen.insert(id.as_str()), "duplicate schema id {}", id.as_str());
            ensure!(LedgerSchemaId::parse(id.as_str()) == Some(*id), "{id} did round-trip");
        }
        ensure!(LedgerSchemaId::parse("nope.v1").is_none(), "unregistered id parsed");
        Ok(())
    }

    // ----- exact-value seam oracles ------------------------------------------
    //
    // The probes in this module's changed lines are only `exposed` for ripr when a
    // related test asserts the exact value the seam produces — a full message
    // string, an exact line number, or an exact decoded field value — via
    // `assert_eq!`. The earlier `ensure!(msg.contains(..))` probes reach these
    // paths but carry no discriminating oracle, so each error value each seam
    // builds is pinned exactly once here.

    fn resolve_errors(content: &str) -> Vec<LedgerError> {
        resolve_schema(content, "test.jsonl").unwrap_err()
    }

    const REGISTERED: &str = "pr-triage.v1, workflow-outcome.v1, ub-review-calibration.v1";

    /// The no-directive error pins the directive syntax, the requirement, every
    /// registered schema id, and cites the first content line.
    #[test]
    fn test_missing_directive_error_value_is_exact() {
        let errs = resolve_errors("{\"pr\":\"1\"}\n");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].file, "test.jsonl");
        assert_eq!(errs[0].line, 1);
        assert_eq!(
            errs[0].message,
            format!(
                "no `#!ledger-schema: <id>` header directive; \
                 every ledger file must declare its row contract. \
                 Registered schemas: {REGISTERED}"
            )
        );
    }

    /// A file with no content line at all cites line 1 — the `unwrap_or(1)`
    /// fallback, not a fabricated line number.
    #[test]
    fn test_missing_directive_on_empty_file_cites_line_one() {
        let errs = resolve_errors("");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 1);
    }

    /// `first_content_line` is the 1-based content line, so leading blank lines
    /// move the cited line exactly that far.
    #[test]
    fn test_missing_directive_cites_first_content_line_exactly() {
        let errs = resolve_errors("\n\n{\"pr\":\"1\"}\n");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 3);
    }

    /// A duplicate directive is reported on the second directive's own line.
    #[test]
    fn test_duplicate_directive_error_value_is_exact() {
        let errs = resolve_errors(
            "#!ledger-schema: pr-triage.v1\n\n#!ledger-schema: workflow-outcome.v1\nrow\n",
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 3);
        assert_eq!(
            errs[0].message,
            "duplicate `#!ledger-schema:` directive; a ledger file declares one schema"
        );
    }

    /// A directive that is not the first non-blank line cites the directive's own
    /// line number in the message.
    #[test]
    fn test_misplaced_directive_error_value_is_exact() {
        let errs = resolve_errors("# a note\n#!ledger-schema: pr-triage.v1\nrow\n");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 2);
        assert_eq!(
            errs[0].message,
            "`#!ledger-schema:` must be the first non-blank line of the file, not line 2"
        );
    }

    /// An unregistered schema id is named verbatim, with the registered list.
    #[test]
    fn test_unregistered_schema_error_value_is_exact() {
        let errs = resolve_errors("#!ledger-schema: some-future-ledger.v1\n{}\n");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 1);
        assert_eq!(
            errs[0].message,
            format!(
                "unregistered ledger schema `some-future-ledger.v1`; registered schemas: {REGISTERED}"
            )
        );
    }

    /// The Ok path returns the parsed schema and the directive's 1-based line.
    #[test]
    fn test_resolve_schema_ok_value_is_exact() {
        let resolved =
            resolve_schema("\n#!ledger-schema: ub-review-calibration.v1\nrow\n", "t.jsonl");
        assert_eq!(resolved.ok(), Some((LedgerSchemaId::UbReviewCalibrationV1, 2)));
    }

    /// `validate_file` checks no rows when the schema cannot be resolved: the
    /// row count it returns is exactly 0 and the errors are the resolver's.
    #[test]
    fn test_validate_file_checks_zero_rows_when_schema_unresolved() {
        let (rows, errs) = validate_file("no directive here\n", "a.jsonl", None);
        assert_eq!(rows, 0);
        assert_eq!(errs, resolve_schema("no directive here\n", "a.jsonl").unwrap_err());
    }

    /// A declared ledger with zero rows fails closed on the directive line.
    #[test]
    fn test_zero_row_ledger_error_value_is_exact() {
        let (rows, errs) =
            validate_file("#!ledger-schema: pr-triage.v1\n\n# only a comment\n", "z.jsonl", None);
        assert_eq!(rows, 0);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 1);
        assert_eq!(errs[0].message, "ledger declares schema `pr-triage.v1` but contains no rows");
    }

    /// Row counting is exact: comments and blank lines are not rows.
    #[test]
    fn test_validate_file_row_count_is_exact() {
        let contents =
            format!("{PR_TRIAGE_HEADER}\n# a comment\n\n{}\n\n{}\n", valid_row(), valid_row());
        let (rows, errs) = validate_file(&contents, "c.jsonl", None);
        assert_eq!(rows, 2);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    }

    /// The expected-schema pin rejects a declared schema with the exact mismatch
    /// message, cited on the directive line, checking zero rows.
    #[test]
    fn test_expected_schema_mismatch_error_value_is_exact() {
        let (rows, errs) = validate_file(
            "#!ledger-schema: ub-review-calibration.v1\n{}\n",
            "m.jsonl",
            Some(LedgerSchemaId::WorkflowOutcomeV1),
        );
        assert_eq!(rows, 0);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 1);
        assert_eq!(
            errs[0].message,
            "declares schema `ub-review-calibration.v1` but `workflow-outcome.v1` was required"
        );
    }

    /// Every decoded `WorkflowOutcomeRow` counter lands in its own exact field.
    #[test]
    fn test_workflow_outcome_counter_fields_decode_to_exact_values() -> Result<()> {
        let row: WorkflowOutcomeRow = serde_json::from_str(
            r#"{"date":"2026-06-05","workflow_type":"issue-triage","repo":"perl-lsp-swarm","agents_used":37,"model_mix":{"haiku":30,"sonnet":7},"items_processed":154,"merged":4,"closed_with_proof":10,"deferred":116,"false_closes_prevented":3,"ci_failures_diagnosed":2,"upstream_gaps_filed":1,"builders_dispatched":5,"known_gaps":[],"cleanup_done":true}"#,
        )
        .with_context(|| "counter-field fixture row must decode")?;
        assert_eq!(row.agents_used, 37);
        assert_eq!(row.items_processed, 154);
        assert_eq!(row.merged, 4);
        assert_eq!(row.closed_with_proof, 10);
        assert_eq!(row.deferred, 116);
        assert_eq!(row.false_closes_prevented, 3);
        assert_eq!(row.ci_failures_diagnosed, 2);
        assert_eq!(row.upstream_gaps_filed, 1);
        assert_eq!(row.builders_dispatched, 5);
        assert!(row.cleanup_done);
        Ok(())
    }

    /// `validate_line` stamps each error with the exact row line and file it was
    /// called with, per schema arm.
    #[test]
    fn test_workflow_outcome_line_error_metadata_is_exact() {
        let errs = validate_line(
            r#"{"date":"junk","workflow_type":"w","repo":"r","agents_used":1,"model_mix":{},"items_processed":1,"merged":0,"closed_with_proof":0,"deferred":0,"false_closes_prevented":0,"ci_failures_diagnosed":0,"upstream_gaps_filed":0,"builders_dispatched":0,"known_gaps":[],"cleanup_done":true}"#,
            "w.jsonl",
            7,
            LedgerSchemaId::WorkflowOutcomeV1,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].file, "w.jsonl");
        assert_eq!(errs[0].line, 7);
        assert_eq!(
            errs[0].message,
            "field `date` must be an ISO 8601 date (YYYY-MM-DD), got `junk`"
        );
    }

    #[test]
    fn test_ub_review_line_error_metadata_is_exact() {
        let errs = validate_line(
            r#"{"date":"2026-06-05","pr":null,"runner":"r","profile":"p","classification":"bogus","category":"c","value":"high","evidence":"e","action_taken":"a"}"#,
            "u.jsonl",
            11,
            LedgerSchemaId::UbReviewCalibrationV1,
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].file, "u.jsonl");
        assert_eq!(errs[0].line, 11);
        assert_eq!(
            errs[0].message,
            "unknown classification `bogus`; valid values: true-positive, false-positive, expected-quiet, infra-excluded"
        );
    }

    /// A malformed JSON row fails with the exact invalid-JSON prefix.
    #[test]
    fn test_invalid_json_row_message_prefix_is_exact() {
        let errs = line_errors("{not json");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 1);
        assert_eq!(errs[0].message.split(": ").next(), Some("invalid JSON"));
    }

    /// A non-object row is rejected with the exact message, for arrays, strings,
    /// and numbers alike.
    #[test]
    fn test_non_object_row_message_is_exact() {
        for line in ["[1,2]", "\"x\"", "42", "null"] {
            assert_eq!(first_msg(line), "line must be a JSON object", "for {line}");
        }
    }

    /// The pr-triage classification and confidence vocabularies are pinned with
    /// their full valid-value lists in the exact error strings.
    #[test]
    fn test_pr_triage_vocabulary_error_values_are_exact() {
        let errs = line_errors(
            r#"{"pr":"1","title":"t","classification":"bogus","confidence":"cosmic","evidence":[],"cleanup_done":false,"known_gaps":[]}"#,
        );
        assert_eq!(errs.len(), 2);
        assert_eq!(
            errs[0].message,
            "unknown classification `bogus`; valid values: unclassified, builder-ready, in-build, \
             in-review, merge-ready, close-superseded, duplicate-of-merged, already-fixed, deferred, \
             needs-plan-review, needs-builder-fix, needs-ci-fix, needs-diff-fix"
        );
        assert_eq!(errs[1].message, "unknown confidence `cosmic`; valid values: high, medium, low");
    }

    /// A gated classification without any `close_proof` is rejected with the exact
    /// message, for both gated classifications.
    #[test]
    fn test_gated_missing_close_proof_error_values_are_exact() {
        for classification in CLOSE_PROOF_REQUIRED {
            let mut errors = Vec::new();
            check_close_proof(None, classification, &mut errors);
            assert_eq!(
                errors,
                vec![format!("classification `{classification}` requires `close_proof` field")]
            );
        }
    }

    /// An explicit null `close_proof` is rejected even on ungated rows.
    #[test]
    fn test_null_close_proof_error_values_are_exact() {
        let mut ungated = Vec::new();
        check_close_proof(Some(&Value::Null), "unclassified", &mut ungated);
        assert_eq!(ungated, vec!["`close_proof` must not be null".to_string()]);

        let mut gated = Vec::new();
        check_close_proof(Some(&Value::Null), "close-superseded", &mut gated);
        assert_eq!(
            gated,
            vec!["classification `close-superseded` requires non-null `close_proof`".to_string()]
        );
    }

    /// Prose never authorizes a gated close: the exact policy message is pinned.
    #[test]
    fn test_gated_prose_close_proof_error_value_is_exact() {
        let mut errors = Vec::new();
        check_close_proof(
            Some(&Value::String("looks merged".to_string())),
            "duplicate-of-merged",
            &mut errors,
        );
        assert_eq!(
            errors,
            vec![
                "
                classification `duplicate-of-merged` requires the structured `close_proof` \
                object; a prose string carries no landing receipt or \
                semantic-completion evidence (CLOSE_PROOF_POLICY.md)"
                    .trim()
                    .to_string()
            ]
        );
    }

    /// A non-string, non-object `close_proof` (e.g. a number) is rejected with the
    /// exact form message.
    #[test]
    fn test_wrong_json_type_close_proof_error_value_is_exact() {
        let mut errors = Vec::new();
        check_close_proof(Some(&Value::Number(42.into())), "unclassified", &mut errors);
        assert_eq!(
            errors,
            vec!["`close_proof` must be a string or a structured close-proof object".to_string()]
        );
    }

    /// A structured close proof that fails its own decode carries the exact
    /// contract-mismatch prefix plus the serde reason.
    #[test]
    fn test_structured_close_proof_decode_error_prefix_is_exact() {
        let mut errors = Vec::new();
        check_close_proof(Some(&serde_json::json!({})), "close-superseded", &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].get(
                .."`close_proof` object does not match the structured close-proof contract:".len()
            ),
            Some("`close_proof` object does not match the structured close-proof contract:")
        );
    }

    fn close_proof_with(mutations: &str) -> Result<StructuredCloseProof> {
        Ok(serde_json::from_str(&format!(
            r#"{{"command":"cargo xtask landing-proof --commit abc1234","receipt":{mutations},"semantic_completion_evidence":"packet #1100","verified_date":"2026-06-07"}}"#
        ))?)
    }

    /// Each structured close-proof field pins its exact rejection message.
    #[test]
    fn test_structured_close_proof_field_messages_are_exact() -> Result<()> {
        let empty_command = close_proof_with(
            r#"{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"","semantic_completion":"not_evaluated"}"#,
        )?;
        assert_eq!(
            check_structured_close_proof(&empty_command),
            vec!["field `close_proof.receipt.canonical_main` must not be empty".to_string()]
        );

        let mut bad_date = close_proof_with(
            r#"{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"}"#,
        )?;
        bad_date.verified_date = "2026/06/07".to_string();
        assert_eq!(
            check_structured_close_proof(&bad_date),
            vec![
                "field `close_proof.verified_date` must be an ISO 8601 date (YYYY-MM-DD), got `2026/06/07`"
                    .to_string()
            ]
        );

        let wrong_version = close_proof_with(
            r#"{"schema_version":"landing_proof.v2","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"}"#,
        )?;
        assert_eq!(
            check_structured_close_proof(&wrong_version),
            vec![
                "field `close_proof.receipt.schema_version` must be `landing_proof.v1`, got `landing_proof.v2`"
                    .to_string()
            ]
        );

        let unreachable = close_proof_with(
            r#"{"schema_version":"landing_proof.v1","commit_reachable":false,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"not_evaluated"}"#,
        )?;
        assert_eq!(
            check_structured_close_proof(&unreachable),
            vec![
                "field `close_proof.receipt.commit_reachable` must be true; an unreachable commit is not landing proof"
                    .to_string()
            ]
        );

        let short_commit = close_proof_with(
            r#"{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc","canonical_main":"origin/main","semantic_completion":"not_evaluated"}"#,
        )?;
        assert_eq!(
            check_structured_close_proof(&short_commit),
            vec![
                "field `close_proof.receipt.commit` must be at least 7 characters, got `abc`"
                    .to_string()
            ]
        );

        let self_asserted = close_proof_with(
            r#"{"schema_version":"landing_proof.v1","commit_reachable":true,"commit":"abc1234","canonical_main":"origin/main","semantic_completion":"done"}"#,
        )?;
        assert_eq!(
            check_structured_close_proof(&self_asserted),
            vec![
                "field `close_proof.receipt.semantic_completion` must be `not_evaluated`, got `done`"
                    .to_string()
            ]
        );
        Ok(())
    }

    /// `check_workflow_outcome` pins each semantic rejection exactly.
    #[test]
    fn test_workflow_outcome_semantic_messages_are_exact() -> Result<()> {
        let mut row: WorkflowOutcomeRow =
            serde_json::from_str(valid_workflow_row()).with_context(|| "fixture must decode")?;
        assert!(check_workflow_outcome(&row).is_empty());

        row.date = "not-a-date".to_string();
        assert_eq!(
            check_workflow_outcome(&row),
            vec![
                "field `date` must be an ISO 8601 date (YYYY-MM-DD), got `not-a-date`".to_string()
            ]
        );

        row.date = "2026-06-05".to_string();
        row.workflow_type = "  ".to_string();
        assert_eq!(
            check_workflow_outcome(&row),
            vec!["field `workflow_type` must not be empty".to_string()]
        );

        row.workflow_type = "issue-triage".to_string();
        row.repo = String::new();
        assert_eq!(
            check_workflow_outcome(&row),
            vec!["field `repo` must not be empty".to_string()]
        );

        row.repo = "perl-lsp-swarm".to_string();
        row.known_gaps = vec![" ".to_string()];
        assert_eq!(
            check_workflow_outcome(&row),
            vec!["`known_gaps` entries must not be empty".to_string()]
        );
        Ok(())
    }

    /// `check_ub_review_calibration` pins each semantic rejection exactly,
    /// including the null-vs-value `pr` vocabulary and the required text fields.
    #[test]
    fn test_ub_review_semantic_messages_are_exact() -> Result<()> {
        let mut row: UbReviewCalibrationRow =
            serde_json::from_str(valid_ub_row()).with_context(|| "fixture must decode")?;
        assert!(check_ub_review_calibration(&row).is_empty());

        for (bad, rendered) in [
            (serde_json::json!(0), "0"),
            (serde_json::json!(-5), "-5"),
            (serde_json::json!("1243"), "\"1243\""),
        ] {
            row.pr = bad;
            assert_eq!(
                check_ub_review_calibration(&row),
                vec![format!("field `pr` must be a positive integer or null, got `{rendered}`")]
            );
        }

        row.pr = Value::Null;
        assert!(check_ub_review_calibration(&row).is_empty(), "null pr must be accepted");

        row.pr = serde_json::json!(1243);
        row.classification = "bogus".to_string();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec![
                "unknown classification `bogus`; valid values: true-positive, false-positive, expected-quiet, infra-excluded"
                    .to_string()
            ]
        );

        row.classification = "true-positive".to_string();
        row.value = "bogus".to_string();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["unknown value `bogus`; valid values: high, medium, low, n/a".to_string()]
        );

        row.value = "high".to_string();
        row.runner = String::new();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["field `runner` must not be empty".to_string()]
        );

        row.runner = "gh-hosted".to_string();
        row.profile = " ".to_string();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["field `profile` must not be empty".to_string()]
        );

        row.profile = "bun-ub-v0".to_string();
        row.category = String::new();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["field `category` must not be empty".to_string()]
        );

        row.category = "proof-gap/docs-drift".to_string();
        row.evidence = String::new();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["field `evidence` must not be empty".to_string()]
        );

        row.evidence = "sensor caught a fabricated breakdown".to_string();
        row.action_taken = String::new();
        assert_eq!(
            check_ub_review_calibration(&row),
            vec!["field `action_taken` must not be empty".to_string()]
        );
        Ok(())
    }

    /// An ISO date check accepts exactly `YYYY-MM-DD` shapes and rejects plausible
    /// near-misses.
    #[test]
    fn test_iso_date_shape_oracle_is_exact() {
        for good in ["2026-06-07", "0000-01-01", "9999-12-31"] {
            assert!(is_iso_date(good), "{good} must parse");
        }
        for bad in [
            "2026-6-07",
            "2026/06/07",
            "26-06-07",
            "2026-06-7",
            "20260607",
            "2026-06-07T00:00:00",
            "202a-06-07",
            "",
        ] {
            assert!(!is_iso_date(bad), "{bad} must not parse");
        }
    }
}
