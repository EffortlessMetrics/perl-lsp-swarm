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

fn use_node_args_for_module(source: &str, module: &str) -> Result<Vec<String>, String> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected Program, got {:?}", ast.kind));
    };
    let statement = statements
        .iter()
        .find(|statement| matches!(&statement.kind, NodeKind::Use { module: name, .. } if name == module))
        .ok_or_else(|| format!("no use statement for {module}"))?;
    let NodeKind::Use { args, .. } = &statement.kind else {
        return Err(format!("expected Use node for {module}, got {:?}", statement.kind));
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
fn use_constant_qw_bracket_in_full_package() -> Result<(), String> {
    let source = "package My::Config;\nuse constant qw [HTTP_OK HTTP_NOT_FOUND];\n1;\n";
    assert_clean_parse(source);
    assert_eq!(use_node_args_for_module(source, "constant")?, vec!["qw(HTTP_OK HTTP_NOT_FOUND)"],);
    Ok(())
}

#[test]
fn use_parent_qw_bracket_delimiter_with_space() -> Result<(), String> {
    let source = "use parent qw [Foo::Bar Other::Base];";
    assert_clean_parse(source);
    assert_eq!(use_node_args(source)?, vec!["qw(Foo::Bar Other::Base)"]);
    Ok(())
}

#[test]
fn use_warnings_qw_bracket_with_space() -> Result<(), String> {
    let source = "use warnings qw [all];";
    assert_clean_parse(source);
    assert_eq!(use_node_args(source)?, vec!["qw(all)"]);
    Ok(())
}

#[test]
fn spaced_qw_preserves_hash_words_and_following_words() -> Result<(), String> {
    let source = "use Module qw [foo #tag bar];";
    assert_clean_parse(source);
    assert_eq!(use_node_args(source)?, vec!["qw(foo #tag bar)"]);
    Ok(())
}
