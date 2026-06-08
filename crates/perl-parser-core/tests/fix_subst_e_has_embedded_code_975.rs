//! Tests for issue #975: s///e (and s///ee) substitution doesn't set has_embedded_code.
//!
//! The `e` modifier causes the replacement to be evaluated as Perl code, which
//! is equivalent to embedded-code risk. This file verifies that `has_embedded_code`
//! is set to `true` whenever the `e` or `ee` modifier is present, and that the
//! S-expression output includes the `(risk:code)` marker accordingly.

use perl_parser_core::{Node, NodeKind, Parser};

fn parse(source: &str) -> Result<Node, perl_parser_core::ParseError> {
    let mut parser = Parser::new(source);
    parser.parse()
}

fn find_substitution(node: &Node) -> Option<&Node> {
    if matches!(node.kind, NodeKind::Substitution { .. }) {
        return Some(node);
    }
    for child in node.children() {
        if let Some(found) = find_substitution(child) {
            return Some(found);
        }
    }
    None
}

#[test]
fn subst_e_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            assert!(
                *has_embedded_code,
                "s///e must set has_embedded_code; modifiers={modifiers:?}"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_ee_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"s/(\w+)/uc($1)/ee;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            assert!(
                *has_embedded_code,
                "s///ee must set has_embedded_code; modifiers={modifiers:?}"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_ge_modifier_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"$s =~ s/(\w+)/lc($1)/ge;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            assert!(
                *has_embedded_code,
                "s///ge must set has_embedded_code; modifiers={modifiers:?}"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"$s =~ s/a/b/gr;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            assert!(
                !*has_embedded_code,
                "s///gr without e must not set has_embedded_code; modifiers={modifiers:?}"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"$s =~ s/(?{ $x++ })/replacement/e;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, .. } => {
            assert!(
                *has_embedded_code,
                "s///e with (?{{...}}) in pattern must set has_embedded_code"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"s{(\w+)}{uc($1)}e;"#;
    let ast = parse(source)?;
    let subst = find_substitution(&ast).ok_or("no Substitution node found")?;
    match &subst.kind {
        NodeKind::Substitution { has_embedded_code, modifiers, .. } => {
            assert!(
                *has_embedded_code,
                "s{{...}}{{...}}e (quote operator form) must set has_embedded_code; modifiers={modifiers:?}"
            );
        }
        other => return Err(format!("expected Substitution, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn subst_e_sexp_contains_risk_code_marker() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let ast = parse(source)?;
    let sexp = ast.to_sexp();
    assert!(sexp.contains("risk:code"), "s///e sexp must contain 'risk:code'; got: {sexp}");
    Ok(())
}
