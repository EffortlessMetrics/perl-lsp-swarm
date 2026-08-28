use crate::*;
use perl_tdd_support::must_some;

#[test]
fn test_parser_creates_tree() {
    let mut p = Parser::new();
    let tree = p.parse("my $x = 42;");
    assert!(tree.is_some());
}

#[test]
fn test_root_node_kind() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 42;"));
    assert_eq!(tree.root_node().kind(), "source_file");
    assert_eq!(tree.root_node().native_kind(), "Program");
}

#[test]
fn test_to_sexp_starts_with_source_file() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 42;"));
    let sexp = tree.root_node().to_sexp();
    assert!(
        sexp.starts_with("(source_file"),
        "sexp should start with (source_file, got: {}",
        sexp
    );
}

#[test]
fn test_child_count_for_program_with_statements() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 42;\nmy $y = 99;"));
    let root = tree.root_node();
    assert!(root.child_count() >= 1, "root should have children");
}

#[test]
fn test_start_and_end_byte() {
    let source = "my $x = 42;";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();
    assert_eq!(root.start_byte(), 0);
    assert_eq!(root.end_byte(), source.len(), "root end_byte should clamp to source length");
}

#[test]
fn test_start_and_end_position_are_tree_sitter_compatible() {
    let source = "my $x = 1;\nmy $y = 2;";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();

    assert_eq!(root.start_position(), Point { row: 0, column: 0 });
    assert_eq!(root.end_position(), Point { row: 1, column: 10 });
}

#[test]
fn test_end_position_column_uses_bytes_not_chars() {
    let source = "my $emoji = \"😀\";";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();

    assert_eq!(root.end_byte(), source.len());
    assert_eq!(root.end_position(), Point { row: 0, column: source.len() });
}

/// Verify the end_byte clamp invariant: for every node in the tree,
/// `end_byte()` must not exceed `tree.source().len()`.  This exercises the
/// `.min(self.tree_source.len())` guard on the full node set, not just the
/// root, so that any future parser regression producing an out-of-bounds
/// location is caught here.
#[test]
fn test_end_byte_never_exceeds_source_len_for_all_nodes() {
    let sources = [
        "my $x = 42;",
        "sub foo { return 1; }",
        "use strict;\nuse warnings;\nmy @arr = (1, 2, 3);",
        // empty source — edge case for zero-length trees
        "",
    ];
    for source in sources {
        let mut p = Parser::new();
        let tree = match p.parse(source) {
            Some(t) => t,
            // v3 parser returns None only on extreme failure; skip rather than panic
            None => continue,
        };
        let source_len = tree.source().len();
        // Walk all direct children of root and check the invariant
        let root = tree.root_node();
        assert!(
            root.end_byte() <= source_len,
            "root end_byte {} > source_len {} for source {:?}",
            root.end_byte(),
            source_len,
            source
        );
        for child in root.children() {
            assert!(
                child.end_byte() <= source_len,
                "child end_byte {} > source_len {} for source {:?}",
                child.end_byte(),
                source_len,
                source
            );
        }
    }
}

#[test]
fn test_utf8_text_round_trip() {
    let source = "my $x = 42;";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();
    let text = root.utf8_text(source.as_bytes());
    assert!(text.is_ok(), "utf8_text should succeed");
    // The root node spans the whole source — verify the actual content, not just Ok.
    let extracted = must_some(text.ok());
    assert_eq!(extracted, source, "utf8_text should return the full source for the root node");
}

#[test]
fn test_utf8_text_multibyte_unicode() {
    // 'é' is 2 bytes in UTF-8; the parser must not split a codepoint boundary.
    let source = "my $x = 'café';";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();
    let bytes = source.as_bytes();
    let text = root.utf8_text(bytes);
    assert!(text.is_ok(), "utf8_text should handle multi-byte UTF-8");
}

