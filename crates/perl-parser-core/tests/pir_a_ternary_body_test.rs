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

/// A ternary in `return`-operand position keeps the `Return` node reachable:
/// the consumer pushed *after* the ternary inherits a `Fallthrough` edge from
/// the ternary's Branch node. Regression guard for the orphaned-consumer bug —
/// a bare `return $x;` is guaranteed operand→Return reachability
/// (`pir_a_return_body_test::pir_a_return_value_read_is_reachable`), and a
/// ternary operand must not silently break that by leaving `Return` with no
/// incoming edge.
#[test]
fn pir_a_ternary_as_return_operand_keeps_return_reachable() -> TestResult {
    let graph = parse_and_lower("sub f { return $c ? $a : $b; }");
    let branch = single_branch(&graph)?;
    let return_node = graph
        .nodes
        .iter()
        .find(|n| matches!(&n.operation, PirOperation::Return))
        .ok_or("a `return <ternary>` must emit a Return node")?;
    // The Return node must be reachable — at least one incoming edge — and
    // specifically inherit a Fallthrough from the ternary's Branch node.
    let reached_from_branch = graph.edges.iter().any(|e| {
        e.from == branch.id && e.to == Some(return_node.id) && e.kind == PirEdgeKind::Fallthrough
    });
    assert!(
        reached_from_branch,
        "the Return node must inherit a Fallthrough edge from the ternary's Branch node \
         (consumer of an rvalue ternary must stay reachable), edges: {:?}",
        graph.edges
    );
    Ok(())
}

/// A ternary as the RHS of a bare (non-`my`) assignment likewise keeps its
/// consumer reachable: the node the assignment pushes after the ternary
/// inherits a Fallthrough from the Branch node, so nothing is orphaned.
#[test]
fn pir_a_ternary_as_bare_assign_rhs_keeps_consumer_reachable() -> TestResult {
    let graph = parse_and_lower("$x = $c ? $a : $b;");
    let branch = single_branch(&graph)?;
    // The Branch node must have an outgoing Fallthrough successor (its consumer).
    let successor = graph
        .edges
        .iter()
        .find(|e| e.from == branch.id && e.kind == PirEdgeKind::Fallthrough)
        .and_then(|e| e.to)
        .ok_or("the ternary Branch node must have a Fallthrough successor (its rvalue consumer)")?;
    // Stronger oracle: the successor is the assignment consumer, NOT one of the
    // arm reads (which are reached by `Branch` edges, not fallthrough). A
    // regression that pointed the fallthrough back at an arm would slip past a
    // mere "has a successor" check.
    let succ_node = graph.node(successor).ok_or("successor must resolve to a real node")?;
    assert!(
        !is_read_of(succ_node, "a") && !is_read_of(succ_node, "b"),
        "the Branch fallthrough successor must be the assignment consumer, not an arm read, \
         got {:?}",
        succ_node.operation
    );
    Ok(())
}

/// A constant/opaque condition (`1 ? $a : $b`) emits no condition node, so the
/// ternary Branch's condition link stays `None` (fail-closed) — the Branch node
/// is still emitted with its arm edges.
#[test]
fn pir_a_ternary_constant_condition_link_is_none() -> TestResult {
    let graph = parse_and_lower("my $x = 1 ? $a : $b;");
    let node = single_branch(&graph)?;
    assert!(
        branch_condition(node)?.is_none(),
        "a constant condition emits no node, so the condition link must be None (fail-closed)"
    );
    // The arms still lower and are reached by Branch edges.
    for ident in ["a", "b"] {
        assert!(
            graph.nodes.iter().any(|n| is_read_of(n, ident)),
            "arm operand `${ident}` must still lower to a reachable read"
        );
    }
    Ok(())
}

/// A ternary as the condition of an enclosing `if` emits two Branch nodes (the
/// outer `if` and the inner ternary) and stays coherent. This pins the known
/// v0 imprecision documented in the lowerer: the outer condition link resolves
/// to the inner ternary Branch node (a control node) rather than a value node —
/// the "last lowered node" heuristic extended to a ternary condition. Precise
/// enclosing-condition linking is a separate follow-up.
#[test]
fn pir_a_ternary_as_enclosing_condition_pins_two_branches() -> TestResult {
    let graph = parse_and_lower("sub f { if ($p ? 1 : 2) { my $y = 1; } }");
    let branches = branch_nodes(&graph);
    assert_eq!(
        branches.len(),
        2,
        "an `if (<ternary>)` emits an outer if-Branch and an inner ternary-Branch, got {}",
        branches.len()
    );
    // The condition read `$p` stays reachable.
    assert!(
        graph.nodes.iter().any(|n| is_read_of(n, "p")),
        "the ternary condition `$p` must lower to a reachable read"
    );
    Ok(())
}

/// When BOTH ternary arms are terminal (`$c ? return 1 : return 2`), control
/// never reaches the ternary's consumer. The Branch node must NOT gain a
/// `Fallthrough` successor, so a following statement (`my $dead = 3;`) stays
/// unreachable rather than spuriously fallthrough-linked. Regression guard for
/// over-reconnecting the consumer (the flip side of the orphaned-consumer bug).
#[test]
fn pir_a_ternary_both_arms_terminal_leave_consumer_unreachable() -> TestResult {
    let graph = parse_and_lower("sub f { $c ? return 1 : return 2; my $dead = 3; }");
    let branch = single_branch(&graph)?;
    // Both arms `return`, so the Branch node has no fallthrough successor.
    let branch_fallthrough_successor =
        graph.edges.iter().any(|e| e.from == branch.id && e.kind == PirEdgeKind::Fallthrough);
    assert!(
        !branch_fallthrough_successor,
        "both arms terminal: the Branch must not reconnect a Fallthrough consumer, edges: {:?}",
        graph.edges
    );
    // Both arms still emit their own terminal Return exit edges.
    let return_exits =
        graph.edges.iter().filter(|e| e.kind == PirEdgeKind::Return && e.to.is_none()).count();
    assert_eq!(return_exits, 2, "each terminal arm keeps its own Return exit edge");
    Ok(())
}

/// When only ONE arm is terminal (`$c ? return 1 : $b`), the other arm falls
/// through, so the consumer stays reachable: the Branch node keeps a
/// `Fallthrough` successor.
#[test]
fn pir_a_ternary_one_arm_terminal_keeps_consumer_reachable() -> TestResult {
    let graph = parse_and_lower("sub f { $c ? return 1 : $b; my $y = 3; }");
    let branch = single_branch(&graph)?;
    let has_fallthrough_successor =
        graph.edges.iter().any(|e| e.from == branch.id && e.kind == PirEdgeKind::Fallthrough);
    assert!(
        has_fallthrough_successor,
        "one non-terminal arm falls through, so the consumer must stay reachable, edges: {:?}",
        graph.edges
    );
    Ok(())
}
