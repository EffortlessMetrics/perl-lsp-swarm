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

/// The primary condition evaluation fallthroughs into the Branch node (control
/// evaluates the condition, then branches). Issue #4795 explicitly retains this
/// edge while suppressing cross-arm and branch→after fallthrough.
#[test]
fn pir_a_branch_condition_fallthrough_to_branch_node() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    let cond_id =
        branch_condition(branch)?.ok_or("condition `$x` lowers to a read, so link must be Some")?;
    assert!(
        graph.edges.iter().any(|e| {
            e.kind == PirEdgeKind::Fallthrough && e.from == cond_id && e.to == Some(branch.id)
        }),
        "condition read must fall through to the Branch node before arm edges fan out"
    );
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

/// if/elsif/else fans a Branch edge to each region entry: the then body, the
/// elsif *condition* (so the else-path condition evaluation is not orphaned),
/// the elsif body, and the else body — exactly four edges, no duplicates.
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
    assert_eq!(
        branch_edges.len(),
        4,
        "if/elsif/else must emit exactly 4 Branch edges (then, elsif condition, elsif body, else), got {}",
        branch_edges.len()
    );

    // Each arm body's first node is reachable by a direct Branch edge.
    for arm in ["left", "middle", "right"] {
        let write = write_node(&graph, arm)?;
        assert!(
            branch_edges.iter().any(|e| e.to == Some(write.id)),
            "arm `{arm}` first node must be reachable by a Branch edge"
        );
    }

    // The elsif condition is connected into the CFG, not orphaned.
    let elsif_cond = graph
        .nodes
        .iter()
        .find(|n| is_read_of(n, "b"))
        .ok_or("elsif condition read `b` was not lowered")?;
    assert!(
        branch_edges.iter().any(|e| e.to == Some(elsif_cond.id)),
        "the elsif condition must be reachable by a Branch edge, not orphaned"
    );
    Ok(())
}

/// `unless` also lowers to a Branch node.
#[test]
fn pir_a_unless_emits_branch_node() -> TestResult {
    let graph = parse_and_lower("unless ($x) { my $y = 1; }");
    single_branch(&graph)?;
    Ok(())
}

/// Two `elsif` arms add one condition edge and one body edge each on top of the
/// then-body edge (then + 2×(elsif cond + elsif body) + else = 6).
#[test]
fn pir_a_branch_two_elsif_arms_six_edges() -> TestResult {
    let graph = parse_and_lower(
        "if ($a) { my $t = 1; } elsif ($b) { my $m1 = 2; } elsif ($c) { my $m2 = 3; } else { my $e = 4; }",
    );
    let branch_id = single_branch(&graph)?.id;
    let branch_edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.from == branch_id && e.kind == PirEdgeKind::Branch)
        .collect();
    assert_eq!(
        branch_edges.len(),
        6,
        "two elsif arms must emit six Branch edges (then + 2×(cond+body) + else)"
    );
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

/// Compound condition ($a && $b) is not modeled as one value node, so the
/// condition link points at the LAST lowered operand ($b), not $a and not
/// a synthetic node for && itself. Locks down the v0 approximation
/// documented at the HirExpr::Branch call site in lower.rs so a future
/// change that accidentally links the FIRST operand (or drops the link
/// entirely) is caught.
#[test]
fn pir_a_branch_compound_condition_links_to_last_operand() -> TestResult {
    let graph = parse_and_lower("if ($a && $b) { my $y = 1; }");
    let branch = single_branch(&graph)?;
    let cond_id = branch_condition(branch)?.ok_or("compound condition must still link")?;
    let cond_node = graph.node(cond_id).ok_or("condition link must resolve to a real node")?;
    assert!(
        is_read_of(cond_node, "b"),
        "condition link for a-and-b must point at the last operand b, got {:?}",
        cond_node.operation
    );
    assert!(!is_read_of(cond_node, "a"), "condition link must not point at the first operand a");
    Ok(())
}

