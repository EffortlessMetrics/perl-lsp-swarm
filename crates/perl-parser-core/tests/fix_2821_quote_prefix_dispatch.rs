//! Pinning tests for transliteration operator prefix dispatch.
//!
//! Background: `extract_transliteration_parts` and `extract_transliteration_parts_strict`
//! previously both contained inline duplicate prefix-stripping logic:
//!
//!   if text.strip_prefix("tr") { … }
//!   else if text.strip_prefix('y') { … }
//!   else { text }
//!
//! That was centralised into a private `strip_transliteration_prefix` helper
//! (#2821 residual). These tests pin the dispatch order so the refactoring
//! cannot silently invert `tr`/`y` precedence or accidentally skip one form.
//!
//! They operate at two layers:
//!   1. Public function contract  — `syntax::quote::{extract_transliteration_parts,
//!                                                    extract_transliteration_parts_strict}`
//!   2. Full-parser integration   — via `assert_clean_parse` / `assert_has_error`

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_tdd_support::must;

use perl_parser_core::syntax::quote::{
    TransliterationError, extract_transliteration_parts, extract_transliteration_parts_strict,
};

// ---------------------------------------------------------------------------
// Unit-level: extract_transliteration_parts (lenient)
// ---------------------------------------------------------------------------

#[test]
fn lenient_tr_prefix_yields_correct_search_and_replacement() {
    let (search, replacement, _mods) = extract_transliteration_parts("tr/a-z/A-Z/");
    assert_eq!(search, "a-z", "tr search list should be 'a-z'");
    assert_eq!(replacement, "A-Z", "tr replacement list should be 'A-Z'");
}

#[test]
fn lenient_y_prefix_yields_correct_search_and_replacement() {
    let (search, replacement, _mods) = extract_transliteration_parts("y/a-z/A-Z/");
    assert_eq!(search, "a-z", "y search list should be 'a-z'");
    assert_eq!(replacement, "A-Z", "y replacement list should be 'A-Z'");
}

#[test]
fn lenient_tr_and_y_agree_on_body() {
    // Both operator spellings must produce identical search/replacement pairs.
    let (tr_search, tr_repl, tr_mods) = extract_transliteration_parts("tr/abc/xyz/ds");
    let (y_search, y_repl, y_mods) = extract_transliteration_parts("y/abc/xyz/ds");
    assert_eq!(tr_search, y_search, "tr and y search lists must agree");
    assert_eq!(tr_repl, y_repl, "tr and y replacement lists must agree");
    assert_eq!(tr_mods, y_mods, "tr and y modifiers must agree");
}

#[test]
fn lenient_tr_two_char_prefix_not_confused_with_single_t() {
    // `tr` is two characters; the dispatch must consume both before looking at
    // the delimiter, not just 't'.  A token starting `t/` (without 'r') must
    // not parse as a transliteration operator — the prefix is not found, so the
    // full string is passed through and the first char 't' is alphanumeric,
    // making the delimiter check fail and returning empty strings.
    let (search, replacement, _mods) = extract_transliteration_parts("t/a/b/");
    assert_eq!(
        (search.as_str(), replacement.as_str()),
        ("", ""),
        "bare 't' is not a transliteration prefix"
    );
}

#[test]
fn lenient_tr_paired_delimiter_search_and_replacement() {
    let (search, replacement, _mods) = extract_transliteration_parts("tr{abc}{xyz}");
    assert_eq!(search, "abc");
    assert_eq!(replacement, "xyz");
}

#[test]
fn lenient_tr_empty_lists_are_valid() {
    // tr/// is the character-count idiom; must produce empty strings, not an error.
    let (search, replacement, mods) = extract_transliteration_parts("tr///");
    assert_eq!(search, "");
    assert_eq!(replacement, "");
    assert_eq!(mods, "");
}

#[test]
fn lenient_tr_only_valid_modifiers_pass_through() {
    let (_s, _r, mods) = extract_transliteration_parts("tr/a/b/cdsrz");
    // 'z' is not a valid modifier and must be silently filtered.
    assert!(!mods.contains('z'), "invalid modifier 'z' must be filtered");
    assert_eq!(mods, "cdsr");
}

// ---------------------------------------------------------------------------
// Unit-level: extract_transliteration_parts_strict
// ---------------------------------------------------------------------------

#[test]
fn strict_tr_prefix_yields_correct_search_and_replacement() {
    let result = extract_transliteration_parts_strict("tr/a-z/A-Z/");
    let (search, replacement, _mods) = must(result);
    assert_eq!(search, "a-z");
    assert_eq!(replacement, "A-Z");
}

#[test]
fn strict_y_prefix_yields_correct_search_and_replacement() {
    let result = extract_transliteration_parts_strict("y/a-z/A-Z/");
    let (search, replacement, _mods) = must(result);
    assert_eq!(search, "a-z");
    assert_eq!(replacement, "A-Z");
}

