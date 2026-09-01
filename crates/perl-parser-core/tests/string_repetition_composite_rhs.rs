mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

fn collect_string_repetitions<'a>(node: &'a Node, repetitions: &mut Vec<&'a Node>) {
    if matches!(&node.kind, NodeKind::Binary { op, .. } if op == "x") {
        repetitions.push(node);
    }
    for child in node.children() {
        collect_string_repetitions(child, repetitions);
    }
}

fn assert_string_repetition(source: &str) {
    assert_clean_parse(source);
    let ast = parse(source);
    let mut repetitions = Vec::new();
    collect_string_repetitions(&ast, &mut repetitions);
    assert_eq!(
        repetitions.len(),
        1,
        "expected exactly one binary string-repetition node for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );

    let Some(repetition) = repetitions.first() else {
        return;
    };
    let NodeKind::Binary { left, right, .. } = &repetition.kind else {
        return;
    };
    assert!(
        matches!(&left.kind, NodeKind::String { .. }),
        "the repetition LHS must be a string literal for source:\n{source}\n\nsexp:\n{}",
        ast.to_sexp(),
    );
    assert!(
        !matches!(&right.kind, NodeKind::Binary { op, .. } if op == "x"),
        "the composite RHS must not be parsed as another repetition for source:\n{source}\n\nsexp:\n{}",
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
    let mut repetitions = Vec::new();
    collect_string_repetitions(&ast, &mut repetitions);
    assert!(
        repetitions.is_empty(),
        "x before a fat arrow must remain a key, not repetition:\n{}",
        ast.to_sexp(),
    );
}

#[test]
fn adjacent_and_chained_x_operators_keep_their_own_shape() {
    for (source, expected_repetitions) in [
        (r#"my $value = "x" x 2 x 3;"#, 2),
        (r#"my $value = "x" x foo(2);"#, 1),
        (r#"my $value = "x" x 2 + 3;"#, 1),
    ] {
        assert_clean_parse(source);
        let ast = parse(source);
        let mut repetitions = Vec::new();
        collect_string_repetitions(&ast, &mut repetitions);
        assert_eq!(
            repetitions.len(),
            expected_repetitions,
            "unexpected repetition-node count for source:\n{source}\n\nsexp:\n{}",
            ast.to_sexp(),
        );
    }
}
