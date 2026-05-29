//! Public API regression coverage for the `tree-sitter-perl-rs` facade.
//!
//! These tests exercise contracts that downstream tree-sitter-style callers rely on:
//! child spans and positions, cursor boundary behavior, invalid caller-supplied text
//! buffers, unchanged incremental reparses, and the public `PerlNodeKind` re-export.

use perl_tdd_support::{must, must_some};
use tree_sitter_perl_rs::{Parser, PerlNodeKind};

fn parse(source: &str) -> tree_sitter_perl_rs::Tree {
    let mut parser = Parser::new();
    must_some(parser.parse(source))
}

#[test]
fn when_child_on_second_line_is_queried_then_byte_position_uses_zero_based_rows_and_columns() {
    let source = "my $first = 1;\nmy $second = 2;";
    let tree = parse(source);
    let root = tree.root_node();
    let second_statement = must_some(root.child(1));

    let start = second_statement.start_position();
    assert_eq!(start.row, 1);
    assert_eq!(start.column, 0);
    assert_eq!(second_statement.start_byte(), must_some(source.find("my $second")));
    assert_eq!(must(second_statement.utf8_text(source.as_bytes())), "my $second");
}

#[test]
fn when_child_text_is_extracted_from_full_source_then_only_child_span_is_returned() {
    let source = "my $answer = 42;\n$answer++;";
    let tree = parse(source);
    let root = tree.root_node();
    let declaration = must_some(root.child(0));

    assert_eq!(declaration.grammar_kind(), "my_declaration");
    assert_eq!(must(declaration.utf8_text(source.as_bytes())), "my $answer");
    assert_ne!(declaration.tree_source(), must(declaration.utf8_text(source.as_bytes())));
}

#[test]
fn when_utf8_text_receives_invalid_caller_buffer_then_error_is_returned_without_panicking() {
    let source = "my $x = 1;";
    let tree = parse(source);
    let invalid_utf8 = vec![0xff; source.len()];

    let result = tree.root_node().utf8_text(&invalid_utf8);

    assert!(result.is_err(), "invalid caller-provided bytes must be reported as UTF-8 errors");
}

#[test]
fn when_parse_with_old_tree_receives_unchanged_source_then_tree_is_reused_observably() {
    let source = "use strict;\nmy $x = 1;\n";
    let mut parser = Parser::new();
    let old_tree = must_some(parser.parse(source));

    let reparsed = must_some(parser.parse_with_old_tree(source, &old_tree));

    assert_eq!(reparsed, old_tree, "unchanged source with no pending edits must clone old tree");
    assert_eq!(reparsed.root_node().to_sexp(), old_tree.root_node().to_sexp());
}

#[test]
fn when_cursor_fails_to_advance_past_last_sibling_then_it_stays_on_last_sibling() {
    let tree = parse("my $a = 1; my $b = 2;");
    let mut cursor = tree.walk();

    assert!(cursor.goto_first_child(), "root should have a first child");
    assert!(cursor.goto_next_sibling(), "first declaration should have a next sibling");
    let last_start = cursor.node().start_byte();

    assert!(!cursor.goto_next_sibling(), "last sibling must not advance past the sibling list");
    assert_eq!(cursor.node().start_byte(), last_start, "failed next-sibling move must be stable");
    assert!(cursor.goto_previous_sibling(), "cursor should still be able to move backward");
    assert_eq!(cursor.node().start_byte(), 0);
}

#[test]
fn when_using_perl_node_kind_reexport_then_callers_can_match_program_without_perl_ast_dep() {
    let tree = parse("my $x = 1;");

    let root = tree.root_node();
    let root_child_count = root.child_count();
    let mut matched_program = false;

    if let PerlNodeKind::Program { statements } = &root.inner().kind {
        matched_program = true;
        assert_eq!(statements.len(), root_child_count);
    }

    assert!(matched_program, "root node should be Program through PerlNodeKind re-export");
}
