use super::*;
use perl_tdd_support::must;

#[test]
fn test_recovery_missing_expression() {
    // Phase 2 recovery: missing RHS after `=` emits Recovered { InfixRhs, MissingOperand }
    // and produces a VariableDeclaration with MissingExpression initializer — not an Error node.
    let code = "my $x = ; print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    match result {
        Ok(ast) => {
            println!("AST: {}", ast.to_sexp());

            // Check that we have 2 statements
            if let NodeKind::Program { statements } = &ast.kind {
                assert_eq!(
                    statements.len(),
                    2,
                    "Should have 2 statements (1 recovered decl, 1 valid)"
                );

                // Phase 2: first statement is a VariableDeclaration with MissingExpression
                // (not a raw Error node — Phase 2 recovers with a structured node)
                assert!(
                    matches!(
                        statements[0].kind,
                        NodeKind::VariableDeclaration { .. } | NodeKind::Error { .. }
                    ),
                    "Expected VariableDeclaration or Error for first statement, got: {:?}",
                    statements[0].kind
                );

                // Second statement should be ExpressionStatement
                match &statements[1].kind {
                    NodeKind::ExpressionStatement { .. } => {
                        println!("Found valid second statement");
                    }
                    _ => unreachable!(
                        "Expected ExpressionStatement for second statement, got: {:?}",
                        statements[1].kind
                    ),
                }
            } else {
                unreachable!("Expected Program node");
            }

            // Check errors list — at minimum one Recovered error
            let errors = parser.errors();
            assert!(!errors.is_empty(), "Should have recorded errors");
            println!("Errors: {:?}", errors);
        }
        Err(e) => {
            unreachable!("Parser failed to recover: {}", e);
        }
    }
}

#[test]
fn test_recovery_missing_rhs_before_sub_declaration_keyword() {
    // Missing RHS before `sub foo { ... }` should recover at `sub` as a
    // statement boundary, instead of consuming `sub` as an identifier.
    let code = "my $x = sub foo { print 1; }";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should recover missing assignment RHS");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 2, "Should recover and keep the following sub declaration");
        assert!(
            matches!(statements[0].kind, NodeKind::VariableDeclaration { .. }),
            "First statement should stay a recovered variable declaration"
        );
        assert!(
            matches!(statements[1].kind, NodeKind::Subroutine { .. }),
            "Second statement should parse as subroutine declaration"
        );
    } else {
        unreachable!("Expected program root");
    }

    assert!(!parser.errors().is_empty(), "Recovery should record a missing operand diagnostic");
}

#[test]
fn test_no_recovery_for_anonymous_sub_assignment_rhs() {
    let code = "local $SIG{__WARN__} = sub { };";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should accept anonymous sub assignment RHS");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 1, "Anonymous sub RHS should stay in a single statement");
    } else {
        unreachable!("Expected program root");
    }

    assert!(
        !ast.to_sexp().contains("missing_expression"),
        "Anonymous sub assignment should not create MissingExpression recovery nodes"
    );
}

#[test]
fn test_recovery_multiple_errors() {
    // Phase 2: `my $x = ;` now produces a VariableDeclaration with MissingExpression
    // instead of an Error node. Each missing RHS emits exactly 1 Recovered error.
    let code = "
        my $a = ;   # Recovered 1
        print 1;    # Valid
        my $b = ;   # Recovered 2
        print 2;    # Valid
    ";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok());
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 4, "Should have 4 statements");
        // Phase 2: VariableDeclaration with MissingExpression (not raw Error)
        assert!(
            matches!(
                statements[0].kind,
                NodeKind::VariableDeclaration { .. } | NodeKind::Error { .. }
            ),
            "Expected VariableDeclaration or Error, got: {:?}",
            statements[0].kind
        );
        assert!(matches!(statements[1].kind, NodeKind::ExpressionStatement { .. }));
        assert!(
            matches!(
                statements[2].kind,
                NodeKind::VariableDeclaration { .. } | NodeKind::Error { .. }
            ),
            "Expected VariableDeclaration or Error, got: {:?}",
            statements[2].kind
        );
        assert!(matches!(statements[3].kind, NodeKind::ExpressionStatement { .. }));
    }

    // Phase 2: 2 Recovered errors (one per missing operand), down from 4 raw errors
    assert!(!parser.errors().is_empty(), "Should have errors");
    assert!(parser.errors().len() >= 2, "Expected at least 2 errors, got: {:?}", parser.errors());
}

#[test]
fn test_recovery_inside_block() {
    // Phase 2: `my $x = ;` inside a block produces VariableDeclaration with MissingExpression
    let code = "sub foo { my $x = ; print 1; }";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    match result {
        Ok(ast) => {
            // Structure: Program -> Subroutine -> Block -> [VariableDeclaration, ExpressionStatement]
            if let NodeKind::Program { statements } = &ast.kind {
                assert_eq!(statements.len(), 1);

                if let NodeKind::Subroutine { body, .. } = &statements[0].kind {
                    if let NodeKind::Block { statements } = &body.kind {
                        assert_eq!(
                            statements.len(),
                            2,
                            "Block should have 2 statements (1 recovered decl, 1 valid)"
                        );

                        // Phase 2: first stmt is VariableDeclaration (not Error)
                        assert!(
                            matches!(
                                statements[0].kind,
                                NodeKind::VariableDeclaration { .. } | NodeKind::Error { .. }
                            ),
                            "Expected VariableDeclaration or Error in block, got: {:?}",
                            statements[0].kind
                        );

                        match &statements[1].kind {
                            NodeKind::ExpressionStatement { .. } => {
                                println!("Found valid statement in block")
                            }
                            _ => unreachable!("Expected ExpressionStatement in block"),
                        }
                    } else {
                        unreachable!("Expected Block in subroutine body");
                    }
                } else {
                    unreachable!("Expected Subroutine node, got: {:?}", statements[0].kind);
                }
            }

            assert!(!parser.errors().is_empty());
        }
        Err(e) => unreachable!("Failed to recover from block error: {}", e),
    }
}

// Issue #451: AC1 - Parser maintains internal errors collection
#[test]
fn test_451_ac1_maintains_error_collection() {
    let code = "my $x = ; my $y = 10;";
    let mut parser = Parser::new(code);
    let _result = parser.parse();

    let errors = parser.errors();
    assert!(!errors.is_empty(), "AC1: Parser should maintain errors collection");
}

