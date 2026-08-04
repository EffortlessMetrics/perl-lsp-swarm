//! Handler-level BDD tests for short `workspace/symbol` query filtering. (#5407)
//!
//! Guards the invariant that every symbol-source feeding a `workspace/symbol`
//! response applies the same short-query narrowing: queries shorter than
//! `MIN_LOOSE_MATCH_QUERY_CHARS` chars (after lowercasing) restrict results to
//! exact and prefix matches, skipping substring and subsequence tiers.
//!
//! Tests operate at the handler level — against the full result set produced by
//! `WorkspaceIndex::search_symbols` and
//! `WorkspaceIndex::search_generated_workspace_symbols` — so a new symbol
//! source that omits the guard will fail here without any per-matcher
//! visibility required. (#5335, #5407)

use perl_symbol::MIN_LOOSE_MATCH_QUERY_CHARS;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

fn file_url(path: &str) -> Result<Url, url::ParseError> {
    Url::parse(&format!("file://{path}"))
}

/// Returns true when `name` is an exact or prefix match for `query`
/// (case-insensitive, after trimming whitespace).
fn is_exact_or_prefix(name: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    let name = name.to_lowercase();
    name == query || name.starts_with(&query)
}

// ── Fixture ──────────────────────────────────────────────────────────────────

/// Index a small workspace with three subroutines:
/// `alpha_sub` (prefix "a"), `main_alpha_fn` (substring "al", not prefix "a"),
/// and `get_all_items` (substring "al", not prefix "a").
fn build_source_index() -> Result<WorkspaceIndex, Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/lib/ShortQuery.pm")?;
    let source = "\
package ShortQuery;

sub alpha_sub     { 1 }
sub main_alpha_fn { 2 }
sub get_all_items { 3 }

1;
";
    index.index_file(uri, source.to_string())?;
    Ok(index)
}

// ── Source-backed symbol tests ────────────────────────────────────────────────

/// The threshold itself must come from the canonical single definition in
/// `perl_symbol`. If either consumer redefined it locally with a different
/// value, this assertion would fail — surfacing the split-brain immediately.
#[test]
fn min_loose_match_query_chars_is_two() {
    assert_eq!(MIN_LOOSE_MATCH_QUERY_CHARS, 2);
}

