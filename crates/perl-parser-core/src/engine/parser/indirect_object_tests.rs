#[cfg(test)]
mod tests {
    use crate::engine::parser::Parser;
    use perl_ast::ast::{Node, NodeKind};

    fn parse_code(input: &str) -> Option<Node> {
        let mut parser = Parser::new(input);
        parser.parse().ok()
    }

    fn first_statement(input: &str) -> Node {
        let ast = parse_code(input).expect("source should parse");
        match ast.kind {
            NodeKind::Program { mut statements } => {
                let statement = statements.drain(..).next().expect("expected one statement");
                match statement.kind {
                    NodeKind::ExpressionStatement { expression } => *expression,
                    _ => statement,
                }
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
    fn test_unknown_lowercase_name_preserves_nested_arguments() {
        let source = "my_custom_method($obj, ($title // 'Untitled'), $options->{limit});";
        let stmt = first_statement(source);
        match stmt.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, NodeKind::Variable { .. }));
                assert_eq!(
                    stmt.location.end,
                    source.len() - 1,
                    "FunctionCall span must cover trailing arguments"
                );
            }
            other => panic!("Expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_lowercase_name_inside_control_flow_is_function_call() {
        let stmt = first_statement("if ($enabled) { my_custom_method($obj, 10, 20); }");
        let then_branch = match stmt.kind {
            NodeKind::If { then_branch, .. } => then_branch,
            other => panic!("Expected If node, got {other:?}"),
        };
        let body = match then_branch.kind {
            NodeKind::Block { statements } => statements,
            other => panic!("Expected Block node, got {other:?}"),
        };
        let call = match &body[0] {
            Node { kind: NodeKind::ExpressionStatement { expression }, .. } => expression.as_ref(),
            node => node,
        };
        match &call.kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
            }
            other => panic!("Expected FunctionCall node in block, got {other:?}"),
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