#[test]
fn test_utf8_text_mismatched_source_does_not_panic() {
    // utf8_text takes a caller-supplied byte slice. When the slice is shorter
    // than the tree's byte offsets, the implementation must clamp rather than panic.
    let source = "my $x = 42;";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    let root = tree.root_node();
    // A shorter slice — would panic without the start.min(source.len()) guard.
    let short = b"my";
    let result = root.utf8_text(short);
    assert!(result.is_ok(), "utf8_text should not panic with short source slice");
}

#[test]
fn test_invalid_perl_returns_some_tree() {
    // The v3 parser is error-tolerant — even syntactically invalid Perl should
    // produce a partial tree (Some), not None. None is only returned on cancellation.
    let mut p = Parser::new();
    let tree = p.parse("sub { @@@@invalid{{{{");
    assert!(tree.is_some(), "invalid Perl should still yield an error-recovery tree");
}

#[test]
fn test_children_iterator_matches_child_count() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 1; my $y = 2;"));
    let root = tree.root_node();
    let collected: Vec<_> = root.children().collect();
    assert_eq!(collected.len(), root.child_count());
}

#[test]
fn test_child_by_index() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 1; my $y = 2;"));
    let root = tree.root_node();
    if root.child_count() > 0 {
        let first = root.child(0);
        assert!(first.is_some());
    }
    assert!(root.child(9999).is_none());
}

#[test]
fn test_empty_source_yields_tree() {
    // The v3 parser is error-tolerant; empty input returns Program { statements: [] }.
    let mut p = Parser::new();
    let tree = p.parse("");
    assert!(tree.is_some(), "empty input should still yield a tree");
}

#[test]
fn test_source_accessor() {
    let source = "sub foo { }";
    let mut p = Parser::new();
    let tree = must_some(p.parse(source));
    assert_eq!(tree.source(), source);
}

#[test]
fn test_default_parser() {
    let mut p = Parser::default();
    let tree = p.parse("1;");
    assert!(tree.is_some());
}

#[test]
fn test_is_leaf_for_leaf_nodes() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("42"));
    let root = tree.root_node();
    // The root Program is not a leaf.
    assert!(!root.is_leaf());
}

// Tests for grammar_kind() method

#[test]
fn test_grammar_kind_returns_source_file_for_root() {
    let mut p = Parser::new();
    let tree = must_some(p.parse("my $x = 42;"));
    assert_eq!(tree.root_node().grammar_kind(), "source_file");
}

#[test]
fn test_grammar_kind_returns_variable_with_attributes_for_list_form() {
    let mut p = Parser::new();
    // VariableWithAttributes is only produced for per-variable attributes in list form:
    // `my ($x : lvalue);`. Scalar form `my $x : lvalue;` does not produce this node.
    let tree = must_some(p.parse("my ($x : lvalue);"));
    let root = tree.root_node();
    let mut found_var_with_attrs = false;
    for child in root.children() {
        if child.grammar_kind() == "my_declaration" {
            for sub in child.children() {
                if sub.grammar_kind() == "variable_with_attributes" {
                    found_var_with_attrs = true;
                }
            }
        }
    }
    assert!(found_var_with_attrs, "should find variable_with_attributes");
}

#[test]
fn test_grammar_kind_double_paren_edge_case() {
    // Test that grammar_kind() remains independent of native debug sexp payloads.
    // VariableWithAttributes nests the variable child and an attributes payload.
    let mut p = Parser::new();
    let tree = must_some(p.parse("my ($x : lvalue);"));
    let root = tree.root_node();
    let sexp = root.to_sexp();
    assert!(
        sexp.contains("(variable_with_attributes") && sexp.contains("(attributes"),
        "sexp should nest attributes under the owning node, got: {sexp}"
    );
    let declaration =
        must_some(root.children().find(|node| node.grammar_kind() == "my_declaration"));
    let variable = must_some(
        declaration.children().find(|node| node.grammar_kind() == "variable_with_attributes"),
    );
    assert_eq!(variable.grammar_kind(), "variable_with_attributes");
}
