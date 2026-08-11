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

fn is_recovery_node(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest
    )
}

#[test]
fn clean_source_has_no_recovery_evidence() {
    let mut parser = Parser::new("my $value = 1 + 2; my $after = 2;");
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes += 1;
        }
    });

    assert!(output.diagnostics.is_empty(), "clean source must not carry diagnostics");
    assert_eq!(recovery_nodes, 0, "clean source must not carry recovery nodes");
    assert!(!output.terminated_early, "clean source must not terminate early");
}

#[test]
fn missing_infix_rhs_emits_local_evidence_and_preserves_following_declaration()
-> Result<(), String> {
    let source = "my $value = 1 +; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut missing_expression_spans = Vec::new();
    let mut after_declaration_spans = Vec::new();

    walk(&output.ast, &mut |node| match &node.kind {
        NodeKind::MissingExpression => {
            missing_expression_spans.push((node.location.start, node.location.end));
        }
        NodeKind::VariableDeclaration { variable, .. }
            if matches!(
                &variable.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"
            ) =>
        {
            if let Some(text) = source.get(node.location.start..node.location.end) {
                after_declaration_spans.push(text.to_owned());
            }
        }
        _ => {}
    });

    assert!(
        !output.diagnostics.is_empty(),
        "malformed source must not be represented as a clean parse"
    );
    assert_eq!(
        missing_expression_spans.len(),
        1,
        "the missing right-hand side must remain one typed MissingExpression node"
    );

    let gap_start = source
        .find("+;")
        .ok_or_else(|| "test source lost the malformed infix boundary".to_string())?;
    let gap_end = gap_start + "+;".len();
    let (missing_start, missing_end) = missing_expression_spans[0];
    assert!(
        missing_start >= gap_start && missing_end <= gap_end,
        "recovery evidence escaped the malformed infix boundary: {missing_start}..{missing_end}"
    );

    assert_eq!(after_declaration_spans.len(), 1);
    assert!(after_declaration_spans[0].starts_with("my $after = 2"));
    assert!(!output.terminated_early, "this local syntax error should remain recoverable");
    Ok(())
}

#[test]
fn missing_initializer_emits_local_evidence_and_preserves_following_declaration()
-> Result<(), String> {
    let source = "my $broken = ; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut missing_expression_spans = Vec::new();
    let mut after_declaration_spans = Vec::new();

    walk(&output.ast, &mut |node| match &node.kind {
        NodeKind::MissingExpression => {
            missing_expression_spans.push((node.location.start, node.location.end));
        }
        NodeKind::VariableDeclaration { variable, .. }
            if matches!(
                &variable.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == "after"
            ) =>
        {
            if let Some(text) = source.get(node.location.start..node.location.end) {
                after_declaration_spans.push(text.to_owned());
            }
        }
        _ => {}
    });

    assert!(
        !output.diagnostics.is_empty(),
        "missing initializer must not be represented as a clean parse"
    );
    assert_eq!(
        missing_expression_spans.len(),
        1,
        "the declaration hole must remain one typed MissingExpression node"
    );

    let gap_start = source
        .find("= ;")
        .ok_or_else(|| "test source lost the initializer hole".to_string())?;
    let gap_end = gap_start + "= ;".len();
    let (missing_start, missing_end) = missing_expression_spans[0];
    assert!(
        missing_start >= gap_start && missing_end <= gap_end,
        "initializer recovery evidence escaped its declaration: {missing_start}..{missing_end}"
    );

    assert_eq!(after_declaration_spans.len(), 1);
    assert!(after_declaration_spans[0].starts_with("my $after = 2"));
    assert!(!output.terminated_early, "initializer recovery must preserve following code");
    Ok(())
}
