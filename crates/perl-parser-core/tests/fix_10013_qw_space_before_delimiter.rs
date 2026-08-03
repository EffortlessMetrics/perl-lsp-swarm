mod cpan_test_helpers;
use cpan_test_helpers::*;

// Perl allows optional whitespace between `qw` and its opening delimiter.
// `qw [a b]`, `qw {a b}`, and `qw <a b>` are all valid in addition to the
// standard `qw(a b)` form. Previously the parser's QuoteWords handler in use
// statements failed to strip the gap, so the args were stored with the raw
// space intact and could not be correctly normalized.  Tracking: #10013.

#[test]
fn use_constant_qw_bracket_delimiter_with_space() {
    assert_clean_parse("use constant qw [FOO BAR];");
}

#[test]
fn use_constant_qw_bracket_delimiter_no_space() {
    assert_clean_parse("use constant qw[FOO BAR];");
}

#[test]
fn use_constant_qw_paren_delimiter_with_space() {
    assert_clean_parse("use constant qw (FOO BAR);");
}

#[test]
fn use_constant_qw_brace_delimiter_with_space() {
    assert_clean_parse("use constant qw {FOO BAR};");
}

#[test]
fn use_constant_qw_angle_delimiter_with_space() {
    assert_clean_parse("use constant qw <FOO BAR>;");
}

#[test]
fn use_constant_qw_bracket_in_full_package() {
    assert_clean_parse("package My::Config;\nuse constant qw [HTTP_OK HTTP_NOT_FOUND];\n1;\n");
}

#[test]
fn use_parent_qw_bracket_delimiter_with_space() {
    assert_clean_parse("use parent qw [Foo::Bar Other::Base];");
}

#[test]
fn use_warnings_qw_bracket_with_space() {
    assert_clean_parse("use warnings qw [all];");
}
