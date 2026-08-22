//! Integration test: Verify the published crate count matches
//! `xtask/published-crate-baseline.txt` (the ratchet record).
//!
//! Wave G3 absorbs 7 crates: governance, protocol, uri, transport, performance,
//! critic-parser, tooling. Reduces published count from 44 → 37.
//! Config and content-length-framing remain published per D3/D4.
//! Wave 4-Completion absorbs 3 parser satellites: dead-code, refactoring, incremental-parsing. 37 → 34.
//! Wave Final PR B absorbs 3 remaining infra crates: feature-catalog, lsp-config, content-length-framing. 34 → 31.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("..").join("..")
}

/// Extract the crate names in the root `[workspace.metadata.publish.allow]` array —
/// the live published set, which the baseline file must match. Derived, not hard-coded.
///
/// The allowlist is densely commented: most entries sit beside a `#` note recording
/// where an absorbed crate went. Those comments are not entries, and one of them
/// quotes an ADR section name (`PLSP-ADR-0006 "Scope boundary"`), so counting quotes
/// across the whole block over-reports by one per quoted phrase. Strip each line's
/// comment before reading its entry, and return the names so failures show which
/// rows drifted instead of only a diverging count.
fn published_allowlist_entries(root: &std::path::Path) -> std::io::Result<Vec<String>> {
    let root_toml = fs::read_to_string(root.join("Cargo.toml"))?;
    let section = root_toml.split("[workspace.metadata.publish]").nth(1).unwrap_or("");
    let allow_start = section.find("allow = [").unwrap_or(0);
    let allow = &section[allow_start..];
    let allow_end = allow.find(']').unwrap_or(allow.len());
    Ok(allow[..allow_end]
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|code| code.starts_with('"'))
        .map(|code| code.trim_end_matches(',').trim().trim_matches('"').to_string())
        .collect())
}

#[test]
fn g3_published_count_matches_allowlist() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();

    // Read xtask/published-crate-baseline.txt (single source of truth for the count)
    let baseline_path = root.join("xtask/published-crate-baseline.txt");
    assert!(
        baseline_path.exists(),
        "baseline file should exist at xtask/published-crate-baseline.txt"
    );

    let content = fs::read_to_string(&baseline_path)?;
    let baseline: usize =
        content.trim().parse().map_err(|_| "failed to parse baseline count as an integer")?;
    let entries = published_allowlist_entries(&root)?;
    let allowlist = entries.len();

    assert_eq!(
        baseline, allowlist,
        "baseline ({baseline}) must match the publish allowlist entry count ({allowlist}) — \
         parsed allowlist entries: {entries:?}"
    );

    Ok(())
}

#[test]
fn g3_absorbed_crates_directories_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();

    // Wave G3 implementation choice: absorbed crate directories are DELETED, not kept with publish=false.
    // This diverges from G2 but matches builder's implementation of full absorption cleanup.
    // Regression guard: verify directories are absent (not left behind as stubs).
    let absorbed = vec![
        "crates/perl-lsp-feature-governance",
        "crates/perl-lsp-protocol",
        "crates/perl-lsp-uri",
        "crates/perl-lsp-transport",
        "crates/perl-lsp-performance",
        "crates/perl-lsp-critic-parser",
        "crates/perl-lsp-tooling",
        // Wave Final PR B absorptions
        "crates/perl-feature-catalog",
        "crates/perl-lsp-config",
        "crates/perl-content-length-framing",
    ];

    for crate_dir in absorbed {
        let dir_path = root.join(crate_dir);
        assert!(!dir_path.exists(), "absorbed crate directory should be deleted: {crate_dir}");
    }

    Ok(())
}
