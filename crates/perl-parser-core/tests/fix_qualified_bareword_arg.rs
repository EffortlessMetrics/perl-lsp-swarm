//! Regression proof for the comp parse_recovery cluster (#4730):
//! upstream `t/comp/parser.t` has `is CORE::print::foo, 43, '...'`, a
//! bareword call whose first argument is a qualified bareword in plain
//! list position. The qualified-name admission arm previously required
//! `->`, `(`, or a sigil after the name, so the call never took arguments.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::must_with;

fn find_function_call<'a>(node: &'a Node, name: &str) -> Option<usize> {
    match &node.kind {
        NodeKind::FunctionCall { name: call_name, args } if call_name == name => Some(args.len()),
        _ => node.children().into_iter().find_map(|child| find_function_call(child, name)),
    }
}

fn assert_call_arg_count(source: &str, name: &str, expected_args: usize) {
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    assert_eq!(
        find_function_call(&ast, name),
        Some(expected_args),
        "expected FunctionCall {name} with {expected_args} args for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn qualified_bareword_as_bare_call_argument_parses() {
    assert_call_arg_count("is CORE::print::foo, 43;", "is", 2);
    assert_clean_parse("is Foo::bar::baz, 43;");
    assert_clean_parse("foo File::Spec, 1;");
    assert_clean_parse("is CORE::print, 43;");
    assert_no_blocking_diagnostics("is CORE::print::foo, 43;");
}

#[test]
fn qualified_bareword_paren_and_fat_arrow_arms_parse() {
    // RightParen arm: `(foo Foo::Bar)` failed on the parent commit.
    assert_call_arg_count("my $x = (foo Foo::Bar);", "foo", 1);
    assert_clean_parse("foo Foo::Bar => 1;");
}

#[test]
fn qualified_call_with_qualified_second_item_parses() {
    assert_call_arg_count("is Foo::Bar, Baz::Qux;", "is", 2);
}

#[test]
fn sort_comparator_with_plain_list_is_not_a_nested_call() {
    let source = "sort mycmp Foo::Bar, 1;";
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    assert_eq!(
        find_function_call(&ast, "mycmp"),
        None,
        "comparator must not become a call:\n{}",
        ast.to_sexp()
    );
    assert_clean_parse(source);
}

#[test]
fn sort_comparator_with_qualified_list_is_not_a_nested_call() {
    // Negative pin: `sort mycmp Foo::Bar, Baz::Qux` is a comparator plus
    // a qualified-bareword list. The comparator must not be promoted to
    // a call that swallows the list.
    let source = "sort mycmp Foo::Bar, Baz::Qux;";
    let mut parser = Parser::new(source);
    let ast = must_with(parser.parse(), format!("parse failed for {source:?}"));
    assert_eq!(
        find_function_call(&ast, "mycmp"),
        None,
        "comparator must not become a call:\n{}",
        ast.to_sexp()
    );
    assert_clean_parse(source);
}

#[test]
fn plain_lists_and_hashes_keep_distinct_elements() {
    assert_clean_parse("my @x = (foo, Foo::Bar, 1);");
    assert_clean_parse("my %h = (k => Foo::Bar, k2 => 1);");
    assert_clean_parse("return foo, Foo::Bar, 1;");
}

#[test]
fn qualified_forms_that_already_parsed_still_parse() {
    assert_clean_parse("my $x = Foo::bar;");
    assert_clean_parse("print Foo::bar;");
    assert_clean_parse("foo File::Spec->catfile('a');");
}
