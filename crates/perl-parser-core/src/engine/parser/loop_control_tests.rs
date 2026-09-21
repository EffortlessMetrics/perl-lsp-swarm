#[cfg(test)]
mod tests {
    use crate::engine::parser::Parser;
    use perl_ast::ast::{Node, NodeKind, SourceLocation};
    use perl_tdd_support::must_some;

    fn parse_code(input: &str) -> Option<perl_ast::ast::Node> {
        let mut parser = Parser::new(input);
        parser.parse().ok()
    }

    #[test]
    fn test_next_last_redo_simple() {
        // AC1: recognize next, last, redo keywords
        let keywords = ["next", "last", "redo"];
        for kw in keywords {
            let source = format!("{};", kw);
            let ast_opt = parse_code(&source);
            assert!(ast_opt.is_some());
            let ast = ast_opt.unwrap_or_else(|| {
                Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
            });
            if let NodeKind::Program { statements } = &ast.kind {
                let stmt = &statements[0];
                if let NodeKind::LoopControl { op, label } = &stmt.kind {
                    assert_eq!(op, kw);
                    assert!(label.is_none());
                } else {
                    unreachable!("Expected LoopControl, got {:?}", stmt.kind);
                }
            }
        }
    }

    #[test]
    fn test_loop_control_with_label() {
        // AC3: labels supported for continue/redo
        let source = "next OUTER;";
        let ast_opt = parse_code(source);
        assert!(ast_opt.is_some());
        let ast = ast_opt.unwrap_or_else(|| {
            Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
        });
        if let NodeKind::Program { statements } = &ast.kind {
            let stmt = &statements[0];
            if let NodeKind::LoopControl { op, label } = &stmt.kind {
                assert_eq!(op, "next");
                assert_eq!(label.as_deref(), Some("OUTER"));
            }
        }
    }

    #[test]
    fn test_loop_control_in_while() {
        // AC2: Correct parsing in while loop
        let source = "while (1) { last; }";
        let ast_opt = parse_code(source);
        assert!(ast_opt.is_some());
        let ast = ast_opt.unwrap_or_else(|| {
            Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
        });
        if let NodeKind::Program { statements } = &ast.kind {
            let while_stmt = &statements[0];
            if let NodeKind::While { body, .. } = &while_stmt.kind
                && let NodeKind::Block { statements } = &body.kind
            {
                let last_stmt = &statements[0];
                assert!(matches!(last_stmt.kind, NodeKind::LoopControl { .. }));
            }
        }
    }

    #[test]
    fn test_last_and_redo_with_labels() {
        // Labels should be accepted for all loop-control ops.
        let cases = [("last OUTER;", "last"), ("redo INNER;", "redo")];

        for (source, expected_op) in cases {
            let ast = must_some(parse_code(source));
            assert!(
                matches!(ast.kind, NodeKind::Program { .. }),
                "expected Program, got {:?}",
                ast.kind
            );
            let NodeKind::Program { statements } = &ast.kind else {
                return;
            };
            let stmt = must_some(statements.first());
            assert!(
                matches!(stmt.kind, NodeKind::LoopControl { .. }),
                "expected LoopControl, got {:?}",
                stmt.kind
            );
            let NodeKind::LoopControl { op, label } = &stmt.kind else {
                return;
            };
            assert_eq!(op, expected_op);
            assert!(label.is_some(), "expected label for source: {source}");
        }
    }

    #[test]
    fn test_loop_control_with_statement_modifier() {
        // Parsing should keep loop-control statements intact when followed by modifiers.
        let source = "next if $should_skip;";
        let ast = must_some(parse_code(source));
        assert!(
            matches!(ast.kind, NodeKind::Program { .. }),
            "expected Program, got {:?}",
            ast.kind
        );
        let NodeKind::Program { statements } = &ast.kind else {
            return;
        };
        let stmt = must_some(statements.first());
        let sexp = stmt.to_sexp();
        assert!(
            sexp.contains("statement_modifier_if"),
            "Expected statement modifier wrapper, got: {sexp}"
        );
        assert!(sexp.contains("next"), "Expected loop control keyword, got: {sexp}");
    }

    #[test]
    fn test_loop_control_in_continue_block() {
        // Continue blocks are a common place for redo/next control flow.
        let source = "while ($x) { $x--; } continue { redo; }";
        let ast = must_some(parse_code(source));

        let sexp = ast.to_sexp();
        assert!(sexp.contains("redo"), "Expected redo in continue block, got: {sexp}");
        assert!(!sexp.contains("ERROR"), "Parse should not emit ERROR nodes: {sexp}");
    }

    #[test]
    fn test_bare_continue_simple() {
        // AC: bare `continue;` at statement level parses as a LoopControl node
        // (the when-block fall-through op), not as a bareword Identifier.
        let source = "continue;";
        let ast = must_some(parse_code(source));
        assert!(
            matches!(ast.kind, NodeKind::Program { .. }),
            "expected Program, got {:?}",
            ast.kind
        );
        let NodeKind::Program { statements } = &ast.kind else {
            return;
        };
        let stmt = must_some(statements.first());
        assert!(
            matches!(stmt.kind, NodeKind::LoopControl { .. }),
            "expected LoopControl, got {:?}",
            stmt.kind
        );
        let NodeKind::LoopControl { op, label } = &stmt.kind else {
            return;
        };
        assert_eq!(op, "continue");
        assert!(label.is_none());
    }

    #[test]
    fn test_continue_with_label() {
        // Real Perl rejects labels on `continue` (`continue OUTER` is a
        // syntax error), so the label must not attach: recovery records an
        // error and no LoopControl node carries a label (#16285).
        let source = "continue OUTER;";
        let mut parser = Parser::new(source);
        let ast = must_some(parser.parse().ok());
        assert!(
            !parser.errors().is_empty(),
            "expected a recorded rejection for `continue OUTER`, got none"
        );
        let NodeKind::Program { statements } = &ast.kind else {
            panic!("expected Program, got {:?}", ast.kind);
        };
        assert!(
            !statements
                .iter()
                .any(|stmt| matches!(&stmt.kind, NodeKind::LoopControl { label: Some(_), .. })),
            "no LoopControl node may carry a label for `continue`"
        );
    }

    #[test]
    fn test_continue_in_when_block() {
        // Inside a `when` block, `continue` falls through to the next case.
        let source = "given ($x) { when (1) { do_thing(); continue } when (2) { do_other(); } }";
        let ast = must_some(parse_code(source));

        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("continue"),
            "Expected `continue` to appear as a loop-control op, got: {sexp}"
        );
        assert!(
            !sexp.contains("Identifier(\"continue\")"),
            "`continue` should not be parsed as an Identifier, got: {sexp}"
        );
    }

    #[test]
    fn test_post_loop_continue_block_unaffected() {
        // The post-loop `continue { BLOCK }` form must still parse as the
        // While/For/Foreach `continue_block`, not as a labeled LoopControl.
        let source = "while (1) { last; } continue { $x++; }";
        let ast = must_some(parse_code(source));

        assert!(
            matches!(ast.kind, NodeKind::Program { .. }),
            "expected Program, got {:?}",
            ast.kind
        );
        let NodeKind::Program { statements } = &ast.kind else {
            return;
        };
        let while_stmt = must_some(statements.first());
        let NodeKind::While { continue_block, .. } = &while_stmt.kind else {
            panic!("expected While, got {:?}", while_stmt.kind);
        };
        assert!(
            continue_block.is_some(),
            "post-loop `continue {{ BLOCK }}` must attach as While.continue_block"
        );
    }
}
