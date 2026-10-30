//! Regression proof for #16373: a declared bareword sub called with
//! space-separated arguments (`T `a`;`, `t "x";`, `T $fh "msg";`) must parse
//! as a function call, not surface as `UnexpectedSameLineResidue`.
//!
//! Ground truth: `t/base/lex.t:225` is `syntax OK` under perl
//! (5.42.2, matching pinned upstream b62845c); the parenthesized spelling
//! `T(`a`, $test++)` already parsed clean, so the gap is specifically
//! bareword-call argument uptake in the unparenthesized form.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::must_with;

fn find_function_call<'a>(node: &'a Node, name: &str) -> Option<(&'a str, usize)> {
    match &node.kind {
        NodeKind::FunctionCall { name: call_name, args } if call_name == name => {
            Some((call_name.as_str(), args.len()))
        }
        _ => node.children().into_iter().find_map(|child| find_function_call(child, name)),
    }
}

fn assert_parses_as_call(source: &str, name: &str, expected_args: usize) {
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    let found = find_function_call(&ast, name);
    assert_eq!(
        found,
        Some((name, expected_args)),
        "expected FunctionCall {name} with {expected_args} args for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
}

fn assert_valid_case(source: &str) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
}

#[test]
fn declared_bareword_call_with_backtick_string_argument_parses() {
    let source = "sub T { }\nT `a`;\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 1);
}

#[test]
fn lex_t_225_shape_parses_as_call_with_following_statement() {
    let source = "sub T { }\nmy $test = 0;\nT `^main:plink:53$`, $test++;\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 2);
}

#[test]
fn declared_bareword_call_with_double_quoted_argument_parses() {
    let source = "sub t { }\nt \"a\";\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "t", 1);
}

#[test]
fn declared_uppercase_bareword_call_with_sigiled_argument_parses() {
    let source = "sub T { }\nmy $msg = 'x';\nT $msg;\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 1);
}

#[test]
fn bareword_call_args_stop_at_statement_modifier_and_semicolon() {
    let source = "sub t { }\nt \"a\" if $x;\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "t", 1);
}

#[test]
fn declared_uppercase_bareword_call_with_double_quoted_argument_parses() {
    let source = "sub T { }\nT \"a\";\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 1);
}

#[test]
fn declared_lowercase_bareword_call_with_backtick_argument_parses() {
    let source = "sub t { }\nt `a`;\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "t", 1);
}

#[test]
fn declared_uppercase_bareword_call_with_q_operator_argument_parses() {
    let source = "sub T { }\nT q(a);\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 1);
}

#[test]
fn declared_uppercase_bareword_call_with_qq_operator_argument_parses() {
    let source = "sub T { }\nT qq(a);\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "T", 1);
}

#[test]
fn declared_lowercase_bareword_call_with_q_operator_argument_parses() {
    let source = "sub t { }\nt q(a);\n";
    assert_valid_case(source);
    assert_parses_as_call(source, "t", 1);
}
