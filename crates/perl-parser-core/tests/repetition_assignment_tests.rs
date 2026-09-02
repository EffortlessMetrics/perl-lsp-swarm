//! Regression coverage for Perl's string-repetition assignment operator (`x=`).

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::hir::{AssignMode, HirExpr, HirKind, HirStmt, lower_ast};
use perl_parser_core::syntax::error::{ParseError, RecoveryKind, RecoverySite};
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

fn find_missing_expression<'a>(node: &'a Node) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::MissingExpression) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_missing_expression)
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
    if assignment.location.start() != 0 || assignment.location.end() != end {
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
    let output = Parser::new(source).parse_with_recovery();

    if find_assignment(&output.ast, "x=").is_some() {
        return Err(format!("spaced x = must not be normalized to x=:\n{}", output.ast.to_sexp()));
    }
    // The claim is only that spaced `x =` stays outside the operator, not
    // that the parser diagnoses the same-line leftover: statement-terminator
    // enforcement deliberately ignores same-line trailing tokens, so the
    // source parses as the variable expression followed by an ordinary `x =
    // 3` assignment with no repetition diagnostic. Pin that exact shape so
    // the test cannot pass vacuously on some future unrelated acceptance.
    let NodeKind::Program { statements, .. } = &output.ast.kind else {
        return Err(format!("expected program root, got {:?}", output.ast.kind));
    };
    if statements.len() != 2 {
        return Err(format!(
            "expected the leftover `x = 3` to parse as a second statement, got {}",
            output.ast.to_sexp()
        ));
    }
    if !output.diagnostics.is_empty() {
        return Err(format!(
            "same-line leftover enforcement is owned by statement termination, not the \
             repetition operator; expected no repetition diagnostic, got {:?}",
            output.diagnostics
        ));
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
        .get(initializer_range.start()..initializer_range.end())
        .ok_or("HIR initializer range must be valid UTF-8 source bounds")?;
    if !initializer_source.contains("x=") {
        return Err(format!("HIR initializer lost x= semantics: {initializer_source:?}"));
    }

    let body = hir.root_body().ok_or("expected production root HIR body")?;
    let root = body.block(body.root_block).ok_or("expected production root block")?;
    let stmt = body
        .stmt(*root.stmts.first().ok_or("expected declaration body statement")?)
        .ok_or("expected declaration body statement")?;
    let init_id = match stmt {
        HirStmt::Let { init: Some(init_id), .. } => *init_id,
        other => return Err(format!("expected lowered declaration initializer, got {other:?}")),
    };
    let HirExpr::Assign { lhs, rhs, mode } =
        body.expr(init_id).ok_or("expected lowered declaration assignment")?
    else {
        return Err("declaration initializer must lower to HirExpr::Assign".to_string());
    };
    if !matches!(mode, AssignMode::Simple) {
        return Err(format!("expected simple declaration assignment, got {mode:?}"));
    }
    if !matches!(body.expr(*lhs), Some(HirExpr::Variable(variable)) if variable.name == "value") {
        return Err(format!("expected declaration lhs operand, got {:?}", body.expr(*lhs)));
    }
    let HirExpr::Assign { lhs: repetition_lhs, rhs: repetition_rhs, mode: repetition_mode } =
        body.expr(*rhs).ok_or("expected lowered x= assignment operand")?
    else {
        return Err("x= initializer must lower to HirExpr::Assign".to_string());
    };
    if !matches!(repetition_mode, AssignMode::ReadModifyWrite)
        || !matches!(body.expr(*repetition_lhs), Some(HirExpr::Variable(variable)) if variable.name == "value")
        || body.expr(*repetition_rhs).is_none()
    {
        return Err(format!("lowered x= operands are incomplete: {:?}", body.expr(*rhs)));
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
fn repetition_assignment_documents_malformed_operator_boundaries() -> Result<(), String> {
    // `x==` and `x=>` lex as the ordinary `==` binary operator and `=>` fat
    // comma; the repetition-assignment operator must not absorb either
    // boundary into `x=`. The parser does not reject these sources: it
    // accepts them with the ordinary-operator shapes pinned below. Renaming
    // or changing the assertions to rejection requires a separate parser
    // decision, not a test-only change.
    for source in ["$value x== 3;", "$value x=> 3;"] {
        let mut parser = Parser::new(source);
        let result = parser.parse();
        let ast = result
            .map_err(|error| format!("unexpected parse failure for {source:?}: {error:?}"))?;
        let sexp = ast.to_sexp();
        if find_assignment(&ast, "x=").is_some() {
            return Err(format!("malformed boundary must not normalize to x=:\n{}", ast.to_sexp()));
        }
        let expected =
            if source.contains("x==") { "(binary_==" } else { "(hash (key (string (value x)))" };
        if !sexp.contains(expected) {
            return Err(format!("malformed boundary lost expected AST {expected:?}:\n{sexp}"));
        }
    }
    Ok(())
}

#[test]
fn repetition_assignment_recovers_missing_rhs_with_exact_spans() -> Result<(), String> {
    for (source, assignment_start, recovery_offset) in
        [("$value x=;", 0, 7), ("my $value x=;", 3, 10)]
    {
        let output = Parser::new(source).parse_with_recovery();
        let assignment = find_assignment(&output.ast, "x=").ok_or_else(|| {
            format!("expected recovered x= assignment:\n{}", output.ast.to_sexp())
        })?;
        let missing = find_missing_expression(assignment)
            .ok_or_else(|| format!("expected missing RHS:\n{}", output.ast.to_sexp()))?;

        if assignment.location.start() != assignment_start
            || assignment.location.end() != recovery_offset
            || missing.location.start() != recovery_offset
            || missing.location.end() != recovery_offset
        {
            return Err(format!(
                "recovery spans must stop at the missing RHS: assignment={:?}, missing={:?}",
                assignment.location, missing.location
            ));
        }
        if !matches!(
            output.diagnostics.as_slice(),
            [ParseError::Recovered {
                site: RecoverySite::InfixRhs,
                kind: RecoveryKind::MissingOperand,
                location,
            }] if *location == recovery_offset
        ) {
            return Err(format!(
                "expected one exact missing-RHS recovery at {recovery_offset}, got {:?}",
                output.diagnostics
            ));
        }
    }
    Ok(())
}

#[test]
fn repetition_assignment_rejects_malformed_missing_rhs_and_triple_equals() -> Result<(), String> {
    let pos_output = Parser::new("pos $value x=;").parse_with_recovery();
    let pos_sexp = pos_output.ast.to_sexp();
    if find_assignment(&pos_output.ast, "x=").is_some()
        || !pos_sexp.contains("(ERROR (message \"expected expression, found ';' at position 13\")")
        || !matches!(
            pos_output.diagnostics.as_slice(),
            [
                ParseError::UnexpectedToken { expected, found, location },
                ParseError::UnexpectedToken { expected: statement_expected, found: statement_found, location: statement_location },
            ] if expected == "expression"
                && found == "';'"
                && *location == 13
                && statement_expected == "statement"
                && statement_found == "';'"
                && *statement_location == 13
        )
    {
        return Err(format!(
            "pos missing RHS must reject x= at the semicolon with exact diagnostics: ast={pos_sexp}, diagnostics={:?}",
            pos_output.diagnostics
        ));
    }

    let triple_output = Parser::new("$value x=== 3;").parse_with_recovery();
    let triple_sexp = triple_output.ast.to_sexp();
    if find_assignment(&triple_output.ast, "x=").is_some()
        || !triple_sexp
            .contains("(ERROR (message \"expected expression, found '=' at position 10\")")
        || !matches!(
            triple_output.diagnostics.as_slice(),
            [
                ParseError::UnexpectedToken { expected, found, location },
                ParseError::UnexpectedToken { expected: statement_expected, found: statement_found, location: statement_location },
            ] if expected == "expression"
                && found == "'='"
                && *location == 10
                && statement_expected == "statement"
                && statement_found == "'='"
                && *statement_location == 10
        )
    {
        return Err(format!(
            "x=== must reject x= and recover at the third operator byte: ast={triple_sexp}, diagnostics={:?}",
            triple_output.diagnostics
        ));
    }
    Ok(())
}

#[test]
fn repetition_assignment_rejects_trivia_between_x_and_equals() -> Result<(), String> {
    // Newline or comment trivia between `x` and `=` keeps the source outside
    // the repetition-assignment operator. Newline trivia terminates the
    // `$value x` statement cleanly, so the source parses as two statements
    // with no diagnostics; comment trivia leaves an unparsable `/ = 3;`
    // remainder that surfaces as recovery diagnostics while still parsing.
    // Pin the exact accepted shapes so the test cannot pass vacuously on a
    // future hard parse error or unrelated acceptance.
    for (source, expects_recovery_diagnostics) in
        [("$value x\n= 3;", false), ("$value x /* separated */ = 3;", true)]
    {
        let mut parser = Parser::new(source);
        let ast = parser
            .parse()
            .map_err(|error| format!("unexpected parse failure for {source:?}: {error:?}"))?;
        if find_assignment(&ast, "x=").is_some() {
            return Err(format!(
                "trivia-separated x = must not normalize to x=:\n{}",
                ast.to_sexp()
            ));
        }
        let NodeKind::Program { statements, .. } = &ast.kind else {
            return Err(format!("expected program root, got {:?}", ast.kind));
        };
        if statements.len() != 2 {
            return Err(format!(
                "expected trivia-separated source to parse as two statements:\n{}",
                ast.to_sexp()
            ));
        }
        if (!parser.get_errors().is_empty()) != expects_recovery_diagnostics {
            return Err(format!(
                "unexpected diagnostics for {source:?}: {:?}",
                parser.get_errors()
            ));
        }
    }
    Ok(())
}
