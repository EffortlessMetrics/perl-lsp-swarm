//! Bounded, snapshot-bound context sections for invoked AI inline completion.
//!
//! This module owns the deterministic budget, truncation metadata, and source
//! identity that extend [`super::PreparedInlineCompletionContext`] with
//! right-of-cursor (suffix) context (#10273). The context stays
//! provider-neutral: it contains no prompt messages, FIM token strings,
//! endpoint/model identity, or consent state.
//!
//! Deterministic automatic completion keeps its existing behavior; the bounded
//! sections are computed from the same single-document snapshot the provider
//! already splits into lines, with compiled hard maxima that no profile can
//! exceed.

use serde::{Deserialize, Serialize};

/// Compiled hard maxima for every bounded section of the prepared context.
///
/// These values are the upper bound for any future user-owned profile budget:
/// profile values may lower a limit, never raise it above these constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineCompletionContextBudget {
    /// Maximum retained bytes of the current-line prefix.
    pub prefix_bytes: usize,
    /// Maximum retained bytes of the current-line suffix.
    pub suffix_bytes: usize,
    /// Maximum number of retained preceding lines.
    pub preceding_lines: usize,
    /// Maximum retained bytes across all preceding lines.
    pub preceding_bytes: usize,
    /// Maximum number of retained following lines.
    pub following_lines: usize,
    /// Maximum retained bytes across all following lines.
    pub following_bytes: usize,
    /// Maximum retained semantic facts (variables or imports) per section.
    pub semantic_fact_count: usize,
    /// Maximum retained bytes per semantic-fact section.
    pub semantic_fact_bytes: usize,
    /// Maximum total retained bytes across every bounded section.
    pub total_context_bytes: usize,
}

impl InlineCompletionContextBudget {
    /// The compiled maxima; also the deterministic default budget.
    pub const COMPILED_MAXIMA: Self = Self {
        prefix_bytes: 4096,
        suffix_bytes: 2048,
        preceding_lines: 16,
        preceding_bytes: 2048,
        following_lines: 16,
        following_bytes: 2048,
        semantic_fact_count: 32,
        semantic_fact_bytes: 1024,
        total_context_bytes: 8192,
    };

    /// The default budget: identical to the compiled maxima.
    pub fn compiled_maxima() -> Self {
        Self::COMPILED_MAXIMA
    }

    /// Build a budget from profile-supplied limits.
    ///
    /// Every `Some(limit)` field may only lower the corresponding compiled
    /// maximum; `None` keeps the maximum. A value above the compiled maximum
    /// saturates at the maximum, so no profile can exceed the hard ceiling.
    pub fn saturating_from_profile(limits: ContextBudgetLimits) -> Self {
        let maxima = Self::COMPILED_MAXIMA;
        let lower = |limit: Option<usize>, hard: usize| limit.map_or(hard, |v| v.min(hard));
        Self {
            prefix_bytes: lower(limits.prefix_bytes, maxima.prefix_bytes),
            suffix_bytes: lower(limits.suffix_bytes, maxima.suffix_bytes),
            preceding_lines: lower(limits.preceding_lines, maxima.preceding_lines),
            preceding_bytes: lower(limits.preceding_bytes, maxima.preceding_bytes),
            following_lines: lower(limits.following_lines, maxima.following_lines),
            following_bytes: lower(limits.following_bytes, maxima.following_bytes),
            semantic_fact_count: lower(limits.semantic_fact_count, maxima.semantic_fact_count),
            semantic_fact_bytes: lower(limits.semantic_fact_bytes, maxima.semantic_fact_bytes),
            total_context_bytes: lower(limits.total_context_bytes, maxima.total_context_bytes),
        }
    }
}

/// Profile-supplied budget limits.
///
/// `None` keeps the compiled maximum for that section. This is the seam a
/// future immutable user-owned profile (#10252) will fill in; it can only
/// lower limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextBudgetLimits {
    /// Optional lower prefix byte limit.
    pub prefix_bytes: Option<usize>,
    /// Optional lower suffix byte limit.
    pub suffix_bytes: Option<usize>,
    /// Optional lower preceding-line count limit.
    pub preceding_lines: Option<usize>,
    /// Optional lower preceding byte limit.
    pub preceding_bytes: Option<usize>,
    /// Optional lower following-line count limit.
    pub following_lines: Option<usize>,
    /// Optional lower following byte limit.
    pub following_bytes: Option<usize>,
    /// Optional lower per-section semantic fact count limit.
    pub semantic_fact_count: Option<usize>,
    /// Optional lower per-section semantic fact byte limit.
    pub semantic_fact_bytes: Option<usize>,
    /// Optional lower total-context byte limit.
    pub total_context_bytes: Option<usize>,
}

/// Why a bounded context section retained less than the document offered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextTruncationReason {
    /// The section is complete; nothing was dropped.
    #[default]
    NotTruncated,
    /// The section's byte budget was reached.
    ByteBudget,
    /// The section's line-count budget was reached.
    LineBudget,
    /// The section's fact-count budget was reached.
    FactCountBudget,
    /// The shared total-context byte budget forced a reduction.
    TotalContextBudget,
}

