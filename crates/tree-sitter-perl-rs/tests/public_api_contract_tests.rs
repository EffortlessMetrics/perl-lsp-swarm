//! Public API contract tests for `tree-sitter-perl-rs`.
//!
//! These scenarios exercise facade behavior from outside the crate so changes to
//! the public interoperability surface are caught by integration tests, not only
//! crate-private unit tests.

use perl_parser_core::edit::Edit;
use perl_position_tracking::Position;
use perl_tdd_support::{must, must_some};
use tree_sitter_perl_rs::{LANGUAGE, Parser, PerlLanguage, PerlNodeKind, language};

fn parse(source: &str) -> tree_sitter_perl_rs::Tree {
    let mut parser = Parser::new();
    must_some(parser.parse(source))
}

fn edit_replacing_byte_range(old_start: usize, old_end: usize, new_end: usize) -> Edit {
    Edit::new(
        old_start,
        old_end,
        new_end,
        Position::new(old_start, 0, old_start as u32),
        Position::new(old_end, 0, old_end as u32),
        Position::new(new_end, 0, new_end as u32),
    )
}

#[test]
fn when_using_default_parser_then_it_behaves_like_new_parser() {
    let mut parser = Parser::default();

    let tree = must_some(parser.parse("my $answer = 42;"));

    assert_eq!(tree.source(), "my $answer = 42;");
    assert_eq!(tree.root_node().kind(), "source_file");
}

#[test]
fn when_language_descriptor_is_obtained_then_public_contract_is_stable() {
    let descriptor = language();
    let default_descriptor = PerlLanguage::default();

    assert_eq!(descriptor, LANGUAGE);
    assert_eq!(default_descriptor, LANGUAGE);
    assert_eq!(descriptor.node_kind_count(), descriptor.node_kind_names().len());
    assert!(descriptor.node_kind_is_named("Program"));
    assert!(descriptor.node_kind_names().contains(&"Subroutine"));
    assert!(!descriptor.node_kind_is_named("source_file"));
    assert!(!descriptor.node_kind_is_named(""));
}

#[test]
fn when_node_kind_reexport_is_used_then_it_matches_inner_ast_kind() {
    let tree = parse("sub greet { return 1; }");
    let sub_node =
        must_some(tree.root_node().children().find(|node| node.native_kind() == "Subroutine"));

    assert!(matches!(sub_node.inner().kind, PerlNodeKind::Subroutine { .. }));
    assert_eq!(sub_node.grammar_kind(), "sub");
}

#[test]
fn when_child_utf8_text_is_requested_then_exact_source_slice_is_returned() {
    let source = "my $greeting = 'café';\nmy $next = 2;";
    let tree = parse(source);
    let first_statement = must_some(tree.root_node().child(0));

    let text = must(first_statement.utf8_text(source.as_bytes()));
    let expected_text = &source[first_statement.start_byte()..first_statement.end_byte()];

    assert_eq!(text, expected_text);
    assert_eq!(first_statement.tree_source(), source);
}

#[test]
fn when_node_walk_starts_from_child_then_cursor_root_is_that_child() {
    let tree = parse("my $a = 1; my $b = 2;");
    let first_statement = must_some(tree.root_node().child(0));
    let first_statement_kind = first_statement.kind();
    let mut cursor = first_statement.walk();

    assert_eq!(cursor.node().kind(), first_statement_kind);
    assert!(!cursor.goto_parent(), "a node-rooted cursor must treat the node as its root");
    assert!(!cursor.goto_next_sibling(), "a node-rooted cursor root has no parent sibling context");
    assert!(cursor.goto_first_child(), "the statement itself should still expose its children");
    assert!(cursor.goto_parent(), "child traversal should return to the node-rooted cursor root");
    assert_eq!(cursor.node().kind(), first_statement_kind);
}

#[test]
fn when_tree_is_cloned_then_editing_clone_does_not_modify_original() {
    let tree = parse("my $x = 1;");
    let mut cloned = tree.clone();
    let edit = edit_replacing_byte_range(8, 9, 10);

    cloned.edit(&edit);

    assert_eq!(tree.source(), "my $x = 1;");
    assert_eq!(cloned.source(), tree.source());
    assert_ne!(cloned, tree, "recording an edit on the clone should only change clone metadata");
}

#[test]
fn when_reparse_uses_edited_clone_then_original_tree_can_still_take_noop_fast_path() {
    let source = "my $x = 1;";
    let mut parser = Parser::new();
    let original = must_some(parser.parse(source));
    let mut edited_clone = original.clone();
    let edit = edit_replacing_byte_range(8, 9, 10);
    edited_clone.edit(&edit);

    let noop_reparse = must_some(parser.parse_with_old_tree(source, &original));
    let edited_reparse = must_some(parser.parse_with_old_tree(source, &edited_clone));

    assert_eq!(noop_reparse, original, "unedited tree should use the unchanged-source fast path");
    assert_eq!(edited_reparse.source(), source);
    assert_ne!(edited_reparse, edited_clone, "pending edits should force a fresh tree");
}
