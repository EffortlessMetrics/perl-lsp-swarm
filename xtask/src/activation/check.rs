//! Fail-closed activation checker over the versioned inventory (#9205).
//!
//! Class-specific connection requirements, not a universal "wired" predicate
//! and not Cargo dependency membership. `unwired-scan` remains topology
//! evidence only. Downstream claims owned by #9207/#9208/#9210/#9211 are
//! not decided here: no product-binary test-API scan, no feature/package
//! reclassification, no live gate enforcement, no public status projection.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::model::{
    ActivationClass, ActivationError, ActivationInventory, ActivationRow, INVENTORY_PATH,
    RegistrationState,
};
use super::validate;

/// Machine identity for a checker report. Distinct from the inventory schema:
/// this document is an evaluation, not a classification.
pub const CHECK_SCHEMA: &str = "activation_check.v1";
pub const CONTROLLING_ISSUE: &str = "#9205";

/// Pass or fail for one surface against its class contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

/// One surface's class-contract evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowFinding {
    pub surface_id: String,
    pub class: String,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}

impl RowFinding {
    fn pass(surface_id: impl Into<String>, class: impl Into<String>) -> Self {
        Self {
            surface_id: surface_id.into(),
            class: class.into(),
            verdict: Verdict::Pass,
            reasons: Vec::new(),
        }
    }

    fn fail(surface_id: impl Into<String>, class: impl Into<String>, reasons: Vec<String>) -> Self {
        Self { surface_id: surface_id.into(), class: class.into(), verdict: Verdict::Fail, reasons }
    }

    #[must_use]
    pub fn is_fail(&self) -> bool {
        self.verdict == Verdict::Fail
    }
}

/// Full checker report over one inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    pub schema: String,
    pub controlling_issue: String,
    pub inventory: String,
    pub findings: Vec<RowFinding>,
    pub passed: usize,
    pub failed: usize,
}

impl CheckReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failed == 0
    }
}

/// Validate the committed inventory, then evaluate every row fail-closed.
pub fn check(root: &Path) -> Result<CheckReport, ActivationError> {
    let inventory = validate::validate(root)?;
    Ok(evaluate_inventory(root, &inventory))
}

/// Evaluate an already-validated inventory. `root` is used only to fail
/// closed on stale path-like proof references; it is not a Cargo graph.
pub fn evaluate_inventory(root: &Path, inventory: &ActivationInventory) -> CheckReport {
    let findings: Vec<RowFinding> =
        inventory.rows.iter().map(|row| evaluate_row(row, Some(root))).collect();
    let failed = findings.iter().filter(|finding| finding.is_fail()).count();
    let passed = findings.len().saturating_sub(failed);
    CheckReport {
        schema: CHECK_SCHEMA.to_string(),
        controlling_issue: CONTROLLING_ISSUE.to_string(),
        inventory: INVENTORY_PATH.to_string(),
        findings,
        passed,
        failed,
    }
}

/// Evaluate one typed inventory row against its class contract.
#[must_use]
pub fn evaluate_row(row: &ActivationRow, root: Option<&Path>) -> RowFinding {
    let mut reasons = Vec::new();
    match row.class {
        ActivationClass::Product => check_product(row, root, &mut reasons),
        ActivationClass::Preview => check_preview(row, &mut reasons),
        ActivationClass::Lab | ActivationClass::Oracle | ActivationClass::Benchmark => {
            check_runnable_non_product(row, &mut reasons);
        }
        ActivationClass::CompatibilityShim => check_shim(row, &mut reasons),
        ActivationClass::TestApi => check_test_api(row, &mut reasons),
        ActivationClass::Gate => check_gate(row, &mut reasons),
    }
    if reasons.is_empty() {
        RowFinding::pass(&row.surface_id, row.class.as_str())
    } else {
        RowFinding::fail(&row.surface_id, row.class.as_str(), reasons)
    }
}

/// Evaluate a raw JSON row so an unknown `class` (which cannot decode as
/// [`ActivationRow`]) still fails closed instead of disappearing as green.
#[must_use]
pub fn evaluate_raw_row(value: &Value, root: Option<&Path>) -> RowFinding {
    let surface_id = value.get("surface_id").and_then(Value::as_str).unwrap_or("<missing>");
    let class = value.get("class").and_then(Value::as_str).unwrap_or("<missing>");
    if ActivationClass::from_str(class).is_none() {
        return RowFinding::fail(
            surface_id,
            class,
            vec![format!("unknown activation class `{class}`")],
        );
    }
    match serde_json::from_value::<ActivationRow>(value.clone()) {
        Ok(row) => evaluate_row(&row, root),
        Err(error) => RowFinding::fail(
            surface_id,
            class,
            vec![format!("typed decode failed, so evidence is unknown: {error}")],
        ),
    }
}

/// Explain one surface from the committed inventory.
pub fn explain(root: &Path, surface_id: &str) -> Result<RowFinding, ActivationError> {
    let report = check(root)?;
    report
        .findings
        .into_iter()
        .find(|finding| finding.surface_id == surface_id)
        .ok_or_else(|| ActivationError::new(format!("unknown activation surface `{surface_id}`")))
}

/// Reviewer-readable rendering. Deterministic: inventory order, one line per
/// failure reason, no wall-clock, no host path.
#[must_use]
pub fn render_report(report: &CheckReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "activation check ({}, {}): {} passed, {} failed, {} row(s)\n",
        report.schema,
        report.controlling_issue,
        report.passed,
        report.failed,
        report.findings.len()
    ));
    if report.failed == 0 {
        out.push_str("all class contracts satisfied\n");
        return out;
    }
    out.push_str("failures\n");
    for finding in report.findings.iter().filter(|finding| finding.is_fail()) {
        out.push_str(&format!("  {} ({})\n", finding.surface_id, finding.class));
        for reason in &finding.reasons {
            out.push_str(&format!("    - {reason}\n"));
        }
    }
    out
}

