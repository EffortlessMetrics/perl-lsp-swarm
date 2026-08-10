//! Compatibility shim for the retired tracked goal-selector surface.
//!
//! Live programme selection now belongs to GitHub issues, PRs, dependencies,
//! and the provider-native `deliver-goal` flow. The old command remains
//! temporarily so existing scripts fail forward without restoring repository
//! authority to `.perl-lsp/goals/`.

use color_eyre::eyre::Result;

/// Stable retirement receipt for `cargo xtask check-active-goal-manifest`.
///
/// The wording is part of the compatibility contract documented in
/// `docs/swarm/operating-model.md`: the command validates nothing, selects no
/// work, and mutates nothing.
pub(crate) const RETIREMENT_RECEIPT: &str = "check-active-goal-manifest: retired: selected_work=none, validation_performed=false, mutation_performed=false; use current GitHub issues/PRs and deliver-goal";

pub fn run() -> Result<()> {
    println!("{RETIREMENT_RECEIPT}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RETIREMENT_RECEIPT;

    #[test]
    fn retirement_receipt_states_the_inert_contract() {
        assert!(RETIREMENT_RECEIPT.contains("retired:"));
        assert!(RETIREMENT_RECEIPT.contains("selected_work=none"));
        assert!(RETIREMENT_RECEIPT.contains("validation_performed=false"));
        assert!(RETIREMENT_RECEIPT.contains("mutation_performed=false"));
        assert!(!RETIREMENT_RECEIPT.contains('—'));
    }
}
