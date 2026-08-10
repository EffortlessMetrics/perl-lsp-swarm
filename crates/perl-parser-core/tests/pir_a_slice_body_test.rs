//! PIR-A canonical body-path slice-expression lowering tests (issue #4989).
//!
//! Slice expressions (`@arr[$idx]`, `@hash{$key}`, `%hash{$key}`) previously
//! lowered to `HirExpr::Opaque`, so variable reads inside slice operands never
//! reached PIR-A `LexicalRead` facts. This slice mirrors the `FunctionCall` →
//! `HirExpr::Call` pattern: the slice itself stays unsupported in PIR, but its
//! operand expressions are walked for read facts.
//!
//! Tests return `Result` and use `.ok_or(...)?` rather than `expect`/`panic`,
//! per the crate's integration-test lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{PIR_RECEIPT_VERSION, PirGraph, PirOperation, lower_hir_bodies};

type TestResult = Result<(), Box<dyn Error>>;

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn is_read_of(node: &perl_parser_core::pir::PirNode, ident: &str) -> bool {
    match &node.operation {
        PirOperation::LexicalRead { name } => name.name == ident,
        PirOperation::StashRead { symbol } => symbol.name == ident,
        _ => false,
    }
}

fn has_read_of(graph: &PirGraph, ident: &str) -> bool {
    graph.nodes.iter().any(|n| is_read_of(n, ident))
}

fn has_write_of(graph: &PirGraph, ident: &str) -> bool {
    graph.nodes.iter().any(|n| match &n.operation {
        PirOperation::LexicalWrite { name } => name.name == ident,
        PirOperation::StashWrite { symbol } => symbol.name == ident,
        _ => false,
    })
}

/// `@arr[$idx]` in an assignment RHS must emit a Read for the index variable.
#[test]
fn pir_a_array_slice_index_produces_read_for_index_var() -> TestResult {
    let graph = parse_and_lower("my @arr; my @s = @arr[$idx];");

    assert!(
        has_read_of(&graph, "idx"),
        "array-slice index $idx must produce a Read; ops: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
    assert!(!has_write_of(&graph, "idx"), "undeclared slice index $idx must not produce a Write");
    assert_eq!(graph.receipt.schema_version, PIR_RECEIPT_VERSION);
    Ok(())
}

/// `@h{$key}` with an undeclared key variable must emit a Read for the key.
#[test]
fn pir_a_hash_slice_key_produces_read_for_key_var() -> TestResult {
    let graph = parse_and_lower("my %h; my @vals = @h{$key};");

    assert!(
        has_read_of(&graph, "key"),
        "hash-slice key $key must produce a Read; ops: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
    assert!(
        !has_write_of(&graph, "key"),
        "undeclared hash-slice key $key must not produce a Write"
    );
    Ok(())
}

/// `%h{$key}` key-value slice must emit a Read for an undeclared key variable.
#[test]
fn pir_a_key_value_slice_key_produces_read_for_key_var() -> TestResult {
    let graph = parse_and_lower("my %h; my %subset = %h{$key};");

    assert!(
        has_read_of(&graph, "key"),
        "key-value-slice key $key must produce a Read; ops: {:?}",
        graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
    );
    assert!(
        !has_write_of(&graph, "key"),
        "undeclared key-value-slice key $key must not produce a Write"
    );
    Ok(())
}

/// Multi-index and multi-key slices unwrap their parser array wrapper so each
/// operand contributes its own read fact.
#[test]
fn pir_a_multi_operand_slices_produce_reads_for_each_operand() -> TestResult {
    let graph =
        parse_and_lower("my @arr; my %h; my @values = @arr[$i, $j]; my @subset = @h{$k1, $k2};");

    for ident in ["i", "j", "k1", "k2"] {
        assert!(
            has_read_of(&graph, ident),
            "slice operand ${ident} must produce a Read; ops: {:?}",
            graph.nodes.iter().map(|n| n.operation.name()).collect::<Vec<_>>()
        );
        assert!(
            !has_write_of(&graph, ident),
            "undeclared slice operand ${ident} must not produce a Write"
        );
    }
    Ok(())
}

/// Slice lowering records the call shell as unsupported, like FunctionCall.
#[test]
fn pir_a_array_slice_records_call_as_unsupported() -> TestResult {
    let graph = parse_and_lower("my @arr; my @s = @arr[$idx];");

    let call_count = graph.receipt.unsupported_construct_counts.get("Call").copied().unwrap_or(0);
    assert!(
        call_count >= 1,
        "array slice must record Call as unsupported; receipt: {:?}",
        graph.receipt.unsupported_construct_counts
    );
    Ok(())
}
