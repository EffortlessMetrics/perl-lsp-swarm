//! PIR-A canonical body-path `Branch` lowering tests (issue #4795).
//!
//! The dormant flat `lower_hir` path already lowers `BranchShell` to
//! `PirOperation::Branch` (see `pir_branch_tests.rs`). These tests cover the
//! **canonical** body-arena path (`lower_hir_bodies`), where `HirExpr::Branch`
//! previously counted as an unsupported construct and emitted no control-flow
//! node. This slice makes it emit a first-class `Branch` node with a condition
//! link and per-arm `PirEdgeKind::Branch` edges.
//!
//! Tests return `Result` and use `.ok_or(...)?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{
    PirAnchorKind, PirContext, PirEdgeKind, PirGraph, PirId, PirNode, PirOperation,
    lower_hir_bodies,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn branch_nodes(graph: &PirGraph) -> Vec<&PirNode> {
    graph.nodes.iter().filter(|n| matches!(n.operation, PirOperation::Branch { .. })).collect()
}

fn single_branch(graph: &PirGraph) -> Result<&PirNode, Box<dyn Error>> {
    let branches = branch_nodes(graph);
    if branches.len() != 1 {
        return Err(format!("expected exactly one Branch node, got {}", branches.len()).into());
    }
    Ok(branches[0])
}

fn branch_condition(node: &PirNode) -> Result<Option<PirId>, Box<dyn Error>> {
    match &node.operation {
        PirOperation::Branch { condition } => Ok(*condition),
        other => Err(format!("expected Branch operation, got {other:?}").into()),
    }
}

fn is_read_of(node: &PirNode, ident: &str) -> bool {
    match &node.operation {
        PirOperation::LexicalRead { name } => name.name == ident,
        PirOperation::StashRead { symbol } => symbol.name == ident,
        _ => false,
    }
}

fn write_node<'g>(graph: &'g PirGraph, ident: &str) -> Result<&'g PirNode, Box<dyn Error>> {
    graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == ident))
        .ok_or_else(|| format!("lexical write `{ident}` was not lowered").into())
}

/// A simple `if` emits exactly one Branch node in Void context.
#[test]
fn pir_a_if_emits_one_branch_node_void() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    assert_eq!(branch.context, PirContext::Void, "a branch statement yields no value");
    Ok(())
}

/// The Branch node is anchored to explicit source with a concrete range.
#[test]
fn pir_a_branch_has_explicit_source_anchor() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    assert_eq!(branch.source_anchor.kind, PirAnchorKind::ExplicitSource);
    assert!(branch.source_anchor.range.is_some(), "Branch must preserve a source range");
    Ok(())
}

/// The condition link points at a lowered read of the condition variable.
#[test]
fn pir_a_branch_condition_links_to_condition_read() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    let cond_id =
        branch_condition(branch)?.ok_or("condition `$x` lowers to a read, so link must be Some")?;
    let cond_node = graph.node(cond_id).ok_or("condition link must resolve to a real node")?;
    assert!(
        is_read_of(cond_node, "x"),
        "condition link must point at a read of `x`, got {:?}",
        cond_node.operation
    );
    Ok(())
}

/// A constant condition emits no PIR node, so the link stays None (fail-closed).
#[test]
fn pir_a_branch_constant_condition_is_none() -> TestResult {
    let graph = parse_and_lower("if (1) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    assert!(branch_condition(branch)?.is_none(), "a constant `1` condition emits no PIR node");
    Ok(())
}

/// The then-arm's first node is reachable from the Branch node via a Branch edge.
#[test]
fn pir_a_branch_edge_to_then_arm() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    let branch_id = single_branch(&graph)?.id;
    let then_write = write_node(&graph, "y")?;
    assert!(
        graph.edges.iter().any(|e| {
            e.from == branch_id && e.to == Some(then_write.id) && e.kind == PirEdgeKind::Branch
        }),
        "a Branch edge must connect the Branch node to the then-arm's first node"
    );
    Ok(())
}

/// if/elsif/else emits a Branch edge to each arm's first node.
#[test]
fn pir_a_branch_edges_to_all_three_arms() -> TestResult {
    let graph = parse_and_lower(
        "if ($a) { my $left = 1; } elsif ($b) { my $middle = 2; } else { my $right = 3; }",
    );
    let branch_id = single_branch(&graph)?.id;

    let branch_edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.from == branch_id && e.kind == PirEdgeKind::Branch)
        .collect();
    assert!(
        branch_edges.len() >= 3,
        "if/elsif/else must emit at least 3 Branch edges, got {}",
        branch_edges.len()
    );

    for arm in ["left", "middle", "right"] {
        let write = write_node(&graph, arm)?;
        assert!(
            branch_edges.iter().any(|e| e.to == Some(write.id)),
            "arm `{arm}` first node must be reachable by a Branch edge"
        );
    }
    Ok(())
}

/// `unless` also lowers to a Branch node.
#[test]
fn pir_a_unless_emits_branch_node() -> TestResult {
    let graph = parse_and_lower("unless ($x) { my $y = 1; }");
    single_branch(&graph)?;
    Ok(())
}

/// Nested `if` produces two independent Branch nodes.
#[test]
fn pir_a_nested_if_emits_two_branch_nodes() -> TestResult {
    let graph = parse_and_lower("if ($x) { if ($y) { my $z = 1; } }");
    assert_eq!(branch_nodes(&graph).len(), 2, "nested `if` must emit two Branch nodes");
    Ok(())
}

/// Branch is now a modeled operation: counted in operation_counts, absent from
/// unsupported_construct_counts.
#[test]
fn pir_a_branch_is_modeled_not_unsupported() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    assert_eq!(
        graph.receipt.operation_counts.get("Branch"),
        Some(&1),
        "receipt must count one modeled Branch operation"
    );
    assert!(
        !graph.receipt.unsupported_construct_counts.contains_key("Branch"),
        "Branch must no longer appear in unsupported_construct_counts: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    Ok(())
}

/// Regression: the Branch node must not introduce a cross-arm Fallthrough edge
/// between the then-arm and else-arm first nodes.
#[test]
fn pir_a_branch_no_cross_arm_fallthrough() -> TestResult {
    let graph = parse_and_lower("if ($flag) { my $left = 1; } else { my $right = 2; }");
    let left = write_node(&graph, "left")?;
    let right = write_node(&graph, "right")?;
    assert!(
        !graph.edges.iter().any(|e| {
            e.kind == PirEdgeKind::Fallthrough && e.from == left.id && e.to == Some(right.id)
        }),
        "arms must not be linked by a cross-arm Fallthrough edge"
    );
    Ok(())
}

/// Invariant: operation counts still total the node count after Branch modeling.
#[test]
fn pir_a_branch_preserves_operation_count_invariant() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; } else { my $z = 2; }");
    let op_total: usize = graph.receipt.operation_counts.values().sum();
    assert_eq!(op_total, graph.nodes.len(), "operation counts must total the node count");
    assert_eq!(graph.receipt.node_count, graph.nodes.len());
    Ok(())
}
