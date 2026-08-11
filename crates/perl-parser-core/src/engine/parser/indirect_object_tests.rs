#[cfg(test)]
mod tests {
    use crate::engine::parser::Parser;
    use perl_ast::ast::{Node, NodeKind, SourceLocation};

    fn parse_code(input: &str) -> Option<perl_ast::ast::Node> {
        let mut parser = Parser::new(input);
        parser.parse().ok()
    }

    #[test]
    fn test_general_indirect_method_call() {
        // AC1: recognize method $object @args
        let source = "move $player 10, 20;";
        let ast_opt = parse_code(source);
        assert!(ast_opt.is_some());
        let ast = ast_opt.unwrap_or_else(|| {
            Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
        });
        if let NodeKind::Program { statements } = &ast.kind {
            let stmt = &statements[0];
            if let NodeKind::IndirectCall { method, object, args } = &stmt.kind {
                assert_eq!(method, "move");
                if let NodeKind::Variable { sigil, name } = &object.kind {
                    assert_eq!(sigil, "$");
                    assert_eq!(name, "player");
                } else {
                    unreachable!("Expected Variable as object, got {:?}", object.kind);
                }
                // Arguments are parsed until statement terminator
                assert_eq!(args.len(), 2);
            } else {
                unreachable!("Expected IndirectCall node, got {:?}", stmt.kind);
            }
        }
    }

    #[test]
    fn test_builtin_indirect_syntax() {
        // AC2: handle builtin indirect syntax (print $fh "text")
        let source = "print $fh \"Hello\";";
        let ast_opt = parse_code(source);
        assert!(ast_opt.is_some());
        let ast = ast_opt.unwrap_or_else(|| {
            Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
        });
        if let NodeKind::Program { statements } = &ast.kind {
            let stmt = &statements[0];
            if let NodeKind::IndirectCall { method, object, args } = &stmt.kind {
                assert_eq!(method, "print");
                if let NodeKind::Variable { sigil, name } = &object.kind {
                    assert_eq!(sigil, "$");
                    assert_eq!(name, "fh");
                }
                assert_eq!(args.len(), 1);
            }
        }
    }

    #[test]
    fn test_new_indirect_syntax() {
        // AC1 variant: new Class(...)
        let source = "new Player \"Steven\";";
        let ast_opt = parse_code(source);
        assert!(ast_opt.is_some());
        let ast = ast_opt.unwrap_or_else(|| {
            Node::new(NodeKind::UnknownRest, SourceLocation { start: 0, end: 0 })
        });
        if let NodeKind::Program { statements } = &ast.kind {
            let stmt = &statements[0];
            if let NodeKind::IndirectCall { method, object, .. } = &stmt.kind {
                assert_eq!(method, "new");
                if let NodeKind::Identifier { name } = &object.kind {
                    assert_eq!(name, "Player");
                }
            }
        }
    }

    #[test]
    fn test_user_defined_indirect_call_with_array_argument() {
        let ast = parse_code("render $renderer @parts;").expect("source should parse");
        let statements = match &ast.kind {
            NodeKind::Program { statements } => statements,
            other => panic!("Expected Program node, got {other:?}"),
        };
        let statement = statements.first().expect("expected one statement");

        match &statement.kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "render");
                assert!(matches!(
                    object.kind,
                    NodeKind::Variable { ref sigil, ref name } if sigil == "$" && name == "renderer"
                ));
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    args[0].kind,
                    NodeKind::Variable { ref sigil, ref name } if sigil == "@" && name == "parts"
                ));
            }
            other => panic!("Expected IndirectCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_user_defined_indirect_call_preserves_multiple_arguments() {
        let ast = parse_code("render $renderer ($title // 'Untitled'), @parts;")
            .expect("source should parse");
        let statements = match &ast.kind {
            NodeKind::Program { statements } => statements,
            other => panic!("Expected Program node, got {other:?}"),
        };
        let statement = statements.first().expect("expected one statement");

        match &statement.kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "render");
                assert!(matches!(object.kind, NodeKind::Variable { .. }));
                assert_eq!(args.len(), 2);
            }
            other => panic!("Expected IndirectCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_comma_separated_user_call_is_not_indirect() {
        let ast = parse_code("render $renderer, @parts;").expect("source should parse");
        let statements = match &ast.kind {
            NodeKind::Program { statements } => statements,
            other => panic!("Expected Program node, got {other:?}"),
        };
        let statement = statements.first().expect("expected one statement");

        assert!(
            !matches!(statement.kind, NodeKind::IndirectCall { .. }),
            "comma-separated call must not be classified as indirect: {:?}",
            statement.kind
        );
    }

    #[test]
    fn test_indirect_call_inside_control_flow_block() {
        let ast = parse_code("if ($enabled) { render $renderer @parts; }")
            .expect("source should parse");
        let statements = match &ast.kind {
            NodeKind::Program { statements } => statements,
            other => panic!("Expected Program node, got {other:?}"),
        };
        let statement = statements.first().expect("expected one statement");

        let then_branch = match &statement.kind {
            NodeKind::If { then_branch, .. } => then_branch,
            other => panic!("Expected If node, got {other:?}"),
        };
        let body_statements = match &then_branch.kind {
            NodeKind::Block { statements } => statements,
            other => panic!("Expected Block node, got {other:?}"),
        };
        let body_statement = body_statements
            .first()
            .expect("expected one statement in the if body");

        assert!(
            matches!(body_statement.kind, NodeKind::IndirectCall { ref method, .. } if method == "render"),
            "expected indirect call in block, got {:?}",
            body_statement.kind
        );
    }

    #[test]
    fn test_common_list_builtins_remain_regular_calls() {
        for source in [
            "push @items, $item;",
            "defined $object->method;",
            "sort @items;",
        ] {
            let ast = parse_code(source).expect("source should parse");
            let statements = match &ast.kind {
                NodeKind::Program { statements } => statements,
                other => panic!("Expected Program node, got {other:?}"),
            };
            let statement = statements.first().expect("expected one statement");

            assert!(
                !matches!(statement.kind, NodeKind::IndirectCall { .. }),
                "{source:?} must not be classified as indirect: {:?}",
                statement.kind
            );
        }
    }
}
