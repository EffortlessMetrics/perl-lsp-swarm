//! Perl 5.44 (PPC0024): named parameters in signatures accept `=`, `//=`, and
//! `||=` default operators. These must parse cleanly without ERROR nodes.

mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn named_param_equals_default_parses() {
    assert_clean_parse("sub f (:$alpha = 1) { }");
}

#[test]
fn named_param_defined_or_default_parses() {
    assert_clean_parse("sub f (:$alpha //= 1) { }");
}

#[test]
fn named_param_logical_or_default_parses() {
    assert_clean_parse("sub f (:$alpha ||= 1) { }");
}

#[test]
fn mixed_named_default_operators_parse() {
    assert_clean_parse("sub configure ($host, :$port = 8080, :$secure //= 0, :$retries ||= 3) { }");
}

#[test]
fn method_named_param_default_operators_parse() {
    assert_clean_parse(
        r#"
use feature 'class';
class C {
    method m (:$alpha //= 1, :$beta ||= 2) { }
}
"#,
    );
}
