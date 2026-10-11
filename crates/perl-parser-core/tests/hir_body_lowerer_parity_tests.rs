//! Program-root convergence for the two exported HIR entries (#5813).
//!
//! Preserve the original divergence specimens and their independent production
//! expectations. The public body entry now projects the canonical file root, so
//! each specimen also compares complete arenas, bindings, owners and source maps.
//! Shared agreement alone is not a semantic oracle. Non-Program subtree inputs
//! retain the explicitly legacy behavior tested in `hir::body_entry`.

use std::error::Error;

use perl_parser_core::{
    Parser,
    hir::{
        AccessMode, AssignMode, BinaryOp, DeclStorageClass, HirBody, HirExpr, HirStmt,
        VariableKind, lower_ast, lower_body,
    },
};

type TestResult = Result<(), Box<dyn Error>>;

fn lowered(source: &str) -> Result<HirBody, Box<dyn Error>> {
    let output = Parser::new(source).parse_with_recovery();
    let body = lower_body(&output.ast);
    let file = lower_ast(&output.ast);
    let canonical = file.root_body().ok_or("canonical program root is missing")?;
    assert_eq!(&body, canonical, "complete Program body differs for {source:?}");
    Ok(body)
}

fn first_statement(body: &HirBody) -> Result<&HirStmt, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or("root block is missing")?;
    let id = root.stmts.first().ok_or("root statement is missing")?;
    body.stmt(*id).ok_or_else(|| "statement is missing from arena".into())
}

fn first_expression(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let HirStmt::Expr(id) = first_statement(body)? else {
        return Err("expected an expression statement".into());
    };
    body.expr(*id).ok_or_else(|| "expression is missing from arena".into())
}

fn initializer(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let HirStmt::Let { init: Some(id), .. } = first_statement(body)? else {
        return Err("expected a declaration with an initializer".into());
    };
    body.expr(*id).ok_or_else(|| "initializer is missing from arena".into())
}

fn rhs(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let HirExpr::Assign { rhs, .. } = initializer(body)? else {
        return Err("expected an assignment initializer".into());
    };
    body.expr(*rhs).ok_or_else(|| "assignment RHS is missing".into())
}

#[test]
fn expression_statement_wrapper_retains_the_variable() -> TestResult {
    let body = lowered("$a;")?;
    assert!(matches!(
        first_expression(&body)?,
        HirExpr::Variable(var)
            if var.name == "a" && var.kind == VariableKind::Package
    ));
    Ok(())
}

#[test]
fn unary_initializer_is_structured() -> TestResult {
    let body = lowered("my $x = !$flag;")?;
    let HirExpr::Unary { operand, op, .. } = rhs(&body)? else {
        return Err("unary initializer lost its structure".into());
    };
    assert_eq!(op, "!");
    assert!(matches!(body.expr(*operand), Some(HirExpr::Variable(var)) if var.name == "flag"));
    Ok(())
}

#[test]
fn ternary_initializer_retains_selected_arm_structure() -> TestResult {
    let body = lowered("my $x = $c ? $a : $b;")?;
    let HirExpr::Ternary { condition, then_expr, else_expr } = rhs(&body)? else {
        return Err("ternary initializer lost its structure".into());
    };
    for (id, name) in [(*condition, "c"), (*then_expr, "a"), (*else_expr, "b")] {
        assert!(matches!(body.expr(id), Some(HirExpr::Variable(var)) if var.name == name));
    }
    Ok(())
}

#[test]
fn call_initializer_retains_its_argument() -> TestResult {
    let body = lowered("my $x = foo($a);")?;
    let HirExpr::Call { args, .. } = rhs(&body)? else {
        return Err("call initializer lost its structure".into());
    };
    assert_eq!(args.len(), 1);
    assert!(matches!(body.expr(args[0]), Some(HirExpr::Variable(var)) if var.name == "a"));
    Ok(())
}

#[test]
fn branch_statement_retains_its_nonempty_block() -> TestResult {
    let body = lowered("if ($flag) { $x = 1; }")?;
    let HirExpr::Branch { then_block, .. } = first_expression(&body)? else {
        return Err("if statement lost its branch".into());
    };
    assert_eq!(body.block(*then_block).ok_or("missing branch block")?.stmts.len(), 1);
    Ok(())
}

