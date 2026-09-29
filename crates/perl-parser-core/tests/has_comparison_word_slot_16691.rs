//! Comparison-word autoquote in the `has <name> =>` slot (#16691).
//!
//! Residual after #16639 / PR #16685: keyword/word-operator admission does not
//! cover `TokenKind::StringCompare` (`cmp`) or Identifier comparison words
//! (`eq`/`ne`/`lt`/`le`/`gt`/`ge`) when argument collection would otherwise
//! treat them as infix operators.
//!
//! Ground truth: Perl autoquotes any bareword before `=>`, including
//! comparison words. `has` is an ordinary imported sub, so `has cmp => 1` is
//! `has("cmp", 1)` while `has eq $x` and `$a cmp $b` remain comparisons.

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

fn find_kind<F>(node: &Node, predicate: F) -> bool
where
    F: Fn(&NodeKind) -> bool + Copy,
{
    predicate(&node.kind) || node.children().into_iter().any(|child| find_kind(child, predicate))
}

fn autoquoted_keys(args: &[Node]) -> Vec<&str> {
    args.iter().filter_map(autoquoted_key).collect()
}

fn assert_has_autoquoted_keys(source: &str, expected: &[&str]) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "has", source);
    assert_eq!(
        autoquoted_keys(args),
        expected,
        "autoquoted has keys for {source:?}:\n{}",
        ast.to_sexp()
    );
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

fn assert_has_leading_block_then_keys(source: &str, expected: &[&str]) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "has", source);
    let first =
        must_some_with(args.first(), format!("expected a leading block argument in {source:?}"));
    assert!(
        matches!(first.kind, NodeKind::Block { .. }),
        "first argument of {source:?} must be the block, got:\n{}",
        ast.to_sexp()
    );
    let after_block: Vec<&str> = args.iter().skip(1).filter_map(autoquoted_key).collect();
    assert_eq!(
        after_block,
        expected,
        "autoquoted keys after the block in {source:?}:\n{}",
        ast.to_sexp()
    );
}

fn assert_hash_autoquoted_key(source: &str, keyword: &str) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let found = find_kind(
        &ast,
        |kind| matches!(kind, NodeKind::String { value, interpolated: false } if value == keyword),
    );
    assert!(
        found,
        "{source:?} must autoquote `{keyword}` as a non-interpolated string:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn has_cmp_string_compare_binds_the_attribute_slot() {
    assert_has_keyword_slot("has cmp => 1;", "cmp");
}

#[test]
fn has_eq_identifier_binds_the_attribute_slot() {
    assert_has_keyword_slot("has eq => 1;", "eq");
}

#[test]
fn has_ne_lt_le_gt_ge_identifier_slots_bind() {
    assert_has_keyword_slot("has ne => 1;", "ne");
    assert_has_keyword_slot("has lt => 1;", "lt");
    assert_has_keyword_slot("has le => 1;", "le");
    assert_has_keyword_slot("has gt => 1;", "gt");
    assert_has_keyword_slot("has ge => 1;", "ge");
}

#[test]
fn fat_arrow_admission_does_not_depend_on_surrounding_whitespace() {
    assert_has_keyword_slot("has cmp=>1;", "cmp");
    assert_has_keyword_slot("has eq\n=> 1;", "eq");
}

#[test]
fn admission_is_not_has_specific() {
    let source = "before eq => sub { 1 };";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "before", source);
    let first = must_some_with(args.first(), "before eq => must have a first argument");
    assert_eq!(autoquoted_key(first), Some("eq"));
}

#[test]
fn parenthesized_has_cmp_binds_in_expression_context() {
    assert_has_keyword_slot("has(cmp => 1);", "cmp");
}

