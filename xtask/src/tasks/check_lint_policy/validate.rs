mod common;
mod config;
mod debt;
mod disposition;

#[cfg(test)]
pub(super) use common::ensure_version_matches;
#[cfg(test)]
pub(super) use config::validate_clippy_config_value;
#[cfg(test)]
pub(super) use debt::validate_debt_ledger;
#[cfg(test)]
pub(super) use disposition::{validate_required_dispositions, validate_workspace_lints};

use super::model::{DebtLedger, LintLedger};
use chrono::NaiveDate;
use color_eyre::eyre::Result;
use std::path::Path;
use toml::Value;

pub(super) fn validate_all(
    root: &Path,
    cargo: &Value,
    lint_ledger: &LintLedger,
    debt_ledger: &DebtLedger,
    today: NaiveDate,
) -> Result<()> {
    config::validate_policy_header(lint_ledger)?;
    config::validate_msrv_sources(root, cargo, lint_ledger)?;
    disposition::validate_workspace_lints(cargo, lint_ledger, today)?;
    disposition::validate_required_dispositions(lint_ledger)?;
    config::validate_workspace_members_inherit_lints(root, cargo)?;
    config::validate_clippy_config(root, lint_ledger)?;
    debt::validate_debt_ledger(root, lint_ledger, debt_ledger, today)?;
    Ok(())
}
