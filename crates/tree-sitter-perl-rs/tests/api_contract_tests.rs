//! Additional API contract coverage for the Rust-native tree-sitter facade.
//!
//! These tests exercise edge-case behavior that downstream tree-sitter-style
//! consumers rely on: source spans, UTF-8 extraction failures, escape hatches,
//! and recursive traversal invariants across representative Perl syntax.

use std::error::Error;

use perl_tdd_support::{must, must_some};
use tree_sitter_perl_rs::{Parser, PerlNodeKind};

type TestResult = Result<(), Box<dyn Error>>;

fn parse(source: &str) -> tree_sitter_perl_rs::Tree {
    let mut parser = Parser::new();
    must_some(parser.parse(source))
}

fn assert_node_contracts(node: tree_sitter_perl_rs::Node<'_>, source: &str) -> TestResult {
    assert!(node.start_byte() <= node.end_byte(), "node span must be ordered");
    assert!(node.end_byte() <= source.len(), "node end must be clamped to source length");
    assert_eq!(node.tree_source(), source, "node should retain owning tree source");
    assert_eq!(node.child_count() == 0, node.is_leaf(), "leaf status should match child count");

    let text = must(node.utf8_text(source.as_bytes()));
    assert!(text.len() <= source.len(), "extracted text must come from the source buffer");

    let children: Vec<_> = node.children().collect();
    assert_eq!(children.len(), node.child_count(), "iterator count should match child_count");
    for (index, child) in children.into_iter().enumerate() {
        let indexed = must_some(node.child(index));
        assert_eq!(indexed.kind(), child.kind(), "indexed child and iterator child should agree");
        assert_node_contracts(child, source)?;
    }
    assert!(node.child(node.child_count()).is_none(), "one-past child index should be absent");

    Ok(())
}

#[test]
fn recursive_node_contracts_hold_for_representative_perl_syntax() -> TestResult {
    let sources = [
        "package Demo;\nuse strict;\nuse warnings;\nsub add { my ($x, $y) = @_; return $x + $y; }\n",
        "for my $item (@items) { print $item, \"\\n\" if defined $item; }\n",
        "my $sql = <<'SQL';\nSELECT * FROM widgets WHERE id = ?\nSQL\n",
        "my $name = $user->{profile}->{name} // 'unknown';\n",
        "my @parts = qw/foo bar baz/; my $regex = qr{^foo\\d+$};\n",
    ];

    for source in sources {
        let tree = parse(source);
        assert_eq!(tree.source(), source);
        assert_eq!(tree.root_node().kind(), "source_file");
        assert_node_contracts(tree.root_node(), source)?;
    }

    Ok(())
}

#[test]
fn escape_hatches_expose_tree_source_and_reexported_node_kind() -> TestResult {
    let source = "my $value = 42;\n";
    let tree = parse(source);
    let root = tree.root_node();

    assert_eq!(root.tree_source(), source);
    assert_eq!(root.inner().location.start, 0);
    assert!(root.inner().location.end <= source.len());
    assert!(matches!(root.inner().kind, PerlNodeKind::Program { .. }));

    let first_child = must_some(root.child(0));
    assert_eq!(first_child.tree_source(), source);
    assert!(first_child.start_byte() >= root.start_byte());
    assert!(first_child.end_byte() <= root.end_byte());

    Ok(())
}

#[test]
fn utf8_text_reports_invalid_caller_buffers_without_panicking() -> TestResult {
    let tree = parse("my $value = 42;\n");
    let root = tree.root_node();
    let invalid_utf8 = [0xff, 0xfe, 0xfd];

    let result = root.utf8_text(&invalid_utf8);

    assert!(result.is_err(), "invalid caller-provided UTF-8 should be reported as an error");
    Ok(())
}

#[test]
fn positions_use_byte_columns_for_crlf_and_multibyte_text() -> TestResult {
    let source = "my $emoji = '😀';\r\nmy $word = 'café';";
    let tree = parse(source);
    let root = tree.root_node();

    let start = root.start_position();
    assert_eq!(start.row, 0);
    assert_eq!(start.column, 0);

    let end = root.end_position();
    assert_eq!(end.row, 1);
    assert_eq!(end.column, "my $word = 'café';".len());
    assert_eq!(root.end_byte(), source.len());

    Ok(())
}

#[test]
fn cursor_navigation_from_subtree_is_rooted_at_that_subtree() -> TestResult {
    let tree = parse("my $first = 1; my $second = 2; my $third = 3;");
    let root = tree.root_node();
    let second_statement = must_some(root.child(1));
    let mut cursor = second_statement.walk();

    assert_eq!(cursor.node().start_byte(), second_statement.start_byte());
    assert!(!cursor.goto_previous_sibling(), "subtree cursor should not escape to root siblings");
    assert!(!cursor.goto_next_sibling(), "subtree cursor should not escape to root siblings");

    if cursor.goto_first_child() {
        assert!(cursor.node().start_byte() >= second_statement.start_byte());
        assert!(cursor.node().end_byte() <= second_statement.end_byte());
        assert!(cursor.goto_parent());
    }
    assert_eq!(cursor.node().start_byte(), second_statement.start_byte());

    Ok(())
}
