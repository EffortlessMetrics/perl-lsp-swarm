use perl_ast::{
    validate_ast, AstInvariantCode, AstInvariantOptions, Node, NodeKind, SourceLocation,
};

fn location(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn number(value: &str, start: usize, end: usize) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, location(start, end))
}

fn codes(source: &str, root: &Node, options: AstInvariantOptions) -> Vec<AstInvariantCode> {
    validate_ast(source, root, options).findings.into_iter().map(|finding| finding.code).collect()
}

#[test]
fn options_builders_are_available_to_external_consumers() {
    let options = AstInvariantOptions::default()
        .with_max_findings(3)
        .with_max_depth(4)
        .with_max_nodes(5)
        .with_child_source_order(false)
        .with_empty_ranges(false);

    assert_eq!(options.max_findings, 3);
    assert_eq!(options.max_depth, 4);
    assert_eq!(options.max_nodes, 5);
    assert!(!options.require_child_source_order);
    assert!(!options.allow_empty_ranges);
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
            statements: vec![number("2", 2, 3), number("1", 0, 1), number("outside", 3, 4)],
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
    let strict = validate_ast("", &root, AstInvariantOptions::default().with_empty_ranges(false));

    assert!(permissive.is_valid());
    assert_eq!(strict.findings.len(), 1);
    assert_eq!(strict.findings[0].code, AstInvariantCode::UnexpectedEmptyRange);
}

#[test]
fn zero_finding_limit_retains_nothing_and_marks_the_report_incomplete() {
    let reversed = number("1", 2, 1);
    let report = validate_ast("12", &reversed, AstInvariantOptions::default().with_max_findings(0));

    assert!(report.findings.is_empty());
    assert!(report.truncated);
    assert!(!report.is_valid());
}

#[test]
fn depth_and_finding_budgets_bound_adversarial_trees() {
    let leaf = number("1", 0, 1);
    let unary =
        Node::new(NodeKind::Unary { op: "+".to_string(), operand: Box::new(leaf) }, location(0, 1));
    let root = Node::new(NodeKind::Program { statements: vec![unary] }, location(0, 1));
    let depth_report = validate_ast("1", &root, AstInvariantOptions::default().with_max_depth(1));

    assert_eq!(depth_report.findings.len(), 1);
    assert_eq!(depth_report.findings[0].code, AstInvariantCode::DepthLimitExceeded);
    assert_eq!(depth_report.findings[0].path, "root:Program/statements[0]:Unary/operand[0]:Number");
    assert_eq!(depth_report.findings[0].related_range, Some(location(0, 1)));
    assert_eq!(depth_report.nodes_visited, 2);
    assert_eq!(depth_report.max_depth_reached, 1);
    assert!(depth_report.truncated);

    let invalid_root =
        Node::new(NodeKind::Program { statements: vec![number("2", 2, 1)] }, location(3, 1));
    let bounded =
        validate_ast("1", &invalid_root, AstInvariantOptions::default().with_max_findings(1));
    assert_eq!(bounded.findings.len(), 1);
    assert!(bounded.truncated);
}

#[test]
fn zero_depth_budget_never_visits_or_buffers_root_children() {
    let root = Node::new(
        NodeKind::Program { statements: (0..256).map(|_| number("1", 0, 1)).collect() },
        location(0, 1),
    );
    let report = validate_ast(
        "1",
        &root,
        AstInvariantOptions::default().with_max_depth(0).with_max_nodes(256),
    );

    assert_eq!(report.nodes_visited, 1);
    assert_eq!(report.max_depth_reached, 0);
    assert!(report.truncated);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].code, AstInvariantCode::DepthLimitExceeded);
    assert_eq!(report.findings[0].path, "root:Program/statements[0]:Number");
}

#[test]
fn wide_tree_respects_the_node_budget_without_collecting_every_child() {
    let statements = (0..256).map(|_| number("1", 0, 1)).collect();
    let root = Node::new(NodeKind::Program { statements }, location(0, 1));
    let report = validate_ast("1", &root, AstInvariantOptions::default().with_max_nodes(8));

    assert_eq!(report.nodes_visited, 8);
    assert!(report.truncated);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| { finding.code == AstInvariantCode::NodeLimitExceeded })
            .count(),
        1
    );
}
