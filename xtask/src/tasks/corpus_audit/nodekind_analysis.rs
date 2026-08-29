//! NodeKind reachability analysis
//!
//! This module analyzes corpus files to determine which NodeKinds are
//! being exercised and identifies gaps in coverage.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Statistics about NodeKind coverage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeKindStats {
    /// Total number of NodeKinds in the parser
    pub total_count: usize,
    /// Number of NodeKinds that were seen in corpus
    pub covered_count: usize,
    /// Coverage percentage
    pub coverage_percentage: f64,
    /// NodeKinds that were never seen
    pub never_seen: Vec<String>,
    /// Never-seen NodeKinds that are intentionally excluded from strict coverage.
    pub allowlisted_never_seen: Vec<AllowlistedNodeKind>,
    /// Never-seen NodeKinds that still need fixture/generator coverage.
    pub actionable_never_seen: Vec<String>,
    /// NodeKinds with low coverage (<5 occurrences)
    pub at_risk: Vec<AtRiskNodeKind>,
    /// Frequency of each NodeKind
    pub frequency: HashMap<String, usize>,
}

/// A never-seen NodeKind that is intentionally allowlisted with rationale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowlistedNodeKind {
    /// NodeKind name
    pub name: String,
    /// Why this NodeKind is intentionally allowlisted.
    pub rationale: String,
}

/// A NodeKind with low coverage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtRiskNodeKind {
    /// NodeKind name
    pub name: String,
    /// Number of occurrences
    pub count: usize,
    /// Risk level
    pub risk_level: RiskLevel,
}

/// Risk level for NodeKind coverage
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Critical - never seen
    Critical,
    /// High - 1-2 occurrences
    High,
    /// Medium - 3-4 occurrences
    Medium,
}

/// Analyze NodeKind coverage from parse results
///
/// This function processes parse results to determine which NodeKinds
/// are being exercised and identifies gaps.
pub fn analyze_nodekind_coverage(
    parse_results: &HashMap<PathBuf, super::timeout_detection::ParseOutcome>,
) -> NodeKindStats {
    let mut nodekind_counts: HashMap<String, usize> = HashMap::new();

    // Collect NodeKind counts from successful parses
    for (path, outcome) in parse_results {
        if let Some(_duration) = outcome.duration_ms() {
            // Parse was successful, extract NodeKinds from content
            // For now, we'll use a simple heuristic based on file content
            // In a real implementation, we would traverse the AST
            let nodekinds = extract_nodekinds_from_content(path);
            for nodekind in nodekinds {
                *nodekind_counts.entry(nodekind).or_insert(0) += 1;
            }
        }
    }

    // Get all NodeKinds from the parser
    let all_nodekinds = get_all_nodekinds();
    let total_count = all_nodekinds.len();
    let covered_count = nodekind_counts.len();
    let coverage_percentage =
        if total_count > 0 { (covered_count as f64 / total_count as f64) * 100.0 } else { 0.0 };

    // Find never-seen NodeKinds
    let never_seen: Vec<String> =
        all_nodekinds.iter().filter(|nk| !nodekind_counts.contains_key(*nk)).cloned().collect();
    let allowlist = recovery_kind_allowlist();
    let mut allowlisted_never_seen = Vec::new();
    let mut actionable_never_seen = Vec::new();
    for name in &never_seen {
        if let Some(rationale) = allowlist.get(name.as_str()) {
            allowlisted_never_seen.push(AllowlistedNodeKind {
                name: name.clone(),
                rationale: (*rationale).to_string(),
            });
        } else {
            actionable_never_seen.push(name.clone());
        }
    }

    // Find at-risk NodeKinds (low coverage)
    let at_risk: Vec<AtRiskNodeKind> = nodekind_counts
        .iter()
        .filter(|(_, count)| **count < 5)
        .map(|(name, count)| {
            let count = *count;
            let risk_level = if count == 0 {
                RiskLevel::Critical
            } else if count <= 2 {
                RiskLevel::High
            } else {
                RiskLevel::Medium
            };

            AtRiskNodeKind { name: name.clone(), count, risk_level }
        })
        .collect();

    NodeKindStats {
        total_count,
        covered_count,
        coverage_percentage,
        never_seen,
        allowlisted_never_seen,
        actionable_never_seen,
        at_risk,
        frequency: nodekind_counts,
    }
}

