//! A mutating `s///` / `tr///` target is a read-modify-write occurrence
//! (#14639).
//!
//! The target is read, transformed, and written back, which is the same
//! occurrence shape as compound assignment. Before this fix the occurrence
//! was lowered as a plain `AccessMode::Read`, so a write query could not
//! see the reassignment. `/r` forms stay reads (they return a copy), a
//! plain match stays a read, and a non-place target stays a read because
//! it is not a valid lvalue.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirExpr, HirExprId, HirRegexTarget, RegexTargetKind, lower_ast,
};
use perl_tdd_support::must_some_with;

/// Access mode of the variable a regex-family target binds.
fn target_variable_access(source: &str) -> AccessMode {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    for body in &hir.bodies {
        for index in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(index as u32);
            let target = match body.expr(id) {
                Some(HirExpr::Substitution(s)) => &s.target,
                Some(HirExpr::Transliteration(t)) => &t.target,
                Some(HirExpr::Match(m)) => &m.target,
                _ => continue,
            };
            let HirRegexTarget::Bound { expr: expr_id, .. } = target else {
                continue;
            };
            if let Some(HirExpr::Variable(v)) = body.expr(*expr_id) {
                return v.access;
            }
        }
    }
    must_some_with(None, format!("no variable regex target found in {source:?}"))
}

#[test]
fn mutating_substitution_and_transliteration_targets_are_read_modify_write() {
    assert_eq!(target_variable_access("$x =~ s/a/b/g;"), AccessMode::ReadModifyWrite);
    assert_eq!(target_variable_access("$x =~ tr/a-z/A-Z/;"), AccessMode::ReadModifyWrite);
}

#[test]
fn non_destructive_and_non_place_targets_stay_reads() {
    // /r leaves the target untouched, and a match never writes.
    assert_eq!(target_variable_access("$x =~ s/a/b/r;"), AccessMode::Read);
    assert_eq!(target_variable_access("$x =~ tr/a-z/A-Z/r;"), AccessMode::Read);
    assert_eq!(target_variable_access("$x =~ /foo/;"), AccessMode::Read);

    // A non-place target is not a valid lvalue and stays a read even for a
    // mutating substitution.
    let mut parser = Parser::new("foo() =~ s/a/b/;");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let mut kinds = Vec::new();
    for body in &hir.bodies {
        for expr in body.exprs.iter() {
            if let HirExpr::Substitution(s) = expr
                && let HirRegexTarget::Bound { kind, .. } = s.target
            {
                kinds.push(kind);
            }
        }
    }
    assert_eq!(kinds, vec![RegexTargetKind::Expression]);
}
