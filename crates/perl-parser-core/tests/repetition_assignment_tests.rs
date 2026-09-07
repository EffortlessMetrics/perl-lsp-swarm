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

fn find_variable_declaration(node: &Node) -> Option<&Node> {
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

fn find_missing_expression(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::MissingExpression) {
        return Some(node);
    }

    node.children().into_iter().find_map(find_missing_expression)
}

fn find_binary_x(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x") {
        return Some(node);
    }

    node.children().into_iter().find_map(find_binary_x)
}

fn program_statements(ast: &Node) -> Result<&[Node], String> {
    match &ast.kind {
        NodeKind::Program { statements, .. } => Ok(statements),
        other => Err(format!("expected program root, got {other:?}")),
    }
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
        .get(initializer_range.start..initializer_range.end)
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

        if assignment.location.start != assignment_start
            || assignment.location.end != recovery_offset
            || missing.location.start != recovery_offset
            || missing.location.end != recovery_offset
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
    // Real Perl trivia between `x` and `=` is whitespace or a `#` line
    // comment. Perl 5.38.2 syntax-errors these sources (`near "x ="`,
    // `near "x\n="`, `near "x # separated\n="`) and never forms `x=`.
    // The native parser currently splits them into two statements with no
    // diagnostics: statement termination owns the leftover `= 3`. Pin that
    // exact shape so the test cannot pass vacuously on a future hard parse
    // error or by normalizing trivia into `x=`.
    //
    // `/* ... */` is not trivia. Perl has no C comments; after infix `x` a
    // `/` opens a bare regex. That boundary is
    // `slash_after_infix_x_scans_as_bare_regex_not_c_comment`, not this test.
    for source in ["$value x\n= 3;", "$value x # separated\n= 3;"] {
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
        let statements = program_statements(&ast)?;
        if statements.len() != 2 {
            return Err(format!(
                "expected trivia-separated source to parse as two statements:\n{}",
                ast.to_sexp()
            ));
        }
        if !parser.get_errors().is_empty() {
            return Err(format!(
                "expected no diagnostics for real Perl trivia {source:?}, got {:?}",
                parser.get_errors()
            ));
        }
    }

    // Opposite-direction control: `#` trivia after infix `x` is skipped, so
    // the following term is the repetition count. perl 5.38.2 accepts this.
    // If `#` stopped being trivia, `count` would become an identifier RHS or
    // the `x` operator would fail to take `3`.
    let commented_count = "$value x # count\n3;";
    assert_clean_parse(commented_count);
    let ast = parse(commented_count);
    if find_assignment(&ast, "x=").is_some() {
        return Err(format!("hash-comment trivia must not form x=:\n{}", ast.to_sexp()));
    }
    let repetition = find_binary_x(&ast).ok_or_else(|| {
        format!("expected binary x with hash-comment trivia skipped:\n{}", ast.to_sexp())
    })?;
    let NodeKind::Binary { right, .. } = &repetition.kind else {
        return Err(format!("expected Binary x, got: {:?}", repetition.kind));
    };
    if !matches!(&right.kind, NodeKind::Number { value } if value == "3") {
        return Err(format!("expected repetition count 3 after # trivia, got: {:?}", right.kind));
    }
    Ok(())
}

#[test]
fn slash_after_infix_x_scans_as_bare_regex_not_c_comment() -> Result<(), String> {
    // Ruling recorded on #14982: after infix `x`, `/` is a term-position
    // regex delimiter. `$value x /* separated */= 3;` is `m/* separated */`,
    // not a skipped C comment. perl 5.38.2 reports `Quantifier follows
    // nothing in regex` for that pattern; this parser does not compile the
    // pattern, but it must still build a Regex node and must not form `x=`.
    //
    // `$value x/* separated */= 3;` is the adjacency falsifier: skipping
    // `/* */` as a comment would glue `x` to `=` and produce `x=`.
    for source in [
        "$value x /* separated */ = 3;",
        "$value x /* separated */= 3;",
        "$value x/* separated */= 3;",
    ] {
        let ast = parse(source);
        if find_assignment(&ast, "x=").is_some() {
            return Err(format!(
                "slash after infix x must not form x= (C comments do not exist):\n{}",
                ast.to_sexp()
            ));
        }
        let statements = program_statements(&ast)?;
        if statements.len() != 1 {
            return Err(format!("expected one assignment of (x /regex/), got:\n{}", ast.to_sexp()));
        }
        let assignment = find_assignment(&ast, "=").ok_or_else(|| {
            format!("expected ordinary = of the x-regex expression:\n{}", ast.to_sexp())
        })?;
        let NodeKind::Assignment { lhs, rhs, .. } = &assignment.kind else {
            return Err(format!("expected Assignment, got: {:?}", assignment.kind));
        };
        let NodeKind::Binary { op, right, .. } = &lhs.kind else {
            return Err(format!("expected binary x as assignment LHS, got: {:?}", lhs.kind));
        };
        if op != "x" {
            return Err(format!("expected binary x, got operator {op:?}"));
        }
        match &right.kind {
            NodeKind::Regex { pattern, modifiers, .. }
                if pattern == "/* separated */" && modifiers.is_empty() => {}
            other => {
                return Err(format!(
                    "expected Regex pattern /* separated */ after infix x, got: {other:?}"
                ));
            }
        }
        if !matches!(&rhs.kind, NodeKind::Number { value } if value == "3") {
            return Err(format!("expected assignment RHS 3, got: {:?}", rhs.kind));
        }
    }

    // Opposite-direction control: a legal regex body after infix `x` is
    // still a Regex RHS, never `x=`. perl 5.38.2 accepts `$value x /foo/;`.
    let legal = "$value x /foo/;";
    assert_clean_parse(legal);
    let ast = parse(legal);
    if find_assignment(&ast, "x=").is_some() || find_assignment(&ast, "=").is_some() {
        return Err(format!("legal /foo/ after x must not be assignment:\n{}", ast.to_sexp()));
    }
    let repetition = find_binary_x(&ast)
        .ok_or_else(|| format!("expected binary x for {legal}:\n{}", ast.to_sexp()))?;
    let NodeKind::Binary { right, .. } = &repetition.kind else {
        return Err(format!("expected Binary x, got: {:?}", repetition.kind));
    };
    match &right.kind {
        NodeKind::Regex { pattern, modifiers, .. }
            if pattern == "/foo/" && modifiers.is_empty() =>
        {
            Ok(())
        }
        other => Err(format!("expected Regex /foo/ after infix x, got: {other:?}")),
    }
}
