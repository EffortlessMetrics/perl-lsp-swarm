//! The `lower_body` / `lower_ast` boundary for regex-family syntax (#7136).
//!
//! `perl-parser-core` exposes two body lowerers that produce the same output
//! type with no shared contract enforced between them, as
//! `hir_body_lowerer_parity_tests.rs` records in detail:
//!
//! > `BodyBuilder` handles only three `NodeKind` variants in `lower_expr`;
//! > everything else silently falls through to `HirExpr::Opaque`.
//! > `BodyBuilder2` handles 13+ variants…
//!
//! The regex families are typed on the canonical `lower_ast` path and remain
//! `Opaque` on the first-slice `lower_body` path, so the two disagree for
//! identical syntax. That divergence predates this work and is shared with
//! `Unary`, `Ternary`, `FunctionCall`, `If`, `While` and `Return` — extending
//! `lower_body` for the regex families alone would leave the same trap for the
//! next construct, so the boundary is proven here instead of papered over.
//!
//! This file is the machine-checked regression gate for that specific
//! boundary. It exists so the divergence is a *stated, tested* API boundary
//! rather than an undocumented surprise for a caller who picks the wrong entry
//! point, and so that closing the gap later fails a test that says exactly
//! what changed.
//!
//! # Which entry point to use
//!
//! Consumers wanting typed regex-family body forms must use `lower_ast`.
//! `lower_body` is the first-slice specimen lowerer; it does not even unwrap
//! `ExpressionStatement`, so a bare `$x =~ /foo/;` collapses before reaching
//! the regex node at all.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`, per
//! the workspace lint policy.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirBody, HirExpr, HirFile, HirStmt, lower_ast, lower_body};

type TestResult = Result<(), Box<dyn Error>>;

fn via_lower_body(source: &str) -> HirBody {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_body(&output.ast)
}

fn via_lower_ast(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// The RHS of the synthesized `Assign` inside a `HirStmt::Let` initializer.
///
/// Used to bypass the `ExpressionStatement` wrapper gap so these tests target
/// the regex NodeKind arms themselves rather than re-testing that known gap.
fn let_init_rhs(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let stmt_id = root.stmts.first().ok_or_else(|| "root block has no statements".to_string())?;
    let stmt =
        body.stmt(*stmt_id).ok_or_else(|| "first statement is missing from arena".to_string())?;
    let init_id = match stmt {
        HirStmt::Let { init, .. } => {
            (*init).ok_or_else(|| "Let statement has no initializer".to_string())?
        }
        other => return Err(format!("expected HirStmt::Let, got {other:?}").into()),
    };
    let assign = body
        .expr(init_id)
        .ok_or_else(|| "Let initializer expression is missing from arena".to_string())?;
    let rhs_id = match assign {
        HirExpr::Assign { rhs, .. } => *rhs,
        other => return Err(format!("expected HirExpr::Assign as Let init, got {other:?}").into()),
    };
    body.expr(rhs_id)
        .ok_or_else(|| "Assign RHS expression is missing from arena".to_string().into())
}

fn ast_root_first_regex_family(file: &HirFile) -> Option<&HirExpr> {
    file.bodies.iter().flat_map(|body| body.exprs.iter()).find(|expr| {
        matches!(
            expr,
            HirExpr::Regex(_)
                | HirExpr::Match(_)
                | HirExpr::Substitution(_)
                | HirExpr::Transliteration(_)
        )
    })
}

/// An unbound regex in an initializer position reaches `lower_expr` in both
/// lowerers, so this isolates the regex arm itself.
#[test]
fn unbound_regex_is_opaque_in_lower_body_but_typed_in_lower_ast() -> TestResult {
    let source = "my $r = qr/foo/i;";

    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Opaque { ast_kind } if ast_kind == "Regex"),
        "lower_body must emit Opaque{{\"Regex\"}} for a qr// initializer, got {lb_rhs:?}\n\
         (If this fails, lower_body now handles Regex — the boundary this file \
          documents has moved and hir_body_lowerer_parity_tests.rs needs updating)"
    );

    let file = via_lower_ast(source);
    let typed = ast_root_first_regex_family(&file)
        .ok_or_else(|| "lower_ast must produce a typed regex-family form".to_string())?;
    assert!(
        matches!(typed, HirExpr::Regex(r) if r.modifiers == "i"),
        "lower_ast must emit a typed HirExpr::Regex carrying its modifiers, got {typed:?}"
    );
    Ok(())
}

/// Every bound family diverges the same way, and for the same reason.
///
/// A bound operator at statement level never even reaches the regex arm in
/// `lower_body`: the `ExpressionStatement` wrapper collapses first. Pinning
/// that keeps the cause of the divergence explicit — it is the wrapper gap,
/// not a regex-specific decision.
#[test]
fn bound_regex_families_collapse_at_the_statement_wrapper_in_lower_body() -> TestResult {
    for source in ["$x =~ /foo/;", "$x =~ s/a/b/g;", "$x =~ tr/a-z/A-Z/;"] {
        let body = via_lower_body(source);
        let root = body.block(body.root_block).ok_or_else(|| "root block missing".to_string())?;
        let stmt_id = root.stmts.first().ok_or_else(|| format!("no statements for {source:?}"))?;
        let stmt = body.stmt(*stmt_id).ok_or_else(|| "statement missing".to_string())?;
        let expr_id = match stmt {
            HirStmt::Expr(id) => *id,
            other => return Err(format!("expected HirStmt::Expr, got {other:?}").into()),
        };
        let expr = body.expr(expr_id).ok_or_else(|| "expression missing".to_string())?;
        assert!(
            matches!(expr, HirExpr::Opaque { ast_kind } if ast_kind == "ExpressionStatement"),
            "lower_body must collapse {source:?} at the ExpressionStatement wrapper, got {expr:?}"
        );

        let file = via_lower_ast(source);
        assert!(
            ast_root_first_regex_family(&file).is_some(),
            "lower_ast must emit a typed regex-family form for {source:?}"
        );
    }
    Ok(())
}

/// The boundary is one-directional and must stay that way.
///
/// No regex-family variant may appear from `lower_body`. If one ever does, the
/// two lowerers have started to converge and this file's premise — that
/// `lower_ast` is the only source of typed regex forms — is no longer true.
#[test]
fn lower_body_never_emits_a_typed_regex_family_variant() -> TestResult {
    for source in [
        "my $r = qr/foo/i;",
        "$x =~ /foo/;",
        "$x !~ /foo/;",
        "$x =~ s/a/b/g;",
        "$x =~ tr/a-z/A-Z/;",
        "s/a/b/;",
    ] {
        let body = via_lower_body(source);
        let typed = body.exprs.iter().find(|expr| {
            matches!(
                expr,
                HirExpr::Regex(_)
                    | HirExpr::Match(_)
                    | HirExpr::Substitution(_)
                    | HirExpr::Transliteration(_)
            )
        });
        assert!(
            typed.is_none(),
            "lower_body must not emit a typed regex-family variant for {source:?}, got {typed:?}\n\
             (If this fails the lowerers have converged — remove this gate and update the parity \
              inventory rather than weakening the assertion)"
        );
    }
    Ok(())
}
