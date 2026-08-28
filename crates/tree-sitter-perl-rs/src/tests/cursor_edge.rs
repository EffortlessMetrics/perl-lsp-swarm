use crate::*;
use perl_tdd_support::must_some;

#[test]
fn test_tree_cursor_empty_source_root_is_valid() {
    // Empty source still produces a (minimal) tree; cursor at root should be valid.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(""));
    let cursor = tree.walk();

    let node = cursor.node();
    assert_eq!(node.grammar_kind(), "source_file");
    assert!(node.child_count() == 0, "empty source tree should have no statements");
}

#[test]
fn test_tree_cursor_empty_source_goto_first_child_returns_false() {
    // Empty source root has no children; goto_first_child must return false.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse(""));
    let mut cursor = tree.walk();

    let result = cursor.goto_first_child();
    assert!(!result, "empty tree root should have no first child");
}

#[test]
fn test_tree_cursor_single_statement_navigation() {
    // Single statement: root -> statement -> (children or leaf).
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("42;"));
    let mut cursor = tree.walk();

    assert_eq!(cursor.node().grammar_kind(), "source_file");
    assert!(cursor.goto_first_child(), "root should have exactly one statement");

    // The single statement should have no next sibling
    assert!(!cursor.goto_next_sibling(), "single statement should be the only child");

    // Going back up should land at root
    assert!(cursor.goto_parent(), "should be able to return to root");
    assert_eq!(cursor.node().grammar_kind(), "source_file");
}

#[test]
fn test_tree_cursor_sibling_navigation_exact_count() {
    // Navigate through all siblings and verify the count matches child_count.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("1; 2; 3; 4;"));
    let root = tree.root_node();
    let child_count = root.child_count();

    let mut cursor = tree.walk();
    assert!(cursor.goto_first_child());

    let mut sibling_count = 1;
    while cursor.goto_next_sibling() {
        sibling_count += 1;
    }

    assert_eq!(sibling_count, child_count, "sibling count should match root.child_count()");
}

#[test]
fn test_tree_cursor_alternating_parent_child_navigation() {
    // Test mixed navigation: down, up, down again at different indices.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("sub a { 1; } sub b { 2; }"));
    let mut cursor = tree.walk();

    // Down to first sub
    assert!(cursor.goto_first_child());
    let first_kind = cursor.node().grammar_kind().to_string();

    // Back to root
    assert!(cursor.goto_parent());
    assert_eq!(cursor.node().grammar_kind(), "source_file");

    // Down again to first sub (should be the same)
    assert!(cursor.goto_first_child());
    assert_eq!(
        cursor.node().grammar_kind(),
        first_kind,
        "re-navigating should reach the same node"
    );
}

#[test]
fn test_tree_cursor_complex_traversal_sequence() {
    // Complex sequence: down, sibling, sibling, up, down, sibling.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $a = 1; my $b = 2; my $c = 3;"));
    let mut cursor = tree.walk();

    // Down to first statement
    assert!(cursor.goto_first_child(), "down to first stmt");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");

    // Move to second statement
    assert!(cursor.goto_next_sibling(), "sibling to second stmt");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");

    // Move to third statement
    assert!(cursor.goto_next_sibling(), "sibling to third stmt");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");

    // No fourth statement
    assert!(!cursor.goto_next_sibling(), "no fourth statement");

    // Back to root
    assert!(cursor.goto_parent(), "back to root");
    assert_eq!(cursor.node().grammar_kind(), "source_file");

    // Down to first again
    assert!(cursor.goto_first_child(), "down to first again");
    assert_eq!(cursor.node().grammar_kind(), "my_declaration");
}

#[test]
fn test_tree_cursor_node_identity_after_traversal() {
    // A node retrieved at the same path should be equal across separate traversals.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $x = 1;"));
    let mut cursor = tree.walk();

    // First traversal: get to first child and extract its kind
    assert!(cursor.goto_first_child());
    let first_kind = cursor.node().grammar_kind().to_string();

    // Reset and repeat
    cursor.reset();
    assert!(cursor.goto_first_child());
    let second_kind = cursor.node().grammar_kind().to_string();

    assert_eq!(
        first_kind, second_kind,
        "node at the same path should have the same kind in both traversals"
    );
}

#[test]
fn test_tree_cursor_sibling_with_unicode_identifiers() {
    // Cursor must correctly navigate siblings even when source contains Unicode.
    let mut parser = Parser::new();
    let tree = must_some(parser.parse("my $café = 1; my $naïve = 2;"));
    let mut cursor = tree.walk();

    let root = tree.root_node();
    let expected_count = root.child_count();

    // Count siblings via cursor
    assert!(cursor.goto_first_child());
    let mut count = 1;
    while cursor.goto_next_sibling() {
        count += 1;
    }

    assert_eq!(
        count, expected_count,
        "sibling count should match even with Unicode identifiers"
    );
}

#[test]
fn test_tree_cursor_deeply_nested_structure() {
    // Verify cursor can navigate a deeply nested structure without stack overflow.
    let mut parser = Parser::new();
    // Create nested blocks: { { { ... } } }
    let mut code = String::new();
    for i in 0..5 {
        code.push_str(&format!("sub level_{} {{ ", i));
    }
    code.push_str("1;");
    for _ in 0..5 {
        code.push_str(" }");
    }

    let tree = must_some(parser.parse(&code));
    let mut cursor = tree.walk();

    // Navigate down as far as possible
    let mut depth = 0;
    while cursor.goto_first_child() && depth < 50 {
        depth += 1;
    }

    // Should have navigated several levels
    assert!(depth > 2, "should navigate multiple levels in nested structure");

    // Navigate back up
    while cursor.goto_parent() {
        depth -= 1;
    }

    // Should be back at root
    assert_eq!(cursor.node().grammar_kind(), "source_file");
    assert_eq!(depth, 0, "should have gone back up to root (depth 0)");
}
