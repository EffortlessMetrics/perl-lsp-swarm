//! Refactoring actions for code transformations
//!
//! Provides automated refactoring operations for improving code structure.

use super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser::ast_utils::{find_node_at_range, find_statement_start};
use perl_parser_core::{Node, NodeKind, SourceLocation};

/// Get refactoring actions for a selection
pub fn get_refactoring_actions(source: &str, ast: &Node, range: (usize, usize)) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // Use the enhanced provider for better refactorings
    let enhanced_provider = super::enhanced::EnhancedCodeActionsProvider::new(source.to_string());
    actions.extend(enhanced_provider.get_enhanced_refactoring_actions(ast, range));

    // Keep basic refactorings as fallback
    if let Some(node) = find_node_at_range(ast, range) {
        match &node.kind {
            // Extract variable (basic version, enhanced version is better)
            NodeKind::FunctionCall { .. } | NodeKind::Binary { .. } if actions.is_empty() => {
                actions.push(CodeAction {
                    title: "Extract to variable".to_string(),
                    kind: CodeActionKind::RefactorExtract,
                    diagnostics: Vec::new(),
                    edit: extract_variable(source, node, range),
                    is_preferred: false,
                });
            }

            _ => {}
        }
    }

    actions
}

/// Extract expression to variable
fn extract_variable(source: &str, node: &Node, _range: (usize, usize)) -> CodeActionEdit {
    let expr_text = &source[node.location.start..node.location.end];
    let var_name = "$extracted_var";

    // Find statement start
    let stmt_start = find_statement_start(source, node.location.start);

    CodeActionEdit {
        changes: vec![
            // Insert variable declaration
            TextEdit {
                location: SourceLocation { start: stmt_start, end: stmt_start },
                new_text: format!("my {} = {};\n", var_name, expr_text),
            },
            // Replace expression with variable
            TextEdit { location: node.location, new_text: var_name.to_string() },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::{must, must_some};

    fn block_actions(source: &str, selection: &str) -> Vec<CodeAction> {
        let start = must_some(source.find(selection));
        let end = start + selection.len();
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        get_refactoring_actions(source, &ast, (start, end))
    }

    #[test]
    fn if_body_with_outer_lexical_does_not_offer_basic_function_extraction() {
        let source =
            "use strict; use warnings; { my $x = 1; if (1) { warn $x; } else { warn 0; } }";
        let actions = block_actions(source, "{ warn $x; }");
        assert!(
            actions.is_empty(),
            "basic fallback cannot preserve the captured lexical: {actions:?}"
        );
    }

    #[test]
    fn if_body_without_capture_does_not_offer_basic_function_extraction() {
        let source = "use strict; use warnings; if (1) { warn 1; } else { warn 0; }";
        let actions = block_actions(source, "{ warn 1; }");
        assert!(
            actions.is_empty(),
            "basic fallback breaks the if-body braces even without a capture: {actions:?}"
        );
    }

    #[test]
    fn standalone_noncapturing_block_keeps_enhanced_extraction() {
        let source = "use strict;\nuse warnings;\nsub worker {\n    {\n        my $x = 1;\n        $x + 1;\n    }\n}\n";
        let actions = block_actions(source, "{\n        my $x = 1;\n        $x + 1;\n    }");
        assert!(
            actions.iter().any(|action| action.title == "Extract to subroutine"),
            "safe enhanced extraction must remain available: {actions:?}"
        );
    }
}
