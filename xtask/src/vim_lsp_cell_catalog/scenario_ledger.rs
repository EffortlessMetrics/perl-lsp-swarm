//! Mirror of the landed #11371 BDD scenario ledger
//! (`.spec/11371-vim-bdd-journeys/`, merged as PR #12070).
//!
//! The spec packet owns the 30 stable `vim.bdd.<family>.<nn>` scenario IDs and
//! their baseline/optional classification. This mirror exists so cell
//! registrations can cite landed IDs at validation time; the contract tests
//! scan the spec files and require set equality with this mirror, so neither
//! side can drift silently. Retiring or renumbering an ID is a #11371
//! authority change, never a mirror edit.
//!
//! Future journey families (#11376-class freshness/save/recovery/reopen/
//! expanded-activation extensions) arrive as their own landed ledger constants
//! beside their family catalogs; this ledger's rows are frozen.

use super::{Scenario, ScenarioClass, ScenarioLedger};

pub const VIM_BDD_LEDGER_ID: &str = "vim.bdd.11371";

fn baseline(id: &str) -> Scenario {
    Scenario { id: id.to_string(), class: ScenarioClass::Baseline }
}

fn optional(id: &str) -> Scenario {
    Scenario { id: id.to_string(), class: ScenarioClass::Optional }
}

/// The landed #11371 ledger: 23 baseline scenarios plus 7 optional
/// `consumes_if_available` inputs, in fixed published family order.
pub fn vim_bdd_ledger_11371() -> ScenarioLedger {
    ScenarioLedger {
        ledger_id: VIM_BDD_LEDGER_ID.to_string(),
        owning_authority: "#11371 (.spec/11371-vim-bdd-journeys, PR #12070)".to_string(),
        scenarios: vec![
            // Feature: Vim attaches vim-lsp to the intended Perl project.
            baseline("vim.bdd.attach.01"),
            baseline("vim.bdd.attach.02"),
            baseline("vim.bdd.attach.03"),
            baseline("vim.bdd.attach.04"),
            baseline("vim.bdd.attach.05"),
            baseline("vim.bdd.attach.06"),
            baseline("vim.bdd.attach.07"),
            // Feature: Vim applies ordinary completion and navigation.
            baseline("vim.bdd.nav.01"),
            baseline("vim.bdd.nav.02"),
            baseline("vim.bdd.nav.03"),
            baseline("vim.bdd.nav.04"),
            baseline("vim.bdd.nav.05"),
            baseline("vim.bdd.nav.06"),
            // Feature: Vim applies server edits and configuration effects.
            baseline("vim.bdd.edit.01"),
            baseline("vim.bdd.edit.02"),
            baseline("vim.bdd.edit.03"),
            baseline("vim.bdd.edit.04"),
            baseline("vim.bdd.edit.05"),
            // Feature: position, synchronization, currentness, lifecycle.
            baseline("vim.bdd.lifecycle.01"),
            baseline("vim.bdd.lifecycle.02"),
            baseline("vim.bdd.lifecycle.03"),
            baseline("vim.bdd.lifecycle.04"),
            baseline("vim.bdd.lifecycle.05"),
            // Optional and stronger-profile inputs (never baseline blockers).
            optional("vim.bdd.opt.01"),
            optional("vim.bdd.opt.02"),
            optional("vim.bdd.opt.03"),
            optional("vim.bdd.opt.04"),
            optional("vim.bdd.opt.05"),
            optional("vim.bdd.opt.06"),
            optional("vim.bdd.opt.07"),
        ],
    }
}
