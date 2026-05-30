//! Cross-consumer span-boundary regression tests.
//!
//! These tests pin the half-open `[start, end)` semantics of
//! `Node::find_deepest_containing_offset` and `Node::contains_offset`,
//! guarding against any future consumer that accidentally reverts to closed
//! `[start, end]` interval semantics.
//!
//! Background: issue #910 — several LSP providers used closed-interval bounds,
//! causing them to resolve a cursor placed exactly at `node.location.end` to the
//! node itself rather than to its parent.  After centralizing all consumers on
//! `find_deepest_containing_offset`, this file is the anti-regression proof.

use perl_ast::{Node, NodeKind, SourceLocation};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

/// Build a minimal `my $x = 1;` shaped AST for offset tests.
///
/// Layout (byte offsets):
///   `my $x = 1;`
///    0123456789A
///
/// Variable node `$x` → [3, 5)
/// Number node `1`    → [8, 9)
/// VarDecl node       → [0, 10)
/// Program            → [0, 11)
fn build_my_x_ast() -> Node {
    let variable = Node::new(
        NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
        loc(3, 5),
    );
    let number = Node::new(NodeKind::Number { value: "1".to_string() }, loc(8, 9));
    let decl = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(variable),
            initializer: Some(Box::new(number)),
            attributes: vec![],
        },
        loc(0, 10),
    );
    Node::new(NodeKind::Program { statements: vec![decl] }, loc(0, 11))
}

// ---------------------------------------------------------------------------
// Half-open boundary: offset == node.location.end returns None (or parent)
// ---------------------------------------------------------------------------

#[test]
fn test_half_open_at_variable_end_returns_none_not_variable() -> TestResult {
    let ast = build_my_x_ast();

    // The `$x` variable node spans [3, 5).
    // A cursor at offset 5 is *past* `$x` — it must NOT resolve to the Variable node.
    let result = ast.find_deepest_containing_offset(5);

    // The VariableDeclaration spans [0, 10) which contains offset 5.
    // So we expect the VarDecl node (or something between VarDecl and Program), NOT Variable.
    match result {
        None => {
            // This could happen if no node spans offset 5 — not the case here since VarDecl covers it.
            return Err("Expected Some(VarDecl) but got None".into());
        }
        Some(found) => {
            assert_ne!(
                found.kind.kind_name(),
                "Variable",
                "cursor at Variable.end (offset 5) must NOT resolve to the Variable node; \
                 got {} at [{}, {})",
                found.kind.kind_name(),
                found.location.start,
                found.location.end,
            );
        }
    }

    Ok(())
}

#[test]
fn test_half_open_at_variable_end_minus_one_returns_variable() -> TestResult {
    let ast = build_my_x_ast();

    // Cursor at offset 4 is the last byte *inside* `$x` [3, 5).
    // Must resolve to the Variable node.
    let result = ast.find_deepest_containing_offset(4);

    let found = result.ok_or("Expected Some(Variable) but got None")?;
    assert_eq!(
        found.kind.kind_name(),
        "Variable",
        "cursor at last byte of Variable (offset 4) must resolve to Variable; \
         got {} at [{}, {})",
        found.kind.kind_name(),
        found.location.start,
        found.location.end,
    );
    Ok(())
}

#[test]
fn test_half_open_at_number_end_resolves_to_parent_not_number() -> TestResult {
    let ast = build_my_x_ast();

    // Number node `1` spans [8, 9).
    // Cursor at offset 9 is past `1` — must NOT resolve to Number.
    let result = ast.find_deepest_containing_offset(9);

    match result {
        None => {
            return Err("Expected Some(VarDecl) but got None — VarDecl spans [0, 10) so 9 is inside".into());
        }
        Some(found) => {
            assert_ne!(
                found.kind.kind_name(),
                "Number",
                "cursor at Number.end (offset 9) must NOT resolve to Number; \
                 got {} at [{}, {})",
                found.kind.kind_name(),
                found.location.start,
                found.location.end,
            );
        }
    }

    Ok(())
}

#[test]
fn test_half_open_at_number_last_byte_returns_number() -> TestResult {
    let ast = build_my_x_ast();

    // Cursor at offset 8 is the only byte inside `1` [8, 9).
    // Must resolve to Number.
    let result = ast.find_deepest_containing_offset(8);

    let found = result.ok_or("Expected Some(Number) but got None")?;
    assert_eq!(
        found.kind.kind_name(),
        "Number",
        "cursor at last byte of Number (offset 8) must resolve to Number; \
         got {} at [{}, {})",
        found.kind.kind_name(),
        found.location.start,
        found.location.end,
    );
    Ok(())
}

#[test]
fn test_half_open_outside_program_returns_none() -> TestResult {
    let ast = build_my_x_ast();

    // Program spans [0, 11). Cursor at 11 is past the program entirely.
    let result = ast.find_deepest_containing_offset(11);

    assert!(
        result.is_none(),
        "cursor past program end (offset 11) must return None; got {:?}",
        result.map(|n| (n.kind.kind_name(), n.location.start, n.location.end)),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// contains_offset: direct boundary tests
// ---------------------------------------------------------------------------

#[test]
fn test_contains_offset_start_is_inclusive() -> TestResult {
    let node = Node::new(NodeKind::Identifier { name: "foo".to_string() }, loc(5, 8));
    assert!(node.contains_offset(5), "start offset 5 must be contained in [5, 8)");
    Ok(())
}

#[test]
fn test_contains_offset_end_is_exclusive() -> TestResult {
    let node = Node::new(NodeKind::Identifier { name: "foo".to_string() }, loc(5, 8));
    assert!(!node.contains_offset(8), "end offset 8 must NOT be contained in [5, 8)");
    Ok(())
}

#[test]
fn test_contains_offset_last_interior_byte() -> TestResult {
    let node = Node::new(NodeKind::Identifier { name: "foo".to_string() }, loc(5, 8));
    assert!(node.contains_offset(7), "offset 7 must be contained in [5, 8)");
    Ok(())
}

#[test]
fn test_contains_offset_before_start() -> TestResult {
    let node = Node::new(NodeKind::Identifier { name: "foo".to_string() }, loc(5, 8));
    assert!(!node.contains_offset(4), "offset 4 must NOT be contained in [5, 8)");
    Ok(())
}
