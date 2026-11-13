use super::super::model::{DebtLedger, LintEntry, LintLedger, RustVersion};
use super::super::read::{load_lint_ledger, read_toml, read_toml_as};
use super::super::validate::{validate_all, validate_cadence_sources, validate_workspace_lints};
use super::super::{DEBT_LEDGER, ROOT_MANIFEST};
use super::{debt_entry, deferred_lint, planned_lint, test_root};
use color_eyre::eyre::{Result, bail, eyre};
use toml::Value;

// Keep the contract independent of REQUIRED_DISPOSITIONS: deleting a production
// pin must not also remove the corresponding regression case.
const CORE_DENIALS: &[(&str, &str)] = &[
    ("unwrap_used", "clippy::unwrap_used"),
    ("expect_used", "clippy::expect_used"),
    ("panic", "clippy::panic"),
    ("todo", "clippy::todo"),
    ("unimplemented", "clippy::unimplemented"),
    ("dbg_macro", "clippy::dbg_macro"),
];

fn repository_inputs() -> Result<(Value, LintLedger, DebtLedger)> {
    let root = test_root();
    Ok((
        read_toml(root.join(ROOT_MANIFEST))?,
        load_lint_ledger(root)?,
        read_toml_as(root.join(DEBT_LEDGER))?,
    ))
}

fn remove_cargo_lint(cargo: &mut Value, key: &str) -> Result<()> {
    cargo
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("lints"))
        .and_then(|lints| lints.get_mut("clippy"))
        .and_then(Value::as_table_mut)
        .ok_or_else(|| eyre!("workspace.lints.clippy should be a table"))?
        .remove(key)
        .ok_or_else(|| eyre!("workspace Clippy lints should carry {key}"))?;
    Ok(())
}

fn remove_ledger_entry(ledger: &mut LintLedger, identity: &str) -> Result<LintEntry> {
    let index = ledger
        .lint
        .iter()
        .position(|lint| lint.name == identity)
        .ok_or_else(|| eyre!("lint ledger should carry {identity}"))?;
    Ok(ledger.lint.remove(index))
}

fn assert_rollback_rejected(
    cargo: &Value,
    ledger: &LintLedger,
    debt: &DebtLedger,
    identity: &str,
    expected: &str,
) -> Result<()> {
    // Every mutation leaves the two authorities agreeing. An unrelated model
    // defect or a simple Cargo/ledger mismatch cannot stand in for the ratchet.
    validate_workspace_lints(cargo, ledger)?;

    for (consumer, result) in [
        ("validate_all", validate_all(test_root(), cargo, ledger, debt).map(|_| ())),
        ("validate_cadence_sources", validate_cadence_sources(test_root(), ledger, debt)),
    ] {
        let Err(error) = result else {
            bail!("{consumer} accepted the {identity} rollback; expected {expected}");
        };
        let message = error.to_string();
        assert!(
            message.contains(identity) && message.contains(expected),
            "{consumer} should reject {identity} for {expected}, got {message}"
        );
    }
    Ok(())
}

#[test]
fn core_denials_current_repository_policy_validates() -> Result<()> {
    // Positive controls bind both consumers to the shipped Cargo and merged
    // catalog before the negative cases change exactly one governed identity.
    let (cargo, ledger, debt) = repository_inputs()?;
    validate_all(test_root(), &cargo, &ledger, &debt)?;
    validate_cadence_sources(test_root(), &ledger, &debt)
}

#[test]
fn core_denials_reject_synchronized_warn_and_allow_rollbacks() -> Result<()> {
    for &(key, identity) in CORE_DENIALS {
        for level in ["warn", "allow"] {
            let (mut cargo, mut ledger, debt) = repository_inputs()?;
            *cargo
                .get_mut("workspace")
                .and_then(|workspace| workspace.get_mut("lints"))
                .and_then(|lints| lints.get_mut("clippy"))
                .and_then(|clippy| clippy.get_mut(key))
                .ok_or_else(|| eyre!("workspace Clippy lints should carry {key}"))? =
                Value::String(level.to_owned());
            ledger
                .lint
                .iter_mut()
                .find(|lint| lint.name == identity)
                .ok_or_else(|| eyre!("lint ledger should carry {identity}"))?
                .level = level.to_owned();

            assert_rollback_rejected(
                &cargo,
                &ledger,
                &debt,
                identity,
                "must remain at level deny",
            )?;
        }
    }
    Ok(())
}

#[test]
fn core_denials_reject_synchronized_removal() -> Result<()> {
    for &(key, identity) in CORE_DENIALS {
        let (mut cargo, mut ledger, debt) = repository_inputs()?;
        remove_cargo_lint(&mut cargo, key)?;
        remove_ledger_entry(&mut ledger, identity)?;

        assert_rollback_rejected(&cargo, &ledger, &debt, identity, "must appear exactly once")?;
    }
    Ok(())
}

#[test]
fn core_denials_cannot_be_demoted_from_active() -> Result<()> {
    for &(key, identity) in CORE_DENIALS {
        for status in ["tracked", "debt", "planned", "deferred_due"] {
            let (mut cargo, mut ledger, mut debt) = repository_inputs()?;
            let mut entry = remove_ledger_entry(&mut ledger, identity)?;
            if status != "debt" {
                remove_cargo_lint(&mut cargo, key)?;
            }
            match status {
                "tracked" | "debt" => {
                    entry.status = status.to_owned();
                    ledger.lint.push(entry);
                    if status == "debt" {
                        // A complete debt row keeps this a valid lifecycle
                        // transition without relying on missing debt metadata.
                        debt.debt.push(debt_entry(identity));
                    }
                }
                "planned" => {
                    let current = RustVersion::from_text(&ledger.msrv)?;
                    let future = format!("{}.{}", current.major, current.minor + 1);
                    ledger.planned.push(planned_lint(identity, &future));
                }
                "deferred_due" => {
                    ledger.deferred_due.push(deferred_lint(identity, &ledger.msrv));
                }
                _ => bail!("unexpected fixture disposition {status}"),
            }

            assert_rollback_rejected(
                &cargo,
                &ledger,
                &debt,
                identity,
                "must remain an active ledger entry",
            )?;
        }
    }
    Ok(())
}
