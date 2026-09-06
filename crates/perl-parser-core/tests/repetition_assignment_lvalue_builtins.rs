//! Exact production-path proof for contextual `x=` after parenthesized lvalue builtins.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::syntax::error::{ParseError, RecoveryKind, RecoverySite};
use perl_parser_core::{Node, NodeKind, Parser};

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_assignment(child, expected_op))
}

fn find_binary<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Binary { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_binary(child, expected_op))
}

fn find_missing_expression(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::MissingExpression) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_missing_expression)
}

fn assert_lvalue_repetition_assignment(source: &str, expected_name: &str) -> Result<(), String> {
    assert_clean_parse(source);
    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected x= assignment for {expected_name}:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };

    if !matches!(&lhs.kind, NodeKind::FunctionCall { name, .. } if name == expected_name) {
        return Err(format!("expected {expected_name} call as x= lhs, got {:?}", lhs.kind));
    }
    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected numeric x= rhs, got {:?}", rhs.kind));
    }

    let statement_end = source.find(';').ok_or("expected statement terminator")?;
    let observed = source
        .get(assignment.location.start..assignment.location.end)
        .ok_or_else(|| format!("invalid assignment span: {:?}", assignment.location))?;
    let expected = source.get(..statement_end).ok_or("expected statement source slice")?;
    if observed != expected {
        return Err(format!(
            "x= assignment span must cover the complete lvalue expression: {observed:?} != {expected:?}"
        ));
    }

    Ok(())
}

#[test]
fn parenthesized_lvalue_builtins_accept_repetition_assignment() -> Result<(), String> {
    for (source, name) in [
        ("substr($s, 0, 1) x= 3;", "substr"),
        ("vec($bits, 0, 1) x= 3;", "vec"),
        ("pos($s) x= 3;", "pos"),
    ] {
        assert_lvalue_repetition_assignment(source, name)?;
    }
    Ok(())
}

#[test]
fn lvalue_repetition_assignment_keeps_rhs_right_associative() -> Result<(), String> {
    let source = "substr($s, 0, 1) x= $count = 2;";
    assert_clean_parse(source);
    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected outer x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };
    if !matches!(&rhs.kind, NodeKind::Assignment { op, .. } if op == "=") {
        return Err(format!("expected ordinary assignment on x= rhs, got {:?}", rhs.kind));
    }
    Ok(())
}

#[test]
fn existing_symbolic_lvalue_assignments_remain_clean() -> Result<(), String> {
    for source in [
        "substr($s, 0, 1) = 'z';",
        "substr($s, 0, 1) .= 'q';",
        "pos($s) = 0;",
        "pos($s) += 1;",
        "vec($bits, 0, 1) = 1;",
        "vec($bits, 0, 1) |= 1;",
    ] {
        assert_clean_parse(source);
        let ast = parse(source);
        if find_assignment(&ast, "x=").is_some() {
            return Err(format!("symbolic assignment became x=:\n{}", ast.to_sexp()));
        }
    }
    Ok(())
}

#[test]
fn ordinary_repetition_after_parenthesized_call_remains_binary() -> Result<(), String> {
    let source = "$value = substr($s, 0, 1) x 3;";
    assert_clean_parse(source);
    let ast = parse(source);
    if find_assignment(&ast, "x=").is_some() {
        return Err(format!("ordinary x became x=:\n{}", ast.to_sexp()));
    }
    if find_binary(&ast, "x").is_none() {
        return Err(format!("expected ordinary binary x:\n{}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn spaced_repetition_assignment_is_never_normalized() -> Result<(), String> {
    for source in ["substr($s, 0, 1) x = 3;", "vec($bits, 0, 1) x\n= 3;", "pos($s) x # gap\n= 3;"] {
        let output = Parser::new(source).parse_with_recovery();
        if find_assignment(&output.ast, "x=").is_some() {
            return Err(format!(
                "trivia-separated x = must not become x=:\n{}",
                output.ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn parenthesized_lvalue_repetition_assignment_recovers_missing_rhs() -> Result<(), String> {
    let source = "substr($s, 0, 1) x=;";
    let output = Parser::new(source).parse_with_recovery();
    let assignment = find_assignment(&output.ast, "x=")
        .ok_or_else(|| format!("expected recovered x= assignment:\n{}", output.ast.to_sexp()))?;
    let missing = find_missing_expression(assignment)
        .ok_or_else(|| format!("expected missing rhs:\n{}", output.ast.to_sexp()))?;
    let operator_start = source.find("x=").ok_or("expected x= operator")?;

    if missing.location.start != operator_start || missing.location.end != operator_start {
        return Err(format!("unexpected missing-rhs span: {:?}", missing.location));
    }
    if !matches!(
        output.diagnostics.as_slice(),
        [ParseError::Recovered {
            site: RecoverySite::InfixRhs,
            kind: RecoveryKind::MissingOperand,
            location,
        }] if *location == operator_start
    ) {
        return Err(format!(
            "expected one exact missing-rhs recovery at {operator_start}, got {:?}",
            output.diagnostics
        ));
    }
    Ok(())
}

#[test]
fn named_unary_pos_route_remains_supported() -> Result<(), String> {
    let source = "pos $s x= 3;";
    assert_clean_parse(source);
    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected named-unary pos x=:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got {:?}", assignment.kind));
    };
    if !matches!(&lhs.kind, NodeKind::FunctionCall { name, .. } if name == "pos") {
        return Err(format!("expected pos call lhs, got {:?}", lhs.kind));
    }
    Ok(())
}
