use crate::TreeCursor;
use crate::*;
use perl_tdd_support::must_some;
use std::ptr;

// Focused owner-exercising proof for `TreeCursor` (`cursor.rs`).
//
// RIPR reports `no_static_path` for the `TreeCursor` owner fields
// (`root`, `tree_source`, `path`) because the existing cursor tests only
// exercise the cursor through method calls (`node()`, `goto_*`, `reset()`)
// without ever naming the `TreeCursor` owner or its fields. These tests
// statically reference the owner type and each field with discriminating
// assertions so the changed seams have a static test path.

#[test]
fn tree_cursor_walk_initializes_owner_fields() {
    let source = "my $x = 1;";
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(source));
    let root = tree.root_node();

    let cursor: TreeCursor<'_> = root.walk();

    // `path` (cursor.rs) starts empty at the root.
    assert!(cursor.path.is_empty(), "fresh cursor path must be empty");
    // `tree_source` (cursor.rs) must carry the owning tree's source.
    assert_eq!(cursor.tree_source, source, "cursor must carry the tree source");
    // `root` (cursor.rs) must be the node that created the cursor.
    assert!(ptr::eq(cursor.root, root.inner()), "cursor root must reference the walk origin");
    assert_eq!(cursor.node().grammar_kind(), "source_file");
}

#[test]
fn tree_cursor_tree_walk_carries_owner_fields() {
    let source = "my $x = 1;";
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(source));

    let cursor: TreeCursor<'_> = tree.walk();

    assert!(cursor.path.is_empty(), "tree-walked cursor path must start empty");
    assert_eq!(cursor.tree_source, tree.source());
    assert_eq!(cursor.tree_source, source);
    assert!(
        ptr::eq(cursor.root, tree.root_node().inner()),
        "tree-walked cursor root must be the tree root"
    );
}

#[test]
fn tree_cursor_path_tracks_navigation_owner() {
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $a = 1; my $b = 2;"));
    let root = tree.root_node();
    let mut cursor: TreeCursor<'_> = root.walk();

    assert!(cursor.goto_first_child());
    assert_eq!(cursor.path, vec![0], "first child must record path [0]");

    assert!(cursor.goto_next_sibling());
    assert_eq!(cursor.path, vec![1], "next sibling must advance path to [1]");

    assert!(cursor.goto_parent());
    assert!(cursor.path.is_empty(), "parent from depth 1 must restore empty path");

    assert!(cursor.goto_first_child());
    assert_eq!(cursor.path, vec![0]);
    cursor.reset();
    assert!(cursor.path.is_empty(), "reset must clear the path owner");
}

#[test]
fn tree_cursor_node_uses_root_and_path_owner() {
    let source = "my $x = 1; my $y = 2;";
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(source));
    let mut cursor: TreeCursor<'_> = tree.walk();

    // Root node comes from `root` with empty `path`.
    assert_eq!(cursor.node().grammar_kind(), "source_file");
    assert_eq!(cursor.node().tree_source(), source);

    // First child resolves through `root` + `path = [0]`.
    assert!(cursor.goto_first_child());
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    assert_eq!(cursor.node().tree_source(), source);
    assert_eq!(cursor.path, vec![0]);

    // `tree_source` stays attached to nodes produced from the cursor.
    let node_source = cursor.node().tree_source().to_string();
    assert_eq!(node_source, source);
}
