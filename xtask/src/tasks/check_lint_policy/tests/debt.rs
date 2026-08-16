use super::super::validate::validate_debt_ledger;
use super::{debt_entry, empty_debt, ledger_with, lint_entry, test_date};
use color_eyre::eyre::{Result, bail};

#[test]
fn debt_lints_require_current_rows() -> Result<()> {
    let ledger = ledger_with(vec![lint_entry("clippy::collapsible_if", "debt")]);

    let result = validate_debt_ledger(&ledger, &empty_debt(), test_date()?);
    let Err(error) = result else {
        bail!("debt lint without rows should fail");
    };
    assert!(error.to_string().contains("has no current debt rows"));
    Ok(())
}

#[test]
fn debt_rows_require_debt_status() -> Result<()> {
    let ledger = ledger_with(vec![lint_entry("clippy::collapsible_if", "active")]);
    let mut debt = empty_debt();
    debt.debt.push(debt_entry("clippy::collapsible_if"));

    let result = validate_debt_ledger(&ledger, &debt, test_date()?);
    let Err(error) = result else {
        bail!("debt row for active lint should fail");
    };
    assert!(error.to_string().contains("requires ledger status debt"));
    Ok(())
}

#[test]
fn debt_rows_require_matching_levels() -> Result<()> {
    let ledger = ledger_with(vec![lint_entry("clippy::collapsible_if", "debt")]);
    let mut entry = debt_entry("clippy::collapsible_if");
    entry.level = "warn".to_owned();
    let mut debt = empty_debt();
    debt.debt.push(entry);

    let result = validate_debt_ledger(&ledger, &debt, test_date()?);
    let Err(error) = result else {
        bail!("debt level mismatch should fail");
    };
    assert!(error.to_string().contains("but ledger has deny"));
    Ok(())
}
