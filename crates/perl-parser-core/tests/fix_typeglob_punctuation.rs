//! Regression coverage for punctuation-suffixed typeglobs.
//!
//! These forms appeared in the original `unexpected_token_in_expr` CPAN
//! bucket. They are aliases for punctuation variables and must be accepted as
//! typeglob names rather than routed through generic expression recovery.

mod cpan_test_helpers;
use cpan_test_helpers::assert_clean_parse;

#[test]
fn typeglob_backtick_name_parses() {
    assert_clean_parse("*STDOUT = *`;");
}

#[test]
fn typeglob_apostrophe_name_parses() {
    assert_clean_parse("*STDOUT = *';");
}

#[test]
fn punctuation_typeglobs_can_be_declared_together() {
    assert_clean_parse(
        r#"
*STDOUT = *`;
*STDERR = *';
"#,
    );
}
