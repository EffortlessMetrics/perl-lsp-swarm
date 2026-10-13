use crate::{FieldId, Point};
use perl_ast::{Node as AstNode, NodeKind};
use std::ops::ControlFlow;

// Collect the direct children of an `AstNode` as a `Vec<&AstNode>`.
//
// This thin wrapper exists because the public `Node::children()` method in `perl_ast`
// has the same name as our facade method and would be ambiguous in `impl` blocks.
#[inline]
pub(crate) fn ast_children(node: &AstNode) -> Vec<&AstNode> {
    node.children()
}

#[inline]
pub(crate) fn ast_child_count(node: &AstNode) -> usize {
    let mut count = 0usize;
    node.for_each_child(|_| count += 1);
    count
}

pub(crate) fn ast_has_error(node: &AstNode) -> bool {
    if matches!(node.kind, NodeKind::Error { .. }) {
        return true;
    }

    node.children().iter().any(|child| ast_has_error(child))
}

#[inline]
pub(crate) fn ast_child_at(node: &AstNode, index: usize) -> Option<&AstNode> {
    ast_child_with_field(node, index).map(|(_, child)| child)
}

#[inline]
pub(crate) fn ast_child_field(node: &AstNode, index: usize) -> Option<FieldId> {
    ast_child_with_field(node, index).and_then(|(field, _)| field)
}

#[inline]
fn ast_child_with_field(node: &AstNode, index: usize) -> Option<(Option<FieldId>, &AstNode)> {
    let mut idx = 0usize;
    let mut found = None;
    let _ = node.try_for_each_child_with_field(|field, child| {
        if idx == index {
            found = Some((field, child));
            ControlFlow::Break(())
        } else {
            idx += 1;
            ControlFlow::Continue(())
        }
    });
    found
}

pub(crate) fn byte_to_point(source: &str, byte: usize) -> Point {
    let clamped = byte.min(source.len());
    let mut row = 0usize;
    let mut column = 0usize;

    for b in source.as_bytes().iter().take(clamped) {
        if *b == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }

    Point { row, column }
}
