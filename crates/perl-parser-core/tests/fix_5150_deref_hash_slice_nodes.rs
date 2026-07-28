//! Issue #5150: Deref-based hash slices (@$href{keys}, @{$href}{keys},
//! %$href{keys}, %{$href}{keys}) must produce HashSlice / KeyValueSlice AST
//! nodes, not the generic Binary { op: "{}" } that loses sigil information.
//!
//! Before this fix the hash-slice early-exit in `parse_postfix_chain` only
//! matched `NodeKind::Variable { sigil: "@"|"%" }`, so deref targets fell
//! through and were emitted as Binary.  The fix extends the guard to also
//! match `NodeKind::Unary { op: "@{}"|"%{}" }`, mirroring the existing
//! array-deref-slice detection at the `[` arm.

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

// ── @$href{keys} — unbraced deref hash slice ─────────────────────────────────

#[test]
fn unbraced_deref_at_sigil_hash_slice_produces_hash_slice_node() {
    let source = r#"my @vals = @$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "expected HashSlice for @$href{{qw(a b)}}, got: {ks:?}");
    assert!(
        !ks.iter().any(|k| *k == "Binary"),
        "@$href{{...}} must not fall through to Binary, got: {ks:?}"
    );
}

#[test]
fn unbraced_deref_pct_sigil_hash_slice_produces_key_value_slice_node() {
    let source = r#"my %sub = %$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "expected KeyValueSlice for %$href{{qw(a b)}}, got: {ks:?}"
    );
    assert!(
        !ks.iter().any(|k| *k == "Binary"),
        "%$href{{...}} must not fall through to Binary, got: {ks:?}"
    );
}

// ── @{$href}{keys} — braced deref hash slice ─────────────────────────────────

#[test]
fn braced_deref_at_sigil_hash_slice_produces_hash_slice_node() {
    let source = r#"my @vals = @{$href}{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "expected HashSlice for @{{$href}}{{qw(a b)}}, got: {ks:?}");
    assert!(
        !ks.iter().any(|k| *k == "Binary"),
        "@{{$href}}{{...}} must not fall through to Binary, got: {ks:?}"
    );
}

#[test]
fn braced_deref_pct_sigil_hash_slice_produces_key_value_slice_node() {
    let source = r#"my %sub = %{$href}{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "expected KeyValueSlice for %{{$href}}{{qw(a b)}}, got: {ks:?}"
    );
    assert!(
        !ks.iter().any(|k| *k == "Binary"),
        "%{{$href}}{{...}} must not fall through to Binary, got: {ks:?}"
    );
}

// ── Common idioms ─────────────────────────────────────────────────────────────

#[test]
fn deref_hash_slice_on_self_hash_produces_hash_slice() {
    // Common OO idiom: @{$self}{@fields}
    let source = r#"my @vals = @{$self}{@fields};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "expected HashSlice for @{{$self}}{{@fields}}, got: {ks:?}");
}

#[test]
fn deref_hash_slice_in_lvalue_context_produces_hash_slice() {
    // @$hash{@keys} = @values — lvalue hash slice via deref
    let source = r#"my ($hash, @keys, @values); @$hash{@keys} = @values;"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "lvalue @$hash{{@keys}} must produce HashSlice, got: {ks:?}"
    );
}

#[test]
fn deref_kv_slice_in_assignment_context() {
    // %$href{qw(a b)} = (1, 2) — key-value slice via deref
    let source = r#"my $href = {}; %$href{qw(a b)} = (1, 2);"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "lvalue %$href{{qw(a b)}} must produce KeyValueSlice, got: {ks:?}"
    );
}

// ── Regression: scalar element access still uses Binary ──────────────────────

#[test]
fn scalar_deref_element_still_uses_binary_not_hash_slice() {
    // $$href{key} — scalar element from dereferenced hash ref, must stay Binary
    let source = r#"my $v = $$href{key};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        !ks.contains(&"HashSlice") && !ks.contains(&"KeyValueSlice"),
        "$$href{{key}} must NOT produce HashSlice/KeyValueSlice, got: {ks:?}"
    );
}

#[test]
fn bare_hash_slice_unaffected_by_deref_fix() {
    // Bare @hash{...} must still produce HashSlice (no regression)
    let source = r#"my @vals = @hash{qw(a b c)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "bare @hash{{...}} must still produce HashSlice, got: {ks:?}"
    );
}

#[test]
fn bare_kv_slice_unaffected_by_deref_fix() {
    // Bare %hash{...} must still produce KeyValueSlice (no regression)
    let source = r#"my %sub = %hash{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "bare %hash{{...}} must still produce KeyValueSlice, got: {ks:?}"
    );
}
