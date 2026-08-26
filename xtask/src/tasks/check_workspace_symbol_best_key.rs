//! Architecture recurrence gate for per-row best-key workspace-symbol
//! matching (#10645).
//!
//! Fails when the canonical source-backed row-search seam regresses to any of
//! the provisional shortcuts #10645 removed:
//!
//! - geometry-based `(uri, start_byte)` deduplication before rows aggregate;
//! - a local numeric tier table reimplementing query admission;
//!
//! and fails when the seam no longer consumes the #10794/#10645 authorities
//! (`match_searchable_key`, `BestRowMatchAccumulator`, typed evidence).

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result};

/// Banned substrings: restored geometry/first-key shortcuts.
const BANNED_PATTERNS: &[(&str, &str)] = &[
    (
        "HashSet<(String, usize)>",
        "geometry `(uri, start)` dedup set — aggregation must group by whole-payload row identity (#10645)",
    ),
    (
        "(sym.uri.clone(), sym.range.start.byte)",
        "first-key-wins geometry dedup key — every admitted key of a row must be evaluated before materialization (#10645)",
    ),
    (
        "WorkspaceSymbolMatchTier::Exact =>",
        "local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "WorkspaceSymbolMatchTier::Prefix =>",
        "local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "WorkspaceSymbolMatchTier::Substring =>",
        "local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "WorkspaceSymbolMatchTier::Subsequence =>",
        "local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "Tier::Exact =>",
        "aliased local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "Tier::Prefix =>",
        "aliased local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "Tier::Substring =>",
        "aliased local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
    (
        "Tier::Subsequence =>",
        "aliased local tier-table arm — tier scoring must stay owned by the pinned `legacy_index_match_rank` projection (#10645 M12)",
    ),
];

/// Required substrings: the seam must keep consuming the shared authorities.
const REQUIRED_INDEX_ANCHORS: &[(&str, &str)] = &[
    ("match_searchable_key(", "admission must stay owned by the compiled query profile (#10794)"),
    (
        "BestRowMatchAccumulator::for_profile",
        "per-row accumulation must consume the shared accumulator (#10645)",
    ),
    (
        "select_best_row_match(",
        "generated/framework projection keys must compete through one best-key selection (#10645)",
    ),
    (
        "legacy_index_match_rank(",
        "the only permitted tier projection is the pinned legacy rank helper",
    ),
];

const REQUIRED_QUERY_ANCHORS: &[(&str, &str)] = &[
    (
        "pub fn select_best_row_match(",
        "the transport-neutral per-row selector must stay public for consumers (#10642)",
    ),
    (
        "evidence.compare(current)",
        "comparison must delegate to the shared evidence comparator, never a local tier table",
    ),
    (
        "RefusedProfileMismatch",
        "foreign-profile evidence must be refused as a typed outcome (#10794/#10645)",
    ),
];

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join("crates").join("perl-workspace").is_dir())
        .map(Path::to_path_buf)
}

/// One lint rule: a substring plus its actionable reason.
type PatternRule = (&'static str, &'static str);

/// A static list of [`PatternRule`]s.
type PatternList = &'static [PatternRule];

fn collect_violations(content: &str, banned: PatternList) -> Vec<String> {
    banned
        .iter()
        .filter(|(pattern, _)| content.contains(pattern))
        .map(|(pattern, reason)| format!("banned pattern `{pattern}`: {reason}"))
        .collect()
}

fn collect_missing(content: &str, required: PatternList) -> Vec<String> {
    required
        .iter()
        .filter(|(anchor, _)| !content.contains(anchor))
        .map(|(anchor, reason)| format!("missing anchor `{anchor}`: {reason}"))
        .collect()
}

fn check_file(
    path: &Path,
    label: &str,
    banned: PatternList,
    required: PatternList,
) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read {label} at {}", path.display()))?;
    let mut violations = collect_violations(&content, banned);
    violations.extend(collect_missing(&content, required));
    Ok(violations)
}

