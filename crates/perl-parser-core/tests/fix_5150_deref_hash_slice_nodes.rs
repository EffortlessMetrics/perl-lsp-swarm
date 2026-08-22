//! Issue #5150: Deref-based hash slices (`@$href{keys}`, `@{$href}{keys}`,
//! `%$href{keys}`, `%{$href}{keys}`) must produce `HashSlice` / `KeyValueSlice`
//! nodes instead of the generic `Binary { op: "{}" }` node.
//!
//! The fix mirrors the array-slice branch (`@$aref[...]` → `ArraySlice`) that
//! already recognises `Unary { op: "@{}" }` deref targets in the `[...]` arm.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind};

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

// ── @$href{...} → HashSlice ──────────────────────────────────────────────────

#[test]
fn deref_hash_slice_unbraced_at_sigil_single_key() {
    let source = r#"my @v = @$href{qw(a)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "@$href{{qw(a)}} must produce HashSlice; got: {ks:?}");
    assert!(
        !ks.iter().any(|&k| k == "Binary" && ks.contains(&"HashSlice")),
        "HashSlice must not coexist with a Binary wrapper: {ks:?}"
    );
}

#[test]
fn deref_hash_slice_unbraced_at_sigil_multiple_keys() {
    let source = r#"my @v = @$href{qw(a b c)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "@$href{{qw(a b c)}} must produce HashSlice; got: {ks:?}");
}

#[test]
fn deref_hash_slice_unbraced_at_sigil_variable_keys() {
    let source = r#"my @v = @$href{@keys};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "@$href{{@keys}} must produce HashSlice; got: {ks:?}");
}

// ── @{$href}{...} → HashSlice ────────────────────────────────────────────────

#[test]
fn deref_hash_slice_braced_at_sigil_single_key() {
    let source = r#"my @v = @{$href}{qw(a)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "@{{$href}}{{qw(a)}} must produce HashSlice; got: {ks:?}");
}

#[test]
fn deref_hash_slice_braced_at_sigil_multiple_keys() {
    let source = r#"my @v = @{$href}{qw(x y)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "@{{$href}}{{qw(x y)}} must produce HashSlice; got: {ks:?}");
}

// ── %$href{...} → KeyValueSlice ──────────────────────────────────────────────

#[test]
fn deref_key_value_slice_unbraced_pct_sigil_single_key() {
    let source = r#"my %kv = %$href{qw(a)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "%$href{{qw(a)}} must produce KeyValueSlice; got: {ks:?}"
    );
}

#[test]
fn deref_key_value_slice_unbraced_pct_sigil_multiple_keys() {
    let source = r#"my %kv = %$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "%$href{{qw(a b)}} must produce KeyValueSlice; got: {ks:?}"
    );
}

// ── %{$href}{...} → KeyValueSlice ────────────────────────────────────────────

#[test]
fn deref_key_value_slice_braced_pct_sigil_multiple_keys() {
    let source = r#"my %kv = %{$href}{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "%{{$href}}{{qw(a b)}} must produce KeyValueSlice; got: {ks:?}"
    );
}

// ── Regression: non-slice deref forms must not be affected ───────────────────

#[test]
fn scalar_deref_element_stays_binary() {
    // $$href{key} — scalar element, NOT a slice
    let source = r#"my $v = $$href{key};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        !ks.contains(&"HashSlice") && !ks.contains(&"KeyValueSlice"),
        "$$href{{key}} must NOT produce HashSlice/KeyValueSlice; got: {ks:?}"
    );
    assert!(ks.contains(&"Binary"), "$$href{{key}} must remain Binary; got: {ks:?}");
}

#[test]
fn arrow_deref_hash_element_stays_binary() {
    // $href->{key} — arrow dereference, not a slice
    let source = r#"my $v = $href->{key};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        !ks.contains(&"HashSlice") && !ks.contains(&"KeyValueSlice"),
        "$href->{{key}} must NOT produce HashSlice/KeyValueSlice; got: {ks:?}"
    );
}

#[test]
fn bare_array_plain_variable_hash_slice_still_works() {
    // @hash{qw(a b)} — plain variable hash slice (existing behavior must not regress)
    let source = r#"my @v = @hash{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "@hash{{qw(a b)}} must still produce HashSlice; got: {ks:?}"
    );
}

#[test]
fn bare_pct_plain_variable_key_value_slice_still_works() {
    // %hash{qw(a b)} — plain variable key-value slice (existing behavior must not regress)
    let source = r#"my %kv = %hash{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "%hash{{qw(a b)}} must still produce KeyValueSlice; got: {ks:?}"
    );
}

// ── Real-world patterns ───────────────────────────────────────────────────────

#[test]
fn real_world_deref_hash_slice_self_fields() {
    // Common OO pattern: @{$self}{@fields}
    let source = r#"
        sub new {
            my ($class, %args) = @_;
            my $self = {};
            @{$self}{@valid_fields} = @args{@valid_fields};
            bless $self, $class;
        }
    "#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "OO field init @{{$self}}{{...}} must produce HashSlice; got: {ks:?}"
    );
}

