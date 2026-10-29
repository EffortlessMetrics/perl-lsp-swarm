//! Regex-family Program bodies use the canonical compiler entry (#5813/#7136).
//!
//! Keep the original divergence specimens as positive convergence controls. The
//! complete body must match `lower_ast`, and independent family/modifier checks
//! prevent two equally opaque results from satisfying the test. These are analysis
//! representations, not proof of regex execution or complete regex value semantics.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirBody, HirExpr, HirStmt, lower_ast, lower_body};

type TestResult = Result<(), Box<dyn Error>>;

fn lowered(source: &str) -> Result<HirBody, Box<dyn Error>> {
    let output = Parser::new(source).parse_with_recovery();
    let body = lower_body(&output.ast);
    let file = lower_ast(&output.ast);
    assert_eq!(Some(&body), file.root_body(), "canonical body mismatch for {source:?}");
    Ok(body)
}

fn let_init_rhs(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or("missing root block")?;
    let stmt = root.stmts.first().ok_or("missing declaration")?;
    let Some(HirStmt::Let { init: Some(init), .. }) = body.stmt(*stmt) else {
        return Err("missing declaration initializer".into());
    };
    let Some(HirExpr::Assign { rhs, .. }) = body.expr(*init) else {
        return Err("missing assignment initializer".into());
    };
    body.expr(*rhs).ok_or_else(|| "missing assignment RHS".into())
}

fn family(expr: &HirExpr) -> Option<&'static str> {
    match expr {
        HirExpr::Regex(_) => Some("regex"),
        HirExpr::Match(_) => Some("match"),
        HirExpr::Substitution(_) => Some("substitution"),
        HirExpr::Transliteration(_) => Some("transliteration"),
        _ => None,
    }
}

#[test]
fn unbound_regex_initializer_retains_its_typed_form_and_modifiers() -> TestResult {
    let body = lowered("my $r = qr/foo/i;")?;
    assert!(matches!(let_init_rhs(&body)?, HirExpr::Regex(regex) if regex.modifiers == "i"));
    Ok(())
}

#[test]
fn bound_regex_families_survive_the_statement_wrapper() -> TestResult {
    for (source, expected) in [
        ("$x =~ /foo/;", "match"),
        ("$x =~ s/a/b/g;", "substitution"),
        ("$x =~ tr/a-z/A-Z/;", "transliteration"),
    ] {
        let body = lowered(source)?;
        let root = body.block(body.root_block).ok_or("missing root block")?;
        let stmt = root.stmts.first().ok_or("missing statement")?;
        let Some(HirStmt::Expr(expr)) = body.stmt(*stmt) else {
            return Err(format!("expected expression statement for {source:?}").into());
        };
        let expr = body.expr(*expr).ok_or("missing expression")?;
        assert_eq!(family(expr), Some(expected), "wrong bound family for {source:?}");
    }
    Ok(())
}

#[test]
fn every_original_regex_specimen_retains_its_expected_family() -> TestResult {
    for (source, expected) in [
        ("my $r = qr/foo/i;", "regex"),
        ("$x =~ /foo/;", "match"),
        ("$x !~ /foo/;", "match"),
        ("$x =~ s/a/b/g;", "substitution"),
        ("$x =~ tr/a-z/A-Z/;", "transliteration"),
        ("s/a/b/;", "substitution"),
    ] {
        let body = lowered(source)?;
        assert!(
            body.exprs.iter().any(|expr| family(expr) == Some(expected)),
            "expected family {expected} missing for {source:?}"
        );
    }
    Ok(())
}
