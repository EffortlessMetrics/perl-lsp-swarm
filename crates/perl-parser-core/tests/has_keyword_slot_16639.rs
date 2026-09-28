//! Keyword-slot admission after Moose/Moo/Mojo-style `has`.
//!
//! #16639: `has <keyword> => ...` must parse as the same list-operator call as
//! `has commands => ...`, including builtin identifiers (`log`) and parser
//! keywords (`class`, `method`, `format`). Existing `assert_clean_parse` tests
//! in `fix_fat_arrow_builtin_autoquote.rs` did not catch `UnexpectedSameLineResidue`
//! diagnostics, so `has log =>` still failed `perllsp --check` on main.
//!
//! Ground truth: Perl autoquotes any bareword before `=>`, including reserved
//! words. `has` is an ordinary imported sub, so `has class => (is => 'rw')`
//! is `has("class", is => "rw")`.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_no_blocking_diagnostics};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::{must_some_with, must_with};

fn parse_source(source: &str) -> Node {
    let mut parser = Parser::new(source);
    must_with(parser.parse(), format!("parse failed for {source:?}"))
}

fn find_named_call<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    match &node.kind {
        NodeKind::FunctionCall { name: call_name, .. } if call_name == name => Some(node),
        _ => node.children().into_iter().find_map(|child| find_named_call(child, name)),
    }
}

fn call_args(node: &Node) -> Option<(&str, &[Node])> {
    match &node.kind {
        NodeKind::FunctionCall { name, args } => Some((name.as_str(), args)),
        _ => None,
    }
}

fn autoquoted_key(arg: &Node) -> Option<&str> {
    match &arg.kind {
        NodeKind::String { value, interpolated: false } => Some(value.as_str()),
        _ => None,
    }
}

fn require_call_args<'a>(ast: &'a Node, name: &str, source: &str) -> (&'a str, &'a [Node]) {
    let call = must_some_with(
        find_named_call(ast, name),
        format!("expected FunctionCall {name} for {source:?}:\n{}", ast.to_sexp()),
    );
    must_some_with(call_args(call), format!("expected FunctionCall shape for {name} in {source:?}"))
}

fn assert_has_keyword_slot(source: &str, keyword: &str) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse_source(source);
    let (name, args) = require_call_args(&ast, "has", source);
    assert_eq!(name, "has");
    let first = must_some_with(
        args.first(),
        format!(
            "has {keyword} => must bind the autoquoted name and a value, got {} args:\n{}\n{source}",
            args.len(),
            ast.to_sexp()
        ),
    );
    assert!(
        args.len() >= 2,
        "has {keyword} => must bind the autoquoted name and a value, got {} args:\n{}\n{source}",
        args.len(),
        ast.to_sexp()
    );
    assert_eq!(
        autoquoted_key(first),
        Some(keyword),
        "first argument of has must be autoquoted `{keyword}`:\n{}\n{source}",
        ast.to_sexp()
    );
}

fn find_kind<F>(node: &Node, predicate: F) -> bool
where
    F: Fn(&NodeKind) -> bool + Copy,
{
    predicate(&node.kind) || node.children().into_iter().any(|child| find_kind(child, predicate))
}

#[test]
fn has_commands_control_still_binds_the_attribute_slot() {
    assert_has_keyword_slot("has commands => 1;", "commands");
}

#[test]
fn has_log_builtin_identifier_already_binds_the_attribute_slot() {
    assert_has_keyword_slot("has log => sub { };", "log");
}

#[test]
fn has_class_keyword_binds_the_attribute_slot() {
    assert_has_keyword_slot("has class => (is => qw(rw));", "class");
}

#[test]
fn has_method_keyword_binds_the_attribute_slot() {
    assert_has_keyword_slot("has method => 1;", "method");
}

#[test]
fn has_format_keyword_binds_the_attribute_slot() {
    assert_has_keyword_slot("has format => undef;", "format");
}

#[test]
fn reported_mojo_catalyst_fixture_has_no_residue() {
    let source = r#"package Dummy;
has log => sub { };
has format => undef;
has class => (is => qw(rw));
has method => 1;
has commands => 1;
1;
"#;
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);

    let ast = parse_source(source);
    let mut names = Vec::new();
    fn collect_has_keys(node: &Node, names: &mut Vec<String>) {
        if let NodeKind::FunctionCall { name, args } = &node.kind
            && name == "has"
            && let Some(key) = args.first().and_then(autoquoted_key)
        {
            names.push(key.to_string());
        }
        for child in node.children() {
            collect_has_keys(child, names);
        }
    }
    collect_has_keys(&ast, &mut names);
    assert_eq!(names, ["log", "format", "class", "method", "commands"]);
}

