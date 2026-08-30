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
    let NodeKind::ExpressionStatement { expression } = &statement.kind else {
        panic!("expected ExpressionStatement, got {:?}", statement.kind);
    };
    expression
}

fn assert_function_call(node: &Node, name: &str, context: &str) {
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { name: actual, args } if actual == name && args.is_empty()),
        "expected nullary {name} call for {context}, got {:?}",
        node.kind
    );
}

fn assert_range_with_nested_symbolic(
    source: &str,
    builtin: &str,
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
    assert_function_call(if nested_side == "left" { nested_left } else { left }, builtin, source);
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
        assert_range_with_nested_symbolic(source, builtin, "left", nested_op);
    }

    for (source, nested_op) in [
        ("time .. $b & $c;", "&"),
        ("time .. $b ^ $c;", "^"),
        ("time .. $b | $c;", "|"),
        ("time .. $b && $c;", "&&"),
        ("time .. $b || $c;", "||"),
        ("time .. $b // $c;", "//"),
    ] {
        assert_range_with_nested_symbolic(source, "time", "right", nested_op);
    }
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