/// Runs the recurrence gate against the current working tree.
pub fn run() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = find_workspace_root(&manifest_dir).ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "could not locate the perl-lsp workspace above {}",
            manifest_dir.display()
        )
    })?;

    let index_path = root
        .join("crates")
        .join("perl-workspace")
        .join("src")
        .join("workspace")
        .join("workspace_index.rs");
    let query_path =
        root.join("crates").join("perl-workspace").join("src").join("workspace_symbol_query.rs");

    let mut violations =
        check_file(&index_path, "workspace_index.rs", BANNED_PATTERNS, REQUIRED_INDEX_ANCHORS)?;
    violations.extend(check_file(
        &query_path,
        "workspace_symbol_query.rs",
        &[],
        REQUIRED_QUERY_ANCHORS,
    )?);

    if violations.is_empty() {
        eprintln!("workspace-symbol best-key architecture gate: OK (#10645)");
        return Ok(());
    }

    eprintln!("workspace-symbol best-key architecture gate: FAILED (#10645)");
    for violation in &violations {
        eprintln!("  - {violation}");
    }
    color_eyre::eyre::bail!(
        "{} workspace-symbol best-key architecture violation(s); \
         restore per-row best-key aggregation before merging (#10645)",
        violations.len()
    )
}

#[cfg(test)]
mod tests {
    use super::{collect_missing, collect_violations};

    #[test]
    fn detects_restored_geometry_dedup_set() {
        let source = "let mut seen: HashSet<(String, usize)> = HashSet::new();";
        let violations = collect_violations(source, super::BANNED_PATTERNS);
        assert_eq!(violations.len(), 1, "{violations:?}");
    }

    /// The claimed local-tier ban must actually fire: every qualified or
    /// aliased tier match-arm in the seam is a tier-table recurrence (#10645
    /// M12), even when the pinned anchors remain incidentally present.
    #[test]
    fn detects_local_tier_table_arms() {
        for arm in [
            "WorkspaceSymbolMatchTier::Exact => 3",
            "WorkspaceSymbolMatchTier::Prefix => 2",
            "WorkspaceSymbolMatchTier::Substring => 2",
            "WorkspaceSymbolMatchTier::Subsequence => 1",
            "Tier::Exact => 3u8",
            "Tier::Subsequence => 1u8",
        ] {
            // Qualified spellings also contain their alias suffix, so a
            // qualified arm may fire two bans; at least one must always fire.
            let violations = collect_violations(arm, super::BANNED_PATTERNS);
            assert!(!violations.is_empty(), "{arm}: {violations:?}");
        }
        // A mutation keeping the required anchors while scoring locally is
        // still rejected: anchors alone cannot silence the ban.
        let mutated = "fn score(t: WorkspaceSymbolMatchTier) -> u8 { \
             match t { Tier::Exact => 3, _ => legacy_index_match_rank(t) } \
         } evidence.compare(current);";
        assert!(
            !collect_violations(mutated, super::BANNED_PATTERNS).is_empty(),
            "local tier table must fail the gate even with anchors present"
        );
        // Non-arm comparisons and assertions stay legal.
        assert!(
            collect_violations(
                "assert_eq!(evidence.tier(), WorkspaceSymbolMatchTier::Exact);",
                super::BANNED_PATTERNS
            )
            .is_empty()
        );
    }

    #[test]
    fn detects_removed_shared_authorities() {
        let missing = collect_missing("fn other() {}", super::REQUIRED_INDEX_ANCHORS);
        assert_eq!(missing.len(), super::REQUIRED_INDEX_ANCHORS.len());
        assert!(
            collect_missing(
                "match_searchable_key(p, k, r); BestRowMatchAccumulator::for_profile(p); \
             select_best_row_match(p, ks); legacy_index_match_rank(t);",
                super::REQUIRED_INDEX_ANCHORS
            )
            .is_empty()
        );
    }

    #[test]
    fn clean_seam_has_no_violations() {
        assert!(collect_violations("", super::BANNED_PATTERNS).is_empty());
    }
}
