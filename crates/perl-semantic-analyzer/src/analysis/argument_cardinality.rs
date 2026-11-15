//! Conservative argument cardinality in Perl list context.

use perl_parser_core::ast::{Node, NodeKind};

/// Count arguments only when each is known to produce one value in list context.
///
/// Arrays, slices, calls, and other unsupported expressions may produce zero or
/// many values. They return `None`, rather than treating each AST argument as one
/// value. Command strings and command heredocs also have unknown cardinality.
/// An empty argument list has a known count of zero.
pub fn known_scalar_argument_count(args: &[Node]) -> Option<usize> {
    args.iter()
        .all(|arg| match &arg.kind {
            NodeKind::Number { .. } | NodeKind::VString { .. } | NodeKind::Undef => true,
            // The parser currently represents qx/backticks as String nodes,
            // but they return output lines in list context.
            NodeKind::String { value, .. } => !value.starts_with("qx") && !value.starts_with('`'),
            NodeKind::Heredoc { command, .. } => !command,
            NodeKind::Variable { sigil, .. } => sigil == "$",
            _ => false,
        })
        .then_some(args.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::ast::SourceLocation;

    fn node(kind: NodeKind) -> Node {
        Node::new(kind, SourceLocation { start: 0, end: 0 })
    }

    fn variable(sigil: &str) -> Node {
        node(NodeKind::Variable { sigil: sigil.to_string(), name: "value".to_string() })
    }

    fn heredoc(command: bool) -> Node {
        node(NodeKind::Heredoc {
            delimiter: "END".to_string(),
            content: "first\nsecond\n".to_string(),
            interpolated: true,
            indented: false,
            command,
            body_span: None,
        })
    }

    #[test]
    fn empty_arguments_have_zero_values() {
        assert_eq!(known_scalar_argument_count(&[]), Some(0));
    }

    #[test]
    fn known_scalars_contribute_one_value_each() {
        let args = [
            node(NodeKind::Number { value: "42".to_string() }),
            node(NodeKind::VString { value: "v1.2.3".to_string() }),
            node(NodeKind::Undef),
            node(NodeKind::String { value: "'literal'".to_string(), interpolated: false }),
            node(NodeKind::String { value: "\"$value\"".to_string(), interpolated: true }),
            variable("$"),
            heredoc(false),
        ];

        assert_eq!(known_scalar_argument_count(&args), Some(7));
    }

    #[test]
    fn non_scalar_variables_make_the_entire_count_unknown() {
        for sigil in ["@", "%", "&", "*"] {
            assert_eq!(known_scalar_argument_count(&[variable(sigil)]), None, "{sigil}");
            assert_eq!(
                known_scalar_argument_count(&[variable("$"), variable(sigil)]),
                None,
                "a known prefix cannot establish the whole count for {sigil}"
            );
        }
    }

    #[test]
    fn calls_slices_and_dereferences_have_unknown_cardinality() {
        let args = [
            node(NodeKind::FunctionCall { name: "values".to_string(), args: Vec::new() }),
            node(NodeKind::ArraySlice {
                target: Box::new(variable("@")),
                indices: Box::new(node(NodeKind::Number { value: "0".to_string() })),
            }),
            node(NodeKind::Unary { op: "@".to_string(), operand: Box::new(variable("$")) }),
        ];

        for arg in args {
            assert_eq!(known_scalar_argument_count(std::slice::from_ref(&arg)), None);
        }
    }

    #[test]
    fn command_output_is_not_a_single_scalar_argument() {
        for value in ["qx{command}", "`command`"] {
            let arg = node(NodeKind::String { value: value.to_string(), interpolated: true });
            assert_eq!(known_scalar_argument_count(&[arg]), None, "{value}");
        }
        assert_eq!(known_scalar_argument_count(&[heredoc(true)]), None);

        // Command-shaped content inside an ordinary quoted string stays scalar.
        let literal =
            node(NodeKind::String { value: "'qx{command}'".to_string(), interpolated: false });
        assert_eq!(known_scalar_argument_count(&[literal]), Some(1));
    }
}
