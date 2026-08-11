#[cfg(test)]
mod tests {
    use crate::engine::parser::Parser;
    use perl_ast::ast::{Node, NodeKind};

    fn first_statement(input: &str) -> Node {
        let mut parser = Parser::new(input);
        let ast = parser.parse().expect("source should parse");
        match ast.kind {
            NodeKind::Program { mut statements } => statements
                .drain(..)
                .next()
                .expect("expected one statement"),
            other => panic!("expected Program node, got {other:?}"),
        }
    }

    #[test]
    fn unknown_lowercase_name_is_regular_function_call() {
        match first_statement("my_custom_method $obj 10, 20;").kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
            }
            other => panic!(
                "unknown lowercase names must remain FunctionCall nodes, got {other:?}"
            ),
        }
    }

    #[test]
    fn unknown_lowercase_name_preserves_nested_arguments() {
        match first_statement(
            "my_custom_method $obj ($title // 'Untitled'), $options->{limit};",
        )
        .kind
        {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, NodeKind::Variable { .. }));
            }
            other => panic!("expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn unknown_lowercase_name_inside_control_flow_is_regular_function_call() {
        let stmt = first_statement("if ($enabled) { my_custom_method $obj 10, 20; }");
        let then_branch = match stmt.kind {
            NodeKind::If { then_branch, .. } => then_branch,
            other => panic!("expected If node, got {other:?}"),
        };
        let body = match then_branch.kind {
            NodeKind::Block { statements } => statements,
            other => panic!("expected Block node, got {other:?}"),
        };
        match &body[0].kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "my_custom_method");
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected FunctionCall node in block, got {other:?}"),
        }
    }

    #[test]
    fn comma_separated_user_call_is_regular_function_call() {
        match first_statement("render $renderer, @parts;").kind {
            NodeKind::FunctionCall { name, args } => {
                assert_eq!(name, "render");
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected FunctionCall node, got {other:?}"),
        }
    }

    #[test]
    fn known_builtin_indirect_syntax_remains_indirect() {
        match first_statement("print $fh \"Hello\";").kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "print");
                assert!(matches!(
                    object.kind,
                    NodeKind::Variable { ref sigil, ref name }
                        if sigil == "$" && name == "fh"
                ));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected IndirectCall node, got {other:?}"),
        }
    }

    #[test]
    fn new_constructor_indirect_syntax_remains_indirect() {
        match first_statement("new Player \"Steven\";").kind {
            NodeKind::IndirectCall { method, object, args } => {
                assert_eq!(method, "new");
                assert!(matches!(
                    object.kind,
                    NodeKind::Identifier { ref name } if name == "Player"
                ));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected IndirectCall node, got {other:?}"),
        }
    }
}