/// A one-character `workspace/symbol` query may only return exact or prefix
/// matches; substring and subsequence matches are excluded.
///
/// The fixture has three subroutines. "alpha_sub" starts with "a" (prefix
/// match) so it must appear. "main_alpha_fn" and "get_all_items" contain "a"
/// as a substring but do not start with "a"; they must be absent. (#5335)
#[test]
fn given_one_char_query_when_searching_workspace_symbols_then_only_exact_and_prefix_returned()
-> Result<(), Box<dyn std::error::Error>> {
    let index = build_source_index()?;
    let query = "a";
    assert_eq!(
        query.chars().count(),
        1,
        "guard: query must be one char to exercise the short-query branch"
    );

    let results = index.search_symbols(query);

    // Every returned symbol must be exact or prefix — no substring/subsequence.
    for sym in &results {
        assert!(
            is_exact_or_prefix(&sym.name, query),
            "symbol '{}' is a substring/subsequence match for one-char query '{}' \
             — the short-query guard is missing or broken",
            sym.name,
            query,
        );
    }

    // "alpha_sub" starts with "a" and must appear.
    assert!(
        results.iter().any(|s| s.name == "alpha_sub"),
        "expected 'alpha_sub' (prefix match) in results for query '{}', got: {:?}",
        query,
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    // "main_alpha_fn" and "get_all_items" are substring-only matches and must
    // not appear in the one-char result set.
    assert!(
        !results.iter().any(|s| s.name == "main_alpha_fn"),
        "unexpected substring match 'main_alpha_fn' for one-char query '{}'",
        query,
    );
    assert!(
        !results.iter().any(|s| s.name == "get_all_items"),
        "unexpected substring match 'get_all_items' for one-char query '{}'",
        query,
    );

    Ok(())
}

/// A two-character `workspace/symbol` query admits the loose (substring +
/// subsequence) tiers. Guards against accidental over-tightening: if the
/// threshold were raised beyond 2, loose matches for "al" would be suppressed.
#[test]
fn given_two_char_query_when_searching_workspace_symbols_then_loose_matches_returned()
-> Result<(), Box<dyn std::error::Error>> {
    let index = build_source_index()?;
    let query = "al";
    assert_eq!(
        query.chars().count(),
        MIN_LOOSE_MATCH_QUERY_CHARS,
        "guard: query length must equal the threshold to confirm the boundary"
    );

    let results = index.search_symbols(query);

    // At least one loose match (substring) must appear — "main_alpha_fn"
    // contains "al" but does not start with "al", so it proves the loose tier
    // is open for this query length.
    assert!(
        results.iter().any(|s| s.name == "main_alpha_fn"),
        "expected substring match 'main_alpha_fn' for two-char query '{}', got: {:?}",
        query,
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    // "get_all_items" also contains "al" — a second loose match.
    assert!(
        results.iter().any(|s| s.name == "get_all_items"),
        "expected substring match 'get_all_items' for two-char query '{}', got: {:?}",
        query,
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    Ok(())
}

// ── Generated-member symbol tests ─────────────────────────────────────────────

/// A one-character query against generated/framework members must also return
/// only exact and prefix matches, not substring matches.
///
/// Uses a Moo-`has` fixture to ensure the generated path
/// (`search_generated_workspace_symbols`) is exercised. A symbol named
/// "attr_value" starts with "at" (prefix match for "a") and must appear.
/// A symbol named "callback_ref" contains "a" as a substring but does not
/// start with "a" and must not appear. (#5335)
#[test]
fn given_one_char_query_when_searching_generated_workspace_symbols_then_only_prefix_returned()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/lib/GenShortQuery.pm")?;
    // Both `has` declarations produce GeneratedMember entities with
    // FrameworkSynthesis provenance + Medium confidence (the criteria that
    // `search_generated_workspace_symbols` requires to include them).
    let source = "\
package GenShortQuery;
use Moo;

has attr_value   => (is => 'ro');
has callback_ref => (is => 'ro');

1;
";
    index.index_file(uri, source.to_string())?;

    let query = "a";
    let results = index.search_generated_workspace_symbols(query, None);

    // Every returned generated symbol must be exact or prefix.
    for sym in &results {
        // Generated symbols use names like "attr_value [generated/framework]" —
        // strip the label suffix before the exact/prefix check.
        let bare = sym.name.split_once(' ').map_or(sym.name.as_str(), |(b, _)| b);
        assert!(
            is_exact_or_prefix(bare, query),
            "generated symbol '{}' (bare: '{}') is a non-prefix match for \
             one-char query '{}' — the generated-member guard is missing or broken",
            sym.name,
            bare,
            query,
        );
    }

    assert!(
        !results.is_empty(),
        "fixture must synthesize generated members before the query boundary is tested"
    );
    // "attr_value" starts with "a" and must appear.
    assert!(
        results.iter().any(|s| s.name.starts_with("attr_value")),
        "expected 'attr_value' (prefix match) in generated results for query '{}', got: {:?}",
        query,
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    // "callback_ref" contains 'a' but does not start with 'a' — must be absent.
    assert!(
        !results.iter().any(|s| s.name.starts_with("callback_ref")),
        "unexpected substring match 'callback_ref' for one-char query '{}' \
         in generated symbols — the guard is missing",
        query,
    );

    Ok(())
}

/// A two-character query against generated members admits loose matches.
/// "cb" is a subsequence of "callback_ref" (c-b appear in order) so it should
/// appear when loose matching is active, confirming the generated path does not
/// over-narrow. (#5407)
#[test]
fn given_two_char_query_when_searching_generated_workspace_symbols_then_loose_matches_allowed()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let uri = file_url("/lib/GenTwoChar.pm")?;
    let source = "\
package GenTwoChar;
use Moo;

has attr_value   => (is => 'ro');
has callback_ref => (is => 'ro');

1;
";
    index.index_file(uri, source.to_string())?;

    // Two-char query that is NOT a prefix of either attribute name, so any
    // result for it is necessarily a substring or subsequence match.
    let query = "ll";
    assert_eq!(query.chars().count(), MIN_LOOSE_MATCH_QUERY_CHARS);

    let results = index.search_generated_workspace_symbols(query, None);

    assert!(
        !results.is_empty(),
        "fixture must synthesize generated members before the loose tier is tested"
    );
    // "callback_ref" contains "ll" (cal-l-back-ref -> positions 3-4), so
    // at least one result must exercise the loose tier.
    let has_loose_match = results.iter().any(|s| {
        let bare = s.name.split_once(' ').map_or(s.name.as_str(), |(b, _)| b);
        !is_exact_or_prefix(bare, query)
    });
    assert!(
        has_loose_match,
        "expected at least one loose (substring/fuzzy) match for two-char query '{}', \
         got only exact/prefix results: {:?}",
        query,
        results.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    Ok(())
}
