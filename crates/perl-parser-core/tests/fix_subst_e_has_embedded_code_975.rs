/// Tests for fix #975: s///e and s///ee must set has_embedded_code = true.
///
/// The `e` modifier causes the replacement to be evaluated as Perl code
/// (equivalent to `eval`), so `has_embedded_code` must be true whenever
/// the modifier string contains 'e'.
mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

/// Recursively search the AST for the first `Substitution` node and return
/// its `has_embedded_code` flag.  Panics if no Substitution node is found.
fn find_subst_has_embedded_code(node: &Node) -> bool {
    if let NodeKind::Substitution { has_embedded_code, .. } = &node.kind {
        return *has_embedded_code;
    }
    for child in node.children() {
        // `children()` only returns direct children; use recursion for depth.
        if let Some(v) = find_subst_has_embedded_code_opt(child) {
            return v;
        }
    }
    panic!("No Substitution node found in AST");
}

fn find_subst_has_embedded_code_opt(node: &Node) -> Option<bool> {
    if let NodeKind::Substitution { has_embedded_code, .. } = &node.kind {
        return Some(*has_embedded_code);
    }
    for child in node.children() {
        if let Some(v) = find_subst_has_embedded_code_opt(child) {
            return Some(v);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Test 1: single `e` modifier via primary.rs path (=~ binding)
// ---------------------------------------------------------------------------
#[test]
fn subst_e_modifier_sets_has_embedded_code() {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source);
    assert!(
        find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=true for s///e, got false"
    );
}

// ---------------------------------------------------------------------------
// Test 2: double-eval `ee` modifier
// ---------------------------------------------------------------------------
#[test]
fn subst_ee_modifier_sets_has_embedded_code() {
    let source = r#"$s =~ s/\$(\w+)/$$1/ee;"#;
    let ast = parse(source);
    assert!(
        find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=true for s///ee, got false"
    );
}

// ---------------------------------------------------------------------------
// Test 3: combined `ge` modifiers
// ---------------------------------------------------------------------------
#[test]
fn subst_ge_modifier_sets_has_embedded_code() {
    let source = r#"$s =~ s/(\w+)/lc($1)/ge;"#;
    let ast = parse(source);
    assert!(
        find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=true for s///ge, got false"
    );
}

// ---------------------------------------------------------------------------
// Test 4: regression guard — no `e` modifier must NOT set has_embedded_code
// ---------------------------------------------------------------------------
#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() {
    let source = r#"$s =~ s/foo/bar/gr;"#;
    let ast = parse(source);
    assert!(
        !find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=false for s///gr, got true"
    );
}

// ---------------------------------------------------------------------------
// Test 5: (?{...}) in pattern AND /e modifier — OR stays true
// ---------------------------------------------------------------------------
#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() {
    // Pattern contains (?{...}), AND modifier is /e — both conditions true.
    let source = r#"$s =~ s/(?{1})/$x/e;"#;
    let ast = parse(source);
    assert!(
        find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=true when both (?{{...}}) and /e present, got false"
    );
}

// ---------------------------------------------------------------------------
// Test 6: quote-operator form s{}{}e (no =~) — exercises the quotes.rs path
// ---------------------------------------------------------------------------
#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() {
    let source = r#"s{(\w+)}{uc($1)}e;"#;
    let ast = parse(source);
    assert!(
        find_subst_has_embedded_code(&ast),
        "Expected has_embedded_code=true for s{{}}{{}}e (quotes.rs path), got false"
    );
}

// ---------------------------------------------------------------------------
// Test 7: S-expression contains (risk:code) marker for s///e
// ---------------------------------------------------------------------------
#[test]
fn subst_e_sexp_contains_risk_code_marker() {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("risk:code"),
        "Expected sexp to contain '(risk:code)' for s///e, got:\n{}",
        sexp
    );
}
