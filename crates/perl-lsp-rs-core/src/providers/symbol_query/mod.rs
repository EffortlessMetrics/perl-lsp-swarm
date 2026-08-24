//! Query matching helpers for workspace symbol search.
//!
//! This crate has a single responsibility: provide reusable matching
//! primitives used by LSP symbol-search providers.
//!
//! # Ownership (#10794)
//!
//! Query policy moved above `perl-symbol`: admission, tiers, digests, and the
//! canonical evidence comparator live in
//! [`perl_workspace::workspace_symbol_query`]. The function here is a thin
//! forwarding shim kept only so existing call sites and their test suites
//! keep compiling; it adds no policy of its own. Canonical paths should
//! consume the compiled [`WorkspaceSymbolQueryProfile`] directly.
//!
//! `compare_names_by_query` was removed in the #10794 repair review: after
//! both provider paths migrated to evidence-based sorting it had zero
//! production callers, and its legacy total-order reproduction was divergent
//! for loose-ineligible queries (an admitted-set substring fell into the
//! non-match slot). Ordering coverage lives at the canonical owner
//! (`WorkspaceSymbolMatchEvidence::compare`).

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

#[cfg(test)]
mod tests {
    use super::matches_query;
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

    proptest! {
        #[test]
        fn case_insensitive_matching_is_equivalent(name in "[a-zA-Z]{1,24}", query in "[a-zA-Z]{0,12}") {
            let expected = matches_query(&name.to_lowercase(), &query.to_lowercase());
            let actual = matches_query(&name.to_uppercase(), &query.to_lowercase());

            prop_assert_eq!(actual, expected);
        }
    }
}
