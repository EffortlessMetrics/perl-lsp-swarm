//! Clippy lint policy coherence checks.

mod model;
mod read;
mod summary;
mod validate;

#[cfg(test)]
mod tests;

use color_eyre::eyre::{Result, eyre};
use model::DebtLedger;
use std::path::Path;

pub(super) const ROOT_MANIFEST: &str = "Cargo.toml";
pub(super) const CLIPPY_CONFIG: &str = "clippy.toml";
pub(super) const RUST_TOOLCHAIN: &str = "rust-toolchain.toml";
pub(super) const GATE_POLICY: &str = ".ci/gate-policy.yaml";
pub(super) const LINT_LEDGER: &str = "policy/clippy-lints.toml";
pub(super) const LINT_CATALOG_DIR: &str = "policy/clippy-lints.d";
pub(super) const DEBT_LEDGER: &str = "policy/clippy-debt.toml";

/// One review-dated Clippy policy row projected into the advisory cadence inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CadenceRow {
    pub(crate) record_id: String,
    pub(crate) source_kind: &'static str,
    pub(crate) source_path: &'static str,
    pub(crate) owner: String,
    pub(crate) owner_issue: Option<String>,
    pub(crate) review_after: String,
    pub(crate) evidence_identity: String,
    pub(crate) expected_debt_class: String,
    pub(crate) required_decision: &'static str,
    pub(crate) invalid_reason: Option<String>,
}

/// Read the same merged Clippy authorities as `check-lint-policy` and expose only
/// their review-dated rows. Candidate validation stays clock-free; the cadence
/// command owns elapsed-time classification without a second policy parser.
pub(crate) fn cadence_rows(root: &Path) -> Result<Vec<CadenceRow>> {
    let lint_ledger = read::load_lint_ledger(root)?;
    let debt_ledger: DebtLedger = read::read_toml_as(root.join(DEBT_LEDGER))?;
    // Malformed policy projects as `Invalid` rows (merged #15277 behavior):
    // cadence reports obligations even when the candidate gate would reject
    // the underlying policy, so owner work stays visible.
    let validation_error = validate::validate_cadence_sources(root, &lint_ledger, &debt_ledger)
        .err()
        .map(|error| format!("{error:#}"));
    let mut rows = Vec::with_capacity(debt_ledger.debt.len() + lint_ledger.deferred_due.len());

    for entry in &debt_ledger.debt {
        rows.push(CadenceRow {
            record_id: format!("{}:{}", entry.lint, entry.path),
            source_kind: "clippy_debt",
            source_path: DEBT_LEDGER,
            owner: entry.owner.clone(),
            owner_issue: issue_owner(&entry.owner),
            review_after: entry.review_after.clone(),
            evidence_identity: format!("{}@{}:{}", entry.lint, entry.path, entry.level),
            expected_debt_class: entry.lint.clone(),
            required_decision: "resolve, narrow, or re-justify the accepted lint debt",
            invalid_reason: validation_error.clone(),
        });
    }

    for entry in &lint_ledger.deferred_due {
        rows.push(CadenceRow {
            record_id: entry.name.clone(),
            source_kind: "clippy_deferred_due",
            source_path: LINT_LEDGER,
            owner: entry.owner.clone(),
            owner_issue: issue_owner(&entry.owner),
            review_after: entry.review_after.clone(),
            evidence_identity: format!(
                "{}@msrv-{}:{}",
                entry.name, entry.activate_when_msrv, entry.level
            ),
            expected_debt_class: entry.class.clone(),
            required_decision: "activate, move to exact debt, or re-justify the deferral",
            invalid_reason: validation_error.clone(),
        });
    }

    if rows.is_empty()
        && let Some(error) = validation_error
    {
        return Err(eyre!(
            "Clippy cadence sources are invalid and contain no review-dated row to carry the diagnosis: {error}"
        ));
    }

    Ok(rows)
}

fn issue_owner(owner: &str) -> Option<String> {
    let owner = owner.trim();
    owner.starts_with('#').then(|| owner.to_string())
}

pub fn run() -> Result<()> {
    let root = Path::new(".");
    let cargo = read::read_toml(root.join(ROOT_MANIFEST))?;
    let lint_ledger = read::load_lint_ledger(root)?;
    let debt_ledger: DebtLedger = read::read_toml_as(root.join(DEBT_LEDGER))?;

    let configured_selector_count =
        validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger)?;

    print!(
        "{}",
        summary::render_policy_summary(&lint_ledger, &debt_ledger, configured_selector_count)
    );
    Ok(())
}
