use super::super::validate::validate_workspace_lints;
use super::{deferred_lint, empty_cargo, ledger_with, lint_entry, planned_lint, test_date};
use color_eyre::eyre::{Result, bail};
use toml::Value;

#[test]
fn lint_entry_accepts_tracked_status() -> Result<()> {
    let cargo = empty_cargo()?;
    let ledger = ledger_with(vec![lint_entry("clippy::indexing_slicing", "tracked")]);
    validate_workspace_lints(&cargo, &ledger, test_date()?)
}

#[test]
fn lint_entry_rejects_unknown_status() -> Result<()> {
    let cargo = empty_cargo()?;
    let ledger = ledger_with(vec![lint_entry("clippy::indexing_slicing", "candidate")]);

    let result = validate_workspace_lints(&cargo, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("candidate status should be rejected");
    };
    assert!(error.to_string().contains("active, debt, or tracked"));
    Ok(())
}

#[test]
fn cargo_lints_require_ledger_entries() -> Result<()> {
    let cargo = toml::from_str::<Value>(
        r#"
        [workspace.lints.clippy]
        manual_take = "deny"
        "#,
    )?;
    let ledger = ledger_with(Vec::new());

    let result = validate_workspace_lints(&cargo, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("unledgered Cargo lint should fail");
    };
    assert!(error.to_string().contains("unledgered lint clippy::manual_take"));
    Ok(())
}

#[test]
fn active_lints_require_matching_cargo_levels() -> Result<()> {
    let cargo = toml::from_str::<Value>(
        r#"
        [workspace.lints.clippy]
        manual_take = "warn"
        "#,
    )?;
    let ledger = ledger_with(vec![lint_entry("clippy::manual_take", "active")]);

    let result = validate_workspace_lints(&cargo, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("level mismatch should fail");
    };
    assert!(error.to_string().contains("level mismatch"));
    Ok(())
}

#[test]
fn duplicate_active_and_planned_dispositions_fail() -> Result<()> {
    let mut ledger = ledger_with(vec![lint_entry("clippy::manual_take", "active")]);
    ledger.planned.push(planned_lint("clippy::manual_take", "1.96"));

    let result = validate_workspace_lints(&empty_cargo()?, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("duplicate disposition should fail");
    };
    assert!(error.to_string().contains("multiple dispositions"));
    Ok(())
}

#[test]
fn due_lint_cannot_remain_future_planned() -> Result<()> {
    let mut ledger = ledger_with(Vec::new());
    ledger.planned.push(planned_lint("clippy::manual_checked_ops", "1.95"));

    let result = validate_workspace_lints(&empty_cargo()?, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("due planned lint should fail");
    };
    assert!(error.to_string().contains("move it to deferred_due"));
    Ok(())
}

#[test]
fn future_planned_lint_remains_absent_from_cargo() -> Result<()> {
    let mut ledger = ledger_with(Vec::new());
    ledger.planned.push(planned_lint("clippy::manual_pop_if", "1.96"));
    validate_workspace_lints(&empty_cargo()?, &ledger, test_date()?)
}

#[test]
fn expired_deferred_lint_fails() -> Result<()> {
    let mut deferred = deferred_lint("clippy::manual_checked_ops", "1.95");
    deferred.review_after = "2026-08-14".to_owned();
    let mut ledger = ledger_with(Vec::new());
    ledger.deferred_due.push(deferred);

    let result = validate_workspace_lints(&empty_cargo()?, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("expired deferral should fail");
    };
    assert!(error.to_string().contains("review date expired"));
    Ok(())
}
