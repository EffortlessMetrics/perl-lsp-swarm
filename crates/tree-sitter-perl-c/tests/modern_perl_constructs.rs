//! Coverage for modern and higher-risk grammar constructs in the C tree-sitter snapshot.
//!
//! These tests intentionally assert stable node kinds instead of complete S-expression
//! snapshots so they catch missing constructs without making unrelated grammar-shape
//! changes expensive to review.

use std::error::Error;

use tree_sitter::{Node, Tree};
use tree_sitter_perl_c::parse_perl_code;

fn parse_without_errors(source: &str) -> Result<Tree, Box<dyn Error>> {
    let tree = parse_perl_code(source)?;
    assert_eq!(tree.root_node().kind(), "source_file");
    assert!(
        !tree.root_node().has_error(),
        "unexpected parse error: {}",
        tree.root_node().to_sexp()
    );
    Ok(tree)
}

fn has_node_kind(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }

    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| has_node_kind(child, kind))
}

fn assert_contains_node(tree: &Tree, kind: &str) {
    assert!(
        has_node_kind(tree.root_node(), kind),
        "expected parse tree to contain `{kind}` node; tree: {}",
        tree.root_node().to_sexp()
    );
}

#[test]
fn modern_perl_subroutine_signatures_cover_parameter_shapes() -> Result<(), Box<dyn Error>> {
    let source = r#"use feature 'signatures';
sub combine ($left, $right = 1, @rest) {
    return $left + $right + scalar @rest;
}
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "signature");
    assert_contains_node(&tree, "mandatory_parameter");
    assert_contains_node(&tree, "optional_parameter");
    assert_contains_node(&tree, "slurpy_parameter");
    Ok(())
}

#[test]
fn modern_perl_method_declarations_cover_attributes_and_signatures() -> Result<(), Box<dyn Error>> {
    let source = r#"use feature 'signatures';
method greet : lvalue ($name = 'world') {
    return $name;
}
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "method_declaration_statement");
    assert_contains_node(&tree, "attribute");
    assert_contains_node(&tree, "signature");
    assert_contains_node(&tree, "optional_parameter");
    Ok(())
}

#[test]
fn modern_perl_try_catch_finally_is_preserved_as_try_statement() -> Result<(), Box<dyn Error>> {
    let source = r#"use feature 'try';
try {
    risky();
} catch ($error) {
    warn $error;
} finally {
    cleanup();
}
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "try_statement");
    assert_contains_node(&tree, "function_call_expression");
    Ok(())
}

#[test]
fn modern_perl_given_when_default_statement_nodes_are_distinct() -> Result<(), Box<dyn Error>> {
    let source = r#"use feature 'switch';
given ($value) {
    when (1) { say 'one'; }
    default { say 'other'; }
}
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "given_statement");
    assert_contains_node(&tree, "when_statement");
    assert_contains_node(&tree, "default_statement");
    Ok(())
}

#[test]
fn modern_perl_map_grep_and_sort_blocks_keep_operator_nodes() -> Result<(), Box<dyn Error>> {
    let source = r#"my @positive = grep { $_ > 0 } @values;
my @doubled = map { $_ * 2 } @positive;
my @sorted = sort { $a <=> $b } @doubled;
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "map_grep_expression");
    assert_contains_node(&tree, "sort_expression");
    Ok(())
}

#[test]
fn modern_perl_anonymous_sub_and_method_expressions_are_distinct() -> Result<(), Box<dyn Error>> {
    let source = r#"my $callback = sub ($value) { return $value + 1; };
my $accessor = method ($self) { return $self->{name}; };
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "anonymous_subroutine_expression");
    assert_contains_node(&tree, "anonymous_method_expression");
    Ok(())
}

#[test]
fn modern_perl_substitution_and_transliteration_keep_quote_operator_nodes()
-> Result<(), Box<dyn Error>> {
    let source = r#"$text =~ s/foo/bar/g;
$text =~ tr/a-z/A-Z/;
"#;

    let tree = parse_without_errors(source)?;

    assert_contains_node(&tree, "substitution_regexp");
    assert_contains_node(&tree, "replacement");
    assert_contains_node(&tree, "transliteration_expression");
    assert_contains_node(&tree, "transliteration_content");
    Ok(())
}
