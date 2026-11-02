//! Checked [`Node::try_new`] local invariant policy (#8833).

#![allow(clippy::expect_used, clippy::panic)]

#[path = "helpers.rs"]
mod helpers;

use helpers::all_nodekind_instances;
use perl_ast::{
    Node, NodeInvariantError, NodeKind, NodeLocalInvariantCode, NodeSpanInvariantCode,
    SourceLocation,
};
use std::ops::ControlFlow;

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn dummy() -> Node {
    Node::new(NodeKind::Undef, loc(0, 1))
}

fn boxed() -> Box<Node> {
    Box::new(dummy())
}

fn assert_span_code(result: Result<Node, NodeInvariantError>, code: NodeSpanInvariantCode) {
    match result {
        Err(NodeInvariantError::Span { code: observed, .. }) => assert_eq!(observed, code),
        other => panic!("expected span {code:?}, got {other:?}"),
    }
}

fn assert_local_code(result: Result<Node, NodeInvariantError>, code: NodeLocalInvariantCode) {
    match result {
        Err(NodeInvariantError::Local(err)) => assert_eq!(err.code, code),
        other => panic!("expected local {code:?}, got {other:?}"),
    }
}

#[test]
fn reversed_span_reports_reversed_range() {
    let kind = NodeKind::Number { value: "1".to_string() };
    assert_span_code(Node::try_new(kind, loc(5, 2)), NodeSpanInvariantCode::ReversedRange);
}

#[test]
fn source_end_out_of_bounds() {
    let kind = NodeKind::Number { value: "1".to_string() };
    assert_span_code(
        Node::try_new_in_source(kind, loc(0, 9), "01234567"),
        NodeSpanInvariantCode::RangeOutOfBounds,
    );
}

#[test]
fn utf8_interior_boundary_rejected_in_source() {
    let source = "a🙂b";
    let kind = NodeKind::Number { value: "1".to_string() };
    assert_span_code(
        Node::try_new_in_source(kind, loc(1, 2), source),
        NodeSpanInvariantCode::NonUtf8Boundary,
    );
}

#[test]
fn unexpected_empty_range_on_non_recovery_kind() {
    let kind = NodeKind::Number { value: "1".to_string() };
    assert_span_code(Node::try_new(kind, loc(3, 3)), NodeSpanInvariantCode::UnexpectedEmptyRange);
}

#[test]
fn recovery_kind_admits_empty_range() {
    let kind = NodeKind::MissingExpression;
    let node = Node::try_new(kind, loc(0, 0)).expect("recovery empty range");
    assert_eq!(node.location.start, 0);
    assert_eq!(node.location.end, 0);
}

#[test]
fn chained_comparison_parallel_length_mismatch() {
    let kind = NodeKind::ChainedComparison {
        operands: vec![dummy(), dummy(), dummy()],
        ops: vec!["<".to_string()],
    };
    assert_local_code(
        Node::try_new(kind, loc(0, 10)),
        NodeLocalInvariantCode::ParallelLengthMismatch,
    );
}

#[test]
fn chained_comparison_fixed_operand_minimum() {
    let kind = NodeKind::ChainedComparison { operands: vec![dummy()], ops: vec![] };
    assert_local_code(
        Node::try_new(kind, loc(0, 10)),
        NodeLocalInvariantCode::FixedCardinalityMismatch,
    );
}

#[test]
fn named_parameter_mutually_exclusive_required_and_default() {
    let kind = NodeKind::NamedParameter {
        variable: boxed(),
        external_name: "alpha".to_string(),
        default_operator: Some("=".to_string()),
        default_operator_span: None,
        default_value: Some(boxed()),
        required: true,
    };
    assert_local_code(
        Node::try_new(kind, loc(0, 10)),
        NodeLocalInvariantCode::MutuallyExclusiveState,
    );
}

#[test]
fn named_parameter_requires_operator_with_default_value() {
    let kind = NodeKind::NamedParameter {
        variable: boxed(),
        external_name: "beta".to_string(),
        default_operator: None,
        default_operator_span: None,
        default_value: Some(boxed()),
        required: false,
    };
    assert_local_code(
        Node::try_new(kind, loc(0, 10)),
        NodeLocalInvariantCode::RequiredLocalPayloadMissing,
    );
}

#[test]
fn valid_chained_comparison_round_trips() {
    let kind = NodeKind::ChainedComparison {
        operands: vec![dummy(), dummy(), dummy()],
        ops: vec!["<".to_string(), "<".to_string()],
    };
    let location = loc(0, 12);
    let node = Node::try_new(kind.clone(), location).expect("valid chained comparison");
    assert_eq!(node.kind.kind_name(), "ChainedComparison");
    assert_eq!(node.location, location);
    let (round_kind, round_loc) = node.into_parts();
    assert_eq!(round_kind.kind_name(), NodeKind::kind_name(&kind));
    assert_eq!(round_loc, location);
}

#[test]
fn fixture_bank_passes_checked_construction() {
    for node in all_nodekind_instances() {
        let kind_name = node.kind.kind_name();
        let location = if kind_name == "MissingExpression"
            || kind_name == "MissingStatement"
            || kind_name == "MissingIdentifier"
            || kind_name == "MissingBlock"
        {
            loc(0, 0)
        } else {
            node.location
        };
        Node::try_new(node.kind.clone(), location)
            .unwrap_or_else(|err| panic!("{kind_name} should construct: {err}"));
    }
}

#[test]
fn parser_sample_nodes_pass_checked_construction() {
    let samples =
        ["my $x = 42;", "sub foo { return 1; }", "if ($x) { print $x; }", "package P; use v5.36;"];
    for source in samples {
        let ast = parse_sample(source);
        walk_try_new(&ast, source);
    }
}

fn parse_sample(source: &str) -> Node {
    use perl_parser_core::Parser;
    let mut parser = Parser::new(source);
    parser.parse().unwrap_or_else(|e| panic!("parse {source:?}: {e:?}"))
}

fn walk_try_new(node: &Node, source: &str) {
    Node::try_new_in_source(node.kind.clone(), node.location, source)
        .unwrap_or_else(|err| panic!("{}: {err}", node.kind.kind_name()));
    let _ = node.try_for_each_child_with_field(|_, child| {
        walk_try_new(child, source);
        ControlFlow::<(), ()>::Continue(())
    });
}
