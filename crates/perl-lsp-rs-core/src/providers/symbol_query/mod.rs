//! Query matching and ranking helpers for workspace symbol search.
//!
//! This crate has a single responsibility: provide reusable matching and
//! ranking primitives used by LSP symbol-search providers.

use std::cmp::Ordering;

/// Minimum query length (in `char`s of the lowercased query) that admits the
/// loose match tiers -- substring and subsequence. (#5335)
///
/// A one-character query is too weak to justify loose matching: every symbol
/// whose name contains that character anywhere would match, which is nearly
/// the whole workspace. Such queries are restricted to the exact and prefix
/// tiers instead.
///
/// The same threshold is applied to the indexed workspace-symbol search in
/// `perl_workspace::workspace::workspace_index::search_source_symbols`. The
/// two matchers are independent implementations, so the constant is
/// deliberately duplicated rather than shared across the crate boundary.
pub const MIN_LOOSE_MATCH_QUERY_CHARS: usize = 2;

/// Returns `true` when a symbol name matches the provided query.
///
/// Matching strategy order after trimming leading/trailing query whitespace:
/// 1. Empty query (matches everything)
/// 2. Exact case-insensitive match
/// 3. Prefix case-insensitive match
/// 4. Contains case-insensitive match
/// 5. Subsequence/fuzzy case-insensitive match
///
/// Tiers 4 and 5 are the *loose* tiers and require a query of at least
/// [`MIN_LOOSE_MATCH_QUERY_CHARS`] characters. A single-character query
/// therefore matches only by exact or prefix. (#5335)
#[must_use]
pub fn matches_query(name: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }

    let name_lower = name.to_lowercase();
    let query_lower = query.to_lowercase();

    if name_lower == query_lower {
        return true;
    }

    if name_lower.starts_with(&query_lower) {
        return true;
    }

    // Length is measured on the *lowercased* query, because lowercasing can
    // lengthen a one-character input -- 'İ' (U+0130) lowercases to the two
    // chars "i\u{307}" -- and it is the lowercased form that the tiers below
    // actually match against.
    if query_lower.chars().count() < MIN_LOOSE_MATCH_QUERY_CHARS {
        return false;
    }

    if name_lower.contains(&query_lower) {
        return true;
    }

    is_subsequence(&name_lower, &query_lower)
}

/// Compares two symbol names by query relevance.
///
/// Ordering (highest to lowest relevance):
/// 1. Exact match (case-insensitive)
/// 2. Prefix match
/// 3. Contains (substring) match
/// 4. Fuzzy/subsequence match
///
/// Within the same tier, shorter names rank higher (closer to the query
/// length), with lexicographic order as the final tiebreaker.
#[must_use]
pub fn compare_names_by_query(a: &str, b: &str, query: &str) -> Ordering {
    let query_lower = query.trim().to_lowercase();
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    let a_tier = match_tier(&a_lower, &query_lower);
    let b_tier = match_tier(&b_lower, &query_lower);

    // Lower tier number = better match
    match a_tier.cmp(&b_tier) {
        Ordering::Equal => {
            // Within the same tier, prefer shorter names (closer to the query)
            match a.len().cmp(&b.len()) {
                Ordering::Equal => a.cmp(b),
                len_ord => len_ord,
            }
        }
        tier_ord => tier_ord,
    }
}

/// Assigns a numeric tier to a symbol name based on how well it matches the query.
///
/// Lower tier = better match:
/// - 0: exact match
/// - 1: prefix match
/// - 2: contains (substring) match
/// - 3: fuzzy/subsequence or no match (fallback)
fn match_tier(name_lower: &str, query_lower: &str) -> u8 {
    if name_lower == query_lower {
        0
    } else if name_lower.starts_with(query_lower) {
        1
    } else if name_lower.contains(query_lower) {
        2
    } else {
        3
    }
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars();
    let mut current = needle_chars.next();

    for ch in haystack.chars() {
        if let Some(target) = current {
            if ch == target {
                current = needle_chars.next();
            }
        } else {
            return true;
        }
    }

    current.is_none()
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
