//! Regression for #5929 item 3 / PR #5942:
//! `looks_like_bare_call` must accept an uppercase identifier argument when it
//! is immediately followed by fat-comma (`=>`), because Perl auto-quotes that
//! bareword. Without the exception, `(foo B => 'test')` failed with
//! "expected ')', found identifier".
//!
//! Uppercase identifiers *without* a following `=>` remain rejected as bare-call
//! arguments (constants / package names).

mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn uppercase_bareword_before_fat_comma_is_bare_call_arg() {
    assert_clean_parse(r#"foo B => 'test';"#);
}

#[test]
fn uppercase_bareword_fat_comma_inside_parens() {
    // Exact failure shape from #5929 item 3 / Carp.pm patterns.
    assert_clean_parse(r#"(foo B => 'test');"#);
}

#[test]
fn multiple_uppercase_fat_comma_pairs_as_bare_call_args() {
    assert_clean_parse(r#"(foo B => 1, C => 2);"#);
}

#[test]
fn uppercase_bareword_fat_comma_with_or_in_parens() {
    assert_clean_parse(r#"(foo B => 'test' or return);"#);
}

#[test]
fn uppercase_identifier_without_fat_comma_still_not_bare_call_arg() {
    // Without `=>`, uppercase `B` must still be rejected as a bare-call argument.
    // That path previously (and correctly) returned false from looks_like_bare_call,
    // so parenthesized `foo B, 'test'` cannot close cleanly.
    assert_has_error(r#"(foo B, 'test');"#, "identifier");
}
