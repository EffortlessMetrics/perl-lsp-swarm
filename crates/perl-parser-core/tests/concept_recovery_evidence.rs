//! Cross-cutting recovery evidence for the parser concept ledger (#6709).
//!
//! `Ok(ast)` is not synonymous with a clean parse. These tests require the public
//! recovery entry point to expose diagnostics and typed recovery nodes while
//! retaining valid source that follows the break.

use perl_parser_core::{Node, NodeKind, Parser};

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn source_text(source: &str, node: &Node) -> Option<String> {
    source
        .get(node.location.start..node.location.end)
        .map(str::to_owned)
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

fn declaration_spans(source: &str, ast: &Node, expected_name: &str) -> Vec<String> {
    let mut spans = Vec::new();
    walk(ast, &mut |node| {
        if let NodeKind::VariableDeclaration { variable, .. } = &node.kind
            && matches!(
                &variable.kind,
                NodeKind::Variable { sigil, name }
                    if sigil == "$" && name == expected_name
            )
            && let Some(text) = source_text(source, node)
        {
            spans.push(text);
        }
    });
    spans
}

#[test]
fn clean_source_has_no_recovery_evidence() {
    let source = "my $value = 1 + 2; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes += 1;
        }
    });

    assert!(output.diagnostics.is_empty(), "clean source must not carry diagnostics");
    assert_eq!(recovery_nodes, 0, "clean source must not carry recovery nodes");
    assert_eq!(declaration_spans(source, &output.ast, "after"), vec!["my $after = 2"]);
    assert!(!output.terminated_early, "clean source must not terminate early");
}

#[test]
fn missing_infix_rhs_emits_local_evidence_and_preserves_following_declaration()
-> Result<(), String> {
    let source = "my $value = 1 +; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;
    let mut missing_expression_spans = Vec::new();

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes += 1;
        }
        if matches!(&node.kind, NodeKind::MissingExpression) {
            missing_expression_spans.push((node.location.start, node.location.end));
        }
    });

    assert!(
        !output.diagnostics.is_empty(),
        "malformed source must not be represented as a clean parse"
    );
    assert_eq!(
        recovery_nodes, 1,
        "the missing infix operand must produce only its local MissingExpression evidence"
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

    assert_eq!(declaration_spans(source, &output.ast, "after"), vec!["my $after = 2"]);
    assert!(!output.terminated_early, "this local syntax error should remain recoverable");
    Ok(())
}

#[test]
fn missing_initializer_emits_local_evidence_and_preserves_following_declaration()
-> Result<(), String> {
    let source = "my $broken = ; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;
    let mut missing_expression_spans = Vec::new();

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes += 1;
        }
        if matches!(&node.kind, NodeKind::MissingExpression) {
            missing_expression_spans.push((node.location.start, node.location.end));
        }
    });

    assert!(
        !output.diagnostics.is_empty(),
        "missing initializer must not be represented as a clean parse"
    );
    assert_eq!(
        recovery_nodes, 1,
        "the initializer hole must produce only its local MissingExpression evidence"
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

    assert_eq!(declaration_spans(source, &output.ast, "after"), vec!["my $after = 2"]);
    assert!(!output.terminated_early, "initializer recovery must preserve following code");
    Ok(())
}

#[test]
fn truncated_arrow_preserves_partial_prefix_and_following_declaration() {
    let source = "$object->; my $after = 2;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut recovery_nodes = 0usize;
    let mut errors = Vec::new();

    walk(&output.ast, &mut |node| {
        if is_recovery_node(node) {
            recovery_nodes += 1;
        }
        if let NodeKind::Error {
            message,
            expected,
            partial,
            ..
        } = &node.kind
        {
            let partial_shape = partial.as_deref().map(|prefix| {
                (
                    source_text(source, prefix),
                    matches!(
                        &prefix.kind,
                        NodeKind::Variable { sigil, name }
                            if sigil == "$" && name == "object"
                    ),
                )
            });
            errors.push((
                message.clone(),
                expected.is_empty(),
                source_text(source, node),
                partial_shape,
            ));
        }
    });

    assert!(
        !output.diagnostics.is_empty(),
        "truncated postfix syntax must emit a recovery diagnostic"
    );
    assert_eq!(
        recovery_nodes, 1,
        "the truncated arrow must produce one local Error node and no hidden recovery footprint"
    );
    assert_eq!(
        errors,
        vec![(
            "Incomplete arrow expression".to_string(),
            true,
            Some("$object->".to_string()),
            Some((Some("$object".to_string()), true)),
        )],
        "the Error node must preserve the usable object prefix and exact malformed span"
    );
    assert_eq!(declaration_spans(source, &output.ast, "after"), vec!["my $after = 2"]);
    assert!(!output.terminated_early, "postfix recovery must preserve following code");
}
