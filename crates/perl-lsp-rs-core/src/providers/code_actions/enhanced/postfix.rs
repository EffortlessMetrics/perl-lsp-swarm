//! Postfix conversion code actions

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::{Node, NodeKind};

/// Convert if statement to postfix form
pub fn convert_to_postfix(node: &Node, source: &str) -> Option<CodeAction> {
    if let NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } = &node.kind {
        // Only convert simple if statements with no elsif/else
        if elsif_branches.is_empty()
            && else_branch.is_none()
            && let NodeKind::Block { statements } = &then_branch.kind
            && statements.len() == 1
        {
            let stmt = &statements[0];
            let stmt_text = source.get(stmt.location.start..stmt.location.end)?;
            let cond_text = source.get(condition.location.start..condition.location.end)?;

            // Check if statement is simple enough for postfix
            if !stmt_text.contains('\n') && stmt_text.len() < 80 {
                return Some(CodeAction {
                    title: "Convert to postfix if".to_string(),
                    kind: CodeActionKind::RefactorRewrite,
                    diagnostics: Vec::new(),
                    edit: CodeActionEdit {
                        changes: vec![TextEdit {
                            location: node.location,
                            new_text: format!("{} if {}", stmt_text, cond_text),
                        }],
                    },
                    is_preferred: false,
                });
            }
        }
    }

    // Similarly for while, until
    // Note: Unless is not a separate node type in this AST

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::{Parser, SourceLocation};
    use perl_tdd_support::{must, must_some};

    fn if_node_from_source(source: &str) -> Node {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        match &ast.kind {
            NodeKind::Program { statements } => must_some(statements.first()).clone(),
            other => panic!("expected program, got {other:?}"),
        }
    }

    fn set_first_statement_location(node: &mut Node, location: SourceLocation) {
        let NodeKind::If { then_branch, .. } = &mut node.kind else {
            panic!("fixture must parse as If");
        };
        let NodeKind::Block { statements } = &mut then_branch.kind else {
            panic!("fixture must have block then-branch");
        };
        assert_eq!(statements.len(), 1, "fixture must be a single-statement if body");
        statements[0].location = location;
    }

    fn set_condition_location(node: &mut Node, location: SourceLocation) {
        let NodeKind::If { condition, .. } = &mut node.kind else {
            panic!("fixture must parse as If");
        };
        condition.location = location;
    }

    #[test]
    fn convert_to_postfix_normal_simple_if() {
        let source = "if ($debug) { print \"Debug\\n\"; }";
        let node = if_node_from_source(source);
        let action = convert_to_postfix(&node, source);
        assert!(action.is_some(), "expected postfix action for simple if");
        assert_eq!(must_some(action).title, "Convert to postfix if");
    }

    #[test]
    fn convert_to_postfix_stale_end_past_source_len_returns_none() {
        let parsed_source = "if ($x) { print 1; }";
        let node = if_node_from_source(parsed_source);
        let stale_source = "if ($x) { print";
        let stmt_end = match &node.kind {
            NodeKind::If { then_branch, .. } => {
                if let NodeKind::Block { statements } = &then_branch.kind {
                    statements[0].location.end
                } else {
                    panic!("expected block");
                }
            }
            _ => panic!("expected if"),
        };
        assert!(
            stmt_end > stale_source.len(),
            "fixture must use AST offsets beyond stale source length"
        );
        assert!(convert_to_postfix(&node, stale_source).is_none());
    }

    #[test]
    fn convert_to_postfix_non_char_boundary_span_returns_none() {
        let source = "if ($x) { é }";
        let mut node = if_node_from_source(source);
        let é_start = must_some(source.find('é'));
        let inside = é_start + 1;
        assert!(!source.is_char_boundary(inside));

        set_first_statement_location(&mut node, SourceLocation { start: inside, end: inside + 1 });

        assert!(convert_to_postfix(&node, source).is_none());
    }

    #[test]
    fn convert_to_postfix_invalid_condition_span_returns_none() {
        let source = "if ($x) { é }";
        let mut node = if_node_from_source(source);
        let é_start = must_some(source.find('é'));
        let inside = é_start + 1;
        assert!(!source.is_char_boundary(inside));

        set_condition_location(&mut node, SourceLocation { start: inside, end: inside + 1 });

        assert!(convert_to_postfix(&node, source).is_none());
    }
}