/// Extract NodeKinds from file content
///
/// This implementation parses the file and traverses the AST to collect
/// all unique NodeKind names.
fn extract_nodekinds_from_content(path: &PathBuf) -> Vec<String> {
    use perl_parser::Parser;
    use std::fs;

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut parser = Parser::new(&content);
    let mut nodekinds = HashSet::new();

    if let Ok(ast) = parser.parse() {
        collect_nodekinds_recursive(&ast, &mut nodekinds);
    } else {
        eprintln!("   Warning: Failed to parse {}", path.display());
    }

    nodekinds.into_iter().collect()
}

fn collect_nodekinds_recursive(node: &perl_parser::ast::Node, out: &mut HashSet<String>) {
    out.insert(node.kind.kind_name().to_string());

    // Traverse children using robust API
    node.for_each_child(|child| {
        collect_nodekinds_recursive(child, out);
    });
}

/// Get all NodeKinds from the parser's canonical list.
fn get_all_nodekinds() -> HashSet<String> {
    perl_parser::ast::NodeKind::ALL_KIND_NAMES.iter().map(|s| (*s).to_string()).collect()
}

fn recovery_kind_allowlist() -> HashMap<&'static str, &'static str> {
    let mut allowlist = HashMap::new();
    for &kind in perl_parser::ast::NodeKind::RECOVERY_KIND_NAMES {
        allowlist.insert(
            kind,
            "Synthetic recovery node emitted by parse_with_recovery() on malformed input, not expected in strict clean-corpus parses.",
        );
    }
    allowlist
}

