mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{NodeKind, Parser};
use perl_tdd_support::must;

// Perl allows optional whitespace between `qw` and its opening delimiter.
// `qw [a b]`, `qw {a b}`, and `qw <a b>` are all valid in addition to the
// standard `qw(a b)` form. Previously the parser's QuoteWords handler in use
// statements failed to strip the gap, so the args were stored with the raw
// space intact and could not be correctly normalized.  Tracking: #10013.

fn use_node_args(source: &str) -> Result<Vec<String>, String> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected Program, got {:?}", ast.kind));
    };
    let statement = statements.first().ok_or("no statements")?;
    let NodeKind::Use { args, .. } = &statement.kind else {
        return Err(format!("expected Use node, got {:?}", statement.kind));
    };
    Ok(args.clone())
}

fn assert_normalized_qw(source: &str, expected: &str) -> Result<(), String> {
    let args = use_node_args(source)?;
    assert_eq!(args, vec![expected.to_string()], "unexpected use args for {source:?}");
    Ok(())
}

#[test]
fn use_constant_qw_bracket_delimiter_with_space() -> Result<(), String> {
    assert_normalized_qw("use constant qw [FOO BAR];", "qw(FOO BAR)")
}

#[test]
fn use_constant_qw_bracket_delimiter_no_space() -> Result<(), String> {
    assert_normalized_qw("use constant qw[FOO BAR];", "qw(FOO BAR)")
}

#[test]
fn use_constant_qw_paren_delimiter_with_space() -> Result<(), String> {
    assert_normalized_qw("use constant qw (FOO BAR);", "qw(FOO BAR)")
}

#[test]
fn use_constant_qw_brace_delimiter_with_space() -> Result<(), String> {
    assert_normalized_qw("use constant qw {FOO BAR};", "qw(FOO BAR)")
}

#[test]
fn use_constant_qw_angle_delimiter_with_space() -> Result<(), String> {
    assert_normalized_qw("use constant qw <FOO BAR>;", "qw(FOO BAR)")
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
