mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

fn contains_string_repetition(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x")
        || node.children().into_iter().any(contains_string_repetition)
}

fn assert_string_repetition(source: &str) {
    assert_clean_parse(source);
    let ast = parse(source);
    assert!(
        contains_string_repetition(&ast),
        "expected a binary string-repetition node for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
}

#[test]
fn repetition_accepts_undef_rhs() {
    assert_string_repetition(r#"my $value = "x" x undef;"#);
}

#[test]
fn repetition_accepts_do_block_rhs() {
    assert_string_repetition(r#"my $value = "x" x do { 3 };"#);
}

#[test]
fn repetition_accepts_anonymous_hash_rhs() {
    assert_string_repetition(r#"my $value = "x" x { count => 3 };"#);
}

#[test]
fn repetition_accepts_anonymous_sub_rhs() {
    assert_string_repetition(r#"my $value = "x" x sub { 3 };"#);
}

#[test]
fn x_before_fat_arrow_remains_a_bare_call_key() {
    let source = r#"sub configure; configure x => 3;"#;
    assert_clean_parse(source);
    let ast = parse(source);
    assert!(
        !contains_string_repetition(&ast),
        "x before a fat arrow must remain a key, not repetition:\n{}",
        ast.to_sexp(),
    );
}