// Issue #451: AC2 - parse_with_recovery method returns both AST and errors
#[test]
fn test_451_ac2_parse_with_recovery_method() {
    let code = "my $x = ; print 1;";
    let mut parser = Parser::new(code);

    let output = parser.parse_with_recovery();

    assert!(matches!(output.ast.kind, NodeKind::Program { .. }), "AC2: Should return AST");
    assert!(!output.diagnostics.is_empty(), "AC2: Should return collected errors");
}

// Issue #451: AC3 - ParseOutput includes ast and diagnostics fields
#[test]
fn test_451_ac3_parse_output_structure() {
    let code = "my $x = ;";
    let mut parser = Parser::new(code);
    let output = parser.parse_with_recovery();

    assert!(matches!(output.ast.kind, NodeKind::Program { .. }), "AC3: ast field present");
    assert!(!output.diagnostics.is_empty(), "AC3: diagnostics field present");
}

// Issue #451: AC4 - Parser continues after storing error (non-fail-fast)
#[test]
fn test_451_ac4_continues_after_error() {
    let code = "my $a = ; print 'hello'; my $b = ; print 'world';";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "AC4: Parser should continue after errors");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 4, "AC4: Should continue parsing after each error");
    }
}

// Issue #451: AC5 - Error limit enforcement prevents unbounded collection
#[test]
fn test_451_ac5_error_limit_enforcement() {
    let mut code = String::new();
    for i in 0..150 {
        code.push_str(&format!("my $x{} = ;\n", i));
    }

    let mut parser = Parser::new(&code);
    let _result = parser.parse();

    let errors = parser.errors();
    assert!(errors.len() < 500, "AC5: Should limit error collection (found {})", errors.len());
}

// Issue #451: AC6 - Recovery doesn't recurse infinitely
#[test]
fn test_451_ac6_recovery_prevents_infinite_loops() {
    // Test that recovery has bounded behavior even with pathological input
    let code = ";;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    // Should complete successfully without hanging or stack overflow
    assert!(result.is_ok(), "AC6: Recovery should complete on pathological input");

    // Test with many syntax errors that recovery can handle
    let code2 = "{ { { { { { { { { {";
    let mut parser2 = Parser::new(code2);
    let result2 = parser2.parse();

    // Should complete without infinite recursion
    assert!(result2.is_ok(), "AC6: Should handle nested unclosed blocks");
}

// Issue #451: AC7 - Statement-level parsing collects errors and continues
#[test]
fn test_451_ac7_statement_level_recovery() {
    // Phase 2: `my $bad = ;` produces VariableDeclaration with MissingExpression,
    // not a raw Error node. The parser still recovers and parses all statements.
    let code = "
        print 1;
        my $bad = ;
        print 2;
        my $good = 42;
    ";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "AC7: Statement-level parsing should recover");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 4, "AC7: Should parse all statements");

        // Phase 2: the bad declaration is recovered as VariableDeclaration, not Error.
        // We still verify recovery happened via the errors collection.
        let has_valid = statements.iter().any(|s| !matches!(s.kind, NodeKind::Error { .. }));
        assert!(has_valid, "AC7: Should have valid statements after error");
    }
    assert!(!parser.errors().is_empty(), "AC7: Should have recorded errors");
}

// Issue #451: AC8 - Expression-level recovery creates error nodes
#[test]
fn test_451_ac8_expression_level_recovery() {
    // Phase 2: `my $x = ;` now recovers with a VariableDeclaration+MissingExpression
    // instead of a raw Error node. The errors collection still has a Recovered entry.
    let code = "my $x = ;";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "AC8: Should recover from expression errors");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert!(!statements.is_empty(), "AC8: Should have statement");

        // Phase 2: either a VariableDeclaration (new, better) or Error (old fallback)
        let has_recovered = statements.iter().any(|s| {
            matches!(
                s.kind,
                NodeKind::VariableDeclaration { .. }
                    | NodeKind::Error { .. }
                    | NodeKind::MissingExpression
            )
        });
        assert!(has_recovered, "AC8: Should produce a recovered or error node");
    }

    assert!(!parser.errors().is_empty(), "AC8: Should record expression-level error");
}

// Issue #451: AC9 - Block-level parsing collects errors for each statement
#[test]
fn test_451_ac9_block_level_recovery() {
    // Phase 2: `my $a = ;` inside a block produces VariableDeclaration+MissingExpression.
    // The block still has all 4 statements and the errors collection has Recovered entries.
    let code = "
        sub test {
            my $a = ;
            print 1;
            my $b = ;
            print 2;
        }
    ";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "AC9: Block-level parsing should recover");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        if let Some(sub_node) = statements.first() {
            if let NodeKind::Subroutine { body, .. } = &sub_node.kind {
                if let NodeKind::Block { statements: block_stmts } = &body.kind {
                    assert_eq!(block_stmts.len(), 4, "AC9: Block should have all statements");

                    // Phase 2: the bad declarations are VariableDeclaration (not Error).
                    // The two print statements are ExpressionStatement.
                    let print_count = block_stmts
                        .iter()
                        .filter(|s| matches!(s.kind, NodeKind::ExpressionStatement { .. }))
                        .count();
                    assert_eq!(
                        print_count, 2,
                        "AC9: Should have 2 valid ExpressionStatement in block"
                    );
                }
            }
        }
    }

    let errors = parser.errors();
    assert!(errors.len() >= 2, "AC9: Should collect multiple errors from block");
}

// Issue #451: AC10 - Multiple error collection scenarios
#[test]
fn test_451_ac10_comprehensive_scenarios() {
    // Scenario 1: Interleaved errors and valid code
    let code1 = "
        my $a = ;
        print 'valid';
        my $b = ;
        my $c = 10;
        my $d = ;
    ";
    let mut parser1 = Parser::new(code1);
    let result1 = parser1.parse();
    assert!(result1.is_ok(), "AC10: Should handle interleaved errors");
    assert!(parser1.errors().len() >= 3, "AC10: Should collect all 3 errors");

    // Scenario 2: Nested blocks with errors
    let code2 = "
        if (1) {
            my $x = ;
            print 1;
        }
        while (1) {
            my $y = ;
            print 2;
        }
    ";
    let mut parser2 = Parser::new(code2);
    let result2 = parser2.parse();
    assert!(result2.is_ok(), "AC10: Should handle nested block errors");
    assert!(parser2.errors().len() >= 2, "AC10: Should collect errors from nested blocks");

    // Scenario 3: Different error types
    let code3 = "my $x = ; my $y = ";
    let mut parser3 = Parser::new(code3);
    let _result3 = parser3.parse();
    assert!(!parser3.errors().is_empty(), "AC10: Should handle different error types");
}

