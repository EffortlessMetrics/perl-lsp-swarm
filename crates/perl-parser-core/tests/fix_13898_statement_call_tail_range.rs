#![allow(clippy::panic)] // Shape assertions report the actual AST kind on mismatch.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

fn expression_statement(ast: &Node) -> &Node {
    let NodeKind::Program { statements } = &ast.kind else {
        panic!("expected Program, got {:?}", ast.kind);
    };
    let Some(statement) = statements.first() else {
        panic!("expected one expression statement");
    };
    match &statement.kind {
        NodeKind::ExpressionStatement { expression } => expression,
        NodeKind::Binary { .. } => statement,
        other => panic!("expected ExpressionStatement or Binary, got {:?}", other),
    }
}

fn assert_function_call(node: &Node, name: &str, args_len: usize, context: &str) {
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { name: actual, args } if actual == name && args.len() == args_len),
        "expected {name} call with {args_len} argument(s) for {context}, got {:?}",
        node.kind
    );
}

fn assert_range_with_nested_symbolic(
    source: &str,
    builtin: Option<(&str, usize)>,
    nested_side: &str,
    nested_op: &str,
) {
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    let NodeKind::Binary { op, left, right } = &expression.kind else {
        panic!("expected outer range for `{source}`, got {:?}", expression.kind);
    };
    assert_eq!(op, "..", "expected range outer for `{source}`:\n{}", ast.to_sexp());

    let nested = match nested_side {
        "left" => left,
        "right" => right,
        other => panic!("unexpected nested side {other}"),
    };
    let NodeKind::Binary { op, left: nested_left, .. } = &nested.kind else {
        panic!("expected nested {nested_op} for `{source}`, got {:?}", nested.kind);
    };
    assert_eq!(op, nested_op, "unexpected nested operator for `{source}`:\n{}", ast.to_sexp());
    if let Some((builtin, args_len)) = builtin {
        assert_function_call(
            if nested_side == "left" { nested_left } else { left },
            builtin,
            args_len,
            source,
        );
    }
}

#[test]
fn statement_call_tail_range_is_outside_all_symbolic_operators() {
    for (source, nested_op) in [
        ("time & $b .. $c;", "&"),
        ("time ^ $b .. $c;", "^"),
        ("time | $b .. $c;", "|"),
        ("time && $b .. $c;", "&&"),
        ("shift || $b .. $c;", "||"),
        ("time // $b .. $c;", "//"),
    ] {
        let builtin = if nested_op == "||" { "shift" } else { "time" };
        assert_range_with_nested_symbolic(source, Some((builtin, 0)), "left", nested_op);
    }

    for (source, nested_op) in [
        ("time .. $b & $c;", "&"),
        ("time .. $b ^ $c;", "^"),
        ("time .. $b | $c;", "|"),
        ("time .. $b && $c;", "&&"),
        ("time .. $b || $c;", "||"),
        ("time .. $b // $c;", "//"),
    ] {
        assert_range_with_nested_symbolic(source, Some(("time", 0)), "right", nested_op);
    }
}

#[test]
fn statement_call_tail_range_reaches_each_call_site() {
    assert_range_with_nested_symbolic("ref $x || $b .. $c;", Some(("ref", 1)), "left", "||");
    assert_range_with_nested_symbolic("ref $x .. $b && $c;", Some(("ref", 1)), "right", "&&");
    assert_range_with_nested_symbolic("lc $x | $b .. $c;", Some(("lc", 1)), "left", "|");
    assert_range_with_nested_symbolic("close FH || $b .. $c;", None, "left", "||");
}

#[test]
fn statement_call_tail_range_stays_inside_ternary_condition() {
    let source = "time .. $b ? $c : $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    let NodeKind::Ternary { condition, .. } = &expression.kind else {
        panic!("expected outer ternary for `{source}`, got {:?}", expression.kind);
    };
    assert!(
        matches!(&condition.kind, NodeKind::Binary { op, .. } if op == ".."),
        "expected range in ternary condition for `{source}`:\n{}",
        ast.to_sexp()
    );
}

#[test]
fn statement_call_tail_word_operators_stay_outside_range() {
    for (source, word_op) in [("time .. $b or $c;", "or"), ("time .. $b and $c;", "and")] {
        assert_clean_parse(source);
        let ast = parse(source);
        let expression = expression_statement(&ast);
        let NodeKind::Binary { op, left, .. } = &expression.kind else {
            panic!("expected outer word operator for `{source}`, got {:?}", expression.kind);
        };
        assert_eq!(
            op,
            word_op,
            "unexpected outer word operator for `{source}`:\n{}",
            ast.to_sexp()
        );
        assert!(
            matches!(&left.kind, NodeKind::Binary { op, .. } if op == ".."),
            "expected range in {word_op} operand for `{source}`:\n{}",
            ast.to_sexp()
        );
    }
}

#[test]
fn statement_call_tail_range_preserves_adjacent_boundaries() {
    assert_range_with_nested_symbolic("time == 1 .. 2;", Some(("time", 0)), "left", "==");

    let source = "time .. $b, $c;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    let NodeKind::ArrayLiteral { elements } = &expression.kind else {
        panic!("expected comma array for `{source}`, got {:?}", expression.kind);
    };
    assert_eq!(elements.len(), 2, "expected two comma elements for `{source}`:\n{}", ast.to_sexp());
    assert!(
        matches!(&elements[0].kind, NodeKind::Binary { op, .. } if op == ".."),
        "expected range in comma operand for `{source}`:\n{}",
        ast.to_sexp()
    );
    assert!(
        matches!(&elements[1].kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "c"),
        "expected `$c` after range comma for `{source}`:\n{}",
        ast.to_sexp()
    );

    let source = "time & $b .. $c ^ $d;";
    assert_clean_parse(source);
    let ast = parse(source);
    let expression = expression_statement(&ast);
    let NodeKind::Binary { op, left, right } = &expression.kind else {
        panic!("expected outer range for `{source}`, got {:?}", expression.kind);
    };
    assert_eq!(op, "..", "unexpected outer range for `{source}`:\n{}", ast.to_sexp());
    assert!(
        matches!(&left.kind, NodeKind::Binary { op, .. } if op == "&"),
        "expected bitwise-and on range left for `{source}`:\n{}",
        ast.to_sexp()
    );
    assert!(
        matches!(&right.kind, NodeKind::Binary { op, .. } if op == "^"),
        "expected bitwise-xor on range right for `{source}`:\n{}",
        ast.to_sexp()
    );
}
