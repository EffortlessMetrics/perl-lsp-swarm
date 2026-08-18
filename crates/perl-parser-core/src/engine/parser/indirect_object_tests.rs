#[cfg(test)]
mod tests {
    use crate::engine::parser::{Parser, ParserDecision, ParserDecisionTrace};
    use crate::error::{ParseError, RecoveryKind};
    use perl_ast::ast::{Node, NodeKind};

    fn parse_code(input: &str) -> Option<Node> {
        let mut parser = Parser::new(input);
        parser.parse().ok()
    }

    /// The sole top-level statement, with its transport node intact.
    ///
    /// Behavior proofs must use this rather than a helper that unwraps
    /// `ExpressionStatement`: the bareword and parenthesized call routes differ
    /// precisely in whether the `FunctionCall` carries that transport, and #6908
    /// forbids a helper that erases the distinction a test is meant to observe.
    fn sole_top_level_statement(ast: Node) -> Result<Node, String> {
        match ast.kind {
            NodeKind::Program { mut statements } => {
                if statements.len() != 1 {
                    return Err(format!(
                        "expected exactly one statement, got {}",
                        statements.len()
                    ));
                }
                statements.drain(..).next().ok_or_else(|| "expected one statement".to_string())
            }
            other => Err(format!("Expected Program node, got {other:?}")),
        }
    }

    fn first_statement(input: &str) -> Node {
        let ast = parse_code(input).expect("source should parse");
        match ast.kind {
            NodeKind::Program { mut statements } => {
                statements.drain(..).next().expect("expected one statement")
            }
            other => panic!("Expected Program node, got {other:?}"),
        }
    }

