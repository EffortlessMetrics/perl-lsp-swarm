mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::Parser;
use perl_tdd_support::must;

#[test]
fn braced_glob_deref_forms_parse_cleanly() {
    for source in ["*{$ref};", "*{$self->{key}};"] {
        assert_clean_parse(source);
    }
}

#[test]
fn braced_glob_deref_uses_last_expression_as_operand() {
    let source = "*{$tmp; 'STDOUT'};";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("unary_*{}"), "expected braced glob dereference: {sexp}");
    assert!(
        sexp.contains("(variable $ tmp)"),
        "expected preceding expression to be preserved: {sexp}"
    );
    assert!(sexp.contains("STDOUT"), "expected final expression operand: {sexp}");
}

#[test]
fn split_token_glob_body_preserves_multiple_expressions() {
    let source = "* { $tmp; 'STDOUT' };";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("unary_*{}"), "expected split-token glob dereference: {sexp}");
    assert!(sexp.contains("(variable $ tmp)"), "expected split-token prefix expression: {sexp}");
    assert!(sexp.contains("STDOUT"), "expected split-token final expression: {sexp}");
}

#[test]
fn split_token_glob_assignment_preserves_typeglob_lhs() {
    let source = "* { $name } = \\&target;";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("typeglob"), "expected Typeglob assignment lhs: {sexp}");
}

#[test]
fn braced_glob_postfix_form_remains_a_deref() {
    let source = "*{$glob}{CODE};";
    assert_clean_parse(source);
    assert_clean_parse("* { $glob }{CODE};");
}