#[test]
fn real_world_deref_hash_slice_in_assignment_lvalue() {
    // Lvalue deref hash slice: @$href{qw(a b c)} = (1, 2, 3)
    let source = r#"@$href{qw(a b c)} = (1, 2, 3);"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(ks.contains(&"HashSlice"), "lvalue @$href{{...}} must produce HashSlice; got: {ks:?}");
}

#[test]
fn real_world_deref_key_value_slice_in_list_assignment_rvalue() {
    // Deref key/value slice on the *right* of a list assignment:
    //   my %pairs = %$href{qw(a b)};
    //
    // Salvaged from #5183, but corrected: that draft used the `%` slice as an
    // assignment *target*, which is not valid Perl. Verified against
    // perl 5.38.2:
    //
    //   $ perl -e 'my $href={}; %$href{qw(a b)} = (1,2);'
    //   Can't modify key/value hash slice in list assignment
    //
    // Unlike an `@` hash slice, a `%` key/value slice cannot be an lvalue, so
    // the rvalue form below is the real user scenario. (`@$href{...} = (...)`
    // is a valid lvalue and is covered by the test above.)
    let source = r#"my %pairs = %$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"KeyValueSlice"),
        "rvalue %$href{{...}} must produce KeyValueSlice; got: {ks:?}"
    );
}

#[test]
fn deref_hash_slice_with_complex_subscript_ref() {
    // @{$obj->{data}}{@keys} — nested deref before hash slice
    let source = r#"my @vals = @{$obj->{data}}{@keys};"#;
    assert_clean_parse(source);
    let ks = kinds(source);
    assert!(
        ks.contains(&"HashSlice"),
        "@{{$obj->{{data}}}}{{@keys}} must produce HashSlice; got: {ks:?}"
    );
}

// ── Node structure assertions ─────────────────────────────────────────────────

#[test]
fn at_deref_hash_slice_node_structure() -> Result<(), String> {
    // @$href{qw(a b)} must be HashSlice { target: Unary{"@{}"}, keys: ... }
    let source = r#"@$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ast = parse(source);

    // drill to the ExpressionStatement's expression
    let ast_sexp = ast.to_sexp();
    let stmt = match ast.into_parts() {
        (NodeKind::Program { mut statements }, _) if !statements.is_empty() => {
            statements.swap_remove(0)
        }
        _ => return Err(format!("Expected Program, got: {ast_sexp}")),
    };
    let expr = match stmt.into_parts().0 {
        NodeKind::ExpressionStatement { expression } => *expression,
        other => return Err(format!("Expected ExpressionStatement, got: {}", other.kind_name())),
    };

    match &expr.kind {
        NodeKind::HashSlice { target, .. } => match &target.kind {
            NodeKind::Unary { op, .. } => {
                if op != "@{}" {
                    return Err(format!("Expected @{{}} deref op on HashSlice target; got: {op}"));
                }
            }
            _ => {
                return Err(format!(
                    "HashSlice target should be Unary(@{{}}); got: {} (sexp: {})",
                    target.kind.kind_name(),
                    expr.to_sexp()
                ));
            }
        },
        _ => {
            return Err(format!(
                "Expected HashSlice; got: {} (sexp: {})",
                expr.kind.kind_name(),
                expr.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn pct_deref_key_value_slice_node_structure() -> Result<(), String> {
    // %$href{qw(a b)} must be KeyValueSlice { target: Unary{"%{}"}, keys: ... }
    let source = r#"%$href{qw(a b)};"#;
    assert_clean_parse(source);
    let ast = parse(source);

    let ast_sexp = ast.to_sexp();
    let stmt = match ast.into_parts() {
        (NodeKind::Program { mut statements }, _) if !statements.is_empty() => {
            statements.swap_remove(0)
        }
        _ => return Err(format!("Expected Program, got: {ast_sexp}")),
    };
    let expr = match stmt.into_parts().0 {
        NodeKind::ExpressionStatement { expression } => *expression,
        other => return Err(format!("Expected ExpressionStatement, got: {}", other.kind_name())),
    };

    match &expr.kind {
        NodeKind::KeyValueSlice { target, .. } => match &target.kind {
            NodeKind::Unary { op, .. } => {
                if op != "%{}" {
                    return Err(format!(
                        "Expected %{{}} deref op on KeyValueSlice target; got: {op}"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "KeyValueSlice target should be Unary(%{{}}); got: {} (sexp: {})",
                    target.kind.kind_name(),
                    expr.to_sexp()
                ));
            }
        },
        _ => {
            return Err(format!(
                "Expected KeyValueSlice; got: {} (sexp: {})",
                expr.kind.kind_name(),
                expr.to_sexp()
            ));
        }
    }
    Ok(())
}
