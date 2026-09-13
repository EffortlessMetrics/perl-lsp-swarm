//! Canonical body-HIR proof for assignments embedded in `local` declarations.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::hir::{
    AccessMode, AssignMode, DeclStorageClass, HirExpr, HirStmt, VariableKind, lower_ast, lower_body,
};
use perl_parser_core::{Node, NodeKind};

fn find_assignment<'a>(node: &'a Node, expected_op: &str) -> Option<&'a Node> {
    if matches!(&node.kind, NodeKind::Assignment { op, .. } if op == expected_op) {
        return Some(node);
    }

    node.children().into_iter().find_map(|child| find_assignment(child, expected_op))
}

fn source_slice<'a>(source: &'a str, start: usize, end: usize) -> Result<&'a str, String> {
    source
        .get(start..end)
        .ok_or_else(|| format!("invalid source range {start}..{end} for {source:?}"))
}

fn assert_local_assignment_hir(
    source: &str,
    expected_op: &str,
    expected_rhs: &str,
    expected_mode: AssignMode,
    expected_access: AccessMode,
) -> Result<(), String> {
    assert_clean_parse(source);
    let ast = parse(source);
    let ast_assignment = find_assignment(&ast, expected_op)
        .ok_or_else(|| format!("expected {expected_op} assignment in AST:\n{}", ast.to_sexp()))?;

    let hir = lower_ast(&ast);
    let body = hir.root_body().ok_or("expected canonical production root body")?;
    let root = body.block(body.root_block).ok_or("expected root HIR block")?;
    if root.stmts.len() != 1 {
        return Err(format!("expected one local statement, got {:?}", root.stmts));
    }
    let stmt_id = root.stmts[0];
    let stmt = body.stmt(stmt_id).ok_or("expected local HIR statement")?;
    let (name, storage, init_id, binding_range) = match stmt {
        HirStmt::Let { name, storage, init: Some(init_id), binding_range, .. } => {
            (name, storage, *init_id, *binding_range)
        }
        other => {
            return Err(format!(
                "local assignment must retain an initialized Let statement, got {other:?}"
            ));
        }
    };

    if name != "main::z" || !matches!(storage, DeclStorageClass::Local) {
        return Err(format!(
            "unexpected local binding identity: name={name:?}, storage={storage:?}"
        ));
    }
    if source_slice(source, binding_range.start, binding_range.end)? != "$main::z" {
        return Err(format!("unexpected local binding range: {binding_range:?}"));
    }

    let (lhs_id, rhs_id, mode) = match body.expr(init_id) {
        Some(HirExpr::Assign { lhs, rhs, mode }) => (*lhs, *rhs, mode),
        other => return Err(format!("local initializer must be HirExpr::Assign, got {other:?}")),
    };
    if mode != &expected_mode {
        return Err(format!("unexpected assignment mode: {mode:?} != {expected_mode:?}"));
    }

    let lhs = body.expr(lhs_id).ok_or("expected local assignment lhs")?;
    let HirExpr::Variable(variable) = lhs else {
        return Err(format!("local assignment lhs must be a variable place, got {lhs:?}"));
    };
    if variable.name != "main::z"
        || !matches!(variable.kind, VariableKind::Package)
        || variable.access != expected_access
    {
        return Err(format!("unexpected local assignment place: {variable:?}"));
    }

    if body.expr(rhs_id).is_none() {
        return Err("local assignment rhs was not lowered".to_string());
    }
    let rhs_range =
        body.source_map.expr_range(rhs_id).ok_or("expected local assignment rhs range")?;
    if source_slice(source, rhs_range.start, rhs_range.end)? != expected_rhs {
        return Err(format!("unexpected local assignment rhs range/value: {:?}", rhs_range));
    }

    let assignment_range =
        body.source_map.expr_range(init_id).ok_or("expected local assignment expression range")?;
    let expected_assignment = source_slice(
        source,
        source.find('$').ok_or("expected local variable")?,
        source.find(';').ok_or("expected statement terminator")?,
    )?;
    if source_slice(source, assignment_range.start, assignment_range.end)? != expected_assignment {
        return Err(format!(
            "local assignment range must preserve the embedded AST payload: {:?} vs {:?}",
            assignment_range, ast_assignment.location
        ));
    }

    let stmt_range = body.source_map.stmt_range(stmt_id).ok_or("expected local statement range")?;
    if source_slice(source, stmt_range.start, stmt_range.end)? != source.trim_end_matches(';') {
        return Err(format!("unexpected local statement range: {stmt_range:?}"));
    }

    let assignment_count =
        body.exprs.iter().filter(|expr| matches!(expr, HirExpr::Assign { .. })).count();
    if assignment_count != 1 {
        return Err(format!(
            "local assignment must lower once, got {assignment_count} assignment expressions"
        ));
    }

    Ok(())
}

#[test]
fn local_simple_assignment_payload_reaches_canonical_hir() -> Result<(), String> {
    assert_local_assignment_hir(
        "local $main::z = 'a';",
        "=",
        "'a'",
        AssignMode::Simple,
        AccessMode::Write,
    )
}

