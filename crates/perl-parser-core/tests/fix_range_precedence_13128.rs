mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::must_some;

#[derive(Clone, Copy)]
enum NestedSide {
    Left,
    Right,
}

fn first_binary(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Binary { .. }) {
        return Some(node);
    }

    for child in node.children() {
        if let Some(binary) = first_binary(child) {
            return Some(binary);
        }
    }

    None
}

fn binary_parts(node: &Node) -> Option<(&str, &Node, &Node)> {
    match &node.kind {
        NodeKind::Binary { op, left, right } => Some((op.as_str(), left.as_ref(), right.as_ref())),
        _ => None,
    }
}

fn first_ternary(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Ternary { .. }) {
        return Some(node);
    }

    for child in node.children() {
        if let Some(ternary) = first_ternary(child) {
            return Some(ternary);
        }
    }

    None
}

fn ternary_condition(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::Ternary { condition, .. } => Some(condition.as_ref()),
        _ => None,
    }
}

fn assert_nested_binary(source: &str, outer_op: &str, nested_side: NestedSide, nested_op: &str) {
    assert_clean_parse(source);
    let ast = parse(source);
    let outer = must_some(first_binary(&ast));
    let (actual_outer, left, right) = must_some(binary_parts(outer));
    assert_eq!(
        actual_outer,
        outer_op,
        "unexpected outer operator for `{source}`:\n{}",
        ast.to_sexp()
    );

    let nested = match nested_side {
        NestedSide::Left => left,
        NestedSide::Right => right,
    };
    let (actual_nested, _, _) = must_some(binary_parts(nested));
    assert_eq!(
        actual_nested,
        nested_op,
        "unexpected nested operator for `{source}`:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn symbolic_logical_and_bitwise_ops_bind_inside_left_range_operand() {
    for (source, nested_op) in [
        ("$a & $b .. $c;", "&"),
        ("$a ^ $b .. $c;", "^"),
        ("$a | $b .. $c;", "|"),
        ("$a && $b .. $c;", "&&"),
        ("$a || $b .. $c;", "||"),
        ("$a // $b .. $c;", "//"),
    ] {
        assert_nested_binary(source, "..", NestedSide::Left, nested_op);
    }
}

#[test]
fn symbolic_logical_and_bitwise_ops_bind_inside_right_range_operand() {
    for (source, nested_op) in [
        ("$a .. $b & $c;", "&"),
        ("$a .. $b ^ $c;", "^"),
        ("$a .. $b | $c;", "|"),
        ("$a .. $b && $c;", "&&"),
        ("$a .. $b || $c;", "||"),
        ("$a .. $b // $c;", "//"),
    ] {
        assert_nested_binary(source, "..", NestedSide::Right, nested_op);
    }
}

#[test]
fn declaration_tail_uses_the_same_range_precedence() {
    assert_nested_binary("(my $x || $fallback .. $end);", "..", NestedSide::Left, "||");
}

#[test]
fn range_remains_inside_ternary_condition() {
    let source = "$a .. $b ? $c : $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let ternary = must_some(first_ternary(&ast));
    let condition = must_some(ternary_condition(ternary));
    let (condition_op, _, _) = must_some(binary_parts(condition));
    assert_eq!(
        condition_op,
        "..",
        "range must bind above ternary for `{source}`:\n{}",
        ast.to_sexp()
    );
}
