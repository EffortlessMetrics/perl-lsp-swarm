//! Tests for targetless `goto` representation — issue #15742.
//!
//! Local Perl 5.42.0 accepts a `goto` with no target expression at compile
//! time. The previous parser implementation manufactured a `MissingExpression`
//! `Box<Node>` operand wrapped in the targeted `Goto { target, form }` variant,
//! forcing downstream consumers to special-case the missing operand. The new
//! childless `NodeKind::TargetlessGoto` variant expresses the omission
//! honestly so consumers can treat it like `LoopControl`: emit one
//! `KeywordControl` token, record no symbol reference, and never fabricate a
//! label or value.
//!
//! These tests pin the new contract at the integration boundary so the
//! targeted `Goto` variant stays stable and targetless source produces only
//! `TargetlessGoto` nodes (no fabricated `MissingExpression` operand, no
//! targeted `Goto { target: MissingExpression, .. }` wrapper).
mod cpan_test_helpers;

use perl_parser_core::{Node, NodeKind};
use perl_tdd_support::{must, must_some_with};

/// Walk the tree and return the first node whose kind matches the predicate.
fn find_kind(node: &Node, pred: &dyn Fn(&NodeKind) -> bool) -> Option<NodeKind> {
    if pred(&node.kind) {
        return Some(node.kind.clone());
    }
    for child in node.children() {
        if let Some(k) = find_kind(child, pred) {
            return Some(k);
        }
    }
    None
}

fn count_kind(node: &Node, pred: &dyn Fn(&NodeKind) -> bool) -> usize {
    let mut count = 0;
    fn walk(node: &Node, pred: &dyn Fn(&NodeKind) -> bool, count: &mut usize) {
        if pred(&node.kind) {
            *count += 1;
        }
        for child in node.children() {
            walk(child, pred, count);
        }
    }
    walk(node, pred, &mut count);
    count
}

#[test]
fn bare_goto_emits_targetless_variant() {
    let ast = cpan_test_helpers::parse("goto;");
    let kind = must_some_with(
        find_kind(&ast, &|k| matches!(k, NodeKind::TargetlessGoto { .. })),
        "`goto;` must emit a TargetlessGoto node",
    );
    assert!(matches!(kind, NodeKind::TargetlessGoto { .. }));
}

#[test]
fn bare_goto_in_short_circuit_emits_targetless_variant() {
    let ast = cpan_test_helpers::parse("foo and goto;");
    let kind = must_some_with(
        find_kind(&ast, &|k| matches!(k, NodeKind::TargetlessGoto { .. })),
        "`foo and goto;` must emit a TargetlessGoto node",
    );
    assert!(matches!(kind, NodeKind::TargetlessGoto { .. }));
}

#[test]
fn bare_goto_in_modifier_emits_targetless_variant() {
    let ast = cpan_test_helpers::parse("goto if 0;");
    let kind = must_some_with(
        find_kind(&ast, &|k| matches!(k, NodeKind::TargetlessGoto { .. })),
        "`goto if 0;` must emit a TargetlessGoto node",
    );
    assert!(matches!(kind, NodeKind::TargetlessGoto { .. }));
}

#[test]
fn bare_goto_does_not_fabricate_missing_expression_operand() {
    // The legacy implementation produced `Goto { target: MissingExpression, .. }`
    // for bare `goto;`. The new contract must not surface any
    // `MissingExpression` node in that input.
    let ast = cpan_test_helpers::parse("goto;");
    let missing_count = count_kind(&ast, &|k| matches!(k, NodeKind::MissingExpression));
    assert_eq!(
        missing_count, 0,
        "targetless `goto;` must not fabricate a MissingExpression operand"
    );
}

#[test]
fn bare_goto_does_not_emit_targeted_goto_variant() {
    // Bare `goto;` must NOT emit a `Goto { target, form }` node anymore —
    // it now uses the dedicated childless variant.
    let ast = cpan_test_helpers::parse("goto;");
    let targeted_count = count_kind(&ast, &|k| matches!(k, NodeKind::Goto { .. }));
    assert_eq!(targeted_count, 0, "targetless `goto;` must not emit a targeted Goto variant");
}

#[test]
fn targeted_goto_still_emits_goto_variant() {
    // Regression guard: the existing `Goto { target, form }` path must keep
    // working exactly as before for any real target.
    let ast = cpan_test_helpers::parse("goto LABEL;");
    let targeted_count = count_kind(&ast, &|k| matches!(k, NodeKind::Goto { .. }));
    let targetless_count = count_kind(&ast, &|k| matches!(k, NodeKind::TargetlessGoto { .. }));
    assert_eq!(targeted_count, 1, "targeted goto must still produce exactly one Goto node");
    assert_eq!(targetless_count, 0, "targeted goto must not produce any TargetlessGoto");
}

#[test]
fn bare_goto_sexp_renders_grammar_atom() {
    // The S-expression projection for a bare `goto;` must surface the new
    // grammar atom `goto_targetless` and must NOT carry a fabricated
    // `missing_expression` operand.
    let mut parser = perl_parser_core::Parser::new("goto;");
    let ast = must(parser.parse());
    let sexp = ast.to_sexp();
    assert!(sexp.contains("goto_targetless"), "sexp must render the new variant atom, got: {sexp}");
    assert!(
        !sexp.contains("missing_expression"),
        "sexp must not fabricate a missing_expression operand, got: {sexp}"
    );
}
