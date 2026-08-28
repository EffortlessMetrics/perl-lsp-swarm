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
    if matches!(&node.kind, NodeKind::VariableDeclaration { .. }) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_variable_declaration)
}

fn find_named_call<'a>(node: &'a Node, expected_name: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::FunctionCall { name, .. } if name == expected_name) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_named_call(child, expected_name))
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
    let end = source.find(';').ok_or("expected statement terminator")?;
    if assignment.location.start != 0 || assignment.location.end != end {
        return Err(format!("unexpected x= assignment span: {:?}", assignment.location));
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
    let ast_declaration = find_variable_declaration(&ast).ok_or("expected AST declaration")?;
    let NodeKind::VariableDeclaration { initializer: Some(ast_initializer), .. } =
        &ast_declaration.kind
    else {
        return Err("expected AST x= initializer".to_string());
    };
    if !matches!(&ast_initializer.kind, NodeKind::Assignment { op, .. } if op == "x=") {
        return Err(format!(
            "expected HIR input to contain x= assignment, got: {:?}",
            ast_initializer.kind
        ));
    }

    let hir = lower_ast(&ast);
    let declaration = hir.items.iter().find_map(|item| match &item.kind {
        HirKind::VariableDecl(declaration) => Some(declaration),
        _ => None,
    });
    let declaration = declaration.ok_or("expected HIR variable declaration")?;
    let Some(initializer_range) = declaration.initializer_range else {
        return Err(format!("expected HIR initializer reachability, got: {declaration:?}"));
    };
    if !declaration.has_initializer || initializer_range != ast_initializer.location {
        return Err(format!(
            "HIR initializer must preserve the x= assignment span, got {initializer_range:?} vs {:?}",
            ast_initializer.location
        ));
    }
    let initializer_source = source
        .get(initializer_range.start..initializer_range.end)
        .ok_or("HIR initializer range must be valid UTF-8 source bounds")?;
    if !initializer_source.contains("x=") {
        return Err(format!("HIR initializer lost x= semantics: {initializer_source:?}"));
    }
    Ok(())
}

#[test]
fn repetition_assignment_preserves_x_call_boundary() -> Result<(), String> {
    let source = "$value = x();";
    assert_clean_parse(source);
    let ast = parse(source);
    if find_assignment(&ast, "x=").is_some() {
        return Err(format!("x() must not become x= assignment:\n{}", ast.to_sexp()));
    }
    if find_named_call(&ast, "x").is_none() {
        return Err(format!("expected named x() call:\n{}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn repetition_assignment_rejects_malformed_operator_boundaries() -> Result<(), String> {
    for source in ["$value x== 3;", "$value x=> 3;"] {
        let mut parser = Parser::new(source);
        if let Ok(ast) = parser.parse()
            && find_assignment(&ast, "x=").is_some()
        {
            return Err(format!("malformed boundary must not normalize to x=:\n{}", ast.to_sexp()));
        }
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
