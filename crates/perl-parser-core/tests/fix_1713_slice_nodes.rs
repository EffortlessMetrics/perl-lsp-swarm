//! Issue #1713: Hash/array/key-value slice expressions must produce dedicated
//! `NodeKind` variants (`ArraySlice`, `HashSlice`, `KeyValueSlice`) rather than
//! the generic `Binary { op: "{}" }` / `Binary { op: "[]" }` nodes that lose
//! sigil information.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::Node;

fn collect_kinds(node: &Node, out: &mut Vec<&'static str>) {
    out.push(node.kind.kind_name());
    for child in node.children() {
        collect_kinds(child, out);
    }
}

fn kinds(source: &str) -> Vec<&'static str> {
    let ast = parse(source);
    let mut out = Vec::new();
    collect_kinds(&ast, &mut out);
    out
}

// ── ArraySlice ───────────────────────────────────────────────────────────────

#[test]
fn array_slice_single_index_produces_array_slice_node() {
    let source = r#"my @s = @arr[0];"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"ArraySlice"), "expected ArraySlice NodeKind for @arr[0], got: {ks:?}");
}

#[test]
fn array_slice_multiple_indices_produces_array_slice_node() {
    let source = r#"my @s = @arr[1, 3, 5];"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"ArraySlice"),
        "expected ArraySlice NodeKind for @arr[1,3,5], got: {ks:?}"
    );
}

#[test]
fn array_slice_with_qw_produces_array_slice_node() {
    let source = r#"my @s = @arr[0 .. $#arr];"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"ArraySlice"),
        "expected ArraySlice NodeKind for @arr[0..$#arr], got: {ks:?}"
    );
}

// ── HashSlice ────────────────────────────────────────────────────────────────

#[test]
fn hash_slice_single_key_produces_hash_slice_node() {
    let source = r#"my @vals = @hash{qw(a)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "expected HashSlice NodeKind for @hash{{qw(a)}}, got: {ks:?}"
    );
}

#[test]
fn hash_slice_multiple_keys_produces_hash_slice_node() {
    let source = r#"my @vals = @hash{qw(a b c)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "expected HashSlice NodeKind for @hash{{qw(a b c)}}, got: {ks:?}"
    );
}

#[test]
fn hash_slice_with_variable_keys_produces_hash_slice_node() {
    let source = r#"my @vals = @hash{@keys};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "expected HashSlice NodeKind for @hash{{@keys}}, got: {ks:?}"
    );
}

// ── KeyValueSlice ─────────────────────────────────────────────────────────────

#[test]
fn key_value_slice_produces_key_value_slice_node() {
    let source = r#"my %sub = %hash{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "expected KeyValueSlice NodeKind for %hash{{qw(a b)}}, got: {ks:?}"
    );
}

#[test]
fn key_value_slice_with_at_sign_var_keys_produces_key_value_slice_node() {
    let source = r#"my %sub = %hash{@keys};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "expected KeyValueSlice NodeKind for %hash{{@keys}}, got: {ks:?}"
    );
}

// ── Regression: scalar element access must stay as Binary ────────────────────

#[test]
fn scalar_hash_element_still_uses_binary_node() {
    let source = r#"my $v = $hash{key};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        !ks.contains(&"HashSlice") && !ks.contains(&"KeyValueSlice"),
        "scalar $hash{{key}} must NOT produce HashSlice/KeyValueSlice, got: {ks:?}"
    );
    assert!(ks.contains(&"Binary"), "scalar $hash{{key}} must produce Binary node, got: {ks:?}");
}

#[test]
fn scalar_array_element_still_uses_binary_node() {
    let source = r#"my $v = $arr[0];"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(!ks.contains(&"ArraySlice"), "scalar $arr[0] must NOT produce ArraySlice, got: {ks:?}");
    assert!(ks.contains(&"Binary"), "scalar $arr[0] must produce Binary node, got: {ks:?}");
}

// ── Arrow subscript regression: ->[] and ->{} stay as Binary ─────────────────

#[test]
fn arrow_hash_subscript_still_uses_binary_node() {
    let source = r#"my $v = $ref->{key};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(!ks.contains(&"HashSlice"), "$ref->{{key}} must NOT produce HashSlice, got: {ks:?}");
    assert!(ks.contains(&"Binary"), "$ref->{{key}} must remain Binary, got: {ks:?}");
}

#[test]
fn arrow_array_subscript_still_uses_binary_node() {
    let source = r#"my $v = $ref->[0];"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(!ks.contains(&"ArraySlice"), "$ref->[0] must NOT produce ArraySlice, got: {ks:?}");
    assert!(ks.contains(&"Binary"), "$ref->[0] must remain Binary, got: {ks:?}");
}

// ── Real-world idioms ─────────────────────────────────────────────────────────

#[test]
fn hash_slice_in_assignment_context() {
    let source = r#"@hash{qw(a b c)} = (1, 2, 3);"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "lvalue @hash{{...}} must produce HashSlice, got: {ks:?}");
}

#[test]
fn array_slice_in_complex_expression() {
    let source = r#"
        my @arr = (10, 20, 30, 40, 50);
        my @odds = @arr[1, 3];
        push @result, @arr[0, 2, 4];
    "#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"ArraySlice"), "expected ArraySlice in complex expression, got: {ks:?}");
}

#[test]
fn hash_slice_with_ops_seen_cpan_pattern() {
    // Exercises the CPAN pattern from the issue spec: `@ops_seen{ map split(/ /), values %ops }`
    let source = r#"
        my %ops = (foo => "a b", bar => "c");
        my %ops_seen;
        @ops_seen{ map { split(/ /, $_) } values %ops } = ();
    "#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "CPAN @ops_seen{{...}} pattern must produce HashSlice, got: {ks:?}"
    );
}
