//! Canonical body-path (`lower_hir_bodies`) proof for the PIR demand and
//! access axes (issue #13806).
//!
//! `pir_tests.rs` and `pir_branch_tests.rs` pin the classifier through the
//! flat `lower_hir` path. Both paths share `demand_for_operation` and
//! `access_for_operation`, but the body lowerer pushes nodes through its own
//! `push_body_node`, so the canonical path needs its own rows or a divergence
//! there would go unnoticed.
//!
//! Tests return `Result` and use `.ok_or(...)?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{
    PirAccessMode, PirContext, PirEvaluationDemand, PirGraph, PirNode, PirOperation,
    lower_hir_bodies,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn single<'a>(
    graph: &'a PirGraph,
    what: &str,
    pick: impl Fn(&PirOperation) -> bool,
) -> Result<&'a PirNode, Box<dyn Error>> {
    let nodes: Vec<&PirNode> = graph.nodes.iter().filter(|n| pick(&n.operation)).collect();
    if nodes.len() != 1 {
        return Err(format!("expected exactly one {what} node, got {}", nodes.len()).into());
    }
    Ok(nodes[0])
}

#[test]
fn body_path_control_nodes_carry_truth_test_demand_and_no_access() -> TestResult {
    // Statement branch, statement loop, ternary, and foreach all lower to a
    // condition-bearing control node. The accepted slice attaches
    // `TruthTest` to that node (operand-level demand is a follow-up) and no
    // control node touches a place.
    for (source, what) in [
        ("if ($x) { 1 }", "Branch"),
        ("my $y = $x ? 1 : 2;", "Branch"),
        ("while ($x) { 1 }", "Loop"),
        ("for my $item (@list) { 1 }", "Loop"),
    ] {
        let graph = parse_and_lower(source);
        let node = single(&graph, what, |op| match what {
            "Branch" => matches!(op, PirOperation::Branch { .. }),
            _ => matches!(op, PirOperation::Loop { .. }),
        })
        .map_err(|e| format!("{source}: {e}"))?;
        assert_eq!(node.demand, PirEvaluationDemand::TruthTest, "{source}");
        assert_eq!(node.access, None, "{source}");
        assert_eq!(graph.receipt.demand_counts.get("TruthTest"), Some(&1), "{source}");
    }
    Ok(())
}

#[test]
fn body_path_return_carries_no_access_fact() -> TestResult {
    // On the canonical path a sub body lowers the declared lexical (a place
    // write), the returned read (a place read), and the `Return` control
    // node, which touches no place. The receipt must not report an access
    // the return never performed.
    let graph = parse_and_lower("sub f { my $x = foo(1); return $x; }");
    let write =
        single(&graph, "LexicalWrite", |op| matches!(op, PirOperation::LexicalWrite { .. }))?;
    assert_eq!(write.access, Some(PirAccessMode::Write));
    assert_eq!(write.demand, PirEvaluationDemand::Value);
    // The declared place's value context is not proven on this path either;
    // it must not inherit the statement's Void.
    assert_eq!(write.context, PirContext::Unknown);

    let read = single(&graph, "LexicalRead", |op| matches!(op, PirOperation::LexicalRead { .. }))?;
    assert_eq!(read.access, Some(PirAccessMode::Read));

    let ret = single(&graph, "Return", |op| matches!(op, PirOperation::Return))?;
    assert_eq!(ret.access, None);
    assert_eq!(ret.demand, PirEvaluationDemand::Value);

    assert_eq!(graph.receipt.access_counts.get("Write"), Some(&1));
    assert_eq!(graph.receipt.access_counts.get("Read"), Some(&1));
    assert_eq!(graph.receipt.access_counts.values().sum::<usize>(), 2);
    assert!(graph.nodes.len() > 2, "the Return node must be present but uncounted");
    assert_eq!(graph.receipt.demand_counts.values().sum::<usize>(), graph.nodes.len());
    Ok(())
}

#[test]
fn body_path_reads_and_compound_writes_classify_by_operation_family() -> TestResult {
    // A read of a lexical is a place read; a compound assignment is a
    // read-modify-write of the same place. Neither is a control node, so
    // demand stays `Value`.
    let graph = parse_and_lower("my $x = 1; $x += 2; my $y = $x;");
    let modify = single(&graph, "Modify", |op| matches!(op, PirOperation::Modify { .. }))?;
    assert_eq!(modify.access, Some(PirAccessMode::ReadModifyWrite));
    assert_eq!(modify.demand, PirEvaluationDemand::Value);

    let read = single(&graph, "LexicalRead", |op| matches!(op, PirOperation::LexicalRead { .. }))?;
    assert_eq!(read.access, Some(PirAccessMode::Read));
    assert_eq!(read.demand, PirEvaluationDemand::Value);

    assert_eq!(graph.receipt.access_counts.get("ReadModifyWrite"), Some(&1));
    assert_eq!(graph.receipt.access_counts.get("Read"), Some(&1));
    assert_eq!(graph.receipt.access_counts.get("Write"), Some(&2));
    Ok(())
}
