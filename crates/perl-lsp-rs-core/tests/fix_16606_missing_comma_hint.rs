//! Issue #16606 (LSP side): the LSP hint table must tell the same story as
//! the parser-core suggestion table for a literal value where a group's
//! closer is expected — the separating comma is missing, not the parenthesis.
//!
//! Both tables share the `found_is_value_like` discriminator from
//! `perl-parser-core`; wording stays per-surface.

use perl_lsp_rs_core::providers::diagnostics::build_parse_error_hint;
use perl_parser_core::error::ParseError;

#[test]
fn hint_names_the_comma_for_a_literal_list_item() {
    let error = ParseError::unexpected("')'", "number", 17);
    let hint = build_parse_error_hint(&error, "").expect("value-like found must produce a hint");

    assert!(hint.contains("`,`"), "hint must name the missing comma, got: {hint}");
    assert!(hint.contains("comma"), "hint must mention the comma, got: {hint}");
    assert!(
        !hint.contains("unmatched opening"),
        "hint must not claim the group is unclosed, got: {hint}"
    );
}

#[test]
fn hint_keeps_closer_advice_for_a_non_literal_found() {
    // A keyword where `)` is expected is not a list item: the original
    // unmatched-parenthesis hint still applies.
    let error = ParseError::unexpected("')'", "'my'", 17);
    assert_eq!(
        build_parse_error_hint(&error, "").as_deref(),
        Some("Add a closing ')' -- there may be an unmatched opening '('"),
    );
}