/// When an arm first statement is a declaration with a multi-node
/// initializer, the Branch edge must land on the declaration Write node --
/// the first node push_body_node emits for that statement -- not on a Read
/// produced while lowering the right-hand side. HirStmt::Let emits the
/// Write before lowering the initializer RHS, so arm_first (captured before
/// lower_block) is the Write id.
#[test]
fn pir_a_branch_edge_targets_arm_write_not_rhs_read() -> TestResult {
    let graph = parse_and_lower("if ($x) { my $y = $a + $b; }");
    let branch = single_branch(&graph)?;
    let y_write = write_node(&graph, "y")?;
    assert!(
        graph.edges.iter().any(|e| {
            e.from == branch.id && e.to == Some(y_write.id) && e.kind == PirEdgeKind::Branch
        }),
        "Branch edge must target the arm Write node y, not an operand Read"
    );
    for ident in ["a", "b"] {
        let operand = graph
            .nodes
            .iter()
            .find(|n| is_read_of(n, ident))
            .ok_or_else(|| format!("read of {ident} was not lowered"))?;
        assert!(
            !graph.edges.iter().any(|e| e.from == branch.id
                && e.to == Some(operand.id)
                && e.kind == PirEdgeKind::Branch),
            "Branch edge must not target the {ident} operand read"
        );
    }
    Ok(())
}

/// Nested if produces two independent Branch structures: the outer Branch
/// edge lands on the inner branch condition read (the inner if first
/// lowered node, per the same arm-entry convention as any other statement),
/// and the inner Branch own condition link and Branch edge are scoped to
/// itself. This guards against the outer and inner branch edges being
/// swapped, merged, or cross-wired.
#[test]
fn pir_a_nested_branch_edges_are_independent() -> TestResult {
    let graph = parse_and_lower("if ($x) { if ($y) { my $z = 1; } }");
    let branches = branch_nodes(&graph);
    if branches.len() != 2 {
        return Err(format!("expected exactly two Branch nodes, got {}", branches.len()).into());
    }
    let outer = branches[0];
    let inner = branches[1];

    let outer_cond = graph
        .node(branch_condition(outer)?.ok_or("outer condition must link")?)
        .ok_or("outer condition link must resolve")?;
    assert!(is_read_of(outer_cond, "x"), "outer Branch condition must read x");
    let inner_cond_id = branch_condition(inner)?.ok_or("inner condition must link")?;
    let inner_cond = graph.node(inner_cond_id).ok_or("inner condition link must resolve")?;
    assert!(is_read_of(inner_cond, "y"), "inner Branch condition must read y");

    assert!(
        graph.edges.iter().any(|e| {
            e.from == outer.id && e.to == Some(inner_cond_id) && e.kind == PirEdgeKind::Branch
        }),
        "outer Branch edge must target the inner branch condition read"
    );
    assert!(
        !graph.edges.iter().any(|e| {
            e.from == outer.id && e.to == Some(inner.id) && e.kind == PirEdgeKind::Branch
        }),
        "outer Branch edge must not target the inner Branch node directly"
    );

    let z_write = write_node(&graph, "z")?;
    assert!(
        graph.edges.iter().any(|e| {
            e.from == inner.id && e.to == Some(z_write.id) && e.kind == PirEdgeKind::Branch
        }),
        "inner Branch edge must target the z write"
    );
    Ok(())
}

/// An arm that lowers zero nodes (empty block) must not emit a Branch edge
/// at all -- there is no node for it to point at. This also guards the
/// next_id greater-than arm_first guard from ever producing a dangling edge
/// to an unrelated LATER statement first node.
#[test]
fn pir_a_branch_empty_then_arm_emits_no_branch_edge() -> TestResult {
    let graph = parse_and_lower("if ($x) { } my $after = 1;");
    let branch = single_branch(&graph)?;
    assert!(
        !graph.edges.iter().any(|e| e.from == branch.id && e.kind == PirEdgeKind::Branch),
        "an empty arm must not emit any Branch edge"
    );
    let after_write = write_node(&graph, "after")?;
    assert!(
        !graph.edges.iter().any(|e| e.to == Some(after_write.id)),
        "the statement after an empty-armed branch must have no incoming edge"
    );
    Ok(())
}

/// The statement following a full if/elsif/else chain must not inherit a
/// Branch or Fallthrough edge from ANY arm -- complements
/// pir_a_branch_no_cross_arm_fallthrough, which only checks the left/right
/// pair, by checking the post-chain statement against all arms and all edge
/// kinds.
#[test]
fn pir_a_branch_no_fallthrough_past_if_elsif_else_chain() -> TestResult {
    let graph = parse_and_lower(
        "if ($x) { my $left = 1; } elsif ($y) { my $mid = 2; } else { my $right = 3; } my $after = 9;",
    );
    let after_write = write_node(&graph, "after")?;
    assert!(
        !graph.edges.iter().any(|e| e.to == Some(after_write.id)),
        "no arm may leave an edge into the statement after the if/elsif/else chain"
    );
    Ok(())
}
