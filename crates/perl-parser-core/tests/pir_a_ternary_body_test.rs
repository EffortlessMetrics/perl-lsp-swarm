//! PIR-A canonical body-path ternary (`?:`) lowering tests (issue #4859).
//!
//! Extends the body-path control-flow track (Branch #4795, Loop #4815, Return
//! #4856) to the one value-producing fork: the ternary conditional
//! `COND ? THEN : ELSE`. Previously `HirExpr::Ternary` counted as an unsupported
//! construct and emitted no control-flow node; this slice makes it emit a
//! first-class `Branch` node with a condition link and per-arm `Branch` edges.
//!
//! Unlike a statement `if`/`unless` (which is Void — it yields no value), a
//! ternary is a value-producing rvalue whose result context is inherited from
//! its enclosing position and is not modeled here, so the node is `Unknown`
//! (fail-closed) rather than Void/Scalar/List.
//!
//! Tests return `Result` and use `.ok_or(...)?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{
    PirContext, PirEdgeKind, PirGraph, PirId, PirNode, PirOperation, lower_hir_bodies,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn branch_nodes(graph: &PirGraph) -> Vec<&PirNode> {
    graph.nodes.iter().filter(|n| matches!(&n.operation, PirOperation::Branch { .. })).collect()
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

/// A ternary emits exactly one Branch node.
#[test]
fn pir_a_ternary_emits_one_branch_node() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? 1 : 2;");
    single_branch(&graph)?;
    Ok(())
}

/// The ternary Branch node is Unknown context (value-producing rvalue whose
/// result context is not statically provable) — NOT Void (which is for
/// statement branches).
#[test]
fn pir_a_ternary_branch_is_unknown_context() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? 1 : 2;");
    let node = single_branch(&graph)?;
    assert_eq!(
        node.context,
        PirContext::Unknown,
        "a ternary is a value-producing rvalue with unprovable context, got {:?}",
        node.context
    );
    Ok(())
}

/// The condition link points at a lowered read of the ternary condition var.
#[test]
fn pir_a_ternary_condition_links_to_condition_read() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? 1 : 2;");
    let node = single_branch(&graph)?;
    let cond_id =
        branch_condition(node)?.ok_or("condition `$c` lowers to a read, so link is Some")?;
    let cond_node = graph.node(cond_id).ok_or("condition link must resolve to a real node")?;
    assert!(
        is_read_of(cond_node, "c"),
        "condition link must point at a read of `c`, got {:?}",
        cond_node.operation
    );
    Ok(())
}

/// Each arm that lowers a variable read is reached from the Branch node by a
/// `Branch` edge (not fallthrough), and the reads stay reachable.
#[test]
fn pir_a_ternary_arms_reached_by_branch_edges() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? $a : $b;");
    let node = single_branch(&graph)?;
    for ident in ["a", "b"] {
        let arm_read = graph
            .nodes
            .iter()
            .find(|n| is_read_of(n, ident))
            .ok_or_else(|| format!("arm operand `${ident}` must lower to a reachable read"))?;
        let reached = graph.edges.iter().any(|e| {
            e.from == node.id && e.to == Some(arm_read.id) && e.kind == PirEdgeKind::Branch
        });
        assert!(
            reached,
            "arm read `${ident}` must be reached from the Branch node by a Branch edge"
        );
    }
    Ok(())
}

/// The two arms are mutually exclusive: no fallthrough edge connects the then
/// arm's read to the else arm's read.
#[test]
fn pir_a_ternary_arms_are_mutually_exclusive() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? $a : $b;");
    let then_read =
        graph.nodes.iter().find(|n| is_read_of(n, "a")).ok_or("then arm `$a` must lower")?;
    let else_read =
        graph.nodes.iter().find(|n| is_read_of(n, "b")).ok_or("else arm `$b` must lower")?;
    let leak = graph.edges.iter().any(|e| e.from == then_read.id && e.to == Some(else_read.id));
    assert!(!leak, "then-arm read must not fall through to the else-arm read");
    Ok(())
}

/// `Ternary` is no longer counted as unsupported, and `Branch` is recorded in
/// operation counts.
#[test]
fn pir_a_ternary_is_not_unsupported() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? 1 : 2;");
    assert_eq!(
        graph.receipt.operation_counts.get("Branch"),
        Some(&1),
        "ternary must be recorded as one Branch in operation_counts"
    );
    let unsupported = graph.receipt.unsupported_construct_counts.get("Ternary").copied();
    assert!(
        unsupported.is_none() || unsupported == Some(0),
        "Ternary must no longer be tallied as unsupported, got {unsupported:?}"
    );
    Ok(())
}

/// A nested ternary (`$a ? $b ? 1 : 2 : 3`) emits two Branch nodes without
/// cross-arm fallthrough leakage.
#[test]
fn pir_a_nested_ternary_emits_two_branch_nodes() -> TestResult {
    let graph = parse_and_lower("my $x = $a ? $b ? 1 : 2 : 3;");
    let branches = branch_nodes(&graph);
    assert_eq!(
        branches.len(),
        2,
        "a nested ternary emits two Branch nodes, got {}",
        branches.len()
    );
    // Both conditions (`$a`, `$b`) stay reachable as reads.
    for ident in ["a", "b"] {
        assert!(
            graph.nodes.iter().any(|n| is_read_of(n, ident)),
            "nested-ternary condition `${ident}` must lower to a reachable read"
        );
    }
    Ok(())
}

/// The `op_total == nodes.len()` invariant holds after ternary lowering (the
/// Branch node is counted exactly once alongside its operand reads).
#[test]
fn pir_a_ternary_operation_counts_match_node_count() -> TestResult {
    let graph = parse_and_lower("my $x = $c ? $a : $b;");
    let op_total: usize = graph.receipt.operation_counts.values().sum();
    assert_eq!(op_total, graph.nodes.len(), "operation_counts must sum to the node count");
    Ok(())
}
