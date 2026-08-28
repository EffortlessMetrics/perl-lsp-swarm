//! Regression coverage for Perl's string-repetition assignment operator (`x=`).

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::hir::{HirKind, lower_ast};
use perl_parser_core::{Node, NodeKind, Parser};

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_assignment(child, expected_op))
}

fn find_variable_declaration<'a>(node: &'a Node) -> Option<&'a Node> {
    if matches!(node.kind, NodeKind::VariableDeclaration { .. }) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_variable_declaration)
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
        return Err(format!("expected numeric repetition count, got: {:?}", rhs.kind));
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
        return Err(format!("expected = assignment on x= RHS, got: {:?}", rhs.kind));
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

    if let Ok(ast) = result
        && find_assignment(&ast, "x=").is_some()
    {
        return Err(format!("spaced x = must not be normalized to x=:\n{}", ast.to_sexp()));
    }

    Ok(())
}

#[test]
fn repetition_assignment_works_in_variable_declarations() -> Result<(), String> {
    let source = "my $value x= 3;";
    assert_clean_parse(source);

    let ast = parse(source);
    let declaration = find_variable_declaration(&ast)
        .ok_or_else(|| format!("expected variable declaration:\n{}", ast.to_sexp()))?;
    let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } = &declaration.kind
    else {
        return Err(format!("expected x= initializer, got: {:?}", declaration.kind));
    };
    if !matches!(&initializer.kind, NodeKind::Assignment { op, .. } if op == "x=") {
        return Err(format!("expected x= assignment initializer, got: {:?}", initializer.kind));
    }
    Ok(())
}

#[test]
fn repetition_assignment_works_after_named_unary_call() -> Result<(), String> {
    let source = "pos $value x= 3;";
    assert_clean_parse(source);

    let ast = parse(source);
    let assignment = find_assignment(&ast, "x=")
        .ok_or_else(|| format!("expected pos x= assignment:\n{}", ast.to_sexp()))?;
    let NodeKind::Assignment { lhs, .. } = &assignment.kind else {
        return Err(format!("expected Assignment, got: {:?}", assignment.kind));
    };
    if !matches!(&lhs.kind, NodeKind::FunctionCall { name, .. } if name == "pos") {
        return Err(format!("expected pos call as x= lhs, got: {:?}", lhs.kind));
    }
    Ok(())
}

#[test]
fn repetition_assignment_preserves_hir_declaration_reachability() -> Result<(), String> {
    let source = "my $value x= 3;";
    let ast = parse(source);
    let hir = lower_ast(&ast);
    let declaration = hir.items.iter().find_map(|item| match &item.kind {
        HirKind::VariableDecl(declaration) => Some(declaration),
        _ => None,
    });
    let declaration = declaration.ok_or("expected HIR variable declaration")?;
    if !declaration.has_initializer || declaration.initializer_range.is_none() {
        return Err(format!("expected HIR initializer reachability, got: {declaration:?}"));
    }
    Ok(())
}

#[test]
fn repetition_assignment_rejects_trivia_between_x_and_equals() -> Result<(), String> {
    for source in ["$value x\n= 3;", "$value x /* separated */ = 3;"] {
        let mut parser = Parser::new(source);
        let result = parser.parse();
        if let Ok(ast) = result
            && find_assignment(&ast, "x=").is_some()
        {
            return Err(format!(
                "trivia-separated x = must not normalize to x=:\n{}",
                ast.to_sexp()
            ));
        }
    }
    Ok(())
}