#[test]
fn admission_is_not_an_allowlist_of_the_reported_keywords() {
    assert_has_keyword_slot("has if => 1;", "if");
    assert_has_keyword_slot("has unless => 1;", "unless");
    assert_has_keyword_slot("has when => 1;", "when");
    assert_has_keyword_slot("has sub => 1;", "sub");
    assert_has_keyword_slot("has package => 1;", "package");
    assert_has_keyword_slot("has return => 1;", "return");
    assert_has_keyword_slot("has and => 1;", "and");
    assert_has_keyword_slot("has or => 1;", "or");
    assert_has_keyword_slot("has not => 1;", "not");
    assert_has_keyword_slot("has xor => 1;", "xor");
    assert_has_keyword_slot("has undef => 1;", "undef");
}

#[test]
fn fat_arrow_admission_does_not_depend_on_surrounding_whitespace() {
    assert_has_keyword_slot("has class=>1;", "class");
    assert_has_keyword_slot("has class\n=> 1;", "class");
}

#[test]
fn admission_is_not_has_specific() {
    let source = "before class => sub { 1 };";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "before", source);
    let first = must_some_with(args.first(), "before class => must have a first argument");
    assert_eq!(autoquoted_key(first), Some("class"));
}

#[test]
fn parenthesized_has_class_already_binds_in_expression_context() {
    assert_has_keyword_slot("has(class => 1);", "class");
}

#[test]
fn quoted_has_class_already_binds_as_a_string_argument() {
    let source = "has 'class' => 1;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "has", source);
    let first = must_some_with(args.first(), "quoted has 'class' must have a first argument");
    assert_eq!(
        autoquoted_key(first),
        Some("'class'"),
        "quoted has 'class' keeps the quoted spelling as the first argument:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn class_declaration_is_not_eaten_as_a_has_argument() {
    let source = "class Foo { method bar () { 1 } }";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Class { .. })),
        "class Foo must remain a class declaration:\n{}",
        ast.to_sexp()
    );
    assert!(find_named_call(&ast, "has").is_none());
}

#[test]
fn method_declaration_is_not_eaten_as_a_has_argument() {
    let source = "class Foo { method bar () { 1 } }";
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Method { .. })),
        "method bar must remain a method declaration:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn format_declaration_is_not_eaten_as_a_has_argument() {
    let source = "format STDOUT =\n@<<<\n$text\n.\n";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Format { .. })),
        "format STDOUT must remain a format declaration:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn keyword_without_fat_arrow_is_not_admitted_as_the_attribute_slot() {
    let source = "has class Foo { method x () { 1 } }";
    let ast = parse_source(source);
    assert!(
        find_named_call(&ast, "has").is_none(),
        "has class Foo must not parse `has` as a list-operator call:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Identifier { name } if name == "has")),
        "has class Foo must leave `has` as an identifier:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Class { .. })),
        "has class Foo must still parse `class Foo` as a class declaration:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn has_if_paren_is_not_an_attribute_slot() {
    let source = "has if (1) { 1 }";
    let ast = parse_source(source);
    assert!(
        find_named_call(&ast, "has").is_none(),
        "has if (1) must not parse `has` as a list-operator call:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(
            &ast,
            |kind| matches!(kind, NodeKind::StatementModifier { modifier, .. } if modifier == "if")
        ),
        "has if (1) must keep `if` as a statement modifier:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn has_or_die_is_not_an_attribute_slot() {
    let source = "has or die;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_named_call(&ast, "has").is_none(),
        "has or die must not parse `has` as a list-operator call:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "or")),
        "has or die must keep `or` as a word operator:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Identifier { name } if name == "has")),
        "has or die must leave `has` as the left operand:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn has_undef_without_fat_arrow_stays_the_undef_value() {
    let source = "has undef;";
    let ast = parse_source(source);
    assert!(
        find_named_call(&ast, "has").is_none(),
        "has undef without => must not parse `has` as a list-operator call:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Identifier { name } if name == "has")),
        "has undef without => must leave `has` as an identifier:\n{}",
        ast.to_sexp()
    );
    assert!(
        !find_kind(
            &ast,
            |kind| matches!(kind, NodeKind::String { value, interpolated: false } if value == "undef")
        ),
        "has undef without => must not autoquote `undef` into a string key:\n{}",
        ast.to_sexp()
    );
}
