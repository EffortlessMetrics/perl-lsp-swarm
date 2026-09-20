//! PIR-A canonical body-path `Return` lowering tests (issue #4856).
//!
//! The dormant flat `lower_hir` path already lowers a `return` (`ControlTransfer`
//! HIR item) to `PirOperation::Return` with a terminal exit edge (see
//! `pir_control_transfer_tests.rs`). These tests cover the **canonical**
//! body-arena path (`lower_hir_bodies`), where `HirExpr::Return` previously
//! counted as an unsupported construct and emitted no control-flow node. This
//! slice makes it emit a first-class `Return` node with a `PirEdgeKind::Return`
//! exit edge, and keeps the returned operand's read reachable.
//!
//! Tests return `Result` and use `.ok_or(...)?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{
    PirAnchorKind, PirContext, PirEdgeKind, PirGraph, PirNode, PirOperation, lower_hir_bodies,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn return_nodes(graph: &PirGraph) -> Vec<&PirNode> {
    graph.nodes.iter().filter(|n| matches!(&n.operation, PirOperation::Return)).collect()
}

fn single_return(graph: &PirGraph) -> Result<&PirNode, Box<dyn Error>> {
    let returns = return_nodes(graph);
    if returns.len() != 1 {
        return Err(format!("expected exactly one Return node, got {}", returns.len()).into());
    }
    Ok(returns[0])
}

fn is_read_of(node: &PirNode, ident: &str) -> bool {
    match &node.operation {
        PirOperation::LexicalRead { name } => name.name == ident,
        PirOperation::StashRead { symbol } => symbol.name == ident,
        _ => false,
    }
}

/// A bare `return;` emits exactly one Return node in Void context.
#[test]
fn pir_a_bare_return_emits_one_return_node_void() -> TestResult {
    let graph = parse_and_lower("sub f { return; }");
    let node = single_return(&graph)?;
    assert_eq!(node.context, PirContext::Void, "a return statement yields no value");
    Ok(())
}

/// The Return node is anchored to explicit source, spanning exactly the
/// `return $x` expression — not the enclosing block or subroutine.
#[test]
fn pir_a_return_has_explicit_source_anchor() -> TestResult {
    let src = "sub f { return $x; }";
    let graph = parse_and_lower(src);
    let node = single_return(&graph)?;
    assert_eq!(node.source_anchor.kind, PirAnchorKind::ExplicitSource);
    let r = node.source_anchor.range.ok_or("Return must preserve a source range")?;
    // Expected span: from the `return` keyword up to (but excluding) the `;`.
    let expected_start = src.find("return").ok_or("source contains `return`")?;
    let expected_end = src.find(';').ok_or("source contains `;`")?;
    assert_eq!(
        (r.start(), r.end()),
        (expected_start, expected_end),
        "Return anchor must span exactly the `return $x` expression, got {:?}",
        src.get(r.start()..r.end())
    );
    Ok(())
}

/// A bare valueless `return;` now carries a well-formed anchor spanning exactly
/// the `return` keyword. Before #4861 the upstream AST produced a degenerate,
/// inverted range for the valueless form, so this was only checked for `return $x`.
#[test]
fn pir_a_bare_return_has_wellformed_source_anchor() -> TestResult {
    let src = "sub f { return; }";
    let graph = parse_and_lower(src);
    let node = single_return(&graph)?;
    assert_eq!(node.source_anchor.kind, PirAnchorKind::ExplicitSource);
    let r = node.source_anchor.range.ok_or("Return must preserve a source range")?;
    assert!(
        r.end() >= r.start(),
        "bare return anchor must not be inverted, got {}..{}",
        r.start(),
        r.end()
    );
    let expected_start = src.find("return").ok_or("source contains `return`")?;
    assert_eq!(
        (r.start(), r.end()),
        (expected_start, expected_start + "return".len()),
        "bare Return anchor must span exactly `return`, got {:?}",
        src.get(r.start()..r.end())
    );
    Ok(())
}

/// A Return node carries a terminal exit edge (`to: None`, kind `Return`).
#[test]
fn pir_a_return_emits_terminal_exit_edge() -> TestResult {
    let graph = parse_and_lower("sub f { return; }");
    let node = single_return(&graph)?;
    let exit = graph
        .edges
        .iter()
        .find(|e| e.from == node.id && e.kind == PirEdgeKind::Return)
        .ok_or("Return node must have an outgoing Return edge")?;
    assert_eq!(exit.to, None, "the Return exit edge leaves the modeled graph");
    Ok(())
}

