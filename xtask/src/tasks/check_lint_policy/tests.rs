mod config;
mod debt;
mod disposition;
mod summary;

use super::model::{
    DebtEntry, DebtLedger, DeferredLint, LintEntry, LintLedger, LintPolicy, PlannedLint,
};
use chrono::NaiveDate;
use color_eyre::eyre::{Result, eyre};
use std::path::Path;
use toml::Value;

pub(super) fn test_date() -> Result<NaiveDate> {
    NaiveDate::from_ymd_opt(2026, 8, 15).ok_or_else(|| eyre!("invalid test date"))
}

pub(super) fn policy() -> LintPolicy {
    LintPolicy {
        panic_free_tests: true,
        allow_test_carveouts: false,
        suppression_style: "expect-with-reason".to_owned(),
        blanket_categories: false,
    }
}

pub(super) fn lint_entry(name: &str, status: &str) -> LintEntry {
    LintEntry {
        name: name.to_owned(),
        level: "deny".to_owned(),
        status: status.to_owned(),
        class: "test".to_owned(),
        reason: "test reason".to_owned(),
    }
}

pub(super) fn planned_lint(name: &str, activate_when_msrv: &str) -> PlannedLint {
    PlannedLint {
        name: name.to_owned(),
        level: "deny".to_owned(),
        activate_when_msrv: activate_when_msrv.to_owned(),
        class: "test".to_owned(),
        reason: "test reason".to_owned(),
    }
}

pub(super) fn deferred_lint(name: &str, activate_when_msrv: &str) -> DeferredLint {
    DeferredLint {
        name: name.to_owned(),
        level: "deny".to_owned(),
        activate_when_msrv: activate_when_msrv.to_owned(),
        class: "test".to_owned(),
        owner: "#1".to_owned(),
        reason: "test reason".to_owned(),
        review_after: "2026-10-15".to_owned(),
        next_status: "active".to_owned(),
    }
}

pub(super) fn ledger_with(lints: Vec<LintEntry>) -> LintLedger {
    LintLedger {
        schema: 2,
        msrv: "1.95".to_owned(),
        policy: policy(),
        lint: lints,
        planned: Vec::new(),
        deferred_due: Vec::new(),
    }
}

pub(super) fn empty_cargo() -> Result<Value> {
    let cargo = toml::from_str(
        r#"
        [workspace.lints.clippy]
        "#,
    )?;
    Ok(cargo)
}

pub(super) fn empty_debt() -> DebtLedger {
    DebtLedger { schema: 2, debt: Vec::new() }
}

pub(super) fn debt_entry(lint: &str) -> DebtEntry {
    DebtEntry {
        lint: lint.to_owned(),
        level: "deny".to_owned(),
        path: "Cargo.toml".to_owned(),
        owner: "#1".to_owned(),
        reason: "test reason".to_owned(),
        review_after: "2026-10-15".to_owned(),
    }
}

pub(super) fn test_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or_else(|| Path::new("."))
}

#[test]
fn repository_catalog_and_workspace_inputs_validate() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| eyre!("xtask manifest should have the workspace root as its parent"))?;
    let cargo = super::read::read_toml(root.join(super::ROOT_MANIFEST))?;
    let lint_ledger = super::read::load_lint_ledger(root)?;
    let debt_ledger: DebtLedger = super::read::read_toml_as(root.join(super::DEBT_LEDGER))?;

    super::validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger, test_date()?)
}
