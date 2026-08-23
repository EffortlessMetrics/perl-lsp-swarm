//! Query matching and ranking helpers for workspace symbol search.
//!
//! This crate has a single responsibility: provide reusable matching and
//! ranking primitives used by LSP symbol-search providers.
//!
//! # Ownership (#10794)
//!
//! Query policy moved above `perl-symbol`: admission, tiers, digests, and the
//! canonical evidence comparator live in
//! [`perl_workspace::workspace_symbol_query`]. The functions here are thin
//! forwarding shims kept only so existing call sites and their test suites
//! keep compiling; they add no policy of their own. Canonical paths should
//! consume the compiled [`WorkspaceSymbolQueryProfile`] directly.
//!
//! Compatibility note: `compare_names_by_query` reproduces the legacy numeric
//! fallback where a non-match shared the subsequence slot during *sorting*.
//! Live provider paths filter non-matches before sorting, so no admitted row
//! is affected; this shim disappears when consumers migrate to evidence-based
//! aggregation (#10645/#10642).

use std::cmp::Ordering;

use perl_workspace::workspace_symbol_query::{
    WorkspaceSymbolQueryProfile, WorkspaceSymbolSearchKeyRole as KeyRole, match_searchable_key,
};

pub use perl_symbol::MIN_LOOSE_MATCH_QUERY_CHARS;

/// Returns `true` when a symbol name matches the provided query.
///
/// Forwarding shim over the canonical owner (#10794). Matching strategy after
/// trimming leading/trailing query whitespace: empty query matches everything,
/// then case-insensitive exact/prefix always run, and substring/subsequence
/// require a folded query of at least [`MIN_LOOSE_MATCH_QUERY_CHARS`] chars.
#[must_use]
pub fn matches_query(name: &str, query: &str) -> bool {
    let profile = WorkspaceSymbolQueryProfile::compile(query);
    match_searchable_key(&profile, name, KeyRole::Other).is_some()
}

