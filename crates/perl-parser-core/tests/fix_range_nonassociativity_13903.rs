//! Focused regression proof for #13903: the two-dot range operator is
//! non-associative. One unparenthesized range parses cleanly, an adjacent
//! second `..` at the same precedence level is a syntax error, and explicit
//! parentheses remain the only way to nest ranges.
//!
//! Precedence-boundary controls keep #13128's symbolic-operator ordering
//! intact.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, assert_has_error, parse};
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
fn single_range_still_parses_cleanly() {
    assert_clean_parse("my @r = (1 .. 3);");
    assert_clean_parse("my $x = $a .. $b;");
    assert_clean_parse("for my $i (1 .. 10) { }");
}

#[test]
fn parenthesized_range_nesting_is_accepted_on_both_sides() {
    // `(1 .. 2) .. 3` — outer range whose left operand is a range.
    assert_nested_binary("my $x = (1 .. 2) .. 3;", "..", NestedSide::Left, "..");
    // `1 .. (2 .. 3)` — outer range whose right operand is a range.
    assert_nested_binary("my $x = 1 .. (2 .. 3);", "..", NestedSide::Right, "..");
}

#[test]
fn unparenthesized_chained_range_is_not_a_clean_parse() {
    // Perl 5.40.1: `1 .. 2 .. 3;` is a syntax error near `2 ..`. The chained
    // form must surface a blocking diagnostic and/or Error/Missing node — it
    // must not silently publish the pre-#13903 clean left-folded AST.
    assert_has_error("my $x = $a .. $b .. $c;", "range");
    assert_has_error("1 .. 2 .. 3;", "range");
}

#[test]
fn symbolic_operators_remain_inside_range_operands() {
    // #13128 precedence boundary: symbolic bitwise/logical operators stay
    // inside range operands after the non-associativity change.
    assert_nested_binary("$a || $b .. $c;", "..", NestedSide::Left, "||");
    assert_nested_binary("$a .. $b || $c;", "..", NestedSide::Right, "||");
    assert_nested_binary("$a & $b .. $c;", "..", NestedSide::Left, "&");
    assert_nested_binary("$a .. $b & $c;", "..", NestedSide::Right, "&");
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
        "range must remain inside the ternary condition:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn chained_range_after_symbolic_operator_is_still_rejected() {
    // The second `..` must not sneak back in through a symbolic-operator
    // operand at the same precedence level.
    assert_has_error("$a || $b .. $c .. $d;", "range");
}
