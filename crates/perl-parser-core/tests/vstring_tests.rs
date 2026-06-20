mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_use_vstring() {
    let source = r#"use v5.38.0;"#;
    assert_clean_parse(source);
}

#[test]
fn test_vstring_in_expression() {
    let source = r#"my $v = v1.2.3;"#;
    assert_clean_parse(source);
}

#[test]
fn test_vstring_comparison() {
    let source = r#"$^V ge v5.10.0"#;
    assert_clean_parse(source);
}

#[test]
fn test_vstring_semantic_type() {
    // Test that v-strings are parsed as NodeKind::VString, not NodeKind::String.
    // Uses sexp output to verify the distinct (vstring ...) node is emitted.
    let source = r#"my $vstr = v1.2.3;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(vstring \"v1.2.3\")"),
        "expected (vstring \"v1.2.3\") in sexp but got: {}",
        sexp
    );
    assert!(
        !sexp.contains("(string \"v1.2.3\")"),
        "v-string must NOT be emitted as (string ...) but got: {}",
        sexp
    );
}

#[test]
fn test_vstring_long_form_semantic_type() {
    // Multi-component v-string: v1.2.3.4.5
    let source = r#"my $v = v1.2.3.4.5;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(vstring \"v1.2.3.4.5\")"),
        "expected (vstring \"v1.2.3.4.5\") in sexp but got: {}",
        sexp
    );
}

#[test]
fn test_ordinary_float_is_not_vstring() {
    // Ordinary floats (single dot) must remain Number nodes, not VString
    let source = r#"my $f = 3.14;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("vstring"),
        "ordinary float 3.14 must not be classified as vstring, but got: {}",
        sexp
    );
}

#[test]
fn test_range_is_not_vstring() {
    // Ranges like 1..10 must not be misclassified as version strings
    let source = r#"my @r = 1..10;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("vstring"),
        "range 1..10 must not contain a vstring node, but got: {}",
        sexp
    );
}

#[test]
fn test_vstring_no_dot_single_component() {
    // Bare single-component v-string: v5 is chr(5) in Perl — must parse cleanly
    // and be classified as VString, not Identifier or Number.
    let source = r#"my $chr5 = v5;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("(vstring \"v5\")"),
        "single-component vstring v5 should produce (vstring \"v5\") but got: {}",
        sexp
    );
}