    #[test]
    fn test_builtin_indirect_syntax() {
        let stmt = first_statement("print $fh \"Hello\";");
        match stmt.kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "print");
                assert!(matches!(
                    object.kind,
                    NodeKind::Variable { ref sigil, ref name }
                        if sigil == "$" && name == "fh"
                ));
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected IndirectCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_new_indirect_syntax() {
        let stmt = first_statement("new Player \"Steven\";");
        match stmt.kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "new");
                assert!(matches!(
                    object.kind,
                    NodeKind::Identifier { ref name } if name == "Player"
                ));
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected IndirectCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_lowercase_name_is_function_call() {
        let source = "my_custom_method $obj 10, 20;";
        let stmt = first_statement(source);
        match stmt.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
                assert_eq!(
                    stmt.location.end,
                    source.len() - 1,
                    "FunctionCall span must cover all arguments"
                );
            }
            other => {
                panic!("Unknown lowercase names must remain FunctionCall nodes, got {other:?}")
            }
        }
    }

    #[test]
    fn test_parenthesized_expression_statement_span_keeps_closer() {
        let source = "(42);";
        let stmt = first_statement(source);
        assert_eq!(
            stmt.location.end,
            source.len() - 1,
            "ExpressionStatement span must include the closing parenthesis"
        );
        match stmt.kind {
            NodeKind::ExpressionStatement { expression } => {
                assert_eq!(expression.location.start, 1);
                assert_eq!(expression.location.end, 3);
            }
            other => panic!("Expected ExpressionStatement node, got {other:?}"),
        }
    }

    #[test]
    fn test_parenthesized_builtin_call_span_keeps_closer() {
        let source = "print(1);";
        let stmt = first_statement(source);
        let expression = match stmt.kind {
            NodeKind::ExpressionStatement { expression } => expression,
            other => panic!("Expected ExpressionStatement node, got {other:?}"),
        };
        match expression.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "print");
                assert_eq!(args.len(), 1);
                assert_eq!(
                    expression.location.end,
                    source.len() - 1,
                    "parenthesized builtin FunctionCall span must include ')'"
                );
            }
            other => panic!("Expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_unparenthesized_builtin_call_span_covers_argument() {
        let source = "print qq/value=$café/;";
        let stmt = first_statement(source);
        let statement_end = stmt.location.end;
        let expression = match stmt.kind {
            NodeKind::ExpressionStatement { expression } => expression,
            other => panic!("Expected ExpressionStatement node, got {other:?}"),
        };
        assert_eq!(
            statement_end,
            source.len() - 1,
            "ExpressionStatement span must cover its expression"
        );
        match expression.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "print");
                assert_eq!(args.len(), 1);
                assert_eq!(
                    expression.location.end,
                    source.len() - 1,
                    "builtin FunctionCall span must cover its argument"
                );
            }
            other => panic!("Expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_require_call_span_covers_string_argument() {
        let source = "require 'relative/file.pl';";
        let stmt = first_statement(source);
        let statement_end = stmt.location.end;
        let expression = match stmt.kind {
            NodeKind::ExpressionStatement { expression } => expression,
            other => panic!("Expected ExpressionStatement node, got {other:?}"),
        };
        assert_eq!(statement_end, source.len() - 1);
        match expression.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "require");
                assert_eq!(args.len(), 1);
                assert_eq!(expression.location.end, source.len() - 1);
            }
            other => panic!("Expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_lowercase_name_preserves_nested_arguments_and_exercises_bareword_path()
    -> Result<(), String> {
        // Keep the bare unknown-lowercase call shape so this exercises
        // `is_unknown_lowercase_bareword_call_pattern` / the specialized
        // production path (not generic parenthesized-call parsing).
        let source = "my_custom_method $obj ($title // 'Untitled'), $options->{limit};";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|error| {
            format!("source should parse through production entrypoint: {error}")
        })?;
        // Route evidence: the production decision must have executed, not merely a
        // predicate that happened to return true beforehand.
        let expected_trace = [ParserDecisionTrace {
            decision: ParserDecision::UnknownLowercaseBarewordCall,
            start: 0,
            end: "my_custom_method".len(),
        }];
        if parser.decision_trace() != expected_trace {
            return Err(format!(
                "public parse did not emit the expected unknown-lowercase route: {:?}",
                parser.decision_trace()
            ));
        }
        // Route geometry: the bareword path returns the call unwrapped. The
        // parenthesized control keeps an ExpressionStatement transport, so this
        // assertion discriminates between the two routes on the AST alone.
        let stmt = sole_top_level_statement(ast)?;
        if matches!(stmt.kind, NodeKind::ExpressionStatement { .. }) {
            return Err("bareword route unexpectedly wrapped the call in an ExpressionStatement"
                .to_string());
        }
        match stmt.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
                assert!(matches!(
                    &args[0].kind,
                    NodeKind::Variable {
                        sigil,
                        name
                    } if sigil == "$" && name == "obj"
                ));
                assert_eq!(args[0].location.start, 17);
                assert_eq!(args[0].location.end, 21);
                match &args[1].kind {
                    NodeKind::Binary { op, left, right } => {
                        assert_eq!(op, "//", "second arg must keep defined-or Binary shape");
                        assert!(matches!(
                            &left.kind,
                            NodeKind::Variable {
                                sigil,
                                name
                            } if sigil == "$" && name == "title"
                        ));
                        assert!(matches!(
                            &right.kind,
                            NodeKind::String { value, .. } if value == "'Untitled'"
                        ));
                        assert_eq!(args[1].location.start, 23);
                        assert_eq!(args[1].location.end, 43);
                    }
                    other => panic!("Expected Binary // for second arg, got {other:?}"),
                }
                match &args[2].kind {
                    NodeKind::Binary { op, left, right } => {
                        assert_eq!(op, "->{}", "third arg must keep arrow-hash-deref shape");
                        assert!(matches!(
                            &left.kind,
                            NodeKind::Variable {
                                sigil,
                                name
                            } if sigil == "$" && name == "options"
                        ));
                        assert!(matches!(
                            &right.kind,
                            NodeKind::Identifier { name } if name == "limit"
                        ));
                        assert_eq!(args[2].location.start, 46);
                        assert_eq!(args[2].location.end, 63);
                    }
                    other => panic!("Expected Binary ->{{}} for third arg, got {other:?}"),
                }
                assert_eq!(
                    stmt.location.end,
                    source.len() - 1,
                    "FunctionCall span must cover trailing arguments"
                );
                Ok(())
            }
            other => Err(format!("Expected FunctionCall node, got {other:?}")),
        }
    }

    /// Negative control: a parenthesized ordinary call reaches the same
    /// `FunctionCall` through the ordinary expression route. It must emit no
    /// bareword-route evidence, and it keeps the `ExpressionStatement` transport
    /// the bareword route does not produce.
    #[test]
    fn test_parenthesized_call_does_not_satisfy_unknown_lowercase_bareword_path()
    -> Result<(), String> {
        let source = "my_custom_method($obj, ($title // 'Untitled'), $options->{limit});";
        let mut parser = Parser::new(source);
        let ast = parser
            .parse()
            .map_err(|error| format!("parenthesized control should parse: {error}"))?;
        if !parser.decision_trace().is_empty() {
            return Err(format!(
                "parenthesized control emitted bareword-route evidence: {:?}",
                parser.decision_trace()
            ));
        }
        let stmt = sole_top_level_statement(ast)?;
        let NodeKind::ExpressionStatement { expression } = stmt.kind else {
            return Err(format!(
                "parenthesized control should keep its ExpressionStatement transport, got {:?}",
                stmt.kind
            ));
        };
        match expression.kind {
            NodeKind::FunctionCall { name, args }
                if name == "my_custom_method" && args.len() == 3 =>
            {
                Ok(())
            }
            other => Err(format!(
                "parenthesized control should remain an ordinary FunctionCall, got {other:?}"
            )),
        }
    }

    /// Negative control: suppressing only the route evidence must leave the public
    /// AST identical while the positive proof fails. This is the AST-equivalent
    /// falsifier, so a regression cannot satisfy the path-specific obligation by
    /// producing the right shape through the wrong decision.
    #[test]
    fn test_ast_equivalent_bypass_does_not_satisfy_bareword_route_proof() -> Result<(), String> {
        let source = "my_custom_method $obj ($title // 'Untitled'), $options->{limit};";
        let mut parser = Parser::new(source);
        parser.set_unknown_lowercase_bareword_decision_bypass_for_test(true);
        let ast = parser
            .parse()
            .map_err(|error| format!("mutation control should preserve the public AST: {error}"))?;
        if !parser.decision_trace().is_empty() {
            return Err(format!(
                "bypassed predicate still emitted route evidence: {:?}",
                parser.decision_trace()
            ));
        }
        let stmt = sole_top_level_statement(ast)?;
        if matches!(stmt.kind, NodeKind::ExpressionStatement { .. }) {
            return Err("mutation control must not move the source onto another route".to_string());
        }
        match stmt.kind {
            NodeKind::FunctionCall { name, args }
                if name == "my_custom_method" && args.len() == 3 =>
            {
                Ok(())
            }
            other => Err(format!(
                "mutation control must preserve the final FunctionCall shape, got {other:?}"
            )),
        }
    }

    /// The accepted recovery boundary for this exact malformed source: the later
    /// declaration stays inside the unclosed quote-word construct.
    #[test]
    fn test_unclosed_non_parenthesized_qw_keeps_trailing_declaration_inside_recovery_boundary()
    -> Result<(), String> {
        let source = "my @items = qw[word\nmy $x = 1;";
        let mut parser = Parser::new(source);
        let ast =
            parser.parse().map_err(|error| format!("malformed qw should recover: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected Program root, got {}", ast.to_sexp()));
        };
        if statements.len() != 1
            || !matches!(
                statements.first().map(|node| &node.kind),
                Some(NodeKind::VariableDeclaration { .. })
            )
        {
            return Err(format!(
                "unclosed qw released a trailing top-level declaration: {}",
                ast.to_sexp()
            ));
        }
        let diagnostics = format!("{:?}", parser.errors());
        if !diagnostics
            .contains("Unclosed qw() delimiter: missing closing delimiter before end of file")
        {
            return Err(format!("unclosed qw diagnostic contract changed: {diagnostics}"));
        }
        // Match the recovery variant structurally. Scanning `Debug` output would
        // couple the proof to a derive, and would also accept an unrelated field
        // whose text happens to contain the variant name.
        if parser.errors().iter().any(|error| {
            matches!(error, ParseError::Recovered { kind: RecoveryKind::InsertedCloser, .. })
        }) {
            return Err(format!(
                "unclosed qw must not report an inserted-closer recovery: {diagnostics}"
            ));
        }
        Ok(())
    }

    /// Negative control: closing the delimiter releases the following declaration
    /// as a second top-level statement, with no diagnostics.
    #[test]
    fn test_closed_non_parenthesized_qw_releases_following_declaration() -> Result<(), String> {
        let source = "my @items = qw[word];\nmy $x = 1;";
        let mut parser = Parser::new(source);
        let ast =
            parser.parse().map_err(|error| format!("closed qw control should parse: {error}"))?;
        let NodeKind::Program { statements } = &ast.kind else {
            return Err(format!("expected Program root, got {}", ast.to_sexp()));
        };
        if statements.len() != 2
            || !statements
                .iter()
                .all(|node| matches!(&node.kind, NodeKind::VariableDeclaration { .. }))
            || !parser.errors().is_empty()
        {
            return Err(format!(
                "closed qw control did not preserve two declarations: {} / {:?}",
                ast.to_sexp(),
                parser.errors()
            ));
        }
        Ok(())
    }

    #[test]
    fn test_unknown_lowercase_name_inside_control_flow_is_function_call() {
        // Bare call inside the block remains the specialized bareword path.
        let stmt = first_statement("if ($enabled) { my_custom_method $obj 10, 20; }");
        let then_branch = match stmt.kind {
            NodeKind::If { then_branch, .. } => then_branch,
            other => panic!("Expected If node, got {other:?}"),
        };
        let body = match then_branch.kind {
            NodeKind::Block { statements } => statements,
            other => panic!("Expected Block node, got {other:?}"),
        };
        // The bareword call path returns the call unwrapped; an
        // `ExpressionStatement` wrapper here is a regression, not an alternative.
        match &body[0].kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
            }
            other => panic!("Expected unwrapped FunctionCall node in block, got {other:?}"),
        }
    }

    #[test]
    fn test_comma_separated_user_call_is_not_indirect() {
        let stmt = first_statement("render $renderer, @parts;");
        assert!(
            !matches!(&stmt.kind, NodeKind::IndirectCall { .. }),
            "comma-separated user call must not be classified as indirect: {:?}",
            stmt.kind
        );
    }

    #[test]
    fn test_common_list_builtins_remain_regular_calls() {
        for source in ["push @items, $item;", "defined $object->method;", "sort @items;"] {
            assert!(
                !matches!(first_statement(source).kind, NodeKind::IndirectCall { .. }),
                "{source:?} must not be classified as indirect"
            );
        }
    }
}