/// Count metadata for one bounded context section.
///
/// Stores only counts and the truncation reason — never omitted text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSectionStats {
    /// Lines the document offered to this section.
    pub original_lines: usize,
    /// Lines retained after budgeting.
    pub included_lines: usize,
    /// Bytes the document offered to this section.
    pub original_bytes: usize,
    /// Bytes retained after budgeting.
    pub included_bytes: usize,
    /// Why this section was (not) truncated.
    pub reason: ContextTruncationReason,
}

/// Per-section truncation metadata for the whole prepared context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedContextTruncation {
    /// Current-line prefix section.
    pub prefix: ContextSectionStats,
    /// Current-line suffix section.
    pub suffix: ContextSectionStats,
    /// Preceding-lines section.
    pub preceding_lines: ContextSectionStats,
    /// Following-lines section.
    pub following_lines: ContextSectionStats,
    /// Visible-variable facts section.
    pub variables: ContextSectionStats,
    /// Import facts section.
    pub imports: ContextSectionStats,
    /// Aggregate over every bounded section versus the total byte budget.
    pub total: ContextSectionStats,
}

/// Identity of the immutable document snapshot a context was prepared from.
///
/// The digest and byte length pin the exact text; the version and generation
/// pin the editor/server lifecycle state it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedContextSourceIdentity {
    /// LSP document version at snapshot time, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_version: Option<i64>,
    /// Server-side document generation counter at snapshot time, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_generation: Option<u64>,
    /// Tagged FNV-1a 64-bit digest of the exact snapshot text.
    pub source_digest: String,
    /// Byte length of the snapshot text.
    pub source_bytes: usize,
}

/// Request position plus snapshot identity for one invoked completion.
///
/// The cursor position plus the current-line suffix and its original byte
/// count are the facts needed to derive the replacement range (cursor to end
/// of visible statement tail) without embedding LSP wire types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedContextRequest {
    /// Zero-based UTF-16 line of the completion request.
    pub line: u32,
    /// Zero-based UTF-16 character of the completion request.
    pub character: u32,
    /// Identity of the immutable snapshot the context was prepared from.
    pub source: PreparedContextSourceIdentity,
}

/// Snapshot identity supplied by the transport when preparing invoked AI
/// context.
///
/// Built under the document lock next to the text snapshot so version,
/// generation, and text describe one immutable state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InlineCompletionSnapshotIdentity {
    /// LSP document version of the snapshot, when the store exposes one.
    pub document_version: Option<i64>,
    /// Server-side generation counter of the snapshot, when exposed.
    pub source_generation: Option<u64>,
}

/// Typed outcome of preparing invoked AI completion context.
///
/// `NoContext` and `Stale` both mean zero backend calls: the invoked request
/// fails closed before any provider work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedInvocationContext {
    /// A bounded context bound to one snapshot.
    Ready(Box<super::PreparedInlineCompletionContext>),
    /// The cursor sits in a hard-reject zone or outside the document.
    NoContext,
    /// The request's document version does not match the snapshot.
    Stale,
}

/// The bounded, budgeted sections of one prepared context.
pub(super) struct BoundedContextSections {
    /// Budgeted current-line prefix (distant head dropped first).
    pub prefix: String,
    /// Budgeted current-line suffix (nearest head kept).
    pub suffix: String,
    /// Budgeted preceding lines in document order.
    pub preceding: Vec<String>,
    /// Budgeted following lines in document order.
    pub following: Vec<String>,
    /// Budgeted visible-variable facts, nearest-first.
    pub variables: Vec<String>,
    /// Budgeted import facts, document order.
    pub imports: Vec<String>,
    /// Per-section truncation metadata.
    pub truncation: PreparedContextTruncation,
}

/// Truncate a string keeping its head (the bytes nearest the cursor).
///
/// Truncation floors to a valid UTF-8 character boundary.
pub(super) fn truncate_bytes_keep_head(
    text: &str,
    max_bytes: usize,
) -> (&str, ContextTruncationReason) {
    if text.len() <= max_bytes {
        return (text, ContextTruncationReason::NotTruncated);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], ContextTruncationReason::ByteBudget)
}

