//! Validate derived workspace-folder configuration-authority evidence (#16827).
//!
//! The catalog/test checker lives in `perl-lsp-rs-core`. This command supplies
//! the current workspace corpus and emits machine-readable status. It does not
//! implement missing writers and is not a CI-policy lane.

use std::collections::BTreeMap;
use std::fs;

use color_eyre::eyre::{Context, Result, bail};

use crate::utils::project_root;

/// Run the derived workspace-folder configuration-authority checker.
pub fn run() -> Result<()> {
    let root = project_root()?;
    let mut owned = BTreeMap::new();
    for path in perl_lsp_rs_core::derived_workspace_corpus_paths() {
        let text = fs::read_to_string(root.join(path))
            .with_context(|| format!("read configuration-authority corpus {path}"))?;
        owned.insert(*path, text);
    }
    let corpus: BTreeMap<&str, &str> =
        owned.iter().map(|(path, text)| (*path, text.as_str())).collect();
    let report = perl_lsp_rs_core::check_configuration_authority(&corpus);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !report.violations.is_empty() {
        bail!(
            "{} derived workspace-folder configuration-authority violation(s)",
            report.violations.len()
        );
    }
    Ok(())
}
