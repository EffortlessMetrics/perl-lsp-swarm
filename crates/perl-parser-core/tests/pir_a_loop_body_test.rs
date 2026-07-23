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
    graph.nodes.iter().filter(|n| matches!(&n.operation, PirOperation::Loop { .. })).collect()
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

/// Regression (#4815 review): a nested loop must not wire an inner-body node
/// back to the OUTER loop header. The outer loop's iteration region is the inner
/// loop, which severs its own fallthrough, so the outer loop gets no back-edge —
/// never a spurious edge derived from the last allocated (inner) node.
#[test]
fn pir_a_nested_loop_back_edge_stays_within_inner() -> TestResult {
    let graph = parse_and_lower("while ($a) { while ($b) { my $x = 1; } }");
    let loops = loop_nodes(&graph);
    if loops.len() != 2 {
        return Err(format!("expected two Loop nodes, got {}", loops.len()).into());
    }
    let outer = loops[0].id;
    let inner = loops[1].id;
    assert!(
        !graph.edges.iter().any(|e| e.kind == PirEdgeKind::Loop && e.to == Some(outer)),
        "the outer loop's region severs, so no Loop back-edge may target it"
    );
    assert!(
        graph.edges.iter().any(|e| e.kind == PirEdgeKind::Loop && e.to == Some(inner)),
        "the inner loop must retain its own back-edge"
    );
    Ok(())
}

/// Regression (#4815 review): a `foreach` binds its loop variable per iteration,
/// so the binding is lowered inside the iteration region (after the Loop node)
/// and the Loop entry edge targets it, rather than modeling a one-time pre-loop
/// write.
#[test]
fn pir_a_foreach_binding_is_inside_iteration_region() -> TestResult {
    let graph = parse_and_lower("foreach my $x (@list) { my $y = 1; }");
    let loop_id = single_loop(&graph)?.id;
    let binding = graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "x"))
        .ok_or("foreach binding `$x` must be lowered")?;
    assert!(binding.id > loop_id, "the foreach binding must come after the Loop node");
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.from == loop_id && e.to == Some(binding.id) && e.kind == PirEdgeKind::Loop),
        "the Loop entry edge must target the per-iteration binding"
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

/// An empty `while` body lowers zero iteration nodes: the guard
/// `next_id > iteration_first` must suppress the entry/back edges rather
/// than emit a dangling edge into whatever (unrelated) node happens to
/// lower next.
#[test]
fn pir_a_while_empty_body_emits_no_loop_edges() -> TestResult {
    let graph = parse_and_lower("while ($c) { }");
    assert!(
        !graph.edges.iter().any(|e| e.kind == PirEdgeKind::Loop),
        "an empty loop body must not emit any Loop entry/back edge, got: {:?}",
        graph.edges
    );
    assert!(loop_condition(single_loop(&graph)?)?.is_some());
    Ok(())
}

/// A C-style `for` with an empty `{ }` body but a non-empty update still
/// iterates (the update runs every pass), so the entry/back edges must
/// still be emitted, targeting the update node.
#[test]
fn pir_a_c_style_for_empty_body_with_update_still_links_update() -> TestResult {
    let graph = parse_and_lower("for (my $i=0; $i<10; $i++) { }");
    let loop_id = single_loop(&graph)?.id;
    let entry = graph
        .edges
        .iter()
        .find(|e| e.from == loop_id && e.kind == PirEdgeKind::Loop)
        .ok_or("expected a Loop entry edge even for an empty body with a C-style update")?;
    let back = graph
        .edges
        .iter()
        .find(|e| e.to == Some(loop_id) && e.kind == PirEdgeKind::Loop)
        .ok_or("expected a Loop back-edge even for an empty body with a C-style update")?;
    assert_eq!(
        entry.to,
        Some(back.from),
        "entry target and back-edge source must be the same node"
    );
    let update_node =
        graph.node(entry.to.ok_or("entry edge must have a concrete target")?).ok_or("missing")?;
    assert!(
        matches!(&update_node.operation, PirOperation::Modify { name, .. } if name.name == "i"),
        "the sole iteration node must be the `$i++` update, got {:?}",
        update_node.operation
    );
    Ok(())
}