/// Compares two symbol names by query relevance.
///
/// Forwarding shim over the canonical owner (#10794). Ordering (highest to
/// lowest relevance): exact, prefix, substring, then subsequence-or-non-match
/// at the legacy fallback slot; within the same slot, shorter raw names first,
/// lexicographic order last.
///
/// Non-matches never reach live sorts (admission filters first); the fallback
/// slot here exists purely to reproduce the legacy total order bit-for-bit
/// until consumers migrate to evidence aggregation (#10645/#10642).
#[must_use]
pub fn compare_names_by_query(a: &str, b: &str, query: &str) -> Ordering {
    let profile = WorkspaceSymbolQueryProfile::compile(query);
    let evidence_a = match_searchable_key(&profile, a, KeyRole::Other);
    let evidence_b = match_searchable_key(&profile, b, KeyRole::Other);

    // Legacy slots: exact 0 < prefix 1 < substring 2 < subsequence/non-match 3.
    let slot = |evidence: Option<
        &perl_workspace::workspace_symbol_query::WorkspaceSymbolMatchEvidence,
    >| { evidence.map_or(3, |e| e.tier() as u8) };
    match slot(evidence_a.as_ref()).cmp(&slot(evidence_b.as_ref())) {
        Ordering::Equal => match a.len().cmp(&b.len()) {
            Ordering::Equal => a.cmp(b),
            len_ord => len_ord,
        },
        tier_ord => tier_ord,
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_names_by_query, matches_query};
    use proptest::prelude::*;

    #[test]
    fn query_matching_covers_exact_prefix_contains_and_fuzzy() {
        assert!(matches_query("foo", "foo"));
        assert!(matches_query("foobar", "foo"));
        assert!(matches_query("foobar", "bar"));
        assert!(matches_query("foobar", "fb"));
        assert!(!matches_query("alpha", "zq"));
    }

    #[test]
    fn query_matching_ignores_outer_whitespace() {
        assert!(matches_query("foobar", "  foo  "));
        assert!(matches_query("foobar", "  fb  "));
        assert!(matches_query("anything", "   "));
    }

    #[test]
    fn query_ranking_ignores_outer_whitespace() {
        let ordered = compare_names_by_query("foo", "foobar", "  foo  ");
        assert!(ordered.is_lt(), "exact trimmed query match must rank before prefix match");
    }

    #[test]
    fn empty_query_matches_anything() {
        assert!(matches_query("anything", ""));
    }

    /// #5335: a one-character query must match by exact name or prefix only,
    /// so that typing one character does not return nearly every symbol.
    ///
    /// Note this is deliberately *not* the fix issue #5335 proposed. Gating the
    /// subsequence matcher on a minimum needle length changes nothing: for a
    /// single-`char` needle `is_subsequence` is equivalent to `contains`, and
    /// `contains` is tested first, so the subsequence branch is unreachable for
    /// a one-character query. The assertions below fail under that no-op fix
    /// and pass under the substring-tier restriction actually implemented.
    #[test]
    fn one_char_query_matches_exact_and_prefix_only() {
        // Exact and prefix tiers survive.
        assert!(matches_query("a", "a"), "exact one-char match must survive");
        assert!(matches_query("alpha", "a"), "prefix one-char match must survive");

        // Substring-only matches are rejected. A one-character query cannot be
        // a *subsequence-only* match -- for a single char subsequence and
        // substring coincide -- so closing the substring tier is what actually
        // narrows the result set.
        assert!(!matches_query("alpha", "l"), "one-char query must not substring-match");
        assert!(!matches_query("normalize", "a"), "one-char query must not substring-match");
    }

    /// Two-character queries keep both loose tiers. `matches_query("foobar", "fb")`
    /// is long-standing asserted behavior (see
    /// `query_matching_covers_exact_prefix_contains_and_fuzzy`); #5335 narrows
    /// one-character queries only and deliberately does not revisit it.
    #[test]
    fn two_char_query_still_matches_substring_and_subsequence() {
        assert!(matches_query("alpha", "ph"), "two-char substring match must survive");
        assert!(matches_query("foobar", "fb"), "two-char subsequence match must survive");
        assert!(!matches_query("alpha", "zq"));
    }

    /// The threshold is measured on the *lowercased* query, because lowercasing
    /// can lengthen a one-character input: 'İ' (U+0130) lowercases to the two
    /// chars "i\u{307}". Such a query keeps the loose tiers.
    #[test]
    fn short_query_length_is_measured_after_lowercasing() {
        let query = "\u{130}"; // 'İ'
        assert_eq!(query.chars().count(), 1, "raw query is one char");
        assert_eq!(query.to_lowercase().chars().count(), 2, "lowercased query is two chars");

        // Substring-only match: neither exact nor a prefix. Admitted because the
        // lowercased query reaches MIN_LOOSE_MATCH_QUERY_CHARS.
        assert!(
            matches_query("x_i\u{307}_y", query),
            "one-char input that lowercases to two chars keeps the loose tiers"
        );
    }

    #[test]
    fn relevance_prefers_exact_then_prefix_then_name_order() {
        let mut names = ["foxtrot", "foo", "foobar", "alpha"];
        names.sort_by(|a, b| compare_names_by_query(a, b, "foo"));

        assert_eq!(names, ["foo", "foobar", "alpha", "foxtrot"]);
    }

    #[test]
    fn contains_matches_rank_above_fuzzy_matches() {
        // "get_bar" contains "bar" (tier 2)
        // "baz_art" has "bar" as subsequence b-a-z-a-r-t: b..a..r (tier 3)
        let mut names = ["baz_art", "get_bar"];
        names.sort_by(|a, b| compare_names_by_query(a, b, "bar"));

        assert_eq!(names[0], "get_bar", "substring match should rank above fuzzy");
    }

    #[test]
    fn exact_match_beats_everything() {
        let mut names = ["get_log", "getLogger", "log", "logging"];
        names.sort_by(|a, b| compare_names_by_query(a, b, "log"));

        assert_eq!(names[0], "log", "exact match should be first");
    }

    #[test]
    fn shorter_names_preferred_within_same_tier() {
        // Both are prefix matches (tier 1), shorter should come first
        let mut names = ["foobarqux", "foobar"];
        names.sort_by(|a, b| compare_names_by_query(a, b, "foo"));

        assert_eq!(names[0], "foobar");
        assert_eq!(names[1], "foobarqux");
    }

    #[test]
    fn four_tier_ranking_order() {
        // exact=0, prefix=1, contains=2, fuzzy=3
        // "lxoxg" is a fuzzy match for "log" (l..o..g subsequence)
        let mut names = ["get_log", "lxoxg", "log", "logger"];
        names.sort_by(|a, b| compare_names_by_query(a, b, "log"));

        assert_eq!(names[0], "log", "tier 0: exact");
        assert_eq!(names[1], "logger", "tier 1: prefix");
        assert_eq!(names[2], "get_log", "tier 2: contains");
        assert_eq!(names[3], "lxoxg", "tier 3: fuzzy");
    }

    proptest! {
        #[test]
        fn case_insensitive_matching_is_equivalent(name in "[a-zA-Z]{1,24}", query in "[a-zA-Z]{0,12}") {
            let expected = matches_query(&name.to_lowercase(), &query.to_lowercase());
            let actual = matches_query(&name.to_uppercase(), &query.to_lowercase());

            prop_assert_eq!(actual, expected);
        }

        #[test]
        fn comparison_is_antisymmetric(a in "[a-zA-Z_]{1,20}", b in "[a-zA-Z_]{1,20}", query in "[a-zA-Z]{0,10}") {
            let ab = compare_names_by_query(&a, &b, &query);
            let ba = compare_names_by_query(&b, &a, &query);

            prop_assert_eq!(ab, ba.reverse());
        }
    }
}
