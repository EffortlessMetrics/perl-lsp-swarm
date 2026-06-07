mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::NodeKind;

// Regression tests for issue #975:
// s///e (and s///ee) substitution must set has_embedded_code=true because
// the `e` modifier evaluates the replacement as Perl code (equivalent to eval).

fn find_substitution_has_embedded_code(node: &perl_parser_core::Node) -> Option<bool> {
    if let NodeKind::Substitution { has_embedded_code, .. } = &node.kind {
        return Some(*has_embedded_code);
    }
    for child in node.children() {
        if let Some(v) = find_substitution_has_embedded_code(child) {
            return Some(v);
        }
    }
    None
}

#[test]
fn subst_e_modifier_sets_has_embedded_code() {
    // primary.rs path: s/// parsed via =~ binding
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(
        has_embedded,
        Some(true),
        "s///e must set has_embedded_code=true (the replacement is evaled)"
    );
}

#[test]
fn subst_ee_modifier_sets_has_embedded_code() {
    // Double-eval form: s///ee evaluates the replacement twice
    let source = r#"$t =~ s/\$(\w+)/$$1/ee;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(has_embedded, Some(true), "s///ee must set has_embedded_code=true (double eval)");
}

#[test]
fn subst_ge_modifier_sets_has_embedded_code() {
    // Combined ge modifiers: g (global) + e (eval replacement)
    let source = r#"$x =~ s/(\w+)/lc($1)/ge;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(has_embedded, Some(true), "s///ge must set has_embedded_code=true");
}

#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() {
    // Regression guard: s///gr without e must keep has_embedded_code=false
    let source = r#"$y =~ s/a/b/gr;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(
        has_embedded,
        Some(false),
        "s///gr (no e modifier) must keep has_embedded_code=false"
    );
}

#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() {
    // Both (?{...}) in pattern AND /e modifier: OR keeps has_embedded_code=true
    let source = r#"$z =~ s/(?{1+1})/something/e;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(
        has_embedded,
        Some(true),
        "s///e with (?{{...}}) in pattern must stay has_embedded_code=true"
    );
}

#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() {
    // quotes.rs path: bracket-delimited s{}{} form without =~ binding
    // This is the critical second-site proof for the quotes.rs fix.
    let source = r#"s{(\w+)}{uc($1)}e;"#;
    let ast = parse(source);
    let has_embedded = find_substitution_has_embedded_code(&ast);
    assert_eq!(
        has_embedded,
        Some(true),
        "s{{}}{{}}e (quote operator form) must set has_embedded_code=true via quotes.rs path"
    );
}

#[test]
fn subst_e_sexp_contains_risk_code_marker() {
    // The (risk:code) marker must appear in the sexp for s///e
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("risk:code"),
        "sexp for s///e must contain '(risk:code)' marker, got:\n{}",
        sexp
    );
}