#[test]
fn test_no_recovery_for_my_code_eq_anon_sub() {
    // Regression test for #5017: `my $code = sub { ... }` must parse as a
    // single VariableDeclaration with an AnonymousSubroutineExpression RHS.
    // Before the fix, this produced MissingExpression + dangling anon-sub.
    let code = "my $code = sub { my ($x) = @_; return $x * 2; };";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should accept `my $var = sub {{...}};`");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(
            statements.len(),
            1,
            "my $code = sub {{...}} must be a single statement, not split in two"
        );
        // Verify no error nodes and no missing_expression
        let sexp = ast.to_sexp();
        assert!(
            !sexp.contains("missing_expression"),
            "RHS anonymous sub must not produce MissingExpression: {sexp}"
        );
        assert!(!sexp.contains("error"), "RHS anonymous sub must not produce Error nodes: {sexp}");
    } else {
        unreachable!("Expected program root");
    }

    assert!(
        parser.errors().is_empty(),
        "No recovery errors expected for valid anonymous sub assignment: {:?}",
        parser.errors()
    );
}

#[test]
fn test_local_as_assignment_rhs() {
    // `local` is a valid expression and must be allowed as an assignment RHS.
    // Regression test: removing Local from is_infix_rhs_absent must not break
    // recovery for genuine missing-RHS cases that happen to follow a `local` expr.
    let code = "local(*RS) = local(*/);";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should accept `local(x) = local(y);`");
    let ast = must(result);

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(statements.len(), 1, "`local(x) = local(y)` must parse as a single statement");
        let sexp = ast.to_sexp();
        assert!(
            !sexp.contains("missing_expression"),
            "local-as-RHS must not produce MissingExpression: {sexp}"
        );
    } else {
        unreachable!("Expected program root");
    }

    assert!(
        parser.errors().is_empty(),
        "No recovery errors expected for `local(x) = local(y)`: {:?}",
        parser.errors()
    );
}

