//! Regression coverage for Perl's string-repetition assignment operator (`x=`).

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind, Parser};

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children()
        .into_iter()
        .find_map(|child| find_assignment(child, expected_op))
}

#[test]
fn repetition_assignment_builds_assignment_ast() -> Result<(), String> {
    let source = "$value x= 3;";
    assert_clean_parse(source);

    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected x= assignment node:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got: {:?}", assignment.kind));
    };

    if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!(
            "expected numeric repetition count, got: {:?}",
            rhs.kind
        ));
    }
    Ok(())
}

#[test]
fn repetition_assignment_is_right_associative() -> Result<(), String> {
    let source = "$left x= $right = 2;";
    assert_clean_parse(source);

    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected outer x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got: {:?}", assignment.kind));
    };

    if !matches!(&rhs.kind, NodeKind::Assignment { op, .. } if op == "=") {
        return Err(format!(
            "expected = assignment on x= RHS, got: {:?}",
            rhs.kind
        ));
    }
    Ok(())
}

#[test]
fn ordinary_repetition_operator_remains_binary() -> Result<(), String> {
    let source = "$value = 'a' x 3;";
    assert_clean_parse(source);

    let ast = parse(source);
    let assignment = find_assignment(&ast, "=")
        .ok_or_else(|| format!("expected outer assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { rhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got: {:?}", assignment.kind));
    };

    if !matches!(&rhs.kind, NodeKind::Binary { op, .. } if op == "x") {
        return Err(format!(
            "expected ordinary x repetition on assignment RHS, got: {:?}",
            rhs.kind
        ));
    }
    Ok(())
}

#[test]
fn whitespace_does_not_form_repetition_assignment() -> Result<(), String> {
    let source = "$value x = 3;";
    let mut parser = Parser::new(source);
    let result = parser.parse();
    let has_diagnostics = !parser.get_errors().is_empty();

    if result.is_ok() && !has_diagnostics {
        return Err("spaced x = must remain invalid Perl".to_string());
    }

    if let Ok(ast) = result
        && find_assignment(&ast, "x=").is_some()
    {
        return Err(format!(
            "spaced x = must not be normalized to x=:\n{}",
            ast.to_sexp()
        ));
    }

    Ok(())
}