/// Truncate a string keeping its tail (drops distant head bytes first).
///
/// Truncation floors to a valid UTF-8 character boundary.
pub(super) fn truncate_bytes_keep_tail(
    text: &str,
    max_bytes: usize,
) -> (&str, ContextTruncationReason) {
    if text.len() <= max_bytes {
        return (text, ContextTruncationReason::NotTruncated);
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    (&text[start..], ContextTruncationReason::ByteBudget)
}

/// Retain the nearest lines of a window under line-count and byte budgets.
///
/// `window` is in nearest-first order. Lines are consumed nearest-first;
/// consumption stops at the line-count budget or when the next line would
/// exceed the byte budget, so distant lines are dropped before nearby syntax.
fn bounded_lines_nearest_first(
    window: &[&str],
    max_lines: usize,
    max_bytes: usize,
) -> (Vec<String>, ContextSectionStats) {
    let original_lines = window.len();
    let original_bytes: usize = window.iter().map(|line| line.len()).sum();

    let mut included: Vec<String> = Vec::new();
    let mut included_bytes = 0usize;
    for line in window {
        if included.len() >= max_lines {
            break;
        }
        let line_bytes = line.len();
        if included_bytes + line_bytes > max_bytes {
            break;
        }
        included_bytes += line_bytes;
        included.push((*line).to_string());
    }

    let included_lines = included.len();
    let reason = if included_lines == original_lines {
        ContextTruncationReason::NotTruncated
    } else if included_lines >= max_lines {
        ContextTruncationReason::LineBudget
    } else {
        ContextTruncationReason::ByteBudget
    };

    (
        included,
        ContextSectionStats {
            original_lines,
            included_lines,
            original_bytes,
            included_bytes,
            reason,
        },
    )
}

/// Retain semantic facts under count and byte budgets.
///
/// Facts arrive in deterministic nearest-first order; retention keeps that
/// order and drops the farthest facts first.
pub(super) fn bounded_facts(
    facts: Vec<String>,
    budget: &InlineCompletionContextBudget,
) -> (Vec<String>, ContextSectionStats) {
    let original_lines = facts.len();
    let original_bytes = facts.iter().map(String::len).sum();

    let mut included: Vec<String> = Vec::new();
    let mut included_bytes = 0usize;
    for fact in facts {
        if included.len() >= budget.semantic_fact_count {
            break;
        }
        let fact_bytes = fact.len();
        if included_bytes + fact_bytes > budget.semantic_fact_bytes {
            break;
        }
        included_bytes += fact_bytes;
        included.push(fact);
    }

    let included_count = included.len();
    let reason = if included_count == original_lines {
        ContextTruncationReason::NotTruncated
    } else if included_count >= budget.semantic_fact_count {
        ContextTruncationReason::FactCountBudget
    } else {
        ContextTruncationReason::ByteBudget
    };

    (
        included,
        ContextSectionStats {
            original_lines,
            included_lines: included_count,
            original_bytes,
            included_bytes,
            reason,
        },
    )
}

/// Build every bounded line section plus per-section metadata.
///
/// `preceding_window` and `following_window` must be nearest-first slices of
/// normalized (CR-stripped) lines around the current line. The total byte
/// budget is enforced afterwards in the documented reduction order: drop
/// following lines farthest-first, then preceding lines farthest-first, then
/// variables and imports farthest-first, and only then trim the suffix tail
/// and the prefix head.
pub(super) fn build_bounded_sections(
    prefix: &str,
    suffix: &str,
    preceding_window: &[&str],
    following_window: &[&str],
    variables: Vec<String>,
    imports: Vec<String>,
    budget: &InlineCompletionContextBudget,
) -> BoundedContextSections {
    let (bounded_prefix, prefix_reason) = truncate_bytes_keep_tail(prefix, budget.prefix_bytes);
    let mut prefix_stats = ContextSectionStats {
        original_lines: 1,
        included_lines: 1,
        original_bytes: prefix.len(),
        included_bytes: bounded_prefix.len(),
        reason: prefix_reason,
    };

    let (bounded_suffix, suffix_reason) = truncate_bytes_keep_head(suffix, budget.suffix_bytes);
    let mut suffix_stats = ContextSectionStats {
        original_lines: 1,
        included_lines: 1,
        original_bytes: suffix.len(),
        included_bytes: bounded_suffix.len(),
        reason: suffix_reason,
    };

    // Windows arrive nearest-first; stored sections are converted back to
    // document order below, after byte/count budgeting.
    let (mut preceding, mut preceding_stats) = bounded_lines_nearest_first(
        preceding_window,
        budget.preceding_lines,
        budget.preceding_bytes,
    );
    let (mut following, mut following_stats) = bounded_lines_nearest_first(
        following_window,
        budget.following_lines,
        budget.following_bytes,
    );
    // `preceding` is nearest-first here; reverse into document order.
    preceding.reverse();

    let (mut variables, mut variables_stats) = bounded_facts(variables, budget);
    // Imports arrive in document order; retention is nearest-first for every
    // section, so budget the reversed list and restore document order.
    let mut imports_nearest_first = imports;
    imports_nearest_first.reverse();
    let (mut imports, mut imports_stats) = bounded_facts(imports_nearest_first, budget);
    imports.reverse();

    let mut bounded_prefix = bounded_prefix.to_string();
    let mut bounded_suffix = bounded_suffix.to_string();

    enforce_total_budget(
        &mut bounded_prefix,
        &mut bounded_suffix,
        &mut preceding,
        &mut following,
        &mut variables,
        &mut imports,
        &mut prefix_stats,
        &mut suffix_stats,
        &mut preceding_stats,
        &mut following_stats,
        &mut variables_stats,
        &mut imports_stats,
        budget,
    );

    let mut truncation = PreparedContextTruncation {
        prefix: prefix_stats,
        suffix: suffix_stats,
        preceding_lines: preceding_stats,
        following_lines: following_stats,
        variables: variables_stats,
        imports: imports_stats,
        total: ContextSectionStats::default(),
    };
    truncation.total = total_stats(&truncation);
    BoundedContextSections {
        prefix: bounded_prefix,
        suffix: bounded_suffix,
        preceding,
        following,
        variables,
        imports,
        truncation,
    }
}

fn included_total_bytes(truncation: &PreparedContextTruncation) -> usize {
    truncation.prefix.included_bytes
        + truncation.suffix.included_bytes
        + truncation.preceding_lines.included_bytes
        + truncation.following_lines.included_bytes
        + truncation.variables.included_bytes
        + truncation.imports.included_bytes
}

fn original_total_bytes(truncation: &PreparedContextTruncation) -> usize {
    truncation.prefix.original_bytes
        + truncation.suffix.original_bytes
        + truncation.preceding_lines.original_bytes
        + truncation.following_lines.original_bytes
        + truncation.variables.original_bytes
        + truncation.imports.original_bytes
}

fn total_stats(truncation: &PreparedContextTruncation) -> ContextSectionStats {
    let reduced_by_total = [
        truncation.prefix.reason,
        truncation.suffix.reason,
        truncation.preceding_lines.reason,
        truncation.following_lines.reason,
        truncation.variables.reason,
        truncation.imports.reason,
    ]
    .contains(&ContextTruncationReason::TotalContextBudget);
    ContextSectionStats {
        original_lines: 0,
        included_lines: 0,
        original_bytes: original_total_bytes(truncation),
        included_bytes: included_total_bytes(truncation),
        reason: if reduced_by_total {
            ContextTruncationReason::TotalContextBudget
        } else {
            ContextTruncationReason::NotTruncated
        },
    }
}

/// Reduce sections until the total byte budget holds.
///
/// Reduction order (distant context first, current-line prefix/suffix last):
/// following lines farthest-first, preceding lines farthest-first, variables
/// farthest-first, imports farthest-first, suffix tail, prefix head. Whole-line
/// and whole-fact removals may undershoot the budget; the current-line
/// sections are byte-truncated by exactly the remaining excess.
#[allow(clippy::too_many_arguments)]
fn enforce_total_budget(
    bounded_prefix: &mut String,
    bounded_suffix: &mut String,
    preceding: &mut Vec<String>,
    following: &mut Vec<String>,
    variables: &mut Vec<String>,
    imports: &mut Vec<String>,
    prefix_stats: &mut ContextSectionStats,
    suffix_stats: &mut ContextSectionStats,
    preceding_stats: &mut ContextSectionStats,
    following_stats: &mut ContextSectionStats,
    variables_stats: &mut ContextSectionStats,
    imports_stats: &mut ContextSectionStats,
    budget: &InlineCompletionContextBudget,
) {
    let total = |suffix: &str,
                 prefix: &str,
                 prec: &[String],
                 foll: &[String],
                 vars: &[String],
                 imps: &[String]| {
        prefix.len()
            + suffix.len()
            + prec.iter().map(String::len).sum::<usize>()
            + foll.iter().map(String::len).sum::<usize>()
            + vars.iter().map(String::len).sum::<usize>()
            + imps.iter().map(String::len).sum::<usize>()
    };

    while total(bounded_suffix, bounded_prefix, preceding, following, variables, imports)
        > budget.total_context_bytes
    {
        let excess =
            total(bounded_suffix, bounded_prefix, preceding, following, variables, imports)
                - budget.total_context_bytes;
        if let Some(line) = following.pop() {
            following_stats.included_bytes -= line.len();
            following_stats.included_lines -= 1;
            mark_total(following_stats);
        } else if !preceding.is_empty() {
            let line = preceding.remove(0);
            preceding_stats.included_bytes -= line.len();
            preceding_stats.included_lines -= 1;
            mark_total(preceding_stats);
        } else if let Some(fact) = variables.pop() {
            variables_stats.included_bytes -= fact.len();
            variables_stats.included_lines -= 1;
            mark_total(variables_stats);
        } else if let Some(fact) = imports.pop() {
            imports_stats.included_bytes -= fact.len();
            imports_stats.included_lines -= 1;
            mark_total(imports_stats);
        } else if !bounded_suffix.is_empty() {
            let target = bounded_suffix.len().saturating_sub(excess);
            let (kept, _) = truncate_bytes_keep_head(bounded_suffix, target);
            let reduced = kept.to_string();
            bounded_suffix.clear();
            bounded_suffix.push_str(&reduced);
            suffix_stats.included_bytes = bounded_suffix.len();
            mark_total(suffix_stats);
        } else if !bounded_prefix.is_empty() {
            let target = bounded_prefix.len().saturating_sub(excess);
            let (kept, _) = truncate_bytes_keep_tail(bounded_prefix, target);
            let reduced = kept.to_string();
            bounded_prefix.clear();
            bounded_prefix.push_str(&reduced);
            prefix_stats.included_bytes = bounded_prefix.len();
            mark_total(prefix_stats);
        } else {
            // Every section is already empty; the budget cannot be exceeded.
            break;
        }
    }
}

fn mark_total(stats: &mut ContextSectionStats) {
    stats.reason = ContextTruncationReason::TotalContextBudget;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::inline_completion::InlineCompletionProvider;
    use perl_test_must::{must_some_with, must_with};

    fn maxima() -> InlineCompletionContextBudget {
        InlineCompletionContextBudget::compiled_maxima()
    }

    #[test]
    fn truncate_keep_head_floors_to_char_boundary() {
        let text = "ab\u{1F600}cd"; // 4-byte emoji after 2 ASCII bytes
        let (kept, reason) = truncate_bytes_keep_head(text, 3);
        assert_eq!(kept, "ab");
        assert_eq!(reason, ContextTruncationReason::ByteBudget);

        let (exact, reason) = truncate_bytes_keep_head(text, text.len());
        assert_eq!(exact, text);
        assert_eq!(reason, ContextTruncationReason::NotTruncated);
    }

    #[test]
    fn truncate_keep_tail_floors_to_char_boundary() {
        let text = "ab\u{1F600}cd";
        // Keeping the final 3 bytes must skip back over the emoji's interior.
        let (kept, reason) = truncate_bytes_keep_tail(text, 3);
        assert_eq!(kept, "cd");
        assert_eq!(reason, ContextTruncationReason::ByteBudget);
    }

    #[test]
    fn profile_limits_can_only_lower_budgets() {
        let limits = ContextBudgetLimits {
            suffix_bytes: Some(64),
            following_lines: Some(2),
            prefix_bytes: Some(999_999), // above the ceiling
            ..ContextBudgetLimits::default()
        };
        let budget = InlineCompletionContextBudget::saturating_from_profile(limits);
        let maxima = maxima();
        assert_eq!(budget.suffix_bytes, 64);
        assert_eq!(budget.following_lines, 2);
        assert_eq!(budget.prefix_bytes, maxima.prefix_bytes, "must saturate at the maximum");
    }

    #[test]
    fn bounded_facts_drop_farthest_first() {
        let budget = InlineCompletionContextBudget {
            semantic_fact_count: 2,
            semantic_fact_bytes: 1024,
            ..maxima()
        };
        let facts = vec!["$nearest".to_string(), "$middle".to_string(), "$farthest".to_string()];
        let (included, stats) = bounded_facts(facts, &budget);
        assert_eq!(included, vec!["$nearest".to_string(), "$middle".to_string()]);
        assert_eq!(stats.included_lines, 2);
        assert_eq!(stats.original_lines, 3);
        assert_eq!(stats.reason, ContextTruncationReason::FactCountBudget);
    }

    // ── Issue #10273 test matrix ─────────────────────────────────────────────

    /// Test 1: a mid-line position yields the exact prefix and the visible
    /// statement tail as suffix.
    #[test]
    fn mid_line_position_yields_exact_prefix_and_suffix() {
        let provider = InlineCompletionProvider::new();
        let source = "my $total = + $other; # trailing note\n";
        let prepared = must_some_with(provider.prepare_context(source, 0, 12), "context");
        assert_eq!(prepared.prefix, "my $total = ");
        assert_eq!(prepared.suffix, "+ $other; # trailing note");
        assert_eq!(prepared.truncation.suffix.reason, ContextTruncationReason::NotTruncated);
    }

    /// Test 2: a mid-block position includes the nearest closing delimiter
    /// and following line under budget.
    #[test]
    fn mid_block_position_includes_closing_delimiter_and_following_line() {
        let provider = InlineCompletionProvider::new();
        let source = "sub helper {\n    my $input = \n    return $input;\n}\n1;\n";
        let prepared = must_some_with(provider.prepare_context(source, 1, 16), "context");
        assert_eq!(prepared.suffix, "");
        assert!(!prepared.following_lines.is_empty(), "following context must be present");
        assert!(
            prepared.following_lines.iter().any(|line| line.contains("}")),
            "the nearby closing delimiter must be visible, got {:?}",
            prepared.following_lines
        );
        assert!(
            prepared.following_lines.iter().any(|line| line.contains("return $input;")),
            "the following statement must be visible"
        );
    }

    /// Test 3: EOF and empty-line positions have exact empty suffix behavior.
    #[test]
    fn eof_and_empty_line_positions_have_exact_empty_suffix() {
        let provider = InlineCompletionProvider::new();

        // Position after a trailing newline: the current line is empty.
        let eof = must_some_with(provider.prepare_context("my $x = 1;\n", 1, 0), "context");
        assert_eq!(eof.suffix, "");
        assert_eq!(eof.prefix, "");
        assert!(eof.following_lines.is_empty());
        assert_eq!(eof.truncation.suffix.reason, ContextTruncationReason::NotTruncated);

        // An interior empty line also has an exact empty suffix.
        let empty_line =
            must_some_with(provider.prepare_context("my $x = 1;\n\nmy $y = 2;\n", 1, 0), "context");
        assert_eq!(empty_line.suffix, "");
        assert_eq!(empty_line.prefix, "");
    }

    /// Test 4: CRLF, LF, bare CR, multibyte, and surrogate-counting positions
    /// follow the canonical position policy.
    #[test]
    fn crlf_bare_cr_and_multibyte_positions_preserve_geometry() {
        let provider = InlineCompletionProvider::new();

        // CRLF: the CR is a line terminator, never part of the suffix.
        let crlf = must_some_with(
            provider.prepare_context("my $a = 1;\r\nmy $b = + $c;\r\n", 1, 8),
            "context",
        );
        assert_eq!(crlf.prefix, "my $b = ");
        assert_eq!(crlf.suffix, "+ $c;");
        // The trailing newline contributes one final empty line.
        assert_eq!(crlf.following_lines, vec!["".to_string()]);

        // Bare CR stays inside the line content (exact geometry).
        let bare_cr =
            must_some_with(provider.prepare_context("my $r = 1;\rmy $s = 2;\n", 0, 19), "context");
        assert_eq!(bare_cr.suffix, "2;");

        // Multibyte: UTF-16 columns split inside the emoji only on char
        // boundaries (the emoji counts as 2 UTF-16 units).
        let multibyte_line = "my \u{1F600}x = ♥;";
        let cursor_utf16 = "my ".encode_utf16().count() as u32; // before the emoji
        let multibyte =
            must_some_with(provider.prepare_context(multibyte_line, 0, cursor_utf16), "context");
        assert_eq!(multibyte.prefix, "my ");
        assert_eq!(multibyte.suffix, "\u{1F600}x = ♥;");

        // A column that lands inside the emoji's surrogate pair clamps to the
        // emoji start (canonical policy: no half-surrogate columns).
        let inside_pair = cursor_utf16 + 1;
        let clamped =
            must_some_with(provider.prepare_context(multibyte_line, 0, inside_pair), "context");
        assert_eq!(clamped.prefix, "my ");
        assert_eq!(clamped.suffix, "\u{1F600}x = ♥;");
    }

    /// Test 6 (core half): a stale version/generation fails closed before any
    /// backend work; a matching or absent version prepares normally.
    #[test]
    fn stale_request_version_returns_typed_stale() {
        let provider = InlineCompletionProvider::new();
        let source = "my $x = 1;\n";
        let snapshot = InlineCompletionSnapshotIdentity {
            document_version: Some(7),
            source_generation: Some(3),
        };

        let stale = provider.prepare_invoked_context(source, 0, 4, snapshot, Some(6));
        assert_eq!(stale, PreparedInvocationContext::Stale);

        let newer = provider.prepare_invoked_context(source, 0, 4, snapshot, Some(8));
        assert_eq!(newer, PreparedInvocationContext::Stale);

        let matching = provider.prepare_invoked_context(source, 0, 4, snapshot, Some(7));
        let context = must_some_with(
            match matching {
                PreparedInvocationContext::Ready(context) => Some(context),
                _ => None,
            },
            "matching version must prepare",
        );
        let request = must_some_with(context.request, "invoked context carries request identity");
        assert_eq!(request.line, 0);
        assert_eq!(request.character, 4);
        assert_eq!(request.source.document_version, Some(7));
        assert_eq!(request.source.source_generation, Some(3));
        assert_eq!(request.source.source_bytes, source.len());
        assert_eq!(request.source.source_digest, crate::hashing::fnv1a64_hex(source.as_bytes()));

        // An absent request version cannot prove staleness and prepares.
        let unknown = provider.prepare_invoked_context(source, 0, 4, snapshot, None);
        assert!(matches!(unknown, PreparedInvocationContext::Ready(_)));

        // A snapshot without a version never claims staleness.
        let no_snapshot_version = InlineCompletionSnapshotIdentity::default();
        let unversioned =
            provider.prepare_invoked_context(source, 0, 4, no_snapshot_version, Some(6));
        assert!(matches!(unversioned, PreparedInvocationContext::Ready(_)));
    }

    /// Test 7 (core half): hard-reject zones return no context, so the routes
    /// make zero backend calls.
    #[test]
    fn hard_reject_zone_returns_no_invoked_context() {
        let provider = InlineCompletionProvider::new();
        let source = "my $s = \"abc\";\n";
        // Cursor inside the string literal.
        let inside_string = provider.prepare_invoked_context(
            source,
            0,
            11,
            InlineCompletionSnapshotIdentity::default(),
            None,
        );
        assert_eq!(inside_string, PreparedInvocationContext::NoContext);

        let comment_source = "# commentary\nmy $x = 1;\n";
        let inside_comment = provider.prepare_invoked_context(
            comment_source,
            0,
            5,
            InlineCompletionSnapshotIdentity::default(),
            None,
        );
        assert_eq!(inside_comment, PreparedInvocationContext::NoContext);
    }

    /// Test 8: prefix/suffix/preceding/following/semantic budgets each hit
    /// the exact boundary and the one-over case.
    #[test]
    fn budgets_hit_exact_boundary_and_one_over() {
        let provider = InlineCompletionProvider::new();
        let maxima = maxima();

        // Prefix: exact boundary keeps everything; one over truncates the
        // distant head, keeping the tail nearest the cursor.
        let exact_prefix = "a".repeat(maxima.prefix_bytes);
        let at_boundary = must_some_with(
            provider.prepare_context(&format!("{exact_prefix}\n"), 0, maxima.prefix_bytes as u32),
            "context",
        );
        assert_eq!(at_boundary.prefix.len(), maxima.prefix_bytes);
        assert_eq!(at_boundary.truncation.prefix.reason, ContextTruncationReason::NotTruncated);

        let over = must_some_with(
            provider.prepare_context(
                &format!("{exact_prefix}Z\n"),
                0,
                (maxima.prefix_bytes + 1) as u32,
            ),
            "context",
        );
        assert!(over.prefix.len() <= maxima.prefix_bytes);
        assert_eq!(over.truncation.prefix.reason, ContextTruncationReason::ByteBudget);
        assert_eq!(over.truncation.prefix.original_bytes, maxima.prefix_bytes + 1);
        assert!(over.prefix.ends_with('Z'), "nearest tail must survive");

        // Suffix: exact boundary keeps everything; one over keeps the head.
        let exact_suffix = "b".repeat(maxima.suffix_bytes);
        let suffix_at =
            must_some_with(provider.prepare_context(&format!("{exact_suffix}\n"), 0, 0), "context");
        assert_eq!(suffix_at.suffix.len(), maxima.suffix_bytes);
        assert_eq!(suffix_at.truncation.suffix.reason, ContextTruncationReason::NotTruncated);

        let suffix_over = must_some_with(
            provider.prepare_context(&format!("W{exact_suffix}\n"), 0, 0),
            "context",
        );
        assert_eq!(suffix_over.suffix.len(), maxima.suffix_bytes);
        assert_eq!(suffix_over.truncation.suffix.reason, ContextTruncationReason::ByteBudget);
        assert!(suffix_over.suffix.starts_with('W'), "nearest suffix head survives");
        assert!(suffix_over.suffix.ends_with('b'));

        // Preceding lines: 16 kept of 17 offered, nearest retained.
        let preceding: Vec<String> =
            (0..(maxima.preceding_lines + 1)).map(|i| format!("line {i}")).collect();
        let source = format!("{}\ncursor here\n", preceding.join("\n"));
        let prepared = must_some_with(
            provider.prepare_context(&source, (maxima.preceding_lines + 1) as u32, 11),
            "context",
        );
        assert_eq!(prepared.preceding_lines.len(), maxima.preceding_lines);
        assert_eq!(prepared.truncation.preceding_lines.reason, ContextTruncationReason::LineBudget);
        assert_eq!(prepared.truncation.preceding_lines.original_lines, maxima.preceding_lines + 1);
        assert_eq!(prepared.preceding_lines.last().map(String::as_str), Some("line 16"));
        assert!(
            !prepared.preceding_lines.iter().any(|l| l == "line 0"),
            "the farthest preceding line must drop"
        );

        // Following lines: 16 kept of 17 offered, nearest retained. (The
        // document's trailing newline contributes one final empty line, so
        // the section's original count is 18.)
        let following: Vec<String> =
            (0..(maxima.following_lines + 1)).map(|i| format!("after {i}")).collect();
        let source = format!("cursor here\n{}\n", following.join("\n"));
        let prepared = must_some_with(provider.prepare_context(&source, 0, 11), "context");
        assert_eq!(prepared.following_lines.len(), maxima.following_lines);
        assert_eq!(prepared.truncation.following_lines.reason, ContextTruncationReason::LineBudget);
        assert_eq!(prepared.truncation.following_lines.original_lines, maxima.following_lines + 2);
        assert_eq!(prepared.following_lines.first().map(String::as_str), Some("after 0"));
        assert!(
            !prepared.following_lines.iter().any(|l| l == "after 16"),
            "the farthest following line must drop"
        );

        // Semantic facts: 33 imports keep the nearest 32. (Declared variables
        // are already bounded at 8 nearest facts by the deterministic
        // engine's collector, below the compiled semantic maximum.)
        let uses: Vec<String> =
            (0..(maxima.semantic_fact_count + 1)).map(|i| format!("use Mod{:02};", i)).collect();
        let source = format!("{}\ncursor here\n", uses.join("\n"));
        let prepared = must_some_with(
            provider.prepare_context(&source, maxima.semantic_fact_count as u32, 11),
            "context",
        );
        assert_eq!(prepared.imports.len(), maxima.semantic_fact_count);
        assert_eq!(prepared.truncation.imports.reason, ContextTruncationReason::FactCountBudget);
        assert_eq!(prepared.truncation.imports.original_lines, maxima.semantic_fact_count + 1);
        assert!(!prepared.imports.iter().any(|i| i == "Mod00"), "the farthest import must drop");

        // The engine's own nearest-8 variable cap stays intact below the
        // compiled semantic maximum.
        let declarations: Vec<String> =
            (0..16).map(|i| format!("    my $var{i:02} = {i};")).collect();
        let source = format!("sub f {{\n{}\n    \n}}\n", declarations.join("\n"));
        let prepared = must_some_with(provider.prepare_context(&source, 17, 4), "context");
        assert_eq!(prepared.variables.len(), 8);
        assert_eq!(prepared.truncation.variables.reason, ContextTruncationReason::NotTruncated);
        assert_eq!(prepared.truncation.variables.original_lines, 8);
    }

    /// Test 8 (total): the shared total-context budget reduces distant
    /// context before the current-line sections.
    #[test]
    fn total_context_budget_reduces_following_lines_first() {
        let provider = InlineCompletionProvider::new();
        let maxima = maxima();
        assert!(
            maxima.prefix_bytes
                + maxima.suffix_bytes
                + maxima.preceding_bytes
                + maxima.following_bytes
                > maxima.total_context_bytes
        );

        let long_prefix = "p".repeat(maxima.prefix_bytes);
        let long_suffix = "s".repeat(maxima.suffix_bytes);
        let preceding: Vec<String> = (0..maxima.preceding_lines).map(|_| "x".repeat(128)).collect();
        let following: Vec<String> = (0..maxima.following_lines).map(|_| "y".repeat(128)).collect();
        let current_line = format!("{long_prefix}{long_suffix}");
        let source =
            format!("{}\n{}\n{}\n", preceding.join("\n"), current_line, following.join("\n"));
        let prepared = must_some_with(
            provider.prepare_context(
                &source,
                maxima.preceding_lines as u32,
                maxima.prefix_bytes as u32,
            ),
            "context",
        );

        let included_total = prepared.truncation.total.included_bytes;
        assert!(
            included_total <= maxima.total_context_bytes,
            "total budget must hold: {included_total} > {}",
            maxima.total_context_bytes
        );
        assert_eq!(
            prepared.truncation.following_lines.reason,
            ContextTruncationReason::TotalContextBudget,
            "distant following lines reduce before the current line"
        );
        assert!(
            prepared.following_lines.is_empty(),
            "all following lines must drop under this extreme"
        );
        assert_eq!(prepared.prefix.len(), maxima.prefix_bytes, "current-line prefix survives");
        assert_eq!(prepared.suffix.len(), maxima.suffix_bytes, "current-line suffix survives");
        assert_eq!(prepared.preceding_lines.len(), maxima.preceding_lines);
        assert_eq!(prepared.truncation.total.reason, ContextTruncationReason::TotalContextBudget);
    }

    /// Test 9: truncation retains nearest lines/facts and the metadata is
    /// deterministic across repeated runs.
    #[test]
    fn truncation_metadata_is_deterministic_across_runs() {
        let provider = InlineCompletionProvider::new();
        let declarations: Vec<String> = (0..40).map(|i| format!("    my $v{i} = {i};")).collect();
        let source = format!(
            "sub f {{\n{}\n    \n}}\nsub g {{\n    return 1;\n}}\n",
            declarations.join("\n")
        );

        let first = must_some_with(provider.prepare_context(&source, 41, 4), "context");
        let second = must_some_with(provider.prepare_context(&source, 41, 4), "context");
        assert_eq!(first, second, "repeated preparation must be byte-identical");

        assert!(first.following_lines.iter().any(|l| l.contains("}")), "nearest syntax kept");
        assert!(first.variables.len() <= 8, "engine's nearest-8 variable cap stays intact");
    }

    /// Test 11: no absolute path, unrelated file text, or workspace-wide
    /// fact set enters the context.
    #[test]
    fn prepared_context_contains_only_source_derived_facts() {
        let provider = InlineCompletionProvider::new();
        let source =
            "use strict;\npackage Demo;\n\nsub helper {\n    my $x = 1;\n    \n}\n\nelsewhere();\n";
        let prepared = must_some_with(provider.prepare_context(source, 5, 4), "context");

        let serialized = must_with(serde_json::to_string(&prepared), "serialize");
        assert!(!serialized.contains("file:///"), "no absolute paths");
        assert!(!serialized.contains("C:\\"), "no windows paths");
        for line in &prepared.preceding_lines {
            assert!(source.contains(line.as_str()), "preceding line must come from the source");
        }
        for line in &prepared.following_lines {
            assert!(source.contains(line.as_str()), "following line must come from the source");
        }
        // Truncation metadata stores counts and reasons only.
        let truncation = must_with(serde_json::to_value(&prepared.truncation), "serialize");
        let text_like = ["originalText", "includedText", "omitted", "content"];
        for key in text_like {
            assert!(
                serialized_truncation_has_no_key(&truncation, key),
                "truncation must not store text under {key}"
            );
        }
        // Plain deterministic preparation carries no request identity.
        assert!(prepared.request.is_none());
    }

    fn serialized_truncation_has_no_key(value: &serde_json::Value, key: &str) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                !map.contains_key(key)
                    && map.values().all(|v| serialized_truncation_has_no_key(v, key))
            }
            serde_json::Value::Array(items) => {
                items.iter().all(|v| serialized_truncation_has_no_key(v, key))
            }
            _ => true,
        }
    }

    /// Legacy serialized contexts decode with default bounded sections.
    #[test]
    fn legacy_serialization_decodes_with_default_sections() {
        let legacy = r#"{
            "prefix": "    ",
            "currentLine": "    ",
            "previousNonEmptyLine": "    my $x = 1;",
            "currentFunction": "helper",
            "currentPackage": "Demo",
            "variables": ["$x"],
            "imports": ["strict"]
        }"#;
        let decoded: super::super::PreparedInlineCompletionContext =
            must_with(serde_json::from_str(legacy), "legacy decode");
        assert_eq!(decoded.suffix, "");
        assert!(decoded.preceding_lines.is_empty());
        assert!(decoded.following_lines.is_empty());
        assert!(decoded.request.is_none());
        assert_eq!(decoded.truncation, PreparedContextTruncation::default());
    }
}
