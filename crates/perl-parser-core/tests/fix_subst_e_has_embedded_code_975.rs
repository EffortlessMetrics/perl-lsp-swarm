//! Tests for issue #975: s///e and s///ee must set has_embedded_code = true.
//!
//! The `e` modifier evaluates the replacement as Perl code (equivalent to eval),
//! so the Substitution AST node must carry has_embedded_code:true whenever `e`
//! or `ee` appears in the modifiers — even when the pattern contains no (?{...}).

use perl_parser_core::{NodeKind, Parser};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Walk the AST depth-first and return the first Substitution node found.
fn find_substitution(node: &perl_parser_core::Node) -> Option<(String, String, String, bool)> {
    if let NodeKind::Substitution { pattern, replacement, modifiers, has_embedded_code, .. } =
        &node.kind
    {
        return Some((pattern.clone(), replacement.clone(), modifiers.clone(), *has_embedded_code));
    }
    for child in node.children() {
        if let Some(found) = find_substitution(child) {
            return Some(found);
        }
    }
    None
}

// ── Site 1: primary.rs path (s/pat/repl/ via TokenKind::Substitution) ────────

#[test]
fn subst_e_modifier_sets_has_embedded_code() -> TestResult {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, modifiers, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert_eq!(modifiers, "e");
    assert!(has_embedded_code, "s///e must set has_embedded_code=true");
    Ok(())
}

#[test]
fn subst_ee_modifier_sets_has_embedded_code() -> TestResult {
    let source = r#"$t =~ s/\$(\w+)/$$1/ee;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, modifiers, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert_eq!(modifiers, "ee");
    assert!(has_embedded_code, "s///ee must set has_embedded_code=true");
    Ok(())
}

#[test]
fn subst_ge_modifier_sets_has_embedded_code() -> TestResult {
    let source = r#"$s =~ s/(\w+)/lc($1)/ge;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, modifiers, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert!(modifiers.contains('e'), "modifiers should contain 'e': got {modifiers:?}");
    assert!(has_embedded_code, "s///ge must set has_embedded_code=true");
    Ok(())
}

#[test]
fn subst_no_e_modifier_does_not_set_has_embedded_code() -> TestResult {
    // Regression guard: plain substitution without /e must stay false.
    let source = r#"$s =~ s/foo/bar/gr;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, _, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert!(!has_embedded_code, "s///gr must NOT set has_embedded_code");
    Ok(())
}

#[test]
fn subst_e_with_embedded_code_in_pattern_stays_true() -> TestResult {
    // Both (?{...}) in pattern AND /e modifier: OR must produce true.
    let source = r#"$s =~ s/(?{1+1})/uc($1)/e;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, _, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert!(has_embedded_code, "(?{{...}}) pattern + /e modifier must set has_embedded_code=true");
    Ok(())
}

// ── Site 2: quotes.rs path (s{pat}{repl} via parse_quote_operator) ───────────

#[test]
fn subst_quote_operator_form_e_sets_has_embedded_code() -> TestResult {
    // s{...}{...}e is parsed via the quote_operator path (quotes.rs), not primary.rs.
    // This is the critical second-site proof.
    let source = r#"s{(\w+)}{uc($1)}e;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let (_, _, modifiers, has_embedded_code) =
        find_substitution(&ast).ok_or("no Substitution node found")?;
    assert!(modifiers.contains('e'), "modifiers should contain 'e': got {modifiers:?}");
    assert!(has_embedded_code, "s{{...}}{{...}}e (quotes.rs path) must set has_embedded_code=true");
    Ok(())
}

// ── S-expression: (risk:code) marker ─────────────────────────────────────────

#[test]
fn subst_e_sexp_contains_risk_code_marker() -> TestResult {
    let source = r#"$s =~ s/(\w+)/uc($1)/e;"#;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("risk:code"),
        "s///e S-expression must contain '(risk:code)' marker; got:\n{sexp}"
    );
    Ok(())
}