#[test]
fn pir_a_c_style_for_loop_edges_exclude_initializer_and_condition() -> TestResult {
    let graph = parse_and_lower("for (my $i=0; $i<10; $i++) { my $x = 1; }");
    let loop_node = single_loop(&graph)?;
    let loop_id = loop_node.id;
    let init_node = graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::LexicalWrite { name } if name.name == "i"))
        .ok_or("initializer write for $i is missing")?;
    assert!(init_node.id.index() < loop_id.index(), "initializer must lower before the Loop node");
    Ok(())
}

#[test]
fn pir_a_c_style_for_entry_edge_targets_body_not_initializer() -> TestResult {
    let graph = parse_and_lower("for (my $i=0; $i<10; $i++) { my $x = 1; }");
    let loop_id = single_loop(&graph)?.id;
    let entry = graph
        .edges
        .iter()
        .find(|e| e.from == loop_id && e.kind == PirEdgeKind::Loop)
        .ok_or("expected a Loop entry edge")?;
    let entry_target =
        graph.node(entry.to.ok_or("entry edge must have a concrete target")?).ok_or("missing")?;
    assert!(
        matches!(&entry_target.operation, PirOperation::LexicalWrite { name } if name.name == "x"),
        "entry edge must point at the body's first node, got {:?}",
        entry_target.operation
    );
    Ok(())
}

#[test]
fn pir_a_c_style_for_back_edge_sources_update_not_condition() -> TestResult {
    let graph = parse_and_lower("for (my $i=0; $i<10; $i++) { my $x = 1; }");
    let loop_node = single_loop(&graph)?;
    let loop_id = loop_node.id;
    let condition_id = loop_condition(loop_node)?.ok_or("C-style for must link a condition")?;
    let back = graph
        .edges
        .iter()
        .find(|e| e.to == Some(loop_id) && e.kind == PirEdgeKind::Loop)
        .ok_or("expected a Loop back-edge")?;
    assert_ne!(back.from, condition_id, "back-edge must not originate at the condition-read node");
    let back_source = graph.node(back.from).ok_or("back-edge source node is missing")?;
    assert!(
        matches!(&back_source.operation, PirOperation::Modify { name, .. } if name.name == "i"),
        "back-edge source must be the `$i++` update, got {:?}",
        back_source.operation
    );
    Ok(())
}

#[test]
fn pir_a_c_style_for_no_condition_still_links_body() -> TestResult {
    let graph = parse_and_lower("for (;;) { my $x = 1; }");
    let node = single_loop(&graph)?;
    assert!(loop_condition(node)?.is_none(), "`for (;;)` has no condition to link");
    let loop_id = node.id;
    assert!(
        graph.edges.iter().any(|e| e.from == loop_id && e.kind == PirEdgeKind::Loop),
        "an infinite `for (;;)` must still emit a Loop entry edge to its body"
    );
    assert!(
        graph.edges.iter().any(|e| e.to == Some(loop_id) && e.kind == PirEdgeKind::Loop),
        "an infinite `for (;;)` must still emit a Loop back-edge from its body"
    );
    Ok(())
}

#[test]
fn pir_a_nested_loop_back_edges_converge_on_innermost_last_node() -> TestResult {
    let graph = parse_and_lower("while ($a) { while ($b) { my $x = 1; } }");
    let loops = loop_nodes(&graph);
    if loops.len() != 2 {
        return Err(format!("expected exactly two Loop nodes, got {}", loops.len()).into());
    }
    let outer_id = loops[0].id;
    let inner_id = loops[1].id;
    assert!(outer_id.index() < inner_id.index(), "outer Loop node must be lowered first");
    let outer_back = graph
        .edges
        .iter()
        .find(|e| e.to == Some(outer_id) && e.kind == PirEdgeKind::Loop)
        .ok_or("outer loop must have a back-edge")?;
    let inner_back = graph
        .edges
        .iter()
        .find(|e| e.to == Some(inner_id) && e.kind == PirEdgeKind::Loop)
        .ok_or("inner loop must have a back-edge")?;
    assert_eq!(
        outer_back.from, inner_back.from,
        "both back-edges must converge on the same innermost last-lowered node"
    );
    assert!(
        outer_back.from.index() > inner_id.index(),
        "shared back-edge source must be lowered after the inner Loop header"
    );
    let source_node = graph.node(outer_back.from).ok_or("back-edge source node is missing")?;
    assert!(
        matches!(&source_node.operation, PirOperation::LexicalWrite { name } if name.name == "x"),
        "shared back-edge source must be the innermost body's last node, got {:?}",
        source_node.operation
    );
    Ok(())
}
