//! Cross-cutting recovery evidence for the parser concept ledger (#6709).
//!
//! `Ok(ast)` is not synonymous with a clean parse. These tests require the public
//! recovery entry point to expose diagnostics and a typed recovery node while
//! retaining valid source that follows the break.

use perl_parser_core::{Node, NodeKind, Parser};

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

#[test]
fn clean_source_has_no_recovery_evidence() {
    let mut parser = Parser::new("my $value = 1 + 2; my $after = 2;");
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;

    walk(&output.ast, &mut |node| {
        if matches!(
            &node.kind,
            NodeKind::Error { .. }
                | NodeKind::MissingExpression
                | NodeKind::MissingStatement
                | NodeKind::MissingIdentifier
                | NodeKind::MissingBlock
                | NodeKind::UnknownRest
        ) {
            recovery_nodes += 1;
        }
    });

    assert!(output.diagnostics.is_empty(), "clean source must not carry diagnostics");
    assert_eq!(recovery_nodes, 0, "clean source must not carry recovery nodes");
    assert!(!output.terminated_early, "clean source must not terminate early");
}

#[test]
fn missing_infix_rhs_emits_evidence_and_preserves_following_declaration() {
    let source = "my $value = 1 +; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut missing_expression_count = 0usize;
    let mut after_declaration_count = 0usize;

    walk(&output.ast, &mut |node| match &node.kind {
        NodeKind::MissingExpression => missing_expression_count += 1,
        NodeKind::VariableDeclaration { variable, .. }
            if matches!(
                &variable.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"
            ) =>
        {
            after_declaration_count += 1;
        }
        _ => {}
    });

    assert!(
        !output.diagnostics.is_empty(),
        "malformed source must not be represented as a clean parse"
    );
    assert_eq!(
        missing_expression_count, 1,
        "the missing right-hand side must remain a typed MissingExpression node"
    );
    assert_eq!(
        after_declaration_count, 1,
        "recovery must preserve the valid declaration after the malformed expression"
    );
    assert!(!output.terminated_early, "this local syntax error should remain recoverable");
}
