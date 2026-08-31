//! Regression coverage for builtin call paths that consume fat commas.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::{Node, NodeKind};

fn find_call<'a>(node: &'a Node, name: &str) -> Option<&'a [Node]> {
    if let NodeKind::FunctionCall { name: call_name, args } = &node.kind {
        if call_name == name {
            return Some(args);
        }
    }
    node.children().into_iter().find_map(|child| find_call(child, name))
}

fn assert_bareword_is_string(source: &str, call_name: &str, argument: usize, value: &str) {
    assert_clean_parse(source);
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = perl_tdd_support::must(parser.parse());
    let args = find_call(&ast, call_name).expect("expected builtin call");
    assert!(args.len() > argument, "expected argument {argument} in {source:?}");
    assert!(
        matches!(&args[argument].kind, NodeKind::String { value: actual, interpolated: false } if actual == value),
        "expected argument {argument} of {call_name} to be string {value:?}, got {:?}",
        args[argument].kind
    );
}

#[test]
fn parenthesized_print_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("print(before => 1);", "print", 0, "before");
}

#[test]
fn bare_print_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("print before => 1;", "print", 0, "before");
}

#[test]
fn block_list_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("map { $_ } before => 1;", "map", 1, "before");
}

#[test]
fn generic_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("push @items, before => 1;", "push", 1, "before");
}

#[test]
fn bless_builtin_autoquotes_bareword_before_fat_comma() {
    assert_bareword_is_string("bless {}, before => 1;", "bless", 1, "before");
}
