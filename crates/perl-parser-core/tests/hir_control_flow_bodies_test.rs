//! Structured HIR body control-flow coverage for issue #2579.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    BodyOwnerKind, ControlTransferKind, HirExpr, HirStmt, LoopKind, StatementModifierKind,
    VariableKind, lower_ast,
};

fn parse(source: &str) -> perl_parser_core::hir::HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn first_expr(body: &perl_parser_core::hir::HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let stmt_id = *root.stmts.first().ok_or_else(|| "root has no statements".to_string())?;
    let stmt = body.stmt(stmt_id).ok_or_else(|| "first statement is missing".to_string())?;
    let expr_id = match stmt {
        HirStmt::Expr(expr_id) => *expr_id,
        other => return Err(format!("expected expression statement, got {other:?}").into()),
    };
    body.expr(expr_id).ok_or_else(|| "first expression is missing".to_string().into())
}

#[test]
fn branch_links_then_and_else_blocks() -> Result<(), Box<dyn Error>> {
    let file = parse("if ($flag) { $then = 1; } else { $else = 2; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Branch { condition, then_block, else_block, keyword, .. } = first_expr(body)?
    else {
        return Err("expected structured branch".into());
    };
    assert!(body.expr(*condition).is_some(), "branch condition must resolve");
    assert!(body.block(*then_block).is_some(), "then block must resolve");
    assert!(else_block.is_some(), "else block must be linked");
    assert!(matches!(keyword, perl_parser_core::hir::BranchKeyword::If));
    Ok(())
}

#[test]
fn branch_links_elsif_condition_and_block() -> Result<(), Box<dyn Error>> {
    let file = parse("if ($a) { $x = 1; } elsif ($b) { $x = 2; } else { $x = 3; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Branch { elsif_arms, .. } = first_expr(body)? else {
        return Err("expected structured branch".into());
    };
    assert_eq!(elsif_arms.len(), 1);
    assert!(body.expr(elsif_arms[0].0).is_some(), "elsif condition must resolve");
    assert!(body.block(elsif_arms[0].1).is_some(), "elsif block must resolve");
    Ok(())
}

#[test]
fn branch_block_uses_its_nested_lexical_scope() -> Result<(), Box<dyn Error>> {
    let file = parse("if ($condition) { my $branch_value = 1; $branch_value; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Branch { then_block, .. } = first_expr(body)? else {
        return Err("expected structured branch".into());
    };
    let branch = body.block(*then_block).ok_or_else(|| "then block is missing".to_string())?;
    let read_stmt = *branch.stmts.get(1).ok_or_else(|| "branch read is missing".to_string())?;
    let HirStmt::Expr(read_expr) =
        body.stmt(read_stmt).ok_or_else(|| "branch statement is missing".to_string())?
    else {
        return Err("expected branch read expression".into());
    };
    let HirExpr::Variable(variable) =
        body.expr(*read_expr).ok_or_else(|| "branch variable is missing".to_string())?
    else {
        return Err("expected branch variable expression".into());
    };
    assert_eq!(variable.name, "branch_value");
    assert_eq!(variable.kind, VariableKind::Lexical);
    Ok(())
}

#[test]
fn ternary_links_all_three_expressions() -> Result<(), Box<dyn Error>> {
    let file = parse("$flag ? $left : $right;");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Ternary { condition, then_expr, else_expr } = first_expr(body)? else {
        return Err("expected structured ternary".into());
    };
    assert!(body.expr(*condition).is_some());
    assert!(body.expr(*then_expr).is_some());
    assert!(body.expr(*else_expr).is_some());
    Ok(())
}

#[test]
fn while_links_condition_body_and_continue_block() -> Result<(), Box<dyn Error>> {
    let file = parse("while ($ready) { $x = 1; } continue { $x++; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { kind, condition, body: loop_body, continue_block, .. } = first_expr(body)?
    else {
        return Err("expected structured loop".into());
    };
    assert!(matches!(kind, LoopKind::While));
    assert!(condition.is_some());
    assert!(body.block(*loop_body).is_some());
    assert!(continue_block.is_some());
    Ok(())
}

#[test]
fn foreach_links_iterator_binding_and_iterable() -> Result<(), Box<dyn Error>> {
    let file = parse("for my $item (@items) { $seen = $item; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { kind, condition, iterator_binding, .. } = first_expr(body)? else {
        return Err("expected structured foreach loop".into());
    };
    assert!(matches!(kind, LoopKind::Foreach));
    assert!(condition.is_some(), "foreach iterable must be linked");
    let binding_id = iterator_binding.ok_or_else(|| "iterator binding is missing".to_string())?;
    assert!(
        matches!(body.expr(binding_id), Some(HirExpr::Variable(variable)) if variable.name == "item")
    );
    Ok(())
}

#[test]
fn c_style_for_links_initializer_and_update() -> Result<(), Box<dyn Error>> {
    let file = parse("for (my $i = 0; $i < 2; $i++) { $seen = $i; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { kind, init, condition, update, .. } = first_expr(body)? else {
        return Err("expected structured C-style loop".into());
    };
    assert!(matches!(kind, LoopKind::CStyleFor));
    let init_block = init
        .and_then(|id| body.block(id))
        .ok_or_else(|| "C-style loop initializer block is missing".to_string())?;
    assert!(matches!(
        init_block.stmts.first().and_then(|id| body.stmt(*id)),
        Some(HirStmt::Let { .. })
    ));
    assert!(condition.is_some(), "C-style loop condition must be linked");
    assert!(matches!(update.and_then(|id| body.expr(id)), Some(HirExpr::Unary { .. })));
    Ok(())
}

#[test]
fn c_style_for_preserves_comma_separated_initializers() -> Result<(), Box<dyn Error>> {
    let file = parse("for (my $i = 0, $j = 0; $i < 2; $i++) { $seen = $j; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { init, .. } = first_expr(body)? else {
        return Err("expected structured C-style loop".into());
    };
    let init_block = init
        .and_then(|id| body.block(id))
        .ok_or_else(|| "C-style loop initializer block is missing".to_string())?;
    assert_eq!(init_block.stmts.len(), 2);
    assert!(matches!(
        init_block.stmts.first().and_then(|id| body.stmt(*id)),
        Some(HirStmt::Let { name, .. }) if name == "i"
    ));
    let second =
        init_block.stmts.get(1).ok_or_else(|| "second initializer is missing".to_string())?;
    let HirStmt::Expr(second_expr) =
        body.stmt(*second).ok_or_else(|| "second initializer statement is missing".to_string())?
    else {
        return Err("expected second initializer assignment expression".into());
    };
    assert!(matches!(body.expr(*second_expr), Some(HirExpr::Assign { .. })));
    Ok(())
}

#[test]
fn c_style_for_header_resolves_initializer_lexical() -> Result<(), Box<dyn Error>> {
    let file = parse("for (my $i = 0; $i < 2; $i++) { $seen = $i; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { condition, update, .. } = first_expr(body)? else {
        return Err("expected structured C-style loop".into());
    };
    let condition = condition.ok_or_else(|| "loop condition is missing".to_string())?;
    let update = update.ok_or_else(|| "loop update is missing".to_string())?;
    let HirExpr::Binary { lhs, .. } =
        body.expr(condition).ok_or_else(|| "loop condition expression is missing".to_string())?
    else {
        return Err("expected binary loop condition".into());
    };
    let HirExpr::Variable(condition_variable) =
        body.expr(*lhs).ok_or_else(|| "loop condition variable is missing".to_string())?
    else {
        return Err("expected loop condition variable".into());
    };
    let HirExpr::Unary { operand, .. } =
        body.expr(update).ok_or_else(|| "loop update expression is missing".to_string())?
    else {
        return Err("expected unary loop update".into());
    };
    let HirExpr::Variable(update_variable) =
        body.expr(*operand).ok_or_else(|| "loop update variable is missing".to_string())?
    else {
        return Err("expected loop update variable".into());
    };
    assert_eq!(condition_variable.kind, VariableKind::Lexical);
    assert_eq!(update_variable.kind, VariableKind::Lexical);
    Ok(())
}

#[test]
fn return_links_binary_value() -> Result<(), Box<dyn Error>> {
    let file = parse("sub answer { return $a + $b; }");
    let body = file
        .bodies
        .iter()
        .find(|body| matches!(&body.owner, BodyOwnerKind::Subroutine { name: Some(name) } if name == "answer"))
        .ok_or_else(|| "subroutine body is missing".to_string())?;
    let expr = first_expr(body)?;
    let HirExpr::Return { value: Some(value) } = expr else {
        return Err("expected return with a value".into());
    };
    assert!(matches!(body.expr(*value), Some(HirExpr::Binary { .. })));
    Ok(())
}

#[test]
fn loop_control_is_a_structured_statement() -> Result<(), Box<dyn Error>> {
    let file = parse("while (1) { last OUTER; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { body: loop_body, .. } = first_expr(body)? else {
        return Err("expected structured loop".into());
    };
    let block = body.block(*loop_body).ok_or_else(|| "loop body is missing".to_string())?;
    let stmt = body
        .stmt(*block.stmts.first().ok_or_else(|| "loop body is empty".to_string())?)
        .ok_or_else(|| "loop-control statement is missing".to_string())?;
    assert!(matches!(
        stmt,
        HirStmt::LoopControl {
            verb: ControlTransferKind::Last,
            written_label: Some(label),
            ..
        } if label == "OUTER"
    ));
    Ok(())
}

#[test]
fn nested_block_binding_resolves_as_lexical() -> Result<(), Box<dyn Error>> {
    let file = parse("if ($flag) { my $x = 1; $x; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Branch { then_block, .. } = first_expr(body)? else {
        return Err("expected structured branch".into());
    };
    let block = body.block(*then_block).ok_or_else(|| "then block is missing".to_string())?;
    let use_stmt = body
        .stmt(*block.stmts.get(1).ok_or_else(|| "use statement is missing".to_string())?)
        .ok_or_else(|| "use statement node is missing".to_string())?;
    let HirStmt::Expr(expr_id) = use_stmt else {
        return Err(format!("expected expression use, got {use_stmt:?}").into());
    };
    assert!(matches!(
        body.expr(*expr_id),
        Some(HirExpr::Variable(variable)) if matches!(variable.kind, VariableKind::Lexical)
    ));
    Ok(())
}

#[test]
fn postfix_condition_links_statement_and_condition() -> Result<(), Box<dyn Error>> {
    let file = parse("say $message if $enabled;");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let stmt = body
        .stmt(*root.stmts.first().ok_or_else(|| "root has no statements".to_string())?)
        .ok_or_else(|| "postfix statement is missing".to_string())?;
    let HirStmt::PostfixCondition { statement, condition, verb, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(matches!(body.stmt(*statement), Some(HirStmt::Expr(_))));
    assert!(body.expr(*condition).is_some());
    assert!(matches!(verb, StatementModifierKind::If));
    Ok(())
}

#[test]
fn postfix_loop_control_preserves_transfer_statement() -> Result<(), Box<dyn Error>> {
    let file = parse("while (1) { last if $done; next unless $ok; }");
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let HirExpr::Loop { body: loop_body, .. } = first_expr(body)? else {
        return Err("expected structured loop".into());
    };
    let block = body.block(*loop_body).ok_or_else(|| "loop body is missing".to_string())?;
    let first = body
        .stmt(*block.stmts.first().ok_or_else(|| "first postfix statement is missing".to_string())?)
        .ok_or_else(|| "first postfix statement node is missing".to_string())?;
    let second = body
        .stmt(*block.stmts.get(1).ok_or_else(|| "second postfix statement is missing".to_string())?)
        .ok_or_else(|| "second postfix statement node is missing".to_string())?;
    assert!(matches!(
        first,
        HirStmt::PostfixCondition { statement, .. }
            if matches!(body.stmt(*statement), Some(HirStmt::LoopControl { verb: ControlTransferKind::Last, .. }))
    ));
    assert!(matches!(
        second,
        HirStmt::PostfixCondition { statement, .. }
            if matches!(body.stmt(*statement), Some(HirStmt::LoopControl { verb: ControlTransferKind::Next, .. }))
    ));
    Ok(())
}
