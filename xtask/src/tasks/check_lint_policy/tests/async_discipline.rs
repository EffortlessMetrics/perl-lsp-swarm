use color_eyre::eyre::{Result, bail, eyre};
use std::path::Path;
use toml::Value;

const ASYNC_DISCIPLINE_LINTS: &[(&str, &str)] = &[
    ("await_holding_lock", "clippy::await_holding_lock"),
    ("await_holding_refcell_ref", "clippy::await_holding_refcell_ref"),
    ("async_yields_async", "clippy::async_yields_async"),
    ("mutex_atomic", "clippy::mutex_atomic"),
];

fn workspace_root() -> Result<&'static Path> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| eyre!("xtask manifest should have the workspace root as its parent"))
}

fn clippy_workspace_lint_level(cargo: &Value, lint_key: &str) -> Result<String> {
    cargo
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .and_then(|lints| lints.get("clippy"))
        .and_then(|clippy| clippy.get(lint_key))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| eyre!("workspace.lints.clippy.{lint_key} missing from Cargo.toml"))
}

fn ledger_level(root: &Path, identity: &str) -> Result<(String, String)> {
    let ledger = super::super::read::load_lint_ledger(root)?;
    let entry = ledger
        .lint
        .iter()
        .find(|lint| lint.name == identity)
        .ok_or_else(|| eyre!("lint ledger missing active entry for {identity}"))?;
    Ok((entry.level.clone(), entry.status.clone()))
}

#[test]
fn issue_2323_async_discipline_lints_are_workspace_deny_and_ledger_active() -> Result<()> {
    let root = workspace_root()?;
    let cargo = super::super::read::read_toml(root.join(super::super::ROOT_MANIFEST))?;

    for (cargo_key, identity) in ASYNC_DISCIPLINE_LINTS {
        let cargo_level = clippy_workspace_lint_level(&cargo, cargo_key)?;
        if cargo_level != "deny" {
            bail!("issue #2323 requires {cargo_key} = deny in Cargo.toml; found {cargo_level}");
        }
        let (ledger_level, status) = ledger_level(root, identity)?;
        if ledger_level != "deny" {
            bail!(
                "issue #2323 requires {identity} at deny in the lint ledger; found {ledger_level}"
            );
        }
        if status != "active" {
            bail!(
                "issue #2323 requires {identity} to be an active ledger row; found status {status}"
            );
        }
    }
    Ok(())
}

#[test]
fn synchronized_async_yields_async_rollback_fails_closed_through_validate_all() -> Result<()> {
    let root = workspace_root()?;
    let mut cargo = super::super::read::read_toml(root.join(super::super::ROOT_MANIFEST))?;
    let mut lint_ledger = super::super::read::load_lint_ledger(root)?;
    let debt_ledger: super::super::model::DebtLedger =
        super::super::read::read_toml_as(root.join(super::super::DEBT_LEDGER))?;

    *cargo
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("lints"))
        .and_then(|lints| lints.get_mut("clippy"))
        .and_then(|clippy| clippy.get_mut("async_yields_async"))
        .ok_or_else(|| eyre!("workspace clippy lints should carry async_yields_async"))? =
        Value::String("warn".to_owned());
    lint_ledger
        .lint
        .iter_mut()
        .find(|lint| lint.name == "clippy::async_yields_async")
        .ok_or_else(|| eyre!("lint ledger should carry clippy::async_yields_async"))?
        .level = "warn".to_owned();

    let Err(error) = super::super::validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger)
    else {
        bail!("synchronized async_yields_async rollback must fail closed through validate_all");
    };
    assert!(error.to_string().contains("clippy::async_yields_async"));
    assert!(error.to_string().contains("must remain at level deny"));
    Ok(())
}

#[test]
fn synchronized_mutex_atomic_rollback_fails_closed_through_validate_all() -> Result<()> {
    let root = workspace_root()?;
    let mut cargo = super::super::read::read_toml(root.join(super::super::ROOT_MANIFEST))?;
    let mut lint_ledger = super::super::read::load_lint_ledger(root)?;
    let debt_ledger: super::super::model::DebtLedger =
        super::super::read::read_toml_as(root.join(super::super::DEBT_LEDGER))?;

    *cargo
        .get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("lints"))
        .and_then(|lints| lints.get_mut("clippy"))
        .and_then(|clippy| clippy.get_mut("mutex_atomic"))
        .ok_or_else(|| eyre!("workspace clippy lints should carry mutex_atomic"))? =
        Value::String("warn".to_owned());
    lint_ledger
        .lint
        .iter_mut()
        .find(|lint| lint.name == "clippy::mutex_atomic")
        .ok_or_else(|| eyre!("lint ledger should carry clippy::mutex_atomic"))?
        .level = "warn".to_owned();

    let Err(error) = super::super::validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger)
    else {
        bail!("synchronized mutex_atomic rollback must fail closed through validate_all");
    };
    assert!(error.to_string().contains("clippy::mutex_atomic"));
    assert!(error.to_string().contains("must remain at level deny"));
    Ok(())
}