#[test]
fn strict_tr_and_y_agree_on_body() {
    let (tr_s, tr_r, tr_m) = must(extract_transliteration_parts_strict("tr/abc/xyz/ds"));
    let (y_s, y_r, y_m) = must(extract_transliteration_parts_strict("y/abc/xyz/ds"));
    assert_eq!(tr_s, y_s, "strict tr and y search lists must agree");
    assert_eq!(tr_r, y_r, "strict tr and y replacement lists must agree");
    assert_eq!(tr_m, y_m, "strict tr and y modifiers must agree");
}

#[test]
fn strict_tr_two_char_prefix_not_confused_with_single_t() {
    // `t/a/b/` — 't' alone is not a transliteration operator; the strict parser
    // must treat 't' as the (invalid, alphanumeric) delimiter and return InvalidDelimiter.
    let result = extract_transliteration_parts_strict("t/a/b/");
    assert!(
        matches!(result, Err(TransliterationError::InvalidDelimiter('t'))),
        "bare 't' must produce InvalidDelimiter, got: {result:?}"
    );
}

#[test]
fn strict_invalid_modifier_returns_error() {
    let result = extract_transliteration_parts_strict("tr/a/b/z");
    assert!(
        matches!(result, Err(TransliterationError::InvalidModifier('z'))),
        "invalid modifier 'z' must produce InvalidModifier, got: {result:?}"
    );
}

#[test]
fn strict_missing_replacement_returns_error() {
    let result = extract_transliteration_parts_strict("tr/a/b");
    assert!(
        matches!(result, Err(TransliterationError::MissingClosingDelimiter)),
        "missing replacement closer must return MissingClosingDelimiter, got: {result:?}"
    );
}

#[test]
fn strict_empty_transliteration_is_valid() {
    let result = extract_transliteration_parts_strict("tr///");
    assert!(result.is_ok(), "tr/// must be valid: {result:?}");
    let (s, r, m) = must(result);
    assert_eq!((s.as_str(), r.as_str(), m.as_str()), ("", "", ""));
}

#[test]
fn strict_tr_all_valid_modifiers_accepted() {
    let result = extract_transliteration_parts_strict("tr/a/b/cdsr");
    assert!(result.is_ok(), "all valid modifiers must be accepted");
    let (_s, _r, mods) = must(result);
    assert_eq!(mods, "cdsr");
}

// ---------------------------------------------------------------------------
// Full-parser integration (BDD-style via cpan_test_helpers)
// ---------------------------------------------------------------------------

#[test]
fn parser_tr_prefix_clean() {
    assert_clean_parse(r#"$x =~ tr/a-z/A-Z/;"#);
}

#[test]
fn parser_y_prefix_clean() {
    assert_clean_parse(r#"$x =~ y/a-z/A-Z/;"#);
}

#[test]
fn parser_tr_and_y_both_succeed_with_same_modifier_set() {
    assert_clean_parse(r#"$x =~ tr/abc/xyz/cdsr;"#);
    assert_clean_parse(r#"$x =~ y/abc/xyz/cdsr;"#);
}

#[test]
fn parser_tr_with_whitespace_before_delimiter() {
    assert_clean_parse(r#"$x =~ tr /a/b/;"#);
}

#[test]
fn parser_y_with_whitespace_before_delimiter() {
    assert_clean_parse(r#"$x =~ y /a/b/;"#);
}

#[test]
fn parser_tr_paired_delimiters() {
    assert_clean_parse(r#"$x =~ tr{abc}{xyz};"#);
    assert_clean_parse(r#"$x =~ tr[abc][xyz];"#);
    assert_clean_parse(r#"$x =~ tr(abc)(xyz);"#);
    assert_clean_parse(r#"$x =~ tr<abc><xyz>;"#);
}

#[test]
fn parser_y_paired_delimiters() {
    assert_clean_parse(r#"$x =~ y{abc}{xyz};"#);
}

#[test]
fn parser_tr_mixed_paired_delimiters() {
    assert_clean_parse(r#"$x =~ tr[abc]{xyz};"#);
    assert_clean_parse(r#"$x =~ tr{abc}[xyz]r;"#);
}

#[test]
fn parser_tr_empty_operator_valid() {
    // The character-count idiom `tr///` must parse cleanly.
    assert_clean_parse(r#"my $count = ($x =~ tr///);"#);
}

#[test]
fn parser_tr_invalid_modifier_z_reported() {
    assert_has_error(r#"$x =~ tr/a/b/z;"#, "invalid transliteration modifier");
}

#[test]
fn parser_y_invalid_modifier_z_reported() {
    assert_has_error(r#"$x =~ y/a/b/z;"#, "invalid transliteration modifier");
}

#[test]
fn parser_tr_missing_replacement_delimiter_reported() {
    assert_has_error(r#"$x =~ tr/a/b;"#, "closing delimiter in transliteration");
}

#[test]
fn parser_tr_unicode_search_and_replacement() {
    assert_clean_parse(r#"$x =~ tr/αβγ/ΑΒΓ/;"#);
}
