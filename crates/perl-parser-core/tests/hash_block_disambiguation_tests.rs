//! Hash/block literal disambiguation tests.
//!
//! Perl uses `{}` for both anonymous hash references and code blocks.
//! The parser must distinguish between them based on context. This file
//! covers hash literals, hash references, block expressions, and the
//! ambiguous edges where the two overlap.

mod cpan_test_helpers;
use cpan_test_helpers::*;

// ── Hash literals ──────────────────────────────────────────────────

#[test]
fn hash_literal_fat_comma_pairs() {
    assert_clean_parse("my %h = (a => 1, b => 2);");
}

#[test]
fn hash_ref_fat_comma_pairs() {
    assert_clean_parse("my $href = { a => 1, b => 2 };");
}

#[test]
fn empty_hash_ref() {
    assert_clean_parse("my $href = {};");
}

#[test]
fn hash_ref_assignment_via_arrow() {
    assert_clean_parse("$obj->{config} = { debug => 1, level => 2 };");
}

// ── Nested hashes ──────────────────────────────────────────────────

#[test]
fn nested_hash_ref() {
    assert_clean_parse("my $h = { a => { b => 1 } };");
}

#[test]
fn hash_with_nested_hash_ref_value() {
    assert_clean_parse("my %h = (key => { nested => 'val' });");
}

// ── Block expressions ──────────────────────────────────────────────

#[test]
fn do_block() {
    assert_clean_parse("do { my $x = 1; $x + 2 };");
}

#[test]
fn eval_block() {
    assert_clean_parse("eval { die 'test' };");
}

#[test]
fn sort_block() {
    assert_clean_parse("sort { $a <=> $b } @list;");
}

// ── Ambiguous cases ────────────────────────────────────────────────

#[test]
fn block_with_statement_inside() {
    // Braces after assignment with a statement inside: code block, not hash.
    assert_clean_parse("my $r = { print 'hello' };");
}

#[test]
fn unary_plus_forces_hash_ref() {
    // Leading `+` disambiguates in favour of an anonymous hash reference.
    assert_clean_parse("+{ key => 'val' };");
}

#[test]
fn unary_plus_forces_singleton_hash_ref() {
    use perl_parser_core::{Node, NodeKind};

    fn find_unary(node: &Node) -> Option<&Node> {
        if matches!(&node.kind, NodeKind::Unary { op, .. } if op == "+") {
            return Some(node);
        }
        node.children().into_iter().find_map(find_unary)
    }

    for (source, key_text) in [("my $h = +{1};", "1"), ("my $h = +{2.5};", "2.5")] {
        let ast = parse(source);
        let unary = find_unary(&ast).unwrap_or_else(|| panic!("missing unary plus: {source}"));
        let NodeKind::Unary { operand, .. } = &unary.kind else { unreachable!() };
        let NodeKind::HashLiteral { pairs } = &operand.kind else {
            panic!("expected hash operand for {source}: {}", ast.to_sexp());
        };
        assert_eq!(pairs.len(), 1, "expected one hash pair for {source}");
        assert!(matches!(pairs[0].0.kind, NodeKind::Number { .. }));
        assert_eq!(&source[pairs[0].0.location.start..pairs[0].0.location.end], key_text);
        assert!(matches!(pairs[0].1.kind, NodeKind::Undef));
        assert_eq!(
            &source[operand.location.start..operand.location.end],
            format!("{{{key_text}}}")
        );
    }

    // qx is represented by String too, but can return multiple list elements.
    // Its unresolved representation must not acquire a fabricated undef pair.
    let qx_ast = parse("my $h = +{qx(foo)};");
    let qx_unary = find_unary(&qx_ast).unwrap_or_else(|| panic!("missing qx unary plus"));
    let NodeKind::Unary { operand, .. } = &qx_unary.kind else { unreachable!() };
    match &operand.kind {
        NodeKind::Block { .. } => {} // Existing unresolved representation.
        NodeKind::HashLiteral { pairs } => assert!(
            !pairs.iter().any(|(key, value)| matches!(key.kind, NodeKind::String { .. })
                && matches!(value.kind, NodeKind::Undef)),
            "qx must not be treated as one scalar"
        ),
        other => panic!("unexpected qx operand: {other:?}"),
    }

    let block = parse("map { 1 } @items;").to_sexp();
    assert!(block.contains("(block"), "map must retain its block: {block}");
}
