//! PIR-A canonical body-path `Loop` lowering tests (issue #4815).
//!
//! The dormant flat `lower_hir` path already lowers `LoopShell` to
//! `PirOperation::Loop` (see `pir_loop_tests.rs`). These tests cover the
//! **canonical** body-arena path (`lower_hir_bodies`), where `HirExpr::Loop`
//! previously counted as an unsupported construct and emitted no control-flow
//! node. This slice makes it emit a first-class `Loop` node with a condition
//! link and `PirEdgeKind::Loop` enter/back edges.
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

fn loop_nodes(graph: &PirGraph) -> Vec<&PirNode> {
    graph.nodes.iter().filter(|n| matches!(n.operation, PirOperation::Loop { .. })).collect()
}

fn single_loop(graph: &PirGraph) -> Result<&PirNode, Box<dyn Error>> {
    let loops = loop_nodes(graph);
    if loops.len() != 1 {
        return Err(format!("expected exactly one Loop node, got {}", loops.len()).into());
    }
    Ok(loops[0])
}

fn loop_condition(node: &PirNode) -> Result<Option<PirId>, Box<dyn Error>> {
    match &node.operation {
        PirOperation::Loop { condition } => Ok(*condition),
        other => Err(format!("expected Loop operation, got {other:?}").into()),
    }
}

fn is_read_of(node: &PirNode, ident: &str) -> bool {
    match &node.operation {
        PirOperation::LexicalRead { name } => name.name == ident,
        PirOperation::StashRead { symbol } => symbol.name == ident,
        _ => false,
    }
}

/// A `while` loop emits exactly one Loop node in Void context.
#[test]
fn pir_a_while_emits_one_loop_node_void() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; }");
    let node = single_loop(&graph)?;
    assert_eq!(node.context, PirContext::Void, "a loop statement yields no value");
    Ok(())
}

/// The Loop node is anchored to explicit source with a concrete range.
#[test]
fn pir_a_loop_has_explicit_source_anchor() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; }");
    let node = single_loop(&graph)?;
    assert_eq!(node.source_anchor.kind, PirAnchorKind::ExplicitSource);
    assert!(node.source_anchor.range.is_some(), "Loop must preserve a source range");
    Ok(())
}

/// The condition link points at a lowered read of the loop condition variable.
#[test]
fn pir_a_while_condition_links_to_condition_read() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; }");
    let node = single_loop(&graph)?;
    let cond_id =
        loop_condition(node)?.ok_or("condition `$c` lowers to a read, so link is Some")?;
    let cond_node = graph.node(cond_id).ok_or("condition link must resolve to a real node")?;
    assert!(
        is_read_of(cond_node, "c"),
        "condition link must point at a read of `c`, got {:?}",
        cond_node.operation
    );
    Ok(())
}

/// `until` also lowers to a Loop node with a condition link.
#[test]
fn pir_a_until_emits_one_loop_node() -> TestResult {
    let graph = parse_and_lower("until ($done) { my $x = 1; }");
    let node = single_loop(&graph)?;
    assert!(loop_condition(node)?.is_some(), "until condition `$done` links to a read");
    Ok(())
}

/// A C-style `for` lowers to one Loop node whose condition links to a read.
#[test]
fn pir_a_c_style_for_emits_loop_with_condition() -> TestResult {
    let graph = parse_and_lower("for (my $i = 0; $i < 10; $i++) { my $x = 1; }");
    let node = single_loop(&graph)?;
    let cond_id = loop_condition(node)?.ok_or("C-style for condition `$i < 10` links to a read")?;
    let cond_node = graph.node(cond_id).ok_or("condition link must resolve")?;
    assert!(
        is_read_of(cond_node, "i"),
        "condition link must point at a read of `i`, got {:?}",
        cond_node.operation
    );
    Ok(())
}

/// `foreach` has no boolean condition, so the condition link stays None.
#[test]
fn pir_a_foreach_condition_is_none() -> TestResult {
    let graph = parse_and_lower("foreach my $x (@list) { my $y = 1; }");
    let node = single_loop(&graph)?;
    assert!(loop_condition(node)?.is_none(), "foreach has no boolean condition to link");
    Ok(())
}

/// The loop emits both a `Loop` entry edge (from the header) and a `Loop`
/// back-edge (into the header), so the body is reachable and the iteration is
/// modeled rather than orphaned.
#[test]
fn pir_a_loop_emits_entry_and_back_edges() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; }");
    let loop_id = single_loop(&graph)?.id;
    assert!(
        graph.edges.iter().any(|e| e.from == loop_id && e.kind == PirEdgeKind::Loop),
        "a Loop entry edge must leave the Loop node toward the body entry"
    );
    assert!(
        graph.edges.iter().any(|e| e.to == Some(loop_id) && e.kind == PirEdgeKind::Loop),
        "a Loop back-edge must return to the Loop node from the iteration"
    );
    Ok(())
}

/// Nested `while` produces two independent Loop nodes.
#[test]
fn pir_a_nested_loop_emits_two_loop_nodes() -> TestResult {
    let graph = parse_and_lower("while ($a) { while ($b) { my $x = 1; } }");
    assert_eq!(loop_nodes(&graph).len(), 2, "nested while must emit two Loop nodes");
    Ok(())
}

/// Loop is now a modeled operation: counted in operation_counts, absent from
/// unsupported_construct_counts.
#[test]
fn pir_a_loop_is_modeled_not_unsupported() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; }");
    assert_eq!(
        graph.receipt.operation_counts.get("Loop"),
        Some(&1),
        "receipt must count one modeled Loop operation"
    );
    assert!(
        !graph.receipt.unsupported_construct_counts.contains_key("Loop"),
        "Loop must no longer appear in unsupported_construct_counts: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    Ok(())
}

/// Invariant: operation counts still total the node count after Loop modeling.
#[test]
fn pir_a_loop_preserves_operation_count_invariant() -> TestResult {
    let graph = parse_and_lower("while ($c) { my $x = 1; } my $after = 2;");
    let op_total: usize = graph.receipt.operation_counts.values().sum();
    assert_eq!(op_total, graph.nodes.len(), "operation counts must total the node count");
    assert_eq!(graph.receipt.node_count, graph.nodes.len());
    Ok(())
}