#[test]
fn local_symbolic_compound_assignment_payloads_reach_canonical_hir() -> Result<(), String> {
    for (source, op, rhs) in
        [("local $main::z += 1;", "+=", "1"), ("local $main::z .= 'q';", ".=", "'q'")]
    {
        assert_local_assignment_hir(
            source,
            op,
            rhs,
            AssignMode::ReadModifyWrite,
            AccessMode::ReadModifyWrite,
        )?;
    }
    Ok(())
}

#[test]
fn local_repetition_assignment_payload_reaches_canonical_hir() -> Result<(), String> {
    assert_local_assignment_hir(
        "local $main::z x= 3;",
        "x=",
        "3",
        AssignMode::ReadModifyWrite,
        AccessMode::ReadModifyWrite,
    )
}

#[test]
fn non_local_repetition_assignment_remains_one_rmw_expression() -> Result<(), String> {
    let source = "$main::z x= 3;";
    assert_clean_parse(source);
    let ast = parse(source);
    let hir = lower_ast(&ast);
    let body = hir.root_body().ok_or("expected canonical production root body")?;
    let assignments = body
        .exprs
        .iter()
        .filter_map(|expr| match expr {
            HirExpr::Assign { lhs, rhs, mode } => Some((*lhs, *rhs, mode)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if assignments.len() != 1 {
        return Err(format!("expected one non-local assignment, got {assignments:?}"));
    }
    let (lhs_id, rhs_id, mode) = assignments[0];
    if !matches!(mode, AssignMode::ReadModifyWrite) || body.expr(rhs_id).is_none() {
        return Err(format!("non-local x= lost RMW semantics: {assignments:?}"));
    }
    if !matches!(
        body.expr(lhs_id),
        Some(HirExpr::Variable(variable))
            if variable.access == AccessMode::ReadModifyWrite
                && variable.kind == VariableKind::Package
    ) {
        return Err(format!("non-local x= lost its package place: {:?}", body.expr(lhs_id)));
    }
    Ok(())
}

#[test]
fn recovered_local_assignment_does_not_become_exact_hir() -> Result<(), String> {
    // A missing RHS is recovered at the AST as `MissingExpression`. The body
    // lowerer must keep the assignment (so the write is still visible) but the
    // RHS must fail closed as an opaque node, never a fabricated exact value.
    let source = "local $main::z x=;";
    let output = perl_parser_core::Parser::new(source).parse_with_recovery();
    if output.diagnostics.is_empty() {
        return Err("expected recovery diagnostics for a missing repetition RHS".to_string());
    }
    let hir = lower_ast(&output.ast);
    let body = hir.root_body().ok_or("expected canonical production root body")?;
    let root = body.block(body.root_block).ok_or("expected root HIR block")?;
    let stmt = root.stmts.first().and_then(|id| body.stmt(*id)).ok_or("expected statement")?;
    let HirStmt::Let { storage: DeclStorageClass::Local, init: Some(init_id), .. } = stmt else {
        return Err(format!("recovered local must stay a local Let, got {stmt:?}"));
    };
    let Some(HirExpr::Assign { rhs, mode: AssignMode::ReadModifyWrite, .. }) = body.expr(*init_id)
    else {
        return Err(format!(
            "recovered local must keep its RMW assignment, got {:?}",
            body.expr(*init_id)
        ));
    };
    match body.expr(*rhs) {
        Some(HirExpr::Opaque { ast_kind }) if ast_kind == "MissingExpression" => Ok(()),
        other => {
            Err(format!("recovered RHS must lower as opaque MissingExpression, got {other:?}"))
        }
    }
}

#[test]
fn mirror_lowerer_keeps_local_assignment_place_and_mode() -> Result<(), String> {
    // The test-only `lower_body` mirror must agree with the canonical builder on
    // the embedded local assignment: package storage for the place, write vs
    // read-modify-write access, and the matching assignment mode.
    for (source, expected_mode, expected_access) in [
        ("local $main::z = 'a';", AssignMode::Simple, AccessMode::Write),
        ("local $x += 1;", AssignMode::ReadModifyWrite, AccessMode::ReadModifyWrite),
    ] {
        assert_clean_parse(source);
        let body = lower_body(&parse(source));
        let root = body.block(body.root_block).ok_or("expected root block")?;
        let stmt = root.stmts.first().and_then(|id| body.stmt(*id)).ok_or("expected statement")?;
        let HirStmt::Let { storage: DeclStorageClass::Local, init: Some(init_id), .. } = stmt
        else {
            return Err(format!("{source}: mirror must keep a local Let, got {stmt:?}"));
        };
        let Some(HirExpr::Assign { lhs, mode, .. }) = body.expr(*init_id) else {
            return Err(format!(
                "{source}: mirror init must be Assign, got {:?}",
                body.expr(*init_id)
            ));
        };
        if mode != &expected_mode {
            return Err(format!("{source}: mirror mode {mode:?} != {expected_mode:?}"));
        }
        match body.expr(*lhs) {
            Some(HirExpr::Variable(variable))
                if variable.kind == VariableKind::Package && variable.access == expected_access => {
            }
            other => {
                return Err(format!(
                    "{source}: mirror place must be a package {expected_access:?} place, got {other:?}"
                ));
            }
        }
    }
    Ok(())
}
