//! Tests for issue #975: s///e (and s///ee) substitution must set has_embedded_code = true.
//!
//! The `e` modifier evaluates the replacement as Perl code (equivalent to eval),
//! so security/diagnostic consumers expect `has_embedded_code: true` for any
//! s///e or s///ee form, regardless of whether the pattern contains `(?{...})`.

mod cpan_test_helpers;
use cpan_test_helpers::*;

use perl_parser_core::{Node, NodeKind};

/// Walk the AST recursively and return the first Substitution node found.
fn find_substitution(node: &Node) -> Option<&Node> {
    if matches!(&node.kind, NodeKind::Substitution { .. }) {
        return Some(node);
    }
    let mut found = None;
    node.for_each_child(|child| {
        if found.is_none() {
            found = find_substitution(child);
        }
    });
    found
}

/// Assert that the first Substitution in the parsed source has has_embedded_code == expected.
fn assert_has_embedded_code(source: &str, expected: bool) -> Result<(), String> {
    let ast = parse(source);
    let subst = find_substitution(&ast)
        .ok_or_else(|| format!("no Substitution node found in: {source}"))?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            if *has_embedded_code != expected {
                return Err(format!(
                    "has_embedded_code mismatch for `{source}` (modifiers={modifiers:?}): \
                     expected {expected}, got {has_embedded_code}"
                ));
            }
            Ok(())
        }
        other => Err(format!("expected Substitution, got {other:?}")),
    }
}

/// s///e — single `e` modifier via =~ binding (primary.rs path).
/// The replacement `uc($1)` is eval'd as Perl, so has_embedded_code must be true.
#[test]
fn subst_e_modifier_sets_has_embedded_code() -> Result<(), String> {
    assert_has_embedded_code(r#"$s =~ s/(\w+)/uc($1)/e;"#, true)
}

/// s///ee — double eval form.
/// The replacement is eval'd twice; has_embedded_code must be true.
#[test]
fn subst_ee_modifier_sets_has_embedded_code() -> Result<(), String> {
    assert_has_embedded_code(r#"$t =~ s/\$(\w+)/$$1/ee;"#, true)
}

/// s///ge — combined global + eval modifiers.
/// has_embedded_code must be true even with additional modifiers present.
#[test]
fn subst_ge_modifier_sets_has_embedded_code() -> Result<(), String> {
    assert_has_embedded_code(r#"$str =~ s/(\w+)/lc($1)/ge;"#, true)
}

/// s///gr — no `e` modifier present.
/// has_embedded_code must stay false; this is the regression guard.
#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() -> Result<(), String> {
    assert_has_embedded_code(r#"$x =~ s/a/b/gr;"#, false)
}

/// s///e with `(?{...})` in the pattern.
/// Both the pattern (`(?{...})`) and the `e` modifier independently justify
/// has_embedded_code = true. The OR must stay true for this combined case.
#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() -> Result<(), String> {
    assert_has_embedded_code(r#"$x =~ s/(?{ 1+1 })/replacement/e;"#, true)
}

/// s{}{}e — quote-operator form (no =~ binding), exercises the quotes.rs path.
/// This is the critical second-site proof: the fix in quotes.rs must also fire.
#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() -> Result<(), String> {
    assert_has_embedded_code(r#"s{(\w+)}{uc($1)}e;"#, true)
}

/// The sexp for s///e must contain `(risk:code)`.
/// ast.rs emits this marker when has_embedded_code is true — verifies the
/// end-to-end signal that security consumers depend on.
#[test]
fn subst_e_sexp_contains_risk_code_marker() -> Result<(), String> {
    let ast = parse(r#"$s =~ s/(\w+)/uc($1)/e;"#);
    let sexp = ast.to_sexp();
    if !sexp.contains("risk:code") {
        return Err(format!("sexp for s///e must contain '(risk:code)' but got:\n{sexp}"));
    }
    Ok(())
}
