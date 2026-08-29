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

use perl_parser::edit::Edit;
use perl_parser::incremental_v2::IncrementalTree;
use perl_parser::position::Position;
use perl_parser::{Node, NodeKind, Parser, SourceLocation};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn tree_for(source: &str) -> Result<IncrementalTree, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    let root = parser.parse()?;
    Ok(IncrementalTree::new(root, source.to_string()))
}

/// Byte span of the first occurrence of `needle` in `source`.
fn span_of(source: &str, needle: &str) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let start =
        source.find(needle).ok_or_else(|| format!("needle {needle:?} must exist in source"))?;
    Ok((start, start + needle.len()))
}

fn kind_name_at(tree: &IncrementalTree, start: usize, end: usize) -> Option<&'static str> {
    tree.find_containing_node(start, end).map(|n| n.kind.kind_name())
}

/// Replace the first occurrence of `find` in `source1` with `replacement` and
/// assert the admitted incremental result equals a fresh parse.
fn value_edit_matches_fresh(source1: &str, find: &str, replacement: &str) -> TestResult {
    let (start, old_end) = span_of(source1, find)?;
    let mut parser = perl_parser::incremental_v2::IncrementalParserV2::new();
    parser.parse(source1)?;
    parser.edit(Edit::new(
        start,
        old_end,
        start + replacement.len(),
        Position::new(start, 0, start as u32),
        Position::new(old_end, 0, old_end as u32),
        Position::new(start + replacement.len(), 0, (start + replacement.len()) as u32),
    ));
    let source2 = format!("{}{}{}", &source1[..start], replacement, &source1[old_end..]);

    let incremental = parser.parse(&source2)?;
    assert!(
        parser.used_incremental_path(),
        "the value edit must be admitted by the incremental path"
    );
    let fresh = Parser::new(&source2).parse()?;
    assert_eq!(
        incremental, fresh,
        "admitted value edit must equal a fresh parse in shape and span geometry"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Completeness: families the retired partial index omitted
// ---------------------------------------------------------------------------

#[test]
fn finds_smallest_node_inside_assignment_rhs() -> TestResult {
    let source = "$count = 42;";
    let tree = tree_for(source)?;
    let (start, end) = span_of(source, "42")?;
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
    let (one_start, one_end) = span_of(source, "1")?;
    let (expr_start, expr_end) = span_of(source, "1 + 2")?;
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
    let (cond_start, cond_end) = span_of(source, "10")?;
    assert_eq!(
        kind_name_at(&tree, cond_start, cond_end),
        Some("Number"),
        "the loop condition literal must be found, not a while ancestor"
    );
    let (body_start, body_end) = span_of(source, "7")?;
    assert_eq!(
        kind_name_at(&tree, body_start, body_end),
        Some("Number"),
        "the loop body literal must be found, not a while ancestor"
    );
    let (var_start, var_end) = span_of(source, "$total")?;
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
    let (start, end) = span_of(source, "\"k\"")?;
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
    let (start, end) = span_of(source, "\"ok\"")?;
    assert_eq!(kind_name_at(&tree, start, end), Some("String"));
    let node = tree.find_containing_node(start, end).ok_or("containing node must exist")?;
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

// ---------------------------------------------------------------------------
// Fresh equivalence of edits the complete lookup newly admits
// ---------------------------------------------------------------------------

#[test]
fn grow_value_edit_inside_subroutine_matches_fresh() -> TestResult {
    value_edit_matches_fresh("sub func { my $x = 123; return $x * 2; }", "123", "12456")
}

#[test]
fn same_length_value_edit_inside_subroutine_matches_fresh() -> TestResult {
    value_edit_matches_fresh("sub func { my $x = 123; return $x * 2; }", "123", "456")
}

#[test]
fn shrink_value_edit_inside_subroutine_matches_fresh() -> TestResult {
    value_edit_matches_fresh("sub func { my $x = 123; return $x * 2; }", "123", "7")
}

#[test]
fn value_edit_inside_while_body_matches_fresh() -> TestResult {
    value_edit_matches_fresh("while (1) { my $x = 5; }", "5", "42")
}

#[test]
fn string_edit_inside_method_call_argument_matches_fresh() -> TestResult {
    value_edit_matches_fresh("$u->get(\"k\");", "\"k\"", "\"j\"")
}

// ---------------------------------------------------------------------------
// Stack safety and non-quadratic construction
// ---------------------------------------------------------------------------

#[test]
fn deep_chain_construction_and_lookup_stay_stack_safe() {
    // A 50,000-node chain is the same adversarial depth the iterative
    // `Node` clone/drop overflow proofs use. The retired index built its
    // map with per-node recursion and an owned subtree clone per entry, so
    // this construction overflows (or retains quadratic clones) on
    // pre-#13237 code; the retired tree stores the root without traversal
    // and looks up through an explicit heap stack instead.
    const DEPTH: usize = 50_000;

    let mut current = Node::new(
        NodeKind::Number { value: "1".to_string() },
        SourceLocation { start: DEPTH, end: DEPTH + 1 },
    );
    for depth in (0..DEPTH).rev() {
        current = Node::new(
            NodeKind::Unary { op: "!".to_string(), operand: Box::new(current) },
            SourceLocation { start: depth, end: DEPTH + 1 },
        );
    }

    let tree = IncrementalTree::new(current, " ".repeat(DEPTH + 1));

    // The deepest node is found by an iterative lookup, and every node on
    // the chain has exactly the narrowest span at its own start byte.
    let found = tree.find_containing_node(DEPTH, DEPTH + 1);
    assert_eq!(found.map(|n| n.kind.kind_name()), Some("Number"));
    let mid = tree.find_containing_node(1, 2);
    assert_eq!(mid.map(|n| n.kind.kind_name()), Some("Unary"));
    assert_eq!(mid.map(|n| n.location.start), Some(1));
}
