//! Focused regression proof for #13906: the three-dot range operator is
//! contextual. Between operands `...` is the range operator (same rung,
//! spelling, and non-associativity as `..`); at term position it remains
//! the standalone yada-yada term.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::{must, must_some};

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

fn has_ellipsis(node: &Node) -> bool {
    if matches!(&node.kind, NodeKind::Ellipsis) {
        return true;
    }
    node.children().iter().any(|child| has_ellipsis(child))
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
fn three_dot_range_parses_as_binary_with_spelling_preserved() {
    assert_clean_parse("$a ... $b;");
    let ast = parse("$a ... $b;");
    let binary = must_some(first_binary(&ast));
    let (op, _, _) = must_some(binary_parts(binary));
    assert_eq!(op, "...", "three-dot range must keep its spelling:\n{}", ast.to_sexp());
    assert_clean_parse("my $x = $a ... $b;");
    assert_clean_parse("for my $i (1 ... 10) { }");
}

#[test]
fn three_dot_range_shares_the_range_rung_in_call_and_declaration_tails() {
    assert_clean_parse("print $a ... $b;");
    let ast = parse("print $a ... $b;");
    let binary = must_some(first_binary(&ast));
    let (op, _, _) = must_some(binary_parts(binary));
    assert_eq!(op, "...", "statement-call tail must admit the three-dot range:\n{}", ast.to_sexp());
}

#[test]
fn symbolic_operators_group_inside_three_dot_operands() {
    for op in ["||", "&&", "//", "&", "|", "^"] {
        assert_nested_binary(&format!("$a {op} $b ... $c;"), "...", NestedSide::Left, op);
        assert_nested_binary(&format!("$a ... $b {op} $c;"), "...", NestedSide::Right, op);
    }
}

#[test]
fn three_dot_range_stays_inside_ternary_condition_and_word_operators() {
    let source = "$a ... $b ? $c : $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let ternary = must_some(first_ternary(&ast));
    let NodeKind::Ternary { condition, .. } = &ternary.kind else {
        panic!("expected ternary");
    };
    let (condition_op, _, _) = must_some(binary_parts(condition));
    assert_eq!(
        condition_op,
        "...",
        "range must stay inside the ternary condition:\n{}",
        ast.to_sexp()
    );

    assert_nested_binary("$a or $b ... $c;", "or", NestedSide::Right, "...");
    assert_nested_binary("$a ... $b or $c;", "or", NestedSide::Left, "...");
}

#[test]
fn standalone_three_dots_remain_yada_yada() {
    assert_clean_parse("...;");
    let ast = parse("...;");
    assert!(has_ellipsis(&ast), "standalone `...` must stay Ellipsis:\n{}", ast.to_sexp());
    assert_clean_parse("sub f { ... }");
    let ast = parse("sub f { ... }");
    assert!(has_ellipsis(&ast), "yada-yada body must stay Ellipsis:\n{}", ast.to_sexp());
}

/// A chained range must not publish a clean folded AST: it surfaces a
/// blocking diagnostic or an Error/Missing node (the wording of the
/// same-level residue diagnostic is not this claim's contract).
fn assert_rejected(source: &str) {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = must(parser.parse());
    let blocking = parser.get_errors().iter().any(|error| error.blocks_clean_parse());
    let sexp = ast.to_sexp();
    let ast_error = sexp.contains("(error") || sexp.contains("(missing");
    assert!(
        blocking || ast_error,
        "expected a rejected chain for `{source}`:\n{sexp}\ndiagnostics: {:?}",
        parser.get_errors()
    );
}

#[test]
fn unparenthesized_mixed_range_chains_are_rejected() {
    assert_rejected("$a ... $b ... $c;");
    assert_rejected("$a .. $b ... $c;");
    assert_rejected("$a ... $b .. $c;");
    assert_rejected("$a || $b ... $c ... $d;");
}

#[test]
fn adjacent_ellipsis_is_not_a_range_endpoint() {
    // Perl rejects a second `...` where the range endpoint belongs; it is
    // not a yada-yada operand of the range.
    assert_rejected("$a ... ...;");
    assert_rejected("$a .. ...;");
}

#[test]
fn builtin_left_operands_keep_the_three_dot_operator() {
    // Nullary/optional-argument builtins end at the infix boundary, so the
    // range operator survives after them (Perl: `shift ... 3` is a range
    // whose left side is the shift call).
    for source in ["shift ... 3;", "my $x = shift ... 3;", "my $y = ref $a ... 3;"] {
        assert_clean_parse(source);
        let ast = parse(source);
        let binary = must_some(first_binary(&ast));
        let (op, _, _) = must_some(binary_parts(binary));
        assert_eq!(
            op,
            "...",
            "builtin range must keep its operator for `{source}`:\n{}",
            ast.to_sexp()
        );
    }
}

#[test]
fn parenthesized_three_dot_nesting_is_accepted() {
    assert_nested_binary("my $x = ($a ... $b) ... $c;", "...", NestedSide::Left, "...");
    assert_nested_binary("my $x = $a ... ($b ... $c);", "...", NestedSide::Right, "...");
    assert_nested_binary("my $x = ($a .. $b) ... $c;", "...", NestedSide::Left, "..");
    assert_nested_binary("my $x = ($a ... $b) .. $c;", "..", NestedSide::Left, "...");
}
