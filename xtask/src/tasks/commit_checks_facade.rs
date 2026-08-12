//! Public dispatch facade for the commit-tier staged checks.
//!
//! The established checks remain in `commit_checks.rs`. Changie is routed
//! through a focused implementation that can materialize the frozen staged
//! tree and run Changie's own dry-render validation without widening the
//! established module or creating a second pre-commit authority.

#[path = "commit_checks_changie.rs"]
mod changie;
#[path = "commit_checks.rs"]
mod established;

pub use established::{CheckReport, CommitCheckOutcome, Posture, REPORT_MARKER, parse_report};

use crate::utils::project_root;
use color_eyre::eyre::Result;

/// Run one named commit-tier check against the captured staged tree.
///
/// All established checks retain their existing implementation. The Changie
/// check is intercepted here so its authoritative dry-render can remain a
/// focused module while preserving the public dispatch and receipt contract.
pub fn run_named_check(name: &str, tree_oid: Option<&str>) -> Result<CommitCheckOutcome> {
    if name != "changie_fragment_staged" {
        return established::run_named_check(name, tree_oid);
    }

    let root = project_root()?;
    Ok(changie::run(&root, tree_oid).unwrap_or_else(|err| {
        CommitCheckOutcome::Flagged(CheckReport {
            check: name.to_string(),
            posture: Posture::NotProven,
            result: format!("the {name} instrument failed to run to completion"),
            why: "a tool/subprocess error or undecodable staged artifact means Changie's own \
                  renderer did not verify the ledger; that is not the same as a clean staged tree"
                .to_string(),
            affected: Vec::new(),
            fix: Some(format!("investigate and re-run: {err:#}")),
            rerun: format!("cargo xtask gates --tier commit --staged --gate {name}"),
            what_remains: "Changie's authoritative staged-tree dry-render is still outstanding"
                .to_string(),
        })
    }))
}