/// Pretty JSON for `activation report --json`.
pub fn report_to_json(report: &CheckReport) -> Result<String, ActivationError> {
    serde_json::to_string_pretty(report).map_err(|error| {
        ActivationError::new(format!("cannot serialize activation check: {error}"))
    })
}

// ---------------------------------------------------------------------------
// Class contracts
// ---------------------------------------------------------------------------

/// Product: executable registration/dispatch/consumer/evidence chain.
/// Dependency membership, source presence, and a listed consumer crate are
/// each insufficient on their own.
fn check_product(row: &ActivationRow, root: Option<&Path>, reasons: &mut Vec<String>) {
    if row.registration.state != RegistrationState::Established {
        reasons.push(
            "product row requires established registration; Cargo/source presence is not an executable route"
                .to_string(),
        );
    }
    if blank(row.registration.authority.as_deref()) {
        reasons.push(
            "product row requires a registration authority naming where it is dispatched"
                .to_string(),
        );
    }
    if blank(row.registration.detail.as_deref()) {
        reasons.push(
            "product row requires dispatch/registration detail; a consumer list is not a route"
                .to_string(),
        );
    }
    if row.consumers.is_empty() {
        reasons.push("product row requires at least one consumer".to_string());
    }
    if row.proof_references.is_empty() {
        reasons.push(
            "product row requires evidence; tests or source without a proof reference do not activate"
                .to_string(),
        );
    }
    for proof in &row.proof_references {
        if proof.class.trim().is_empty() || proof.id.trim().is_empty() {
            reasons.push("product row has unknown or blank proof evidence".to_string());
            continue;
        }
        if let Some(root) = root
            && let Some(missing) = missing_path_like_evidence(root, &proof.id)
        {
            reasons.push(format!(
                "product row has stale proof evidence `{missing}` (id `{}`)",
                proof.id
            ));
        }
    }
}

/// Preview: explicit limitation, no accidental GA. Missing proof is allowed.
fn check_preview(row: &ActivationRow, reasons: &mut Vec<String>) {
    let limited = row.registration.state == RegistrationState::NotEstablished
        || row.owner == super::derive::UNOWNED
        || row.notes.as_deref().is_some_and(|notes| !notes.trim().is_empty());
    if !limited {
        reasons.push(
            "preview row has no limitation state (not-established registration, unowned, or notes) \
             and would contribute as accidental GA"
                .to_string(),
        );
    }
}

/// Lab / oracle / benchmark: runnable profile, receipt identity, decision
/// consumer. Product routing is not required — a legitimate lab with no
/// `perl-lsp-rs` consumer still passes.
fn check_runnable_non_product(row: &ActivationRow, reasons: &mut Vec<String>) {
    let runnable = !row.compile_profiles.is_empty()
        || row.registration.state == RegistrationState::Established;
    if !runnable {
        reasons.push(format!(
            "{} row requires a runnable compile profile or established registration",
            row.class.as_str()
        ));
    }
    if row.consumers.is_empty() {
        reasons.push(format!(
            "{} row requires a decision consumer; product routing is not that consumer",
            row.class.as_str()
        ));
    }
    if blank(row.registration.detail.as_deref()) {
        reasons.push(format!(
            "{} row requires a receipt identity in registration detail",
            row.class.as_str()
        ));
    }
}

/// Compatibility shim: canonical authority plus retirement owner/boundary.
fn check_shim(row: &ActivationRow, reasons: &mut Vec<String>) {
    if row.semantic_authority.trim().is_empty() {
        reasons.push("compatibility shim requires a canonical semantic authority".to_string());
    }
    match &row.retirement {
        None => {
            reasons.push("compatibility shim requires a retirement owner and boundary".to_string())
        }
        Some(plan) if plan.owner.trim().is_empty() || plan.boundary.trim().is_empty() => {
            reasons.push(
                "compatibility shim requires a non-blank retirement owner and boundary".to_string(),
            );
        }
        Some(_) => {}
    }
}

/// Test API: named harness/profile and a harness consumer. Absence from
/// ordinary product binaries is #9207, not this checker.
fn check_test_api(row: &ActivationRow, reasons: &mut Vec<String>) {
    if row.compile_profiles.is_empty() {
        reasons.push("test_api row requires a named harness/compile profile".to_string());
    }
    if row.consumers.is_empty() {
        reasons.push("test_api row requires a named harness consumer".to_string());
    }
}

/// Gate: declared policy identity and established registration. Live
/// reachability/enforcement is #9210, not this checker.
fn check_gate(row: &ActivationRow, reasons: &mut Vec<String>) {
    if row.semantic_authority.trim().is_empty() {
        reasons.push("gate row requires a policy identity".to_string());
    }
    if row.registration.state != RegistrationState::Established {
        reasons.push("gate row requires established registration in the gate policy".to_string());
    }
    if blank(row.registration.authority.as_deref()) {
        reasons.push("gate row requires a registration authority".to_string());
    }
}

fn blank(value: Option<&str>) -> bool {
    value.map(str::trim).unwrap_or("").is_empty()
}

/// Path-like proof ids must resolve inside `root`. Values without `/` are
/// named evidence, not paths, and are left to their own authority.
fn missing_path_like_evidence<'a>(root: &Path, value: &'a str) -> Option<&'a str> {
    if !value.contains('/') {
        return None;
    }
    let path_part = value.split_once('#').map_or(value, |(path, _)| path);
    if !super::validate::is_repository_relative(path_part) {
        return Some(path_part);
    }
    if root.join(path_part).exists() { None } else { Some(path_part) }
}
