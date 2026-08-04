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

/// The release runbook and its published mirror. Every step in them is a
/// template an operator copies verbatim into a tag, a Homebrew formula, release
/// notes, or social copy, so a literal in either file becomes a literal in a
/// shipped artifact.
const RELEASE_RUNBOOK_FILES: &[&str] =
    &["docs/project/GA_RUNBOOK.md", "book/src/resources/ga-runbook.md"];

/// Literals the runbook must not contain, and why.
///
/// Three different hardcoded versions and a push to the wrong default branch
/// coexisted here, so following the runbook end to end tagged one release,
/// published a formula for a second, and bumped the extension to a third
/// (#5464). The coverage headlines were templated unconditionally, which
/// republished unverified numbers every release.
const RELEASE_RUNBOOK_FORBIDDEN: &[(&str, &str)] = &[
    ("git push origin master", "the default branch is `main`; use `git push origin main`"),
    ("100% Edge Case Coverage", "quote a verified figure from docs/project/CURRENT_STATUS.md"),
    ("141 edge cases", "quote a verified figure from docs/project/CURRENT_STATUS.md"),
];

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
        check_release_runbook_is_parameterised(&root)?;
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

/// Versions the runbook used to hardcode. Scope is deliberately the same as
/// `STALE_PATTERNS`: named literals, not a general version regex. A regex would
/// have to accept the factual `v0.17.0` in the header note and the asset names
/// in the step-5 evidence block, which are statements about the tree rather
/// than templates an operator copies.
const RELEASE_RUNBOOK_FORBIDDEN_VERSIONS: &[&str] = &["0.8.3", "0.13.1", "0.6.0"];

/// The runbook must keep defining its version once and deriving the tag from
/// it. Without this, the forbidden-literal list above could be satisfied by
/// deleting the parameterization rather than keeping it.
const RELEASE_RUNBOOK_REQUIRED: &[(&str, &str)] = &[
    ("VERSION=", "the runbook must set `VERSION` once in step 0"),
    ("TAG=\"v$VERSION\"", "the tag must derive from `$VERSION`, not be typed again"),
];

fn check_release_runbook_is_parameterised(root: &std::path::Path) -> Result<()> {
    for rel in RELEASE_RUNBOOK_FILES {
        let path = root.join(rel);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read release runbook {}", path.display()))?;

        for &(forbidden, why) in RELEASE_RUNBOOK_FORBIDDEN {
            if text.contains(forbidden) {
                bail!("{rel}: contains {forbidden:?} — {why} (#5464)");
            }
        }
        for &version in RELEASE_RUNBOOK_FORBIDDEN_VERSIONS {
            if text.contains(version) {
                bail!(
                    "{rel}: contains the hardcoded version {version:?}. Every step reads \
                     `$VERSION`, set once in step 0 — a literal here ships in a tag, a formula, \
                     or release notes for a different release than the one being cut (#5464)"
                );
            }
        }
        for &(required, why) in RELEASE_RUNBOOK_REQUIRED {
            if !text.contains(required) {
                bail!("{rel}: missing {required:?} — {why} (#5464)");
            }
        }
    }
    Ok(())
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
    fn release_runbook_guard_passes_on_the_current_tree() -> Result<()> {
        // The guard is only worth having if it is live against the real files;
        // a table checked against nothing would pass forever.
        check_release_runbook_is_parameterised(&project_root()?)
    }

    #[test]
    fn release_runbook_guard_covers_both_the_source_and_its_published_mirror() {
        // `scripts/populate-book.sh` copies the runbook into the book. Guarding
        // only the source would let the copy users actually read go stale —
        // which is exactly how the broken Windows command reached the published
        // book (#5461).
        assert!(RELEASE_RUNBOOK_FILES.contains(&"docs/project/GA_RUNBOOK.md"));
        assert!(RELEASE_RUNBOOK_FILES.contains(&"book/src/resources/ga-runbook.md"));
    }

    #[test]
    fn release_runbook_guard_names_every_defect_the_issue_found() {
        // #5464 found four: three hardcoded versions, a push to `master`, and
        // unconditional coverage headlines. Dropping any entry silently narrows
        // the guard to less than the issue it closes.
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN
                .iter()
                .any(|(literal, _)| *literal == "git push origin master")
        );
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN.iter().any(|(literal, _)| literal.contains("Edge Case")),
            "the templated coverage headline must stay forbidden"
        );
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN.iter().any(|(literal, _)| *literal == "141 edge cases"),
            "the templated coverage figure must stay forbidden"
        );
        assert_eq!(
            RELEASE_RUNBOOK_FORBIDDEN_VERSIONS,
            ["0.8.3", "0.13.1", "0.6.0"],
            "the tag, formula, and extension versions that disagreed"
        );
        assert!(!RELEASE_RUNBOOK_REQUIRED.is_empty(), "the parameterization must stay asserted");
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