#[test]
fn test_recovery_unclosed_qw() -> Result<(), String> {
    let code = "my @items = qw(word1 word2\nmy $x = 42;\nprint \"x is $x\\n\";";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    let ast =
        result.map_err(|error| format!("parser did not recover from unclosed qw: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 3 {
        return Err(format!(
            "expected malformed declaration plus two recovered statements: {sexp}"
        ));
    }
    if !matches!(
        statements.first().map(|node| &node.kind),
        Some(NodeKind::VariableDeclaration { .. })
    ) || !matches!(
        statements.get(1).map(|node| &node.kind),
        Some(NodeKind::VariableDeclaration { .. })
    ) || !matches!(
        statements.get(2).map(|node| &node.kind),
        Some(NodeKind::ExpressionStatement { .. })
    ) {
        return Err(format!("recovered statements had unexpected shapes: {sexp}"));
    }
    if !sexp.contains("@ items")
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"word2\"")
        || !sexp.contains("$ x")
        || !sexp.contains("print")
    {
        return Err(format!("qw recovery did not preserve declaration/print boundaries: {sexp}"));
    }
    if parser.errors().is_empty() {
        return Err("unclosed qw recovery did not record an error".to_string());
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_ignores_nested_close_in_following_statement() -> Result<(), String> {
    let code = "my @items = qw(word1 word2\nmy $x = foo();\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parser did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("nested call delimiter disabled qw recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_ignores_close_in_following_string() -> Result<(), String> {
    let code = "my @items = qw(word1 word2\nmy $x = \"not a qw close )\";\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parser did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("string delimiter disabled qw recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_before_declaration_with_multiline_string() -> Result<(), String> {
    let code = "my @items = qw(word1 word2\nmy $x = \"multi\nline\";\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parser did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("multiline string disabled qw recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_ignores_close_in_following_comment() -> Result<(), String> {
    let code = "my @items = qw(word1 word2\nmy $x = 1; # not a qw close )\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parser did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("comment delimiter disabled qw recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_keeps_same_line_keyword_as_word() -> Result<(), String> {
    let code = "my @items = qw(word1 my word2\nprint 1;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("parser did not recover: {error}"))?;
    let sexp = ast.to_sexp();
    if !sexp.contains("\"my\"") || !sexp.contains("\"word2\"") || !sexp.contains("print") {
        return Err(format!("same-line qw keyword triggered false synchronization: {sexp}"));
    }
    if parser.errors().is_empty() {
        return Err("unclosed same-line qw did not record an error".to_string());
    }
    Ok(())
}

#[test]
fn test_closed_qw_keeps_statement_keywords_as_words() -> Result<(), String> {
    let code = "my @items = qw(word1 my print word2);";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("closed qw did not parse: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 1
        || !parser.errors().is_empty()
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"my\"")
        || !sexp.contains("\"print\"")
        || !sexp.contains("\"word2\"")
    {
        return Err(format!("closed qw changed behavior: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_closed_multiline_qw_keeps_statement_keywords_as_words() -> Result<(), String> {
    let code = "my @items = qw(word1\nmy word2\nprint word3);";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("closed multiline qw did not parse: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 1
        || !parser.errors().is_empty()
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"my\"")
        || !sexp.contains("\"print\"")
        || !sexp.contains("\"word3\"")
    {
        return Err(format!("closed multiline qw changed behavior: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_closed_multiline_qw_keeps_line_start_keywords_as_words() -> Result<(), String> {
    let code = "my @items = qw(\nword1\nmy\nprint\nword2\n);";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("closed multiline qw failed: {error}"))?;
    let sexp = ast.to_sexp();
    if !parser.errors().is_empty()
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"my\"")
        || !sexp.contains("\"print\"")
        || !sexp.contains("\"word2\"")
    {
        return Err(format!("closed multiline qw changed behavior: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_spaced_qw_recovers_following_declaration() -> Result<(), String> {
    let code = "my @items = qw (word1 word2\nmy $x = 42;\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("spaced qw did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 3
        || parser.errors().is_empty()
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"word2\"")
        || sexp.contains("\"(word1\"")
    {
        return Err(format!("spaced qw recovery lost following statements: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_closed_multiline_qw_keeps_declaration_shaped_words() -> Result<(), String> {
    let code = "my @items = qw(\nword1\nmy $x;\nword2\n);";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("closed declaration-like qw failed: {error}"))?;
    let sexp = ast.to_sexp();
    if !parser.errors().is_empty()
        || !sexp.contains("\"my\"")
        || !sexp.contains("\"$x;\"")
        || !sexp.contains("\"word2\"")
    {
        return Err(format!("closed declaration-like qw changed behavior: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_ignores_close_in_following_print_string() -> Result<(), String> {
    let code = "my @items = qw(word\nprint \"x)\";";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("print-string recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 2 || parser.errors().is_empty() {
        return Err(format!("print string closer disabled recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_ignores_close_in_following_quote_operator() -> Result<(), String> {
    let code = "my @items = qw(word\nmy $x = q/)/;\nprint $x;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("quote-operator recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("quote-operator closer disabled recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

/// CR-only source is a valid Perl line ending and must take the same lexer
/// recovery boundary as LF source. Without CR-aware line-prefix detection,
/// this named subroutine is swallowed into the unclosed qw list.
#[test]
fn test_unclosed_qw_recovers_named_subroutine_after_cr_only_line_break() -> Result<(), String> {
    let code = "my @items = qw(word\rsub run { print 1; }";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("CR-only named-sub recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.get(1).map(|statement| &statement.kind),
            Some(NodeKind::Subroutine { .. })
        )
        || !sexp.contains("(sub run")
        || parser.errors().is_empty()
    {
        return Err(format!("CR-only named subroutine was swallowed: {sexp}"));
    }
    Ok(())
}

/// A closer inside a following print string is still content, while the
/// CR-only boundary before the statement must remain recoverable.
#[test]
fn test_unclosed_qw_preserves_print_closer_before_cr_only_statement() -> Result<(), String> {
    let code = "my @items = qw(word\rprint \"x)\";";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("CR-only print recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !sexp.contains("x)")
        || !sexp.contains("print")
        || parser.errors().is_empty()
    {
        return Err(format!("CR-only quote closer changed recovery boundaries: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_suffix_scan_disables_nested_qw_recovery() -> Result<(), String> {
    let code = "my @items = qw(word\nmy @nested = qw(inner\nprint 1;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("nested malformed qw failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.first().map(|node| &node.kind),
            Some(NodeKind::VariableDeclaration { .. })
        )
        || !matches!(
            statements.get(1).map(|node| &node.kind),
            Some(NodeKind::ExpressionStatement { .. })
        )
        || !sexp.contains("@nested")
        || !sexp.contains("print")
        || parser.errors().is_empty()
    {
        return Err(format!("nested malformed qw lost recovered statements: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_parenthesized_lexical_declaration() -> Result<(), String> {
    let code = "my @items = qw(word\nmy ($x) = 1;\nprint $x;";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("parenthesized my recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("parenthesized my was swallowed: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_compact_lexical_declarations() -> Result<(), String> {
    for (declaration, variable) in [
        ("my$x = 1;", "$ x"),
        ("our@x = ();", "@ x"),
        ("state%x = ();", "% x"),
        ("local$x = 1;", "$ x"),
    ] {
        let code = format!("my @items = qw(word\n{declaration}\nprint 1;");
        let mut parser = Parser::new(&code);
        let ast = parser.parse().map_err(|error| format!("compact recovery failed: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 3
            || !matches!(
                statements.get(1).map(|node| &node.kind),
                Some(NodeKind::VariableDeclaration { .. })
            )
            || !sexp.contains(variable)
            || parser.errors().is_empty()
        {
            return Err(format!("compact declaration was swallowed: {}", ast.to_sexp()));
        }
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_semicolon_probe_ignores_nested_declaration() -> Result<(), String> {
    let code = "my @items = qw(word\nmy $x = do {\nmy $inner = 1;\n$inner;\n};\nprint $x;";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("nested declaration recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 3 || parser.errors().is_empty() {
        return Err(format!("nested declaration interrupted semicolon probe: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_long_single_line_stays_linear_candidate_scan() -> Result<(), String> {
    let words = "word ".repeat(8_192);
    let code = format!("my @items = qw({words}");
    let mut parser = Parser::new(&code);
    let ast = parser.parse().map_err(|error| format!("long single-line qw failed: {error}"))?;
    if parser.errors().is_empty() {
        return Err(format!("long unclosed qw recorded no recovery: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_non_parenthesized_qw_keeps_existing_behavior() -> Result<(), String> {
    let code = "my @items = qw[word\nmy $x = 1;";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("non-parenthesized qw failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 1 {
        return Err(format!("non-parenthesized qw recovery behavior changed: {}", ast.to_sexp()));
    }
    let errors = format!("{:?}", parser.errors());
    if !errors.contains("Unclosed qw() delimiter: missing closing delimiter before end of file")
        || errors.contains("InsertedCloser")
    {
        return Err(format!("non-parenthesized qw diagnostic changed: {errors}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_semicolonless_trailing_declaration() -> Result<(), String> {
    // #4494: a trailing declaration at EOF with no terminating semicolon must
    // still synchronize out of the unclosed qw( body instead of being swallowed.
    let code = "my @items = qw(word1 word2\nmy $x = 42";
    let mut parser = Parser::new(code);
    let ast = parser
        .parse()
        .map_err(|error| format!("semicolonless trailing declaration failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.first().map(|node| &node.kind),
            Some(NodeKind::VariableDeclaration { .. })
        )
        || !matches!(
            statements.get(1).map(|node| &node.kind),
            Some(NodeKind::VariableDeclaration { .. })
        )
    {
        return Err(format!("semicolonless trailing declaration was swallowed: {sexp}"));
    }
    if !sexp.contains("\"word1\"") || !sexp.contains("\"word2\"") || !sexp.contains("$ x") {
        return Err(format!("recovery lost qw words or trailing declaration: {sexp}"));
    }
    if parser.errors().is_empty() {
        return Err("semicolonless recovery did not record a recovery diagnostic".to_string());
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_semicolonless_trailing_print() -> Result<(), String> {
    // #4494: a trailing print statement at EOF without a semicolon must recover.
    let code = "my @items = qw(word1 word2\nprint \"tail\"";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("semicolonless trailing print failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.get(1).map(|node| &node.kind),
            Some(NodeKind::ExpressionStatement { .. })
        )
        || !sexp.contains("print")
        || !sexp.contains("\"word1\"")
    {
        return Err(format!("semicolonless trailing print was swallowed: {sexp}"));
    }
    if parser.errors().is_empty() {
        return Err("semicolonless print recovery did not record an error".to_string());
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_recovers_semicolonless_compact_declarations() -> Result<(), String> {
    // #4494: sigil-adjacent declaration keywords must recover at EOF without a semicolon.
    for (declaration, variable) in [
        ("my$x = 1", "$ x"),
        ("our@x = ()", "@ x"),
        ("state%x = ()", "% x"),
        ("local$x = 1", "$ x"),
    ] {
        let code = format!("my @items = qw(word\n{declaration}");
        let mut parser = Parser::new(&code);
        let ast = parser
            .parse()
            .map_err(|error| format!("compact semicolonless recovery failed: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 2
            || !matches!(
                statements.get(1).map(|node| &node.kind),
                Some(NodeKind::VariableDeclaration { .. })
            )
            || !sexp.contains(variable)
            || parser.errors().is_empty()
        {
            return Err(format!("compact semicolonless declaration was swallowed: {sexp}"));
        }
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_semicolonless_keeps_unbalanced_trailing_as_content() -> Result<(), String> {
    // #4494 negative control: a trailing statement whose delimiters do not balance
    // at EOF is not a clean recovery boundary and must not be treated as a statement.
    let code = "my @items = qw(word\nmy $x = (1, 2";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("unbalanced trailing recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 1 || parser.errors().is_empty() {
        return Err(format!("unbalanced trailing statement was falsely split: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_closed_qw_semicolonless_keeps_keyword_words_at_eof() -> Result<(), String> {
    // #4494 negative control: a closed qw ending at EOF with no trailing semicolon and a
    // line-start declaration-shaped word must stay closed content, not trigger recovery.
    let code = "my @items = qw(word1\nmy $x word2)";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("closed eof qw failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 1
        || !parser.errors().is_empty()
        || !sexp.contains("\"word1\"")
        || !sexp.contains("\"my\"")
        || !sexp.contains("\"word2\"")
    {
        return Err(format!("closed eof qw changed behavior: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_semicolonless_keeps_unterminated_regex_as_content() -> Result<(), String> {
    // #4494 negative control: a trailing statement whose own bare `/regex/` is unterminated
    // consumes to EOF without emitting a token. It must not be misclassified as a clean
    // trailing statement (which would split the qw and silently drop the regex text).
    let code = "my @items = qw(word1 word2\nprint $y =~ /unterminated";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("unterminated regex recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 1 || parser.errors().is_empty() {
        return Err(format!("unterminated trailing regex was falsely split: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_semicolonless_keeps_unterminated_heredoc_as_content() -> Result<(), String> {
    // #4494 negative control: a trailing declaration opening a heredoc with no closing
    // terminator is not a complete statement and must stay swallowed, not split the qw.
    let code = "my @items = qw(word1 word2\nmy $x = <<EOF\nbody line with no terminator";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("unterminated heredoc recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 1 || parser.errors().is_empty() {
        return Err(format!("unterminated trailing heredoc was falsely split: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_semicolonless_recovers_regex_match_statement() -> Result<(), String> {
    // #4494: a trailing statement containing a *closed* bind/match still recovers at EOF —
    // the unterminated-construct guard must not over-reject well-formed trailing statements.
    let code = "my @items = qw(word1 word2\nprint $y =~ /done/";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("closed regex recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.get(1).map(|node| &node.kind),
            Some(NodeKind::ExpressionStatement { .. })
        )
        || !sexp.contains("\"word1\"")
        || parser.errors().is_empty()
    {
        return Err(format!("closed trailing match failed to recover: {sexp}"));
    }
    Ok(())
}

// -- #4491: block-form and parenthesized statement starters after unclosed qw( --

/// Every supported block/parenthesized starter must synchronize out of the
/// unclosed qw into its own statement instead of being swallowed as words.
#[test]
fn test_unclosed_qw_recovers_block_form_starters() -> Result<(), String> {
    // Each row also names the recovered node's sexp head so the assertion proves the
    // starter parsed into its *own* declaration/phase/expression node, not merely that
    // the statement count rose (a fallback Error node would also lift the count).
    for (label, code, swallowed_marker, recovered_head) in [
        ("sub block", "my @a = qw(word\nsub run { print 1; }", "\"sub\"", "(sub run"),
        ("sub multiline block", "my @a = qw(word\nsub run\n{ print 1; }", "\"sub\"", "(sub run"),
        (
            "sub multiline prototype block",
            "my @a = qw(word\nsub run\n($) { print 1; }",
            "\"sub\"",
            "(sub run",
        ),
        (
            "sub multiline prototype string block",
            "my @a = qw(word\nsub run\n($x = \")\") { print 1; }",
            "\"sub\"",
            "(sub run",
        ),
        (
            "sub multiline prototype regex block",
            "my @a = qw(word\nsub run\n($x = m/'foo/) { print 1; }",
            "\"sub\"",
            "(sub run",
        ),
        ("package block", "my @a = qw(word\npackage Foo { 1; }", "\"package\"", "(package Foo"),
        (
            "package multiline block",
            "my @a = qw(word\npackage Foo\n{ 1; }",
            "\"package\"",
            "(package Foo",
        ),
        (
            "package version block",
            "my @a = qw(word\npackage Foo 1.23 { 1; }",
            "\"package\"",
            "(package Foo",
        ),
        ("package semi", "my @a = qw(word\npackage Foo;", "\"package\"", "(package Foo"),
        ("class block", "my @a = qw(word\nclass Foo { 1; }", "\"class\"", "(class Foo"),
        ("BEGIN block", "my @a = qw(word\nBEGIN { 1; }", "\"BEGIN\"", "(BEGIN"),
        ("END block", "my @a = qw(word\nEND { 1; }", "\"END\"", "(END"),
        ("INIT block", "my @a = qw(word\nINIT { 1; }", "\"INIT\"", "(INIT"),
        ("CHECK block", "my @a = qw(word\nCHECK { 1; }", "\"CHECK\"", "(CHECK"),
        ("UNITCHECK block", "my @a = qw(word\nUNITCHECK { 1; }", "\"UNITCHECK\"", "(UNITCHECK"),
        ("print paren", "my @a = qw(word\nprint(\"hi\");", "\"print(\\\"hi\\\");\"", "print"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        // Recovered as a second statement, and the starter is no longer a qw word.
        if statements.len() != 2 || sexp.contains(swallowed_marker) {
            return Err(format!("[{label}] starter was swallowed by unclosed qw: {sexp}"));
        }
        // The recovered node is the real construct (not an Error fallback) and carries
        // its declared name / phase.
        let recovered = &statements[1];
        if matches!(recovered.kind, NodeKind::Error { .. }) {
            return Err(format!(
                "[{label}] recovered into an Error node, not a declaration: {sexp}"
            ));
        }
        if !recovered.to_sexp().contains(recovered_head) {
            return Err(format!(
                "[{label}] recovered node is not the expected starter (want {recovered_head:?}): {}",
                recovered.to_sexp()
            ));
        }
        if !sexp.contains("\"word\"") {
            return Err(format!("[{label}] lost the recovered qw content: {sexp}"));
        }
        if parser.errors().is_empty() {
            return Err(format!("[{label}] unclosed qw recorded no error"));
        }
    }
    Ok(())
}

/// Leading-qualified declaration names are valid parser forms and must remain
/// recovery boundaries after an unclosed `qw(`.
#[test]
fn test_unclosed_qw_recovers_leading_qualified_declarations() -> Result<(), String> {
    for (label, code, name, marker, expected_kind) in [
        (
            "sub leading-qualified",
            "my @a = qw(word\nsub ::PCDATA { 1; }",
            "PCDATA",
            "\"sub\"",
            "sub",
        ),
        (
            "package leading-qualified",
            "my @a = qw(word\npackage ::My::App { 1; }",
            "My::App",
            "\"package\"",
            "package",
        ),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] parse failed: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        let recovered_kind_matches =
            statements.get(1).is_some_and(|statement| match expected_kind {
                "sub" => matches!(&statement.kind, NodeKind::Subroutine { .. }),
                "package" => matches!(&statement.kind, NodeKind::Package { .. }),
                _ => false,
            });
        if statements.len() != 2
            || matches!(&statements[1].kind, NodeKind::Error { .. })
            || !recovered_kind_matches
            || sexp.contains(marker)
            || !sexp.contains(name)
            || parser.errors().is_empty()
        {
            return Err(format!("[{label}] qualified declaration was swallowed: {sexp}"));
        }
    }
    Ok(())
}

#[test]
fn test_unclosed_qw_rejects_extra_named_header_words() -> Result<(), String> {
    for (label, code, marker) in [
        ("sub extra header", "my @a = qw(word\nsub run extra { 1; }", "\"sub\""),
        ("package extra header", "my @a = qw(word\npackage Foo extra { 1; }", "\"package\""),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] parse failed: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 1 || !sexp.contains(marker) || !sexp.contains("extra") {
            return Err(format!("[{label}] invalid header became a recovery boundary: {sexp}"));
        }
    }
    Ok(())
}

/// #4491 review (codex P2): a strong block starter must recover even when another
/// line-start statement follows it — the block shape is self-contained, so the
/// declaration is not swallowed into the qw just because more code trails it.
#[test]
fn test_unclosed_qw_block_starter_recovers_before_trailing_statement() -> Result<(), String> {
    for (label, code, recovered_head, trailing_is_decl) in [
        ("sub then my", "my @a = qw(word\nsub run { print 1; }\nmy $x = 1;", "(sub run", true),
        (
            "package then print",
            "my @a = qw(word\npackage Foo { 1; }\nprint 2;",
            "(package Foo",
            false,
        ),
        ("BEGIN then my", "my @a = qw(word\nBEGIN { 1; }\nmy $y = 2;", "(BEGIN", true),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        // decl (qw) · recovered starter · trailing statement == three separate nodes.
        if statements.len() != 3 {
            return Err(format!(
                "[{label}] block starter did not split from trailing code: {sexp}"
            ));
        }
        if !statements[1].to_sexp().contains(recovered_head) {
            return Err(format!("[{label}] middle node is not the recovered starter: {sexp}"));
        }
        // The trailing statement is the real parsed construct, not an Error fallback —
        // otherwise a boundary that mis-parses the tail would still satisfy the count.
        let trailing = &statements[2];
        let trailing_ok = if trailing_is_decl {
            matches!(trailing.kind, NodeKind::VariableDeclaration { .. })
        } else {
            matches!(trailing.kind, NodeKind::ExpressionStatement { .. })
        };
        if !trailing_ok {
            return Err(format!(
                "[{label}] trailing statement did not parse cleanly, got {:?}: {sexp}",
                trailing.kind
            ));
        }
        if !sexp.contains("\"word\"") {
            return Err(format!("[{label}] lost the recovered qw content: {sexp}"));
        }
    }
    Ok(())
}

/// A `sub`/`package`-shaped word with no block or terminating `;` is ordinary qw
/// content and must not create a false boundary at EOF.
#[test]
fn test_unclosed_qw_block_starter_word_without_shape_stays_word() -> Result<(), String> {
    for (label, code) in [
        ("bare sub words", "my @a = qw(word\nsub run more"),
        ("bare package words", "my @a = qw(word\npackage more words"),
        ("bare class words", "my @a = qw(word\nclass more words"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 1 || !sexp.contains("\"more\"") {
            return Err(format!("[{label}] keyword-like word triggered a false boundary: {sexp}"));
        }
        if parser.errors().is_empty() {
            return Err(format!("[{label}] unclosed qw recorded no error"));
        }
    }
    Ok(())
}

/// Comments after a named block starter do not satisfy its required name. The
/// following-line identifier and block opener belong to qw content instead of a
/// declaration borrowed from later source.
#[test]
fn test_unclosed_qw_block_starter_comment_does_not_fake_name() -> Result<(), String> {
    for (label, code, retained_word) in [
        ("sub comment", "my @a = qw(word\nsub # still qw content\nrun\n{ 1; }", "run"),
        ("package comment", "my @a = qw(word\npackage # still qw content\nFoo\n{ 1; }", "Foo"),
        ("class comment", "my @a = qw(word\nclass # still qw content\nFoo\n{ 1; }", "Foo"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 1 || !sexp.contains(&format!("\"{retained_word}\"")) {
            return Err(format!("[{label}] comment faked a block-starter name: {sexp}"));
        }
        if parser.errors().is_empty() {
            return Err(format!("[{label}] unclosed qw recorded no error"));
        }
    }
    Ok(())
}

/// General parser synchronization must preserve phaser blocks just like the
/// delimiter-recovery path does after a malformed preceding statement.
#[test]
fn test_synchronize_preserves_phaser_blocks() -> Result<(), String> {
    for (label, phaser) in [
        ("BEGIN", "BEGIN"),
        ("END", "END"),
        ("INIT", "INIT"),
        ("CHECK", "CHECK"),
        ("UNITCHECK", "UNITCHECK"),
    ] {
        let code = format!("my $x = 1; ???\n{phaser} {{ do_thing() }}\nprint 2;");
        let mut parser = Parser::new(&code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let sexp = ast.to_sexp();
        if !sexp.contains(&format!("({label}")) || !sexp.contains("print") {
            return Err(format!("[{label}] phaser was consumed during synchronization: {sexp}"));
        }
    }
    Ok(())
}

/// Keyword and v-string names accepted by the parser must also be recognized
/// as `sub` recovery boundaries by the lexer.
#[test]
fn test_unclosed_qw_recovers_keyword_and_vstring_sub_names() -> Result<(), String> {
    for (label, code, recovered) in [
        ("keyword name", "my @a = qw(word\nsub return { 1; }", "(sub return"),
        ("v-string name", "my @a = qw(word\nsub v5 { 1; }", "(sub v5"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        if statements.len() != 2 || !sexp.contains(recovered) {
            return Err(format!("[{label}] sub name was swallowed: {sexp}"));
        }
    }
    Ok(())
}

/// Dotted v-strings are not valid named subroutine declarations; they remain
/// quote-word content instead of creating a false recovery boundary.
#[test]
fn test_unclosed_qw_rejects_dotted_vstring_sub_name() -> Result<(), String> {
    let code = "my @a = qw(word\nsub v1.2 { 1; }";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("dotted v-string parse failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 1 || !sexp.contains("v1.2") || parser.errors().is_empty() {
        return Err(format!("dotted v-string became a false boundary: {sexp}"));
    }
    Ok(())
}

/// Phaser attributes still split a recovered unclosed `qw(` at their block,
/// preserving the labeled statement for the parser rather than swallowing it.
#[test]
fn test_unclosed_qw_recovers_phaser_attribute_block() -> Result<(), String> {
    let code = "my @a = qw(word\nBEGIN :lvalue { 1; }";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("phaser attribute parse failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            &statements[1].kind,
            NodeKind::LabeledStatement { label, .. } if label == "BEGIN"
        )
        || parser.errors().is_empty()
    {
        return Err(format!("phaser attribute was swallowed: {sexp}"));
    }
    Ok(())
}

/// A mismatched nested delimiter is not a complete subroutine body and must
/// not become a recovery boundary for an unclosed `qw(`.
#[test]
fn test_unclosed_qw_rejects_mismatched_block_delimiters() -> Result<(), String> {
    let code = "my @a = qw(word\nsub run { ( ] }";
    let mut parser = Parser::new(code);
    let ast = parser.parse().map_err(|error| format!("mismatched block parse failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 1 || !sexp.contains("\"sub\"") || parser.errors().is_empty() {
        return Err(format!("mismatched block became a false boundary: {sexp}"));
    }
    Ok(())
}

/// Closed multiline qw content containing block-starter-shaped words (including a
/// balanced `{ }` group) must keep its existing single-declaration behavior.
#[test]
fn test_closed_multiline_qw_keeps_block_starter_words() -> Result<(), String> {
    for (label, code) in [
        ("closed sub words", "my @a = qw(word\nsub run more);"),
        ("closed package words", "my @a = qw(alpha\npackage beta\ngamma);"),
        ("closed brace group", "my @a = qw(word\nsub run { 1 } more);"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not parse: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        if statements.len() != 1 || !parser.errors().is_empty() {
            return Err(format!("[{label}] closed qw changed behavior: {}", ast.to_sexp()));
        }
    }
    Ok(())
}

/// Parenthesized `print(...)` recovers, but the whitespace form `print ...` keeps
/// its existing behavior (both should recover, via different paths).
#[test]
fn test_unclosed_qw_parenthesized_print_recovers() -> Result<(), String> {
    let code = "my @a = qw(word\nprint(join q{,}, 1, 2);";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("parenthesized print did not recover: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(
            statements.get(1).map(|node| &node.kind),
            Some(NodeKind::ExpressionStatement { .. })
        )
        || !sexp.contains("print")
    {
        return Err(format!("parenthesized print was swallowed: {sexp}"));
    }
    if parser.errors().is_empty() {
        return Err("unclosed qw before parenthesized print recorded no error".to_string());
    }
    Ok(())
}

/// #4491 review (blocker): a starter-shaped word that is bare quote-word content
/// must not borrow the block `{`/`;` of an unrelated statement on a *later* line.
/// Before the header-on-one-line guard these silently dropped the word and
/// mis-parsed the following real statement as a bogus declaration.
#[test]
fn test_unclosed_qw_block_starter_word_does_not_borrow_later_statement() -> Result<(), String> {
    for (label, code, keyword_word) in [
        ("sub then return-hashref", "my @a = qw(word\nsub\nreturn { a => 1 };", "\"sub\""),
        ("package then return", "my @a = qw(word\npackage\nreturn 5;", "\"package\""),
        (
            "class then method call",
            "my @a = qw(word\nclass->new(1)->run({ x => 1 });",
            "class->new",
        ),
        ("sub name on later line", "my @a = qw(word\nsub\nrun\n{ 1 }", "sub"),
        ("package name on later line", "my @a = qw(alpha\npackage\nbeta\n{ 1 }", "package"),
    ] {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("[{label}] did not recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("[{label}] expected program root, got {}", ast.to_sexp()));
        };
        let sexp = ast.to_sexp();
        // The keyword-shaped word stays inside the qw list; it is not consumed as a
        // declaration, and the following statement is not mis-parsed.
        if !sexp.contains(keyword_word) {
            return Err(format!("[{label}] keyword word was wrongly consumed: {sexp}"));
        }
        if statements.len() != 1 {
            return Err(format!("[{label}] borrowed a later statement's boundary: {sexp}"));
        }
    }
    Ok(())
}

/// An incomplete block remains quote-word content while the editor is still
/// typing it; recovery should not split on a declaration that has no closer.
#[test]
fn test_unclosed_qw_keeps_incomplete_block_as_content() -> Result<(), String> {
    let code = "my @items = qw(word1\nsub run { print 1;";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("incomplete block recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    if statements.len() != 1 || parser.errors().is_empty() {
        return Err(format!("incomplete trailing block was falsely split: {}", ast.to_sexp()));
    }
    Ok(())
}

/// Prefixes of supported starter keywords are ordinary qw words and must not
/// trigger the lexer boundary classifier.
#[test]
fn test_unclosed_qw_ignores_block_keyword_prefixes() -> Result<(), String> {
    for trailing in
        ["substr($x, 0, 1)", "classify { }", "packaged Foo { }", "printf(\"x\")", "BEGINNER { }"]
    {
        let code = format!("my @items = qw(word1 word2\n{trailing}");
        let mut parser = Parser::new(&code);
        let ast =
            parser.parse().map_err(|error| format!("`{trailing}` prefix parse failed: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected program root, got {}", ast.to_sexp()));
        };
        if statements.len() != 1 {
            return Err(format!("`{trailing}` prefix falsely split the qw: {}", ast.to_sexp()));
        }
    }
    Ok(())
}

/// Multibyte qw content must not disturb the byte offsets used when recovering
/// the following declaration.
#[test]
fn test_unclosed_qw_recovers_block_after_multibyte_content() -> Result<(), String> {
    let code = "my @items = qw(café 😀 word2\nsub run { print 1; }";
    let mut parser = Parser::new(code);
    let ast =
        parser.parse().map_err(|error| format!("multibyte block recovery failed: {error}"))?;
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected program root, got {}", ast.to_sexp()));
    };
    let sexp = ast.to_sexp();
    if statements.len() != 2
        || !matches!(&statements[1].kind, NodeKind::Subroutine { .. })
        || !sexp.contains("sub run")
        || parser.errors().is_empty()
    {
        return Err(format!("multibyte block recovery lost the sub: {sexp}"));
    }
    Ok(())
}

#[test]
fn test_recovery_unclosed_q_brace() {
    let code = "my $str = q{ hello world print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    assert!(result.is_ok(), "Parser should recover from unclosed q braces");
    let ast = must(result);
    if let NodeKind::Program { statements } = &ast.kind {
        assert!(!statements.is_empty(), "Should have recovered statements");
    }
    assert!(!parser.errors().is_empty(), "Should record unclosed brace error");
}

#[test]
fn test_recovery_unclosed_qq() {
    let code = "my $name = \"unknown; print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    assert!(result.is_ok(), "Parser should recover from unclosed qq string");
    assert!(!parser.errors().is_empty(), "Should record unclosed quote error");
}

#[test]
fn test_recovery_missing_rhs_before_class_declaration_keyword() {
    // `class` is a Perl 5.38+ declaration starter that can never be a valid
    // expression RHS.  Without the fix, `is_infix_rhs_absent` did not treat
    // `class` as a strong follower, so the parser consumed the class declaration
    // as the assignment RHS, producing a wrong AST shape and losing the Class node.
    // With the fix, the incomplete assignment recovers and the class declaration
    // is preserved as a separate top-level statement.
    let code = "my $x = class Foo { }";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should recover from missing RHS before class declaration");
    let ast = must(result);
    let sexp = ast.to_sexp();

    assert!(matches!(&ast.kind, NodeKind::Program { .. }), "Expected program root; sexp: {sexp}");

    if let NodeKind::Program { statements } = &ast.kind {
        assert_eq!(
            statements.len(),
            2,
            "Should recover into 2 statements (incomplete decl + class); sexp: {sexp}"
        );
        assert!(
            matches!(statements[1].kind, NodeKind::Class { .. }),
            "Second statement should be a Class node; sexp: {sexp}"
        );
    }
}

#[test]
fn test_recovery_missing_rhs_before_method_declaration_keyword() {
    // `method` is a Perl 5.38+ declaration keyword that is never a valid
    // expression RHS at the top level.  The parser must treat it as a strong
    // follower so the assignment emits a missing-RHS recovery and the method
    // declaration is not swallowed as an expression operand.
    let code = "my $x = method foo { }";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should recover from missing RHS before method declaration");
    let ast = must(result);
    let sexp = ast.to_sexp();

    assert!(matches!(&ast.kind, NodeKind::Program { .. }), "Expected program root; sexp: {sexp}");

    if let NodeKind::Program { statements } = &ast.kind {
        assert!(statements.len() >= 2, "Should recover into at least 2 statements; sexp: {sexp}");
    }
}

#[test]
fn test_recovery_missing_rhs_before_format_declaration_keyword() {
    // `format` is a declaration starter that should never be consumed as an
    // expression RHS.  The is_infix_rhs_absent fix ensures recovery fires
    // before the format body lexer mode is entered in the wrong context.
    let code = "my $x = format Foo =\n.\n";
    let mut parser = Parser::new(code);
    let result = parser.parse();

    assert!(result.is_ok(), "Parser should recover from missing RHS before format declaration");
    let ast = must(result);
    let sexp = ast.to_sexp();

    assert!(matches!(&ast.kind, NodeKind::Program { .. }), "Expected program root; sexp: {sexp}");

    if let NodeKind::Program { statements } = &ast.kind {
        assert!(statements.len() >= 2, "Should recover into at least 2 statements; sexp: {sexp}");
        assert!(
            statements.iter().any(|s| matches!(s.kind, NodeKind::Format { .. })),
            "Should contain a Format node; sexp: {sexp}"
        );
    }
}

#[test]
fn test_recovery_nested_qw_paren_mismatch() {
    let code = "my @list = qw(one (two three) print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    assert!(result.is_ok(), "Parser should recover from nested paren in qw");
    assert!(!parser.errors().is_empty(), "Should record delimiter mismatch error");
}

#[test]
fn test_recovery_unclosed_s_slash() {
    // `s/pattern` with no replacement or closing delimiter
    let code = "my $x = s/pattern; print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    assert!(result.is_ok(), "Parser should recover from unclosed s///");
    assert!(!parser.errors().is_empty(), "Should record unclosed s delimiter error");
}

#[test]
fn test_recovery_unclosed_s_replacement() {
    // Pattern closes but replacement delimiter is never opened
    let code = "s/find/; print 1;";
    let mut parser = Parser::new(code);
    let result = parser.parse();
    assert!(result.is_ok(), "Parser should recover from s/ with unclosed replacement");
    assert!(!parser.errors().is_empty(), "Should record unclosed s delimiter error");
}
