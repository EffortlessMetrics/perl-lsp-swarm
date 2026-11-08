//! Targeted checks for changed crates
//!
//! Detects which crates have changed since a base git ref and runs
//! clippy and/or tests only for those crates. This gives fast feedback
//! during active development without running the full CI suite.

use std::collections::BTreeSet;
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::utils::project_root;

/// Check mode: which checks to run on changed crates.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum CheckMode {
    /// Run only clippy
    Clippy,
    /// Run only tests
    Test,
    /// Run both clippy and tests
    All,
}

/// Extract unique crate directory prefixes (e.g., "crates/perl-parser") from changed files.
fn extract_crate_dirs(files: &[String]) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for file in files {
        // Match files under crates/<name>/...
        let parts: Vec<&str> = file.splitn(3, '/').collect();
        if parts.len() >= 2 && parts[0] == "crates" && !parts[1].is_empty() {
            dirs.insert(format!("crates/{}", parts[1]));
        }
    }
    dirs
}

pub use perl_ci_hygiene::package_resolver::{resolve_package_names, resolve_single_package_name};

/// Run targeted checks (clippy and/or tests) for the given packages.
fn run_checks(
    project_root: &Path,
    packages: &BTreeSet<String>,
    mode: &CheckMode,
    spinner: &ProgressBar,
) -> Result<()> {
    // Build -p args for all packages at once (matches the bash script behavior)
    let mut package_args: Vec<String> = Vec::new();
    for pkg in packages {
        package_args.push("-p".to_string());
        package_args.push(pkg.clone());
    }

    let run_clippy = matches!(mode, CheckMode::Clippy | CheckMode::All);
    let run_tests = matches!(mode, CheckMode::Test | CheckMode::All);

    if run_clippy {
        spinner.println("");
        spinner.set_message("Running clippy for changed packages...");

        let mut args: Vec<&str> = vec!["clippy"];
        for a in &package_args {
            args.push(a.as_str());
        }
        args.extend_from_slice(&["--locked", "--", "-D", "warnings", "-A", "missing_docs"]);

        let result = cmd("cargo", &args)
            .dir(project_root)
            .unchecked()
            .run()
            .context("Failed to run cargo clippy")?;

        if !result.status.success() {
            return Err(eyre!("Clippy failed for changed packages"));
        }
        spinner.println("Clippy passed for changed packages");
    }

    if run_tests {
        spinner.println("");
        spinner.set_message("Running tests for changed packages...");

        let mut args: Vec<&str> = vec!["test"];
        for a in &package_args {
            args.push(a.as_str());
        }
        args.extend_from_slice(&["--lib", "--locked"]);

        let result = cmd("cargo", &args)
            .dir(project_root)
            .unchecked()
            .run()
            .context("Failed to run cargo test")?;

        if !result.status.success() {
            return Err(eyre!("Tests failed for changed packages"));
        }
        spinner.println("Tests passed for changed packages");
    }

    Ok(())
}

/// Entry point for the targeted-checks subcommand.
///
/// Base resolution and the changed-path diff are delegated to the shared
/// `change_set::resolve_change_set` resolver (#3985 Slice 2) rather than a
/// private copy of the main-first candidate chain + three-dot/two-dot diff.
pub fn run(base: String, mode: CheckMode) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    let style = ProgressStyle::default_spinner()
        .template("{spinner:.green} {wide_msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner());
    spinner.set_style(style);

    let root = project_root()?;
    spinner.set_message("Resolving base ref...");

    let identity = ArtifactIdentity::CommitRange { base, head: "HEAD".to_string() };
    let resolved = change_set::resolve_change_set(identity, &root)?;
    let base_ref = match resolved.identity {
        ArtifactIdentity::CommitRange { base, .. } => base,
        ArtifactIdentity::StagedTree { .. } => {
            return Err(eyre!(
                "resolve_change_set returned a StagedTree identity for a CommitRange input"
            ));
        }
    };

    spinner.set_message(format!("Detecting changes since {}...", base_ref));
    let files = resolved.changed_paths;

    let crate_dirs = extract_crate_dirs(&files);
    if crate_dirs.is_empty() {
        spinner.finish_with_message(format!(
            "No crate changes detected since {}; skipping targeted checks",
            base_ref,
        ));
        return Ok(());
    }

    spinner.set_message("Resolving package names...");
    let packages = resolve_package_names(&root, &crate_dirs)?;

    if packages.is_empty() {
        spinner.finish_with_message(
            "Changed crate directories found, but no workspace package names could be resolved",
        );
        return Err(eyre!(
            "Changed crate directories found, but no package names could be resolved"
        ));
    }

    println!("Detected changed packages since {}:", base_ref);
    for pkg in &packages {
        println!("  - {}", pkg);
    }

    run_checks(&root, &packages, &mode, &spinner)?;

    spinner.finish_with_message("Targeted checks completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_crate_dirs_basic() {
        let files = vec![
            "crates/perl-parser/src/lib.rs".to_string(),
            "crates/perl-lsp-rs/src/main.rs".to_string(),
            "crates/perl-parser/tests/test.rs".to_string(),
            "README.md".to_string(),
            "scripts/something.sh".to_string(),
        ];

        let dirs = extract_crate_dirs(&files);
        assert_eq!(dirs.len(), 2);
        assert!(dirs.contains("crates/perl-parser"));
        assert!(dirs.contains("crates/perl-lsp-rs"));
    }

    #[test]
    fn test_extract_crate_dirs_empty() {
        let files = vec!["README.md".to_string(), "justfile".to_string()];

        let dirs = extract_crate_dirs(&files);
        assert!(dirs.is_empty());
    }

    #[test]
    fn test_extract_crate_dirs_nested() {
        let files = vec![
            "crates/perl-lsp-navigation/src/lib.rs".to_string(),
            "crates/perl-lsp-navigation/src/goto.rs".to_string(),
        ];

        let dirs = extract_crate_dirs(&files);
        assert_eq!(dirs.len(), 1);
        assert!(dirs.contains("crates/perl-lsp-navigation"));
    }

    #[test]
    fn test_extract_crate_dirs_ignores_non_crate_paths() {
        let files = vec![
            "docs/reference/STABILITY.md".to_string(),
            "xtask/src/main.rs".to_string(),
            ".github/workflows/ci.yml".to_string(),
            "crates/".to_string(), // bare crates dir, no sub-crate
        ];

        let dirs = extract_crate_dirs(&files);
        assert!(dirs.is_empty());
    }
}