/// `return $x` keeps the returned operand's read reachable, and control flows
/// operand -> Return (a fallthrough edge into the Return node).
#[test]
fn pir_a_return_value_read_is_reachable() -> TestResult {
    let graph = parse_and_lower("sub f { my $x = 1; return $x; }");
    let ret = single_return(&graph)?;
    let read = graph
        .nodes
        .iter()
        .find(|n| is_read_of(n, "x"))
        .ok_or("returned operand `$x` must lower to a reachable read node")?;
    let has_operand_edge = graph
        .edges
        .iter()
        .any(|e| e.from == read.id && e.to == Some(ret.id) && e.kind == PirEdgeKind::Fallthrough);
    assert!(
        has_operand_edge,
        "control must flow operand -> Return via a Fallthrough edge (operand evaluated before return)"
    );
    Ok(())
}

/// A statement after a `return` in the same block does NOT inherit a fallthrough
/// predecessor from the Return node — the return is terminal.
#[test]
fn pir_a_statement_after_return_is_not_reached_from_return() -> TestResult {
    let graph = parse_and_lower("sub f { return; my $dead = 1; }");
    let ret = single_return(&graph)?;
    // The only outgoing edge from the Return node is the terminal exit edge.
    let non_exit = graph.edges.iter().find(|e| e.from == ret.id && e.kind != PirEdgeKind::Return);
    assert!(
        non_exit.is_none(),
        "Return must not fall through to a later statement, found {non_exit:?}"
    );
    Ok(())
}

/// `return $a + $b` lowers both operands as reachable reads before the Return.
#[test]
fn pir_a_return_binary_value_lowers_both_operand_reads() -> TestResult {
    let graph = parse_and_lower("sub f { my $a = 1; my $b = 2; return $a + $b; }");
    single_return(&graph)?;
    assert!(
        graph.nodes.iter().any(|n| is_read_of(n, "a")),
        "operand `$a` must lower to a reachable read"
    );
    assert!(
        graph.nodes.iter().any(|n| is_read_of(n, "b")),
        "operand `$b` must lower to a reachable read"
    );
    Ok(())
}

/// Two returns in separate subs each emit their own Return node and exit edge.
#[test]
fn pir_a_two_subs_each_emit_a_return() -> TestResult {
    let graph = parse_and_lower("sub f { return; } sub g { return; }");
    let returns = return_nodes(&graph);
    assert_eq!(returns.len(), 2, "each sub's return emits a distinct Return node");
    for node in returns {
        let has_exit = graph
            .edges
            .iter()
            .any(|e| e.from == node.id && e.kind == PirEdgeKind::Return && e.to.is_none());
        assert!(has_exit, "each Return node must carry a terminal exit edge");
    }
    Ok(())
}

/// A `return` inside an `if` arm still emits a Return node with an exit edge,
/// reached from the branch rather than orphaned.
#[test]
fn pir_a_return_inside_branch_arm_emits_return() -> TestResult {
    let graph = parse_and_lower("sub f { if ($c) { return; } }");
    let node = single_return(&graph)?;
    let has_exit = graph
        .edges
        .iter()
        .any(|e| e.from == node.id && e.kind == PirEdgeKind::Return && e.to.is_none());
    assert!(has_exit, "a return inside a branch arm still carries its terminal exit edge");
    // The Return node is reached (has at least one incoming edge from the arm).
    let has_incoming = graph.edges.iter().any(|e| e.to == Some(node.id));
    assert!(has_incoming, "the Return node inside the arm must be reachable");
    Ok(())
}

/// `return` is now tallied as a first-class `Return` operation and is absent
/// from the unsupported-construct counts. Both halves matter: the receipt's
/// operation accounting must record the Return so a downstream regression that
/// silently drops the node (leaving the graph-node tests green) is still caught.
#[test]
fn pir_a_return_is_not_unsupported() -> TestResult {
    let graph = parse_and_lower("sub f { return; }");
    // Positive accounting: exactly one Return in the operation counts.
    assert_eq!(
        graph.receipt.operation_counts.get("Return"),
        Some(&1),
        "Return must be recorded once in operation_counts"
    );
    // Negative accounting: Return must not carry an unsupported tally.
    let unsupported = graph.receipt.unsupported_construct_counts.get("Return").copied();
    assert!(
        unsupported.is_none() || unsupported == Some(0),
        "Return must no longer be tallied as unsupported, got count {unsupported:?}"
    );
    Ok(())
}
