//! Tests for issue #975: s///e and s///ee substitutions must set has_embedded_code=true.
//!
//! The `e` modifier evaluates the replacement as Perl code (equivalent to eval),
//! so has_embedded_code must be true even when the pattern contains no (?{...}).

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_ast::NodeKind;
use perl_parser_core::Node;
use perl_tdd_support::must_some;

fn find_substitution_has_embedded_code(node: &Node) -> Option<bool> {
    match &node.kind {
        NodeKind::Substitution { has_embedded_code, .. } => Some(*has_embedded_code),
        _ => {
            for child in node.children() {
                if let Some(v) = find_substitution_has_embedded_code(child) {
                    return Some(v);
                }
            }
            None
        }
    }
}

/// s///e (primary.rs path, =~ binding): single `e` modifier sets has_embedded_code.
#[test]
fn subst_e_modifier_sets_has_embedded_code() {
    let ast = parse(r#"$s =~ s/(\w+)/uc($1)/e;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(has_embedded, "s///e must set has_embedded_code=true");
}

/// s///ee (double-eval form): `ee` modifier also sets has_embedded_code.
#[test]
fn subst_ee_modifier_sets_has_embedded_code() {
    let ast = parse(r#"$t =~ s/\$(\w+)/$$1/ee;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(has_embedded, "s///ee must set has_embedded_code=true");
}

/// s///ge: combined modifiers including `e` set has_embedded_code.
#[test]
fn subst_ge_modifier_sets_has_embedded_code() {
    let ast = parse(r#"$s =~ s/(\w+)/lc($1)/ge;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(has_embedded, "s///ge must set has_embedded_code=true");
}

/// Regression guard: s///gr without `e` must NOT set has_embedded_code.
#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() {
    let ast = parse(r#"my $r = $s =~ s/foo/bar/gr;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(!has_embedded, "s///gr must NOT set has_embedded_code");
}

/// s///e where pattern also has (?{...}): both conditions true, OR stays true.
#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() {
    let ast = parse(r#"$s =~ s/(?{1+1})/uc($1)/e;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(has_embedded, "s///e with (?{{...}}) in pattern must stay has_embedded_code=true");
}

/// s{}{}e quote-operator form (quotes.rs path): critical second-site coverage.
/// This exercises the quotes.rs path independently of primary.rs.
#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() {
    let ast = parse(r#"$s =~ s{(\w+)}{uc($1)}e;"#);
    let has_embedded = must_some(find_substitution_has_embedded_code(&ast));
    assert!(has_embedded, "s{{}}{{}}e (quote-operator form) must set has_embedded_code=true");
}

/// S-expression must contain (risk:code) marker for s///e.
#[test]
fn subst_e_sexp_contains_risk_code_marker() {
    let ast = parse(r#"$s =~ s/(\w+)/uc($1)/e;"#);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("risk:code"),
        "s///e sexp must contain '(risk:code)' marker, got: {}",
        sexp
    );
}
