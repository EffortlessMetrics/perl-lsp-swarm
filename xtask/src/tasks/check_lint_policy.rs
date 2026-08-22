//! Clippy lint policy coherence checks.

mod model;
mod read;
mod summary;
mod validate;

#[cfg(test)]
mod tests;

use chrono::Utc;
use color_eyre::eyre::Result;
use model::DebtLedger;
use std::path::Path;

pub(super) const ROOT_MANIFEST: &str = "Cargo.toml";
pub(super) const CLIPPY_CONFIG: &str = "clippy.toml";
pub(super) const RUST_TOOLCHAIN: &str = "rust-toolchain.toml";
pub(super) const GATE_POLICY: &str = ".ci/gate-policy.yaml";
pub(super) const LINT_LEDGER: &str = "policy/clippy-lints.toml";
pub(super) const LINT_CATALOG_DIR: &str = "policy/clippy-lints.d";
pub(super) const DEBT_LEDGER: &str = "policy/clippy-debt.toml";

pub fn run() -> Result<()> {
    let root = Path::new(".");
    let cargo = read::read_toml(root.join(ROOT_MANIFEST))?;
    let lint_ledger = read::load_lint_ledger(root)?;
    let debt_ledger: DebtLedger = read::read_toml_as(root.join(DEBT_LEDGER))?;
    let today = Utc::now().date_naive();

    validate::validate_all(root, &cargo, &lint_ledger, &debt_ledger, today)?;

    print!("{}", summary::render_policy_summary(&lint_ledger, &debt_ledger));
    Ok(())
}