/// Validate that the omission classification is trustworthy evidence.
///
/// A report may only be treated as zero-actionable when:
/// - `never_seen` is exactly the duplicate-free disjoint union of
///   `actionable_never_seen` and `allowlisted_never_seen` names,
/// - every omission name is a canonical `NodeKind` name from
///   `NodeKind::ALL_KIND_NAMES`, and
/// - every allowlisted entry carries a non-empty rationale.
///
/// Each violation yields one human-readable failure message; an empty result
/// means the omission partition can be trusted.
pub fn omission_partition_failures(stats: &NodeKindStats) -> Vec<String> {
    let mut failures = Vec::new();

    let canonical: HashSet<&str> =
        perl_parser::ast::NodeKind::ALL_KIND_NAMES.iter().copied().collect();
    let recovery_allowlist = recovery_kind_allowlist();

    // Classification bucket contents, with multiplicity for duplicate detection.
    let mut classified_counts: HashMap<&str, usize> = HashMap::new();
    let mut allowlisted: HashSet<&str> = HashSet::new();
    for entry in &stats.allowlisted_never_seen {
        *classified_counts.entry(entry.name.as_str()).or_insert(0) += 1;
        allowlisted.insert(entry.name.as_str());
        if entry.rationale.trim().is_empty() {
            failures.push(format!(
                "Allowlisted never-seen NodeKind '{}' has an empty rationale",
                entry.name
            ));
        }
        if !recovery_allowlist.contains_key(entry.name.as_str()) {
            failures.push(format!(
                "Allowlisted never-seen NodeKind '{}' is not a recovery kind",
                entry.name
            ));
        }
    }
    let mut actionable: HashSet<&str> = HashSet::new();
    for name in &stats.actionable_never_seen {
        *classified_counts.entry(name.as_str()).or_insert(0) += 1;
        actionable.insert(name.as_str());
        if recovery_allowlist.contains_key(name.as_str()) {
            failures.push(format!("Actionable never-seen NodeKind '{}' is a recovery kind", name));
        }
    }

    // The two classification buckets must be disjoint.
    let overlap: Vec<&&str> = allowlisted.intersection(&actionable).collect();
    if !overlap.is_empty() {
        let mut names: Vec<&str> = overlap.into_iter().copied().collect();
        names.sort_unstable();
        failures.push(format!(
            "Omission classification overlap: {} appear in both the actionable and allowlisted buckets",
            names.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ")
        ));
    }

    // No duplicates within the combined classification.
    let mut duplicated: Vec<&str> =
        classified_counts.iter().filter(|(_, count)| **count > 1).map(|(name, _)| *name).collect();
    duplicated.sort_unstable();
    if !duplicated.is_empty() {
        failures.push(format!(
            "Duplicate NodeKind omission entries: {}",
            duplicated.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ")
        ));
    }

    // Partition completeness: never_seen must be exactly the union of the two
    // classification buckets (member for member, no drops, no extras).
    let mut never_seen_counts: HashMap<&str, usize> = HashMap::new();
    for name in &stats.never_seen {
        *never_seen_counts.entry(name.as_str()).or_insert(0) += 1;
    }
    let mut dropped: Vec<&str> = never_seen_counts
        .iter()
        .filter(|(name, count)| classified_counts.get(*name).copied().unwrap_or(0) < **count)
        .map(|(name, _)| *name)
        .collect();
    let mut extra: Vec<&str> = classified_counts
        .iter()
        .filter(|(name, count)| never_seen_counts.get(*name).copied().unwrap_or(0) < **count)
        .map(|(name, _)| *name)
        .collect();
    if !dropped.is_empty() || !extra.is_empty() {
        dropped.sort_unstable();
        extra.sort_unstable();
        failures.push(format!(
            "Omission partition mismatch: never_seen must equal actionable + allowlisted; dropped from classification: [{}]; classified but not never-seen: [{}]",
            dropped.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", "),
            extra.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ")
        ));
    }

    // Canonicality: every omission name must exist in the parser's kind list.
    let mut non_canonical: Vec<&str> = never_seen_counts
        .keys()
        .copied()
        .chain(classified_counts.keys().copied())
        .filter(|name| !canonical.contains(name))
        .collect();
    non_canonical.sort_unstable();
    non_canonical.dedup();
    if !non_canonical.is_empty() {
        failures.push(format!(
            "Non-canonical NodeKind omission names (absent from NodeKind::ALL_KIND_NAMES): {}",
            non_canonical.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ")
        ));
    }

    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_ord() {
        assert!(RiskLevel::Critical < RiskLevel::High, "Critical must be less than High");
        assert!(RiskLevel::High < RiskLevel::Medium, "High must be less than Medium");
    }

    #[test]
    fn test_get_all_nodekinds() {
        let nodekinds = get_all_nodekinds();
        assert!(nodekinds.len() > 50, "should have more than 50 NodeKinds");
        assert!(nodekinds.contains("ExpressionStatement"), "should contain ExpressionStatement");
        assert!(nodekinds.contains("Binary"), "should contain Binary");
        assert!(nodekinds.contains("Subroutine"), "should contain Subroutine");
    }

    #[test]
    fn test_extract_nodekinds_from_content() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new()?;
        writeln!(tmp, "my $x = 1;\nprint $x;\nsub foo {{ return 42; }}")?;
        let path = PathBuf::from(tmp.path());
        let nodekinds = extract_nodekinds_from_content(&path);
        assert!(!nodekinds.is_empty(), "should extract at least one NodeKind");
        Ok(())
    }

    #[test]
    fn test_nodekind_stats_requires_explicit_omission_classification() {
        let missing_actionable = r#"{
            "total_count": 76,
            "covered_count": 71,
            "coverage_percentage": 93.4,
            "never_seen": ["KeyValueSlice"],
            "allowlisted_never_seen": [],
            "at_risk": [],
            "frequency": {}
        }"#;
        assert!(
            serde_json::from_str::<NodeKindStats>(missing_actionable).is_err(),
            "a report without actionable_never_seen must not deserialize as zero actionable gaps"
        );

        let missing_allowlisted = r#"{
            "total_count": 76,
            "covered_count": 71,
            "coverage_percentage": 93.4,
            "never_seen": ["MissingBlock"],
            "actionable_never_seen": [],
            "at_risk": [],
            "frequency": {}
        }"#;
        assert!(
            serde_json::from_str::<NodeKindStats>(missing_allowlisted).is_err(),
            "a report without allowlisted_never_seen must not deserialize as a complete omission partition"
        );
    }

    /// Deserialize a presence-valid NodeKindStats JSON fixture.
    fn omission_fixture(json: &str) -> NodeKindStats {
        serde_json::from_str(json).expect("fixture must deserialize (presence is not the question)")
    }

    #[test]
    fn test_partition_validator_rejects_presence_valid_inconsistent_reports() {
        let base = r#""total_count": 76, "covered_count": 73, "coverage_percentage": 96.1,
            "at_risk": [], "frequency": {}"#;

        // Overlap: 'AmperCall' in both classification buckets.
        let overlap = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["AmperCall"],
                "allowlisted_never_seen": [{{"name": "AmperCall", "rationale": "x"}}],
                "actionable_never_seen": ["AmperCall"] }}"#
        ));
        let failures = omission_partition_failures(&overlap);
        assert!(
            failures.iter().any(|s| s.contains("Omission classification overlap")),
            "overlap must be rejected: {failures:?}"
        );

        // Dropped member: never_seen has two entries, only one is classified.
        let dropped = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["AmperCall", "VString"],
                "allowlisted_never_seen": [], "actionable_never_seen": ["AmperCall"] }}"#
        ));
        let failures = omission_partition_failures(&dropped);
        assert!(
            failures.iter().any(|s| s.contains("Omission partition mismatch")),
            "dropped member must be rejected: {failures:?}"
        );

        // Non-canonical name: typo absent from NodeKind::ALL_KIND_NAMES.
        let non_canonical = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["AmperCal!"],
                "allowlisted_never_seen": [], "actionable_never_seen": ["AmperCal!"] }}"#
        ));
        let failures = omission_partition_failures(&non_canonical);
        assert!(
            failures.iter().any(|s| s.contains("Non-canonical NodeKind omission names")),
            "non-canonical name must be rejected: {failures:?}"
        );

        // Duplicate within a bucket.
        let duplicate = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["AmperCall", "AmperCall"],
                "allowlisted_never_seen": [], "actionable_never_seen": ["AmperCall", "AmperCall"] }}"#
        ));
        let failures = omission_partition_failures(&duplicate);
        assert!(
            failures.iter().any(|s| s.contains("Duplicate NodeKind omission entries")),
            "duplicate must be rejected: {failures:?}"
        );

        // Empty rationale on an allowlisted entry.
        let empty_rationale = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["MissingBlock"],
                "allowlisted_never_seen": [{{"name": "MissingBlock", "rationale": "  "}}],
                "actionable_never_seen": [] }}"#
        ));
        let failures = omission_partition_failures(&empty_rationale);
        assert!(
            failures.iter().any(|s| s.contains("empty rationale")),
            "empty rationale must be rejected: {failures:?}"
        );

        // A canonical but non-recovery kind cannot be placed in the recovery-only
        // bucket merely by supplying a rationale.
        let non_recovery_allowlist = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["AmperCall"],
                "allowlisted_never_seen": [{{"name": "AmperCall", "rationale": "incorrect recovery"}}],
                "actionable_never_seen": [] }}"#
        ));
        let failures = omission_partition_failures(&non_recovery_allowlist);
        assert!(
            failures.iter().any(|s| s.contains("is not a recovery kind")),
            "non-recovery allowlist entry must be rejected: {failures:?}"
        );

        // Valid recovery-only control: never_seen is exactly the allowlisted
        // union, actionable is empty, rationales present → no failures.
        let valid = omission_fixture(&format!(
            r#"{{ {base}, "never_seen": ["Error", "MissingBlock"],
                "allowlisted_never_seen": [
                    {{"name": "Error", "rationale": "recovery kind"}},
                    {{"name": "MissingBlock", "rationale": "recovery kind"}}],
                "actionable_never_seen": [] }}"#
        ));
        let failures = omission_partition_failures(&valid);
        assert!(failures.is_empty(), "recovery-only control must validate: {failures:?}");
    }

    #[test]
    fn test_recovery_allowlist_contains_known_kinds() {
        let allowlist = recovery_kind_allowlist();
        // All 6 members of RECOVERY_KIND_NAMES must appear in the allowlist.
        assert!(allowlist.contains_key("Error"), "allowlist should contain Error");
        assert!(allowlist.contains_key("MissingBlock"), "allowlist should contain MissingBlock");
        assert!(
            allowlist.contains_key("MissingExpression"),
            "allowlist should contain MissingExpression"
        );
        assert!(
            allowlist.contains_key("MissingIdentifier"),
            "allowlist should contain MissingIdentifier"
        );
        assert!(
            allowlist.contains_key("MissingStatement"),
            "allowlist should contain MissingStatement"
        );
        assert!(allowlist.contains_key("UnknownRest"), "allowlist should contain UnknownRest");
        // The allowlist must have exactly as many entries as RECOVERY_KIND_NAMES (no extras, no dups).
        assert_eq!(
            allowlist.len(),
            perl_parser::ast::NodeKind::RECOVERY_KIND_NAMES.len(),
            "allowlist size must equal RECOVERY_KIND_NAMES.len()"
        );
    }

    #[test]
    fn test_allowlist_classification_partition() {
        // Verify that allowlisted_never_seen and actionable_never_seen form an
        // exact partition of never_seen: no element is in both, all are in one.
        let allowlist = recovery_kind_allowlist();
        let all_nodekinds = get_all_nodekinds();
        // Simulate a corpus where nothing was seen.
        let never_seen: Vec<String> = {
            let mut v: Vec<String> = all_nodekinds.iter().cloned().collect();
            v.sort();
            v
        };
        let mut allowlisted: Vec<String> = Vec::new();
        let mut actionable: Vec<String> = Vec::new();
        for name in &never_seen {
            if allowlist.contains_key(name.as_str()) {
                allowlisted.push(name.clone());
            } else {
                actionable.push(name.clone());
            }
        }
        assert_eq!(
            allowlisted.len() + actionable.len(),
            never_seen.len(),
            "allowlisted + actionable must equal never_seen (partition invariant)"
        );
        // No overlap between the two sub-lists.
        let allowlisted_set: HashSet<&str> = allowlisted.iter().map(|s| s.as_str()).collect();
        let actionable_set: HashSet<&str> = actionable.iter().map(String::as_str).collect();
        let overlap: Vec<&&str> = allowlisted_set.intersection(&actionable_set).collect();
        assert!(
            overlap.is_empty(),
            "allowlisted and actionable must be disjoint; overlap: {:?}",
            overlap
        );
        // Recovery kinds must all land in the allowlisted bucket.
        for &kind in perl_parser::ast::NodeKind::RECOVERY_KIND_NAMES {
            assert!(
                allowlisted_set.contains(kind),
                "recovery kind '{kind}' should be in allowlisted bucket"
            );
        }
    }

    #[test]
    fn test_analyze_nodekind_coverage_classifies_allowlisted_vs_actionable_never_seen() {
        let parse_results: HashMap<PathBuf, super::super::timeout_detection::ParseOutcome> =
            HashMap::new();

        let stats = analyze_nodekind_coverage(&parse_results);
        assert_eq!(stats.covered_count, 0, "empty corpus should cover zero NodeKinds");
        assert_eq!(
            stats.never_seen.len(),
            stats.total_count,
            "empty corpus should mark all NodeKinds as never seen"
        );

        let allowlisted_names: HashSet<&str> =
            stats.allowlisted_never_seen.iter().map(|entry| entry.name.as_str()).collect();
        let actionable_names: HashSet<&str> =
            stats.actionable_never_seen.iter().map(String::as_str).collect();

        for &kind in perl_parser::ast::NodeKind::RECOVERY_KIND_NAMES {
            assert!(allowlisted_names.contains(kind), "recovery kind '{kind}' must be allowlisted");
            assert!(
                !actionable_names.contains(kind),
                "recovery kind '{kind}' must not be actionable"
            );
        }
    }

    #[test]
    fn test_analyze_nodekind_coverage_marks_low_frequency_as_at_risk()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;

        let mut tmp = tempfile::NamedTempFile::new()?;
        writeln!(tmp, "my $x = 1;")?;

        let mut parse_results: HashMap<PathBuf, super::super::timeout_detection::ParseOutcome> =
            HashMap::new();
        parse_results.insert(
            PathBuf::from(tmp.path()),
            super::super::timeout_detection::ParseOutcome::Ok { duration_ms: 1 },
        );

        let stats = analyze_nodekind_coverage(&parse_results);
        assert!(
            !stats.at_risk.is_empty(),
            "single-file corpus should have at-risk NodeKinds (<5 occurrences)"
        );
        assert!(
            stats.at_risk.iter().any(|entry| entry.risk_level == RiskLevel::High),
            "single occurrence NodeKinds should be marked as High risk"
        );
        Ok(())
    }
}
