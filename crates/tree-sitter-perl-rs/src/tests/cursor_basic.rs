use crate::*;
use perl_tdd_support::must_some;

#[test]
fn test_tree_cursor_walks_children_and_siblings() {
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1; my $y = 2;"));
    let root = tree.root_node();
    let mut cursor = root.walk();

    assert_eq!(cursor.node().grammar_kind(), "source_file");
    assert!(cursor.goto_first_child(), "source_file should have at least one child");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    assert!(cursor.goto_next_sibling(), "first statement should have a sibling");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    assert!(!cursor.goto_next_sibling(), "second statement should be the last sibling");
}

#[test]
fn test_tree_walk_starts_cursor_at_root() {
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1;"));
    let mut cursor = tree.walk();

    assert_eq!(cursor.node().grammar_kind(), "source_file");
    assert!(cursor.goto_first_child(), "root should have a child");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
}

#[test]
fn test_tree_cursor_parent_and_reset_behavior() {
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1;"));
    let root = tree.root_node();
    let mut cursor = root.walk();

    assert!(!cursor.goto_parent(), "cursor at root must not move to parent");
    assert!(cursor.goto_first_child(), "root should have a child");
    assert!(cursor.goto_parent(), "child should have root as parent");
    assert_eq!(cursor.node().grammar_kind(), "source_file");

    assert!(cursor.goto_first_child(), "root should still have a child");
    cursor.reset();
    assert_eq!(cursor.node().grammar_kind(), "source_file");
}

#[test]
fn test_tree_cursor_last_child_and_previous_sibling_behavior() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $a = 1; my $b = 2;"));
    let root = tree.root_node();
    let mut cursor = root.walk();

    assert!(cursor.goto_last_child(), "root should have a last child");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    assert!(cursor.goto_previous_sibling(), "last child should have a previous sibling");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
    assert!(!cursor.goto_previous_sibling(), "first sibling should not have previous sibling");
}

#[test]
fn test_tree_cursor_last_child_returns_false_for_leaf() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 42;"));
    let root = tree.root_node();
    let mut cursor = root.walk();

    assert!(cursor.goto_first_child(), "root should have a child");
    assert!(cursor.goto_first_child(), "my_declaration should have a child");
    let at_leaf = !cursor.goto_last_child();
    assert!(at_leaf, "leaf nodes should not have a last child");
}

#[test]
fn test_tree_cursor_goto_first_child_returns_false_for_leaf() {
    // A leaf node has no children; goto_first_child must return false and
    // leave the cursor positioned at the leaf rather than panicking.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1;"));
    let root = tree.root_node();
    let mut cursor = root.walk();

    // Navigate to a leaf: root -> first child (my_declaration) -> first child (leaf token).
    assert!(cursor.goto_first_child(), "root should have a child");
    assert!(cursor.goto_first_child(), "my_declaration should have a child");
    // The leaf must refuse another goto_first_child.
    let at_leaf = !cursor.goto_first_child();
    assert!(at_leaf, "goto_first_child must return false on a leaf node");
}

#[test]
fn test_tree_cursor_multiple_goto_next_sibling_exhausts() {
    // When repeatedly calling goto_next_sibling, the cursor must eventually
    // return false and stay positioned at the last sibling.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("1; 2; 3;"));
    let mut cursor = tree.walk();

    // Navigate to first statement
    assert!(cursor.goto_first_child());
    let mut _count = 1;
    // Keep advancing siblings until we can't
    while cursor.goto_next_sibling() {
        _count += 1;
    }
    // After last goto_next_sibling returns false, cursor should still be valid
    // and still have a node (the last sibling).
    let node = cursor.node();
    assert!(
        !node.kind().is_empty(),
        "cursor should remain at valid node after exhausting siblings"
    );
}

#[test]
fn test_tree_cursor_goto_parent_at_root_repeatedly() {
    // Calling goto_parent at root should return false every time, keeping cursor at root.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1;"));
    let mut cursor = tree.walk();

    // We should always be at root initially
    assert_eq!(cursor.node().grammar_kind(), "source_file");

    // Try to go up multiple times — must stay at root
    for _ in 0..3 {
        let result = cursor.goto_parent();
        assert!(!result, "goto_parent at root must return false");
        assert_eq!(
            cursor.node().grammar_kind(),
            "source_file",
            "cursor must remain at root after failed goto_parent"
        );
    }
}

#[test]
fn test_tree_cursor_reset_from_deep_nesting() {
    // reset() must return cursor to root regardless of depth.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("sub foo { my $x = 1; }"));
    let mut cursor = tree.walk();

    // Navigate deep into the tree
    let mut depth = 0;
    while cursor.goto_first_child() && depth < 10 {
        depth += 1;
    }
    assert!(depth > 0, "should have navigated at least one level deep");

    // reset() should bring us back to root
    cursor.reset();
    assert_eq!(cursor.node().grammar_kind(), "source_file", "reset must return cursor to root");
}