#[test]
fn loop_statement_retains_its_nonempty_block() -> TestResult {
    let body = lowered("while ($ready) { $x = 1; }")?;
    let HirExpr::Loop { body: loop_body, .. } = first_expression(&body)? else {
        return Err("while statement lost its loop".into());
    };
    assert_eq!(body.block(*loop_body).ok_or("missing loop block")?.stmts.len(), 1);
    Ok(())
}

#[test]
fn return_statement_retains_its_value() -> TestResult {
    let body = lowered("return $x;")?;
    let HirExpr::Return { value: Some(value) } = first_expression(&body)? else {
        return Err("return statement lost its value".into());
    };
    assert!(matches!(body.expr(*value), Some(HirExpr::Variable(var)) if var.name == "x"));
    Ok(())
}

#[test]
fn try_statement_retains_regions_instead_of_a_call_shell() -> TestResult {
    let body = lowered("try { $x = 1; } catch ($e) { $x = 2; }")?;
    let HirExpr::Try { body: try_body, catch_handlers, .. } = first_expression(&body)? else {
        return Err("try statement regressed to an opaque/call shell".into());
    };
    assert_eq!(body.block(*try_body).ok_or("missing try block")?.stmts.len(), 1);
    assert_eq!(catch_handlers.len(), 1);
    let handler = catch_handlers.first().ok_or("missing catch handler")?;
    assert_eq!(body.block(handler.block).ok_or("missing catch block")?.stmts.len(), 1);
    Ok(())
}

#[test]
fn unbound_initializer_variable_is_package_not_lexical() -> TestResult {
    let body = lowered("my $x = $a;")?;
    assert!(matches!(
        rhs(&body)?,
        HirExpr::Variable(var)
            if var.name == "a" && var.kind == VariableKind::Package && var.binding.is_none()
    ));
    Ok(())
}

#[test]
fn binary_initializer_preserves_add_and_operand_order() -> TestResult {
    let body = lowered("my $x = $a + $b;")?;
    let HirExpr::Binary { lhs, rhs, op: BinaryOp::Add } = rhs(&body)? else {
        return Err("binary initializer lost addition".into());
    };
    assert!(matches!(body.expr(*lhs), Some(HirExpr::Variable(var)) if var.name == "a"));
    assert!(matches!(body.expr(*rhs), Some(HirExpr::Variable(var)) if var.name == "b"));
    Ok(())
}

#[test]
fn declaration_retains_name_simple_assignment_and_binding() -> TestResult {
    let body = lowered("my $x = $y;")?;
    let HirStmt::Let { name, binding: Some(binding), .. } = first_statement(&body)? else {
        return Err("declaration lost its binding".into());
    };
    assert_eq!(name, "x");
    let HirExpr::Assign { lhs, mode: AssignMode::Simple, .. } = initializer(&body)? else {
        return Err("declaration lost its simple assignment".into());
    };
    assert!(matches!(
        body.expr(*lhs),
        Some(HirExpr::Variable(var))
            if var.name == "x" && var.access == AccessMode::Write
                && var.binding == Some(*binding)
    ));
    Ok(())
}

#[test]
fn legacy_field_call_assignment_uses_the_resolved_argument() -> TestResult {
    let body = lowered("field $x = 1;\n")?;
    assert!(matches!(
        first_statement(&body)?,
        HirStmt::Let { storage: DeclStorageClass::Unknown, .. }
    ));
    let HirExpr::Assign { lhs, .. } = initializer(&body)? else {
        return Err("legacy field call lost assignment argument".into());
    };
    assert!(matches!(
        body.expr(*lhs),
        Some(HirExpr::Variable(var)) if var.name == "x" && var.kind == VariableKind::Package
    ));
    Ok(())
}

#[test]
fn bare_legacy_field_call_retains_a_read_argument() -> TestResult {
    let body = lowered("field $x;\n")?;
    assert!(matches!(
        first_statement(&body)?,
        HirStmt::Let { storage: DeclStorageClass::Unknown, .. }
    ));
    assert!(matches!(
        initializer(&body)?,
        HirExpr::Variable(var)
            if var.name == "x" && var.access == AccessMode::Read
                && var.kind == VariableKind::Package
    ));
    Ok(())
}
