//! Regression coverage for Perl's postfix hash-slice dereference form.
//!
//! `EXPR->@{KEYS}` is the postfix equivalent of `@{EXPR}{KEYS}`. It is
//! distinct from both hash-element access (`EXPR->{KEY}`) and postfix array
//! slicing (`EXPR->@[INDICES]`), so the parser must retain a `HashSlice` node.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind};

fn count_hash_slices(node: &Node) -> usize {
    let mut count = usize::from(matches!(&node.kind, NodeKind::HashSlice { .. }));
    for child in node.children() {
        count += count_hash_slices(child);
    }
    count
}

fn assert_one_hash_slice(source: &str) {
    assert_clean_parse(source);
    let ast = parse(source);
    assert_eq!(
        count_hash_slices(&ast),
        1,
        "expected exactly one HashSlice for source:\n{source}\n\nAST:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn postfix_hash_slice_with_qw_keys() {
    assert_one_hash_slice("my @values = $href->@{qw(alpha beta)};");
}

#[test]
fn postfix_hash_slice_with_variable_keys() {
    assert_one_hash_slice("my @values = $href->@{@keys};");
}

#[test]
fn postfix_hash_slice_remains_an_lvalue() {
    assert_one_hash_slice("$href->@{qw(alpha beta)} = (1, 2);");
}

#[test]
fn postfix_hash_slice_after_chained_receiver() {
    assert_one_hash_slice("my @values = $object->{payload}->@{qw(alpha beta)};");
}

#[test]
fn neighboring_postfix_forms_keep_their_existing_nodes() {
    let source = "my @values = $aref->@[0, 2]; my %pairs = $href->%{qw(alpha beta)};";
    assert_clean_parse(source);
    let ast = parse(source);
    assert_eq!(
        count_hash_slices(&ast),
        0,
        "array and key/value postfix slices must not be reclassified as HashSlice: {}",
        ast.to_sexp()
    );
}
