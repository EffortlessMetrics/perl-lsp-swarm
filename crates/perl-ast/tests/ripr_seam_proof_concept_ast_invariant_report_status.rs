use perl_ast::{AstInvariantOptions, Node, NodeKind, SourceLocation, validate_ast};

fn number(start: usize, end: usize) -> Node {
    Node::new(NodeKind::Number { value: "1".to_string() }, SourceLocation { start, end })
}

#[test]
fn report_status_keeps_findings_and_completeness_independent() {
    let clean = validate_ast("1", &number(0, 1), AstInvariantOptions::default());
    assert!(!clean.has_findings());
    assert!(clean.is_complete());
    assert!(clean.is_valid());

    let invalid = validate_ast("1", &number(1, 0), AstInvariantOptions::default());
    assert!(invalid.has_findings());
    assert!(invalid.is_complete());
    assert!(!invalid.is_valid());

    let incomplete =
        validate_ast("1", &number(1, 0), AstInvariantOptions::default().with_max_findings(0));
    assert!(!incomplete.has_findings());
    assert!(!incomplete.is_complete());
    assert!(!incomplete.is_valid());
}
