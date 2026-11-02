//! Regression tests for issue #2017: PIR fallthrough edges follow evaluation order.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_parser_core::pir::{PirCallee, PirEdgeKind, PirGraph, PirOperation, lower_hir};
use perl_tdd_support::must_some;

fn lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir: HirFile = lower_ast(&output.ast);
    lower_hir(&hir)
}

#[test]
fn nested_call_argument_precedes_enclosing_call() {
    // #2017 headline case: HIR pre-order emits the outer `foo` before `bar()`,
    // but Perl evaluates the argument first. Fallthrough must read `bar -> foo`.
    let graph = lower("foo(bar());");
    let calls: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| matches!(node.operation, PirOperation::Call { .. }))
        .collect();
    assert_eq!(calls.len(), 2, "expected exactly one outer and one inner call");
    let outer_id = must_some(
        graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.operation,
                    PirOperation::Call {
                        callee: PirCallee::Named { name, .. },
                        arg_count: 1
                    } if name == "foo"
                )
            })
            .map(|node| node.id),
    );
    let inner_id = must_some(
        graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.operation,
                    PirOperation::Call {
                        callee: PirCallee::Named { name, .. },
                        arg_count: 0
                    } if name == "bar"
                )
            })
            .map(|node| node.id),
    );
    let fallthroughs: Vec<_> =
        graph.edges.iter().filter(|edge| edge.kind == PirEdgeKind::Fallthrough).collect();
    assert_eq!(fallthroughs.len(), 1, "single-statement nested call yields one fallthrough");
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == PirEdgeKind::Fallthrough && edge.from == inner_id && edge.to == Some(outer_id)
    }));
    assert!(!graph.edges.iter().any(|edge| {
        edge.kind == PirEdgeKind::Fallthrough && edge.from == outer_id && edge.to == Some(inner_id)
    }));
}

#[test]
fn nested_call_in_declaration_initializer_precedes_assignment() {
    let graph = lower("my $x = foo(bar());");
    let calls: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| matches!(node.operation, PirOperation::Call { .. }))
        .collect();
    assert_eq!(calls.len(), 2);
    let assign_id = must_some(
        graph
            .nodes
            .iter()
            .find(|node| matches!(node.operation, PirOperation::Assign))
            .map(|node| node.id),
    );
    let outer_id = must_some(
        graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.operation,
                    PirOperation::Call {
                        callee: PirCallee::Named { name, .. },
                        arg_count: 1
                    } if name == "foo"
                )
            })
            .map(|node| node.id),
    );
    let inner_id = must_some(
        graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    &node.operation,
                    PirOperation::Call {
                        callee: PirCallee::Named { name, .. },
                        arg_count: 0
                    } if name == "bar"
                )
            })
            .map(|node| node.id),
    );
    let eval_chain_fallthroughs: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == PirEdgeKind::Fallthrough)
        .filter(|edge| {
            (edge.from == inner_id && edge.to == Some(outer_id))
                || (edge.from == outer_id && edge.to == Some(assign_id))
        })
        .collect();
    assert_eq!(
        eval_chain_fallthroughs.len(),
        2,
        "expected exactly bar -> foo and foo -> Assign fallthrough edges"
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == PirEdgeKind::Fallthrough && edge.from == inner_id)
            .count(),
        1,
        "inner call must have a single fallthrough successor"
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == PirEdgeKind::Fallthrough && edge.from == outer_id)
            .count(),
        1,
        "outer call must have a single fallthrough successor"
    );
    assert!(!graph.edges.iter().any(|edge| {
        edge.kind == PirEdgeKind::Fallthrough && edge.from == outer_id && edge.to == Some(inner_id)
    }));
    assert!(!graph.edges.iter().any(|edge| {
        edge.kind == PirEdgeKind::Fallthrough && edge.from == inner_id && edge.to == Some(assign_id)
    }));
}