#[test]
fn quoted_has_cmp_already_binds_as_a_string_argument() {
    let source = "has 'cmp' => 1;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    let (_, args) = require_call_args(&ast, "has", source);
    let first = must_some_with(args.first(), "quoted has 'cmp' must have a first argument");
    assert_eq!(
        autoquoted_key(first),
        Some("'cmp'"),
        "quoted has 'cmp' keeps the quoted spelling as the first argument:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn later_comparison_word_args_remain_autoquoted_slots() {
    assert_has_autoquoted_keys("has commands => 1, cmp => 2;", &["commands", "cmp"]);
    assert_has_autoquoted_keys("has commands => 1, eq => 2;", &["commands", "eq"]);
    assert_has_autoquoted_keys("has foo => 1, lt => 2;", &["foo", "lt"]);
}

#[test]
fn after_block_comparison_word_args_remain_autoquoted_slots() {
    assert_has_leading_block_then_keys("has { 1 } cmp => 2;", &["cmp"]);
    assert_has_leading_block_then_keys("has { 1 } eq => 2;", &["eq"]);
    assert_has_leading_block_then_keys("has { 1 } ge => 2;", &["ge"]);
}

#[test]
fn after_block_eq_without_fat_arrow_stays_a_comparison() {
    let source = "has { 1 } eq $x;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "eq")),
        "has {{ 1 }} eq $x must remain a comparison:\n{}",
        ast.to_sexp()
    );
    let (_, args) = require_call_args(&ast, "has", source);
    assert!(
        !args.iter().any(|arg| autoquoted_key(arg) == Some("eq")),
        "has {{ 1 }} eq $x must not autoquote `eq` without =>:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn after_block_cmp_without_fat_arrow_stays_a_comparison() {
    let source = "has { 1 } cmp $x;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "cmp")),
        "has {{ 1 }} cmp $x must remain a comparison:\n{}",
        ast.to_sexp()
    );
    let (_, args) = require_call_args(&ast, "has", source);
    assert!(
        !args.iter().any(|arg| autoquoted_key(arg) == Some("cmp")),
        "has {{ 1 }} cmp $x must not autoquote `cmp` without =>:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn hash_constructor_autoquotes_comparison_words() {
    assert_hash_autoquoted_key("my %h = (cmp => 1);", "cmp");
    assert_hash_autoquoted_key("my %h = (eq => 1);", "eq");
    assert_hash_autoquoted_key("my %h = (ne => 1, lt => 2);", "ne");
    assert_hash_autoquoted_key("my %h = (ne => 1, lt => 2);", "lt");
}

#[test]
fn has_eq_without_fat_arrow_stays_a_comparison() {
    let source = "has eq $x;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "eq")),
        "has eq $x must remain a comparison:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_named_call(&ast, "has").is_none()
            || require_call_args(&ast, "has", source)
                .1
                .iter()
                .all(|arg| autoquoted_key(arg) != Some("eq")),
        "has eq $x must not autoquote `eq` as a list-operator argument:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn scalar_cmp_without_fat_arrow_stays_a_comparison() {
    let source = "$a cmp $b;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "cmp")),
        "$a cmp $b must remain a comparison:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn has_cmp_without_fat_arrow_stays_a_comparison() {
    let source = "has cmp $x;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "cmp")),
        "has cmp $x must remain a comparison:\n{}",
        ast.to_sexp()
    );
    assert!(
        find_named_call(&ast, "has").is_none()
            || require_call_args(&ast, "has", source)
                .1
                .iter()
                .all(|arg| autoquoted_key(arg) != Some("cmp")),
        "has cmp $x must not autoquote `cmp` as a list-operator argument:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn later_eq_without_fat_arrow_stays_a_comparison() {
    let source = "has commands => 1 eq $x;";
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
    let ast = parse_source(source);
    assert!(
        find_kind(&ast, |kind| matches!(kind, NodeKind::Binary { op, .. } if op == "eq")),
        "{source:?} must keep `eq` as a comparison:\n{}",
        ast.to_sexp()
    );
    let (_, args) = require_call_args(&ast, "has", source);
    assert!(
        !args.iter().any(|arg| autoquoted_key(arg) == Some("eq")),
        "{source:?} must not autoquote `eq` without =>:\n{}",
        ast.to_sexp()
    );
}
