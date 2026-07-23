//! Validate known stale publication claims inside docs/articles.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::{fs, path::PathBuf};

const ARTICLES_DIR: &str = "docs/articles";

const STALE_PATTERNS: &[(&str, &str, &str)] = &[
    ("563,228 lines", "591,034 lines", "LOC claim (563K is stale; ledger: 591,034)"),
    ("563K lines", "591K lines", "LOC claim (563K is stale; ledger: 591K)"),
    ("546,000", "591,034", "LOC claim (546K is stale; ledger: 591,034)"),
    ("546K lines", "591K lines", "LOC claim (546K is stale; ledger: 591K)"),
    ("131 crates", "133 crates", "Crate count (131 is stale; ledger: 133)"),
    ("131 workspace crates", "133 workspace crates", "Crate count (131 is stale; ledger: 133)"),
    ("132 workspace crates", "133 workspace crates", "Crate count (132 is stale; ledger: 133)"),
    ("132 crates", "133 crates", "Crate count (132 is stale; ledger: 133)"),
    (
        "97 LSP and DAP features",
        "98 LSP and DAP features",
        "Feature count (97 is stale; ledger: 98)",
    ),
    ("97 LSP/DAP features", "98 LSP/DAP features", "Feature count (97 is stale; ledger: 98)"),
    ("97 features defined", "98 features defined", "Feature count (97 is stale; ledger: 98)"),
    ("97 features governed", "98 features governed", "Feature count (97 is stale; ledger: 98)"),
    ("97 features:", "98 features:", "Feature count (97 is stale; ledger: 98)"),
    ("2,700+ commits", "3,200+ commits", "Commit count (2,700+ is stale; ledger: 3,210)"),
    ("2,200+ pull requests", "2,646+ pull requests", "PR count (2,200+ is stale; ledger: 2,646+)"),
    ("2,200+ PRs", "2,646+ PRs", "PR count (2,200+ is stale; ledger: 2,646+)"),
];

type ClaimHit = (PathBuf, usize, &'static str, &'static str, &'static str);
const FORBIDDEN_CRATE_NAME: &str = "`perl-workspace-index`";
const CRATE_NAME_GUARD_FILES: &[&str] = &[
    "README.md",
    "crates/perl-workspace/README.md",
    "crates/perl-workspace/src/api.rs",
    "docs/project/status/workspace.md",
];
const CRATE_NAME_EXCEPTIONS: &[&str] = &["docs/MIGRATION_v0.13.md"];

pub fn run() -> Result<()> {
    let root = project_root()?;
    let articles_dir = root.join(ARTICLES_DIR);
    let mut files = Vec::new();

    if !articles_dir.is_dir() {
        bail!("expected articles directory not found at {}", articles_dir.display());
    }

    for entry in fs::read_dir(&articles_dir).context("failed to read docs/articles directory")? {
        let entry = entry.context("failed to read directory entry")?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();

    let mut hits: Vec<ClaimHit> = Vec::new();
    for md_file in &files {
        let text = fs::read_to_string(md_file)
            .with_context(|| format!("failed to read article file {}", md_file.display()))?;
        for (line_no, line) in text.lines().enumerate() {
            for &(stale, replacement, description) in STALE_PATTERNS {
                if line.contains(stale) {
                    hits.push((md_file.clone(), line_no + 1, stale, replacement, description));
                }
            }
        }
    }

    if hits.is_empty() {
        check_forbidden_workspace_crate_name(&root)?;
        // #4649: this validator only checks a fixed list of hardcoded stale
        // literals. It cannot detect new staleness patterns (e.g. a crate count
        // drifting past the last hand-edited value); it only catches
        // regressions of the literals listed below. State that scope explicitly
        // so "0 violations" is not mistaken for a clean bill of health.
        println!("{}", success_message(files.len()));
        eprintln!(
            "doc-claims scope (#4649): checked {} hardcoded stale literals: {}",
            STALE_PATTERNS.len(),
            STALE_PATTERNS.iter().map(|(stale, _, _)| *stale).collect::<Vec<_>>().join(", ")
        );
        return Ok(());
    }

    eprintln!("DOC CLAIM VIOLATIONS:");
    eprintln!("{}", "=".repeat(60));
    for (file, line_no, stale, replacement, description) in &hits {
        let rel = file.strip_prefix(&root).unwrap_or(file.as_path());
        eprintln!("  {}:{}: {}", rel.display(), line_no, description);
        eprintln!("    found:    {:?}", stale);
        eprintln!("    expected: {:?}", replacement);
    }
    eprintln!("{}", "=".repeat(60));
    eprintln!("{} stale claim(s) found in docs/articles.", hits.len());
    eprintln!("\nTo fix: update the article to match docs/project/PUBLICATION_FACTS_LEDGER.md");
    bail!("doc claim check failed");
}

fn check_forbidden_workspace_crate_name(root: &std::path::Path) -> Result<()> {
    for rel in CRATE_NAME_GUARD_FILES {
        if CRATE_NAME_EXCEPTIONS.contains(rel) {
            continue;
        }
        let path = root.join(rel);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read guard file {}", path.display()))?;
        if text.contains(FORBIDDEN_CRATE_NAME) || text.contains(" -p perl-workspace-index ") {
            bail!("forbidden stale crate name '{}' found in {}", FORBIDDEN_CRATE_NAME, rel);
        }
    }
    Ok(())
}

/// Build the success message reported when no stale-literal regressions are
/// found. Extracted so the #4649 scope caveat ("only N hardcoded literals are
/// checked; new staleness patterns are NOT caught") can be unit-tested.
fn success_message(files_count: usize) -> String {
    format!(
        "Doc claims OK: {files_count} articles scanned, {n} hardcoded stale literals checked, \
         0 regressions found. Scope: only the {n} hardcoded literals below are checked; \
         new staleness patterns are NOT caught.",
        n = STALE_PATTERNS.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_message_states_hardcoded_literal_scope() {
        let msg = success_message(7);
        // #4649 acceptance: the OK message must explicitly state that only
        // hardcoded literals are checked and new staleness is NOT caught.
        assert!(msg.contains("hardcoded stale literals checked"), "msg: {msg}");
        assert!(msg.contains("new staleness patterns are NOT caught"), "msg: {msg}");
        assert!(msg.contains("0 regressions found"), "msg: {msg}");
        assert!(msg.contains("7 articles scanned"), "msg: {msg}");
    }

    #[test]
    fn stale_patterns_table_is_non_empty() {
        // A non-empty table is what makes the scope count meaningful; if it
        // ever empties the message would be misleading.
        assert!(!STALE_PATTERNS.is_empty());
        for (stale, replacement, _desc) in STALE_PATTERNS {
            assert!(!stale.is_empty(), "stale literal must not be empty");
            assert!(!replacement.is_empty(), "replacement literal must not be empty");
            assert_ne!(stale, replacement, "stale and replacement must differ");
        }
    }
}
