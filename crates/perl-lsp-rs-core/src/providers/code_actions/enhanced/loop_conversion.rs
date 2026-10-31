//! Loop conversion code actions

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind};

/// Every `$iter` reference in the loop body must be the index in `$array[$iter]`.
fn body_uses_iterator_only_as_array_subscript(
    body: &Node,
    array_scalar_name: &str,
    iter_name: &str,
) -> bool {
    fn visit(node: &Node, parent: Option<&Node>, array_scalar_name: &str, iter_name: &str) -> bool {
        if let NodeKind::Variable { name, sigil, .. } = &node.kind
            && sigil == "$"
            && name == iter_name
            && !is_array_subscript_index(node, parent, array_scalar_name)
        {
            return false;
        }

        let mut ok = true;
        node.for_each_child(|child| {
            if ok && !visit(child, Some(node), array_scalar_name, iter_name) {
                ok = false;
            }
        });
        ok
    }

    visit(body, None, array_scalar_name, iter_name)
}

fn is_array_subscript_index(node: &Node, parent: Option<&Node>, array_scalar_name: &str) -> bool {
    let Some(parent) = parent else {
        return false;
    };
    let NodeKind::Binary { op, left, right, .. } = &parent.kind else {
        return false;
    };
    op == "[]"
        && matches!(
            &left.kind,
            NodeKind::Variable { name, sigil, .. } if sigil == "$" && name == array_scalar_name
        )
        && std::ptr::eq(right.as_ref(), node)
}

fn replace_array_subscript_with_item(body_text: &str, array_scalar_name: &str, iter_name: &str) -> String {
    let pattern = format!("${}[${}]", array_scalar_name, iter_name);
    body_text.replace(&pattern, "$item")
}

/// Convert old-style for loops to modern foreach
pub fn convert_loop_style(node: &Node, source: &str) -> Option<CodeAction> {
    if let NodeKind::For { init, condition, update, body, .. } = &node.kind {
        // Check if it's a C-style for loop that can be converted
        if let Some(converted) = try_convert_c_style_loop(init, condition, update, body, source) {
            return Some(CodeAction {
                title: "Convert to foreach loop".to_string(),
                kind: CodeActionKind::RefactorRewrite,
                diagnostics: Vec::new(),
                edit: CodeActionEdit {
                    changes: vec![TextEdit { location: node.location, new_text: converted }],
                },
                is_preferred: false,
            });
        }
    }

    // Check for foreach that could be improved
    if let NodeKind::Foreach { variable, list, body, continue_block: _ } = &node.kind {
        // Check if using implicit $_
        if let NodeKind::Variable { name, sigil } = &variable.kind
            && name == "_"
            && sigil == "$"
        {
            let list_text = &source[list.location.start..list.location.end];
            let body_text = &source[body.location.start..body.location.end];

            return Some(CodeAction {
                title: "Use explicit loop variable instead of $_".to_string(),
                kind: CodeActionKind::RefactorRewrite,
                diagnostics: Vec::new(),
                edit: CodeActionEdit {
                    changes: vec![TextEdit {
                        location: node.location,
                        new_text: format!("foreach my $item ({}) {}", list_text, body_text),
                    }],
                },
                is_preferred: false,
            });
        }
    }

    None
}

/// Try to convert C-style for loop to foreach
pub fn try_convert_c_style_loop(
    init: &Option<Box<Node>>,
    condition: &Option<Box<Node>>,
    update: &Option<Box<Node>>,
    body: &Node,
    source: &str,
) -> Option<String> {
    // Pattern: for (my $i = 0; $i < @array; $i++)
    // Convert to: foreach my $item (@array)

    // Check that we have all parts of a C-style for loop
    if let (Some(init), Some(condition), Some(update)) = (init, condition, update) {
        // Check if init is "my $i = 0" pattern
        if let NodeKind::VariableDeclaration { variable, initializer, .. } = &init.kind {
            // Get iterator variable name
            if let NodeKind::Variable { name: iter_name, .. } = &variable.kind {
                // Check if initialized to 0
                if let Some(init_val) = initializer
                    && let NodeKind::Number { value } = &init_val.kind
                    && value == "0"
                    // Check if condition is $i < @array or $i < scalar @array
                    && let NodeKind::Binary { op, left, right } = &condition.kind
                    && op == "<"
                    // Check if left is our iterator variable
                    && let NodeKind::Variable { name: left_name, .. } = &left.kind
                    && left_name == iter_name
                {
                    // Check if right is an array
                    let array_name = if let NodeKind::Variable { sigil, name, .. } = &right.kind {
                        if sigil == "@" { Some(format!("@{}", name)) } else { None }
                    } else if let NodeKind::FunctionCall { name: func_name, args } = &right.kind {
                        if func_name == "scalar" && args.len() == 1 {
                            if let NodeKind::Variable { sigil, name, .. } = &args[0].kind {
                                if sigil == "@" { Some(format!("@{}", name)) } else { None }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(array) = array_name {
                        // Check if update is $i++ or ++$i
                        let is_increment = match &update.kind {
                            NodeKind::Unary { op, operand } => {
                                matches!(op.as_str(), "++" | "--")
                                    && if let NodeKind::Variable { name, .. } = &operand.kind {
                                        name == iter_name
                                    } else {
                                        false
                                    }
                            }
                            _ => false,
                        };

                        if is_increment {
                            let array_scalar_name = array.trim_start_matches('@');
                            if !body_uses_iterator_only_as_array_subscript(
                                body,
                                array_scalar_name,
                                iter_name,
                            ) {
                                return None;
                            }

                            let body_text = &source[body.location.start..body.location.end];
                            let modified_body =
                                replace_array_subscript_with_item(body_text, array_scalar_name, iter_name);

                            return Some(format!("foreach my $item ({}) {}", array, modified_body));
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::try_convert_c_style_loop;
    use perl_parser_core::Parser;

    fn parse_for_loop(source: &str) -> perl_parser_core::ast::Node {
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("fixture must parse");
        let perl_parser_core::ast::NodeKind::Program { statements } = &ast.kind else {
            panic!("expected program");
        };
        statements
            .first()
            .expect("expected one statement")
            .clone()
    }

    fn converted_body(source: &str) -> Option<String> {
        let for_node = parse_for_loop(source);
        let perl_parser_core::ast::NodeKind::For { init, condition, update, body, .. } =
            &for_node.kind
        else {
            panic!("expected for loop");
        };
        try_convert_c_style_loop(init, condition, update, body, source)
    }

    #[test]
    fn c_style_loop_replaces_full_subscript_not_dangling_bracket() {
        let source = "for (my $i=0; $i < @items; $i++) { print $items[$i]; }\n";
        let converted = converted_body(source).expect("conversion must be offered");
        assert!(
            converted.contains("print $item;"),
            "expected $item without dangling bracket, got: {converted}"
        );
        assert!(
            !converted.contains("$item]"),
            "must not leave dangling bracket: {converted}"
        );
        assert_eq!(
            converted,
            "foreach my $item (@items) { print $item; }"
        );

        let mut parser = Parser::new(&converted);
        parser.parse().expect("converted Perl must reparse cleanly");
    }

    #[test]
    fn c_style_loop_skipped_when_iterator_used_outside_subscript() {
        let source = "for (my $i=0; $i < @items; $i++) { print $i, $items[$i]; }\n";
        assert!(
            converted_body(source).is_none(),
            "must not offer conversion when $i appears outside array subscript"
        );
    }
}
