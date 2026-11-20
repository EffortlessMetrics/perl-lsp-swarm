//! Provider-owned folding eligibility over canonical AST shape (#8465).
//!
//! Folding authority lives here: lexical scope, binding, and execution
//! classifications from `perl_ast::classification` must not decide folds.
//! Structural child traversal uses [`perl_ast::Node::for_each_child_with_field`]
//! (#8424); this module never rebuilds child order from a second match table.

use perl_parser_core::ast::{Node, NodeKind, SourceLocation};

use super::{FoldingRangeExtractor, FoldingRangeKind};

/// Version of the folding policy contract checked by registry tests.
pub const FOLDING_POLICY_SCHEMA_VERSION: u32 = 1;

const _: () = assert!(FOLDING_POLICY_SCHEMA_VERSION >= 1);

/// Where a fold span comes from when a node is eligible.
#[derive(Debug, Clone)]
pub enum FoldSpanSource<'a> {
    /// Fold the node's own `location` span.
    NodeLocation { kind: Option<FoldingRangeKind> },
    /// Fold an inner content span (heredoc bodies).
    ContentSpan { span: &'a SourceLocation, kind: Option<FoldingRangeKind> },
}

/// Returns a fold span when this node should contribute a folding range.
///
/// `None` means the node itself is not foldable; children may still fold when
/// visited through the canonical field walk.
pub fn fold_span_for_node(node: &Node) -> Option<FoldSpanSource<'_>> {
    match &node.kind {
        NodeKind::Package { block, .. } => {
            block.as_ref().map(|_| FoldSpanSource::NodeLocation { kind: None })
        }

        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {
            Some(FoldSpanSource::NodeLocation { kind: None })
        }

        NodeKind::Block { statements } if !statements.is_empty() => {
            Some(FoldSpanSource::NodeLocation { kind: None })
        }

        NodeKind::If { .. }
        | NodeKind::While { .. }
        | NodeKind::For { .. }
        | NodeKind::Foreach { .. }
        | NodeKind::Do { .. }
        | NodeKind::Eval { .. }
        | NodeKind::Defer { .. }
        | NodeKind::Try { .. }
        | NodeKind::Given { .. }
        | NodeKind::When { .. }
        | NodeKind::Default { .. }
        | NodeKind::PhaseBlock { .. }
        | NodeKind::Class { .. }
        | NodeKind::LabeledStatement { .. }
        | NodeKind::Tie { .. } => Some(FoldSpanSource::NodeLocation { kind: None }),

        NodeKind::Heredoc { body_span, .. } => body_span
            .as_ref()
            .map(|span| FoldSpanSource::ContentSpan { span, kind: Some(FoldingRangeKind::Region) }),

        NodeKind::ArrayLiteral { elements } if !elements.is_empty() => {
            Some(FoldSpanSource::NodeLocation { kind: None })
        }

        NodeKind::HashLiteral { pairs } if !pairs.is_empty() => {
            Some(FoldSpanSource::NodeLocation { kind: None })
        }

        NodeKind::DataSection { body, .. } if body.is_some() => {
            Some(FoldSpanSource::NodeLocation { kind: Some(FoldingRangeKind::Comment) })
        }

        NodeKind::Format { .. } => {
            Some(FoldSpanSource::NodeLocation { kind: Some(FoldingRangeKind::Region) })
        }

        NodeKind::VariableDeclaration { .. }
        | NodeKind::VariableListDeclaration { .. }
        | NodeKind::Use { .. }
        | NodeKind::No { .. }
        | NodeKind::StatementModifier { .. }
        | NodeKind::ExpressionStatement { .. }
        | NodeKind::Program { .. } => None,

        _ => None,
    }
}

impl FoldingRangeExtractor {
    pub(super) fn apply_fold_policy(&mut self, node: &Node) {
        let Some(source) = fold_span_for_node(node) else {
            return;
        };
        match source {
            FoldSpanSource::NodeLocation { kind } => self.add_range_from_node(node, kind),
            FoldSpanSource::ContentSpan { span, kind } => {
                self.add_range_from_content_span(span, kind)
            }
        }
    }
}
