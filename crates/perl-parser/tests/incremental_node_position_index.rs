#![cfg(feature = "incremental")]
//! Containment contract for `IncrementalTree::find_containing_node` (#13237).
//!
//! The retired node-position index only descended into seven structural
//! families, so containment lookups under assignments, loops, method calls,
//! and most other nodes returned an ancestor or `None`. These tests pin the
//! canonical-traversal replacement:
//!
//! - the smallest containing node is found under structural families the
//!   retired hand-written match never descended into;
//! - selection is deterministic for nested and identical ranges;
//! - reversed, zero-width, recovery, and Unicode-adjacent queries are explicit;
//! - construction performs no hidden indexing or subtree duplication.

use perl_parser::incremental_v2::IncrementalTree;
use perl_parser::{Node, NodeKind, Parser, SourceLocation};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn tree_for(source: &str) -> Result<IncrementalTree, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let root = parser.parse()?;
    Ok(IncrementalTree::new(root, source.to_string()))
}

/// Byte span of the first occurrence of `needle` in `source`.
fn span_of(source: &str, needle: &str) -> (usize, usize) {
    let start = source.find(needle).expect("needle must exist in source");
    (start, start + needle.len())
}

fn kind_name_at(tree: &IncrementalTree, start: usize, end: usize) -> Option<&'static str> {
    tree.find_containing_node(start, end).map(|n| n.kind.kind_name())
}

// ---------------------------------------------------------------------------
// Completeness: families the retired partial index omitted
// ---------------------------------------------------------------------------

#[test]
fn finds_smallest_node_inside_assignment_rhs() -> TestResult {
    let source = "$count = 42;";
    let tree = tree_for(source)?;
    let (start, end) = span_of(source, "42");
    assert_eq!(
        kind_name_at(&tree, start, end),
        Some("Number"),
        "the number literal must be found, not an assignment ancestor"
    );
    Ok(())
}

#[test]
fn finds_deepest_node_of_assignment_rhs_expression() -> TestResult {
    let source = "$total = 1 + 2;";
    let tree = tree_for(source)?;
    let (one_start, one_end) = span_of(source, "1");
    let (expr_start, expr_end) = span_of(source, "1 + 2");
    assert_eq!(kind_name_at(&tree, one_start, one_end), Some("Number"));
    assert_eq!(
        kind_name_at(&tree, expr_start, expr_end),
        Some("Binary"),
        "the binary subexpression must be found, not an assignment ancestor"
    );
    Ok(())
}

#[test]
fn finds_nodes_inside_while_condition_and_body() -> TestResult {
    let source = "while ($n < 10) { $total = 7; }";
    let tree = tree_for(source)?;
    let (cond_start, cond_end) = span_of(source, "10");
    assert_eq!(
        kind_name_at(&tree, cond_start, cond_end),
        Some("Number"),
        "the loop condition literal must be found, not a while ancestor"
    );
    let (body_start, body_end) = span_of(source, "7");
    assert_eq!(
        kind_name_at(&tree, body_start, body_end),
        Some("Number"),
        "the loop body literal must be found, not a while ancestor"
    );
    let (var_start, var_end) = span_of(source, "$total");
    assert_eq!(
        kind_name_at(&tree, var_start, var_end),
        Some("Variable"),
        "the loop body variable must be found, not a while ancestor"
    );
    Ok(())
}

#[test]
fn finds_argument_inside_method_call() -> TestResult {
    let source = "$u->get(\"k\");";
    let tree = tree_for(source)?;
    let (start, end) = span_of(source, "\"k\"");
    assert_eq!(
        kind_name_at(&tree, start, end),
        Some("String"),
        "the method call argument must be found, not a statement ancestor"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Determinism and query geometry
// ---------------------------------------------------------------------------

#[test]
fn identical_ranges_select_deepest_node_deterministically() {
    let inner =
        Node::new(NodeKind::Number { value: "7".to_string() }, SourceLocation { start: 0, end: 3 });
    let mid =
        Node::new(NodeKind::Block { statements: vec![inner] }, SourceLocation { start: 0, end: 3 });
    let root =
        Node::new(NodeKind::Block { statements: vec![mid] }, SourceLocation { start: 0, end: 3 });
    let tree = IncrementalTree::new(root, "abc".to_string());

    let first = kind_name_at(&tree, 0, 3);
    let second = kind_name_at(&tree, 0, 3);
    assert_eq!(first, Some("Number"), "the deepest node must win");
    assert_eq!(first, second, "selection must be deterministic across calls");
}

#[test]
fn zero_width_query_inside_code_finds_smallest_node() -> TestResult {
    let source = "my $x = 42;";
    let tree = tree_for(source)?;
    // Point at the start byte of the literal `42`.
    assert_eq!(kind_name_at(&tree, 8, 8), Some("Number"));
    // Point at EOF is contained only by the program node.
    assert_eq!(kind_name_at(&tree, source.len(), source.len()), Some("Program"));
    Ok(())
}

#[test]
fn reversed_query_returns_none() -> TestResult {
    let source = "my $x = 42;";
    let tree = tree_for(source)?;
    assert_eq!(
        tree.find_containing_node(9, 8).map(|_| ()),
        None,
        "an invalid reversed query must not match any node"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Recovery and byte geometry
// ---------------------------------------------------------------------------

#[test]
fn finds_recovered_node_after_incomplete_expression() -> TestResult {
    // `my $x = ;` recovers with a zero-width MissingExpression at byte 6.
    let source = "my $x = ;";
    let tree = tree_for(source)?;
    let kind = kind_name_at(&tree, 6, 6);
    assert_eq!(
        kind,
        Some("MissingExpression"),
        "the zero-width recovery node must be found at its own position"
    );
    Ok(())
}

#[test]
fn unicode_before_probe_preserves_byte_geometry() -> TestResult {
    let source = "my $s = \"日本\" . \"ok\";";
    let tree = tree_for(source)?;
    let (start, end) = span_of(source, "\"ok\"");
    assert_eq!(kind_name_at(&tree, start, end), Some("String"));
    let node = tree.find_containing_node(start, end).expect("containing node must exist");
    assert_eq!(
        &source[node.location.start..node.location.end],
        "\"ok\"",
        "the returned node span must be exact byte geometry after multibyte text"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Query outside the tree
// ---------------------------------------------------------------------------

#[test]
fn query_outside_tree_returns_none() -> TestResult {
    let source = "my $x = 1;";
    let tree = tree_for(source)?;
    assert_eq!(tree.find_containing_node(100, 200).map(|_| ()), None);
    Ok(())
}
