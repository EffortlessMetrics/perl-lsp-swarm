use super::super::validate::{validate_required_dispositions, validate_workspace_lints};
use super::{deferred_lint, empty_cargo, ledger_with, lint_entry, planned_lint, test_date};
use color_eyre::eyre::{Result, bail};
use toml::Value;

const REQUIRED: [&str; 9] = [
    "rust::const_item_interior_mutations",
    "rust::function_casts_as_integer",
    "clippy::same_length_and_capacity",
    "clippy::manual_checked_ops",
    "clippy::manual_ilog2",
    "clippy::manual_take",
    "clippy::manual_pop_if",
    "rust::let_underscore_lock",
    "clippy::let_underscore_lock",
];

fn required_ledger() -> super::super::model::LintLedger {
    let mut ledger = ledger_with(vec![
        lint_entry(REQUIRED[0], "active"),
        lint_entry(REQUIRED[1], "debt"),
        lint_entry(REQUIRED[2], "tracked"),
    ]);
    ledger.planned.push(planned_lint(REQUIRED[3], "1.96"));
    ledger.lint.push(lint_entry(REQUIRED[4], "active"));
    ledger.deferred_due.push(deferred_lint(REQUIRED[5], "1.95"));
    ledger.deferred_due.push(deferred_lint(REQUIRED[6], "1.95"));
    ledger.lint.push(lint_entry(REQUIRED[7], "active"));
    ledger.lint.push(lint_entry(REQUIRED[8], "active"));
    ledger
}

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
fn active_lints_cannot_leave_cargo_unratcheted() -> Result<()> {
    let ledger = ledger_with(vec![lint_entry("clippy::collapsible_if", "active")]);

    let result = validate_workspace_lints(&empty_cargo()?, &ledger, test_date()?);
    let Err(error) = result else {
        bail!("active lint without a Cargo.toml activation should fail");
    };
    // The validator names the lint status first (#10135); the assertion must
    // match the status-first message contract (#12772). The branch's earlier
    // note: the original assertion (#12737) transposed that word order and
    // could never match; an active ledger row without a Cargo.toml activation
    // still fails with this exact typed reason.
    assert!(
        error.to_string().contains("active lint clippy::collapsible_if is missing from Cargo.toml")
    );
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

#[test]
fn required_lint_identity_cannot_be_deleted_from_the_merged_model() -> Result<()> {
    let mut ledger = required_ledger();
    ledger.planned.retain(|lint| lint.name != REQUIRED[3]);

    let result = validate_required_dispositions(&ledger);
    let Err(error) = result else {
        bail!("missing required lint identity should fail closed");
    };
    assert!(error.to_string().contains(REQUIRED[3]));
    assert!(error.to_string().contains("exactly once"));
    Ok(())
}

#[test]
fn disallowed_fields_phase1_does_not_depend_on_required_dispositions() -> Result<()> {
    let ledger = required_ledger();
    validate_required_dispositions(&ledger)
}

#[test]
fn required_manual_ilog2_cannot_be_demoted_out_of_active_enforcement() -> Result<()> {
    // Demotion keeps identity count at one and keeps level "deny" (planned rows
    // carry deny too), so neither the identity nor the level pin can catch it.
    // Only the pinned active status fails this closed.
    let mut demoted = required_ledger();
    demoted.lint.retain(|lint| lint.name != "clippy::manual_ilog2");
    demoted.planned.push(planned_lint("clippy::manual_ilog2", "1.99"));

    let Err(error) = validate_required_dispositions(&demoted) else {
        bail!("demoting manual_ilog2 to a planned row should fail closed");
    };
    assert!(error.to_string().contains("clippy::manual_ilog2"));
    assert!(error.to_string().contains("must remain an active ledger entry"));
    Ok(())
}

#[test]
fn required_manual_ilog2_level_and_identity_cannot_be_rolled_back() -> Result<()> {
    let mut rolled_back = required_ledger();
    rolled_back
        .lint
        .iter_mut()
        .find(|lint| lint.name == "clippy::manual_ilog2")
        .ok_or_else(|| color_eyre::eyre::eyre!("manual_ilog2 fixture entry missing"))?
        .level = "warn".to_owned();

    let result = validate_required_dispositions(&rolled_back);
    let Err(error) = result else {
        bail!("synchronized manual_ilog2 rollback to warn should fail closed");
    };
    assert!(error.to_string().contains("clippy::manual_ilog2"));
    assert!(error.to_string().contains("must remain at level deny"));

    let mut partially_removed = required_ledger();
    partially_removed.lint.retain(|lint| lint.name != "clippy::manual_ilog2");
    let result = validate_required_dispositions(&partially_removed);
    let Err(error) = result else {
        bail!("removing the required manual_ilog2 disposition should fail closed");
    };
    assert!(error.to_string().contains("clippy::manual_ilog2"));
    assert!(error.to_string().contains("exactly once"));
    Ok(())
}

#[test]
fn each_lock_guard_row_is_independently_pinned_to_active_deny() -> Result<()> {
    // The lock-guard invariant spans two tools with non-overlapping coverage,
    // so the ratchet has to hold each row on its own. A single pin would let a
    // rollback drop one tool's row while the other row still reads as coverage
    // — which is exactly the dishonest half-rollback #14444 rules out.
    for identity in ["rust::let_underscore_lock", "clippy::let_underscore_lock"] {
        let mut demoted = required_ledger();
        demoted.lint.retain(|lint| lint.name != identity);
        demoted.planned.push(planned_lint(identity, "1.99"));

        let Err(error) = validate_required_dispositions(&demoted) else {
            bail!("demoting {identity} to a planned row should fail closed");
        };
        assert!(error.to_string().contains(identity));
        assert!(error.to_string().contains("must remain an active ledger entry"));

        let mut downgraded = required_ledger();
        downgraded
            .lint
            .iter_mut()
            .find(|lint| lint.name == identity)
            .ok_or_else(|| color_eyre::eyre::eyre!("{identity} fixture entry missing"))?
            .level = "warn".to_owned();

        let Err(error) = validate_required_dispositions(&downgraded) else {
            bail!("downgrading {identity} to warn should fail closed");
        };
        assert!(error.to_string().contains(identity));
        assert!(error.to_string().contains("must remain at level deny"));

        let mut removed = required_ledger();
        removed.lint.retain(|lint| lint.name != identity);

        let Err(error) = validate_required_dispositions(&removed) else {
            bail!("removing the required {identity} disposition should fail closed");
        };
        assert!(error.to_string().contains(identity));
        assert!(error.to_string().contains("exactly once"));
    }
    Ok(())
}

#[test]
fn dropping_one_lock_guard_row_does_not_hide_behind_its_sibling() -> Result<()> {
    // Negative control for the pin above: the fixture that keeps both rows must
    // pass, so the failures asserted there come from the removed row and not
    // from an unrelated defect in the shared fixture.
    validate_required_dispositions(&required_ledger())?;

    let mut clippy_only = required_ledger();
    clippy_only.lint.retain(|lint| lint.name != "rust::let_underscore_lock");
    let Err(error) = validate_required_dispositions(&clippy_only) else {
        bail!("keeping only the Clippy row must not satisfy the standard-library coverage pin");
    };
    assert!(error.to_string().contains("rust::let_underscore_lock"));

    let mut rust_only = required_ledger();
    rust_only.lint.retain(|lint| lint.name != "clippy::let_underscore_lock");
    let Err(error) = validate_required_dispositions(&rust_only) else {
        bail!("keeping only the rustc row must not satisfy the parking_lot coverage pin");
    };
    assert!(error.to_string().contains("clippy::let_underscore_lock"));
    Ok(())
}
