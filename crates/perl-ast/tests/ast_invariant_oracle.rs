use perl_ast::{
    AstInvariantCode, AstInvariantOptions, Node, NodeKind, SourceLocation, validate_ast,
};

fn location(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn number(value: &str, start: usize, end: usize) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, location(start, end))
}

fn codes(source: &str, root: &Node, options: AstInvariantOptions) -> Vec<AstInvariantCode> {
    validate_ast(source, root, options)
        .findings
        .into_iter()
        .map(|finding| finding.code)
        .collect()
}

#[test]
fn valid_tree_has_a_deterministic_clean_report() {
    let source = "1 2";
    let root = Node::new(
        NodeKind::Program { statements: vec![number("1", 0, 1), number("2", 2, 3)] },
        location(0, source.len()),
    );

    let first = validate_ast(source, &root, AstInvariantOptions::default());
    let second = validate_ast(source, &root, AstInvariantOptions::default());

    assert!(first.is_valid());
    assert_eq!(first, second);
    assert_eq!(first.nodes_visited, 3);
    assert_eq!(first.max_depth_reached, 1);
}

#[test]
fn reversed_and_out_of_bounds_ranges_are_distinct() {
    let reversed = number("1", 2, 1);
    let out_of_bounds = number("1", 0, 4);

    assert_eq!(
        codes("12", &reversed, AstInvariantOptions::default()),
        vec![AstInvariantCode::ReversedRange]
    );
    assert_eq!(
        codes("12", &out_of_bounds, AstInvariantOptions::default()),
        vec![AstInvariantCode::RangeOutOfBounds]
    );
}

#[test]
fn utf8_boundary_failure_is_source_backed_and_precise() {
    let source = "é";
    let root = number("é", 1, 2);
    let report = validate_ast(source, &root, AstInvariantOptions::default());

    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].code, AstInvariantCode::NonUtf8Boundary);
    assert_eq!(report.findings[0].range, location(1, 2));
    assert_eq!(report.findings[0].path, "root:Number");
}

#[test]
fn child_containment_and_source_order_are_independent_findings() {
    let source = "1 2";
    let root = Node::new(
        NodeKind::Program {
            statements: vec![
                number("2", 2, 3),
                number("1", 0, 1),
                number("outside", 3, 4),
            ],
        },
        location(0, source.len()),
    );
    let report = validate_ast(source, &root, AstInvariantOptions::default());
    let finding_codes = report.findings.iter().map(|finding| finding.code).collect::<Vec<_>>();

    assert!(finding_codes.contains(&AstInvariantCode::ChildOrderRegression));
    assert!(finding_codes.contains(&AstInvariantCode::ChildOutsideParent));
    assert!(finding_codes.contains(&AstInvariantCode::RangeOutOfBounds));
    assert!(report.findings.iter().any(|finding| {
        finding.code == AstInvariantCode::ChildOrderRegression
            && finding.path.contains("statements[1]:Number")
            && finding.related_range == Some(location(2, 3))
    }));
}

#[test]
fn zero_width_policy_is_explicit() {
    let root = number("", 0, 0);
    let permissive = validate_ast("", &root, AstInvariantOptions::default());
    let strict = validate_ast(
        "",
        &root,
        AstInvariantOptions { allow_empty_ranges: false, ..AstInvariantOptions::default() },
    );

    assert!(permissive.is_valid());
    assert_eq!(strict.findings.len(), 1);
    assert_eq!(strict.findings[0].code, AstInvariantCode::UnexpectedEmptyRange);
}

#[test]
fn depth_and_finding_budgets_bound_adversarial_trees() {
    let leaf = number("1", 0, 1);
    let unary = Node::new(
        NodeKind::Unary { op: "+".to_string(), operand: Box::new(leaf) },
        location(0, 1),
    );
    let root = Node::new(NodeKind::Program { statements: vec![unary] }, location(0, 1));
    let depth_report = validate_ast(
        "1",
        &root,
        AstInvariantOptions { max_depth: 1, ..AstInvariantOptions::default() },
    );

    assert_eq!(depth_report.findings.len(), 1);
    assert_eq!(depth_report.findings[0].code, AstInvariantCode::DepthLimitExceeded);
    assert_eq!(depth_report.findings[0].path, "root:Program/statements[0]:Unary/operand[0]:Number");

    let invalid_root = Node::new(
        NodeKind::Program { statements: vec![number("2", 2, 1)] },
        location(3, 1),
    );
    let bounded = validate_ast(
        "1",
        &invalid_root,
        AstInvariantOptions { max_findings: 1, ..AstInvariantOptions::default() },
    );
    assert_eq!(bounded.findings.len(), 1);
    assert!(bounded.truncated);
}
