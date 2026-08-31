//! Regression coverage for builtin call paths that consume fat commas.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::{must, must_some};

fn find_call<'a>(node: &'a Node, name: &str) -> Option<&'a [Node]> {
    if let NodeKind::FunctionCall { name: call_name, args } = &node.kind
        && call_name == name
    {
        return Some(args);
    }
    node.children().into_iter().find_map(|child| find_call(child, name))
}

fn call_argument_kind(source: &str, call_name: &str, argument: usize) -> NodeKind {
    assert_clean_parse(source);
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = must(parser.parse());
    let args = must_some(find_call(&ast, call_name));
    must_some(args.get(argument)).kind.clone()
}

fn assert_bareword_is_string(source: &str, call_name: &str, argument: usize, value: &str) {
    let kind = call_argument_kind(source, call_name, argument);
    assert!(
        matches!(&kind, NodeKind::String { value: actual, interpolated: false } if actual == value),
        "expected argument of {call_name} to be string {value:?}, got {:?}",
        kind
    );
}

fn assert_bareword_is_identifier(source: &str, call_name: &str, argument: usize, value: &str) {
    let kind = call_argument_kind(source, call_name, argument);
    assert!(
        matches!(&kind, NodeKind::Identifier { name } if name == value),
        "expected argument of {call_name} to remain identifier {value:?}, got {:?}",
        kind
    );
}

#[test]
fn parenthesized_print_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my $result = print(before => 1);", "print", 0, "before");
}

#[test]
fn bare_print_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my $result = print before => 1;", "print", 0, "before");
}

#[test]
fn block_list_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my @result = map { $_ } before => 1;", "map", 1, "before");
}

#[test]
fn generic_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my $result = push @items, before => 1;", "push", 1, "before");
}

#[test]
fn bless_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my $result = bless before => 1;", "bless", 0, "before");
}

#[test]
fn expression_filehandle_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("my $result = print { $fh } before => 1;", "print", 1, "before");
}

#[test]
fn plain_comma_does_not_autoquote_a_bareword() {
    assert_bareword_is_identifier("my $result = push @items, before, 1;", "push", 1, "before");
}

#[test]
fn fat_comma_does_not_rewrite_a_variable_operand() {
    let kind = call_argument_kind("my $result = push @items, $before => 1;", "push", 1);
    assert!(
        matches!(
            &kind,
            NodeKind::Variable { sigil, name } if sigil == "$" && name == "before"
        ),
        "expected variable operand to remain unchanged, got {:?}",
        kind
    );
}

#[test]
fn fat_comma_does_not_rewrite_an_expression_operand() {
    assert!(matches!(
        &call_argument_kind("my $result = push @items, before() => 1;", "push", 1),
        NodeKind::FunctionCall { name, .. } if name == "before"
    ));
}
