//! Unit shape coverage for four proven parser arms (#16300).
//!
//! Language behavior was ground-truthed with `perl -c` (this environment:
//! perl 5.38.2; the issue used Strawberry perl 5.42). These tests pin the
//! parser AST so a refactor cannot silently reroute an arm. They do not
//! invoke `perl` at runtime.
//!
//! | Arm | `perl -c` | Parser contract |
//! | --- | --- | --- |
//! | orphaned `else` / `elsif` | syntax error (`near "else"` / `near "elsif"`) | recover: diagnostic + synthetic `If`, no `ERROR` nodes |
//! | `try($x)` | syntax OK as a call | `FunctionCall { name: "try" }`, never `Try` |
//! | `CHECK: for (...)` | syntax OK | `LabeledStatement { label: "CHECK" }` over `For` |
//! | `last CHECK` | syntax OK | `LoopControl { op: "last", label: Some("CHECK") }` |

use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::{must, must_some_with};

fn parse_ast(source: &str) -> (Node, Parser) {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    (ast, parser)
}

fn first_error_kind(node: &Node) -> Option<&'static str> {
    match &node.kind {
        NodeKind::Error { .. }
        | NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock => return Some(node.kind.kind_name()),
        _ => {}
    }
    for child in node.children() {
        if let Some(name) = first_error_kind(child) {
            return Some(name);
        }
    }
    None
}

fn assert_no_error_nodes(source: &str, ast: &Node) -> Result<(), String> {
    match first_error_kind(ast) {
        None => Ok(()),
        Some(kind) => Err(format!(
            "expected no ERROR/Missing nodes for `{source}`, found {kind} in {}",
            ast.to_sexp()
        )),
    }
}

fn first_statement<'a>(source: &str, ast: &'a Node) -> Result<&'a Node, String> {
    let NodeKind::Program { statements } = &ast.kind else {
        return Err(format!("expected Program for `{source}`, got {}", ast.kind.kind_name()));
    };
    statements.first().ok_or_else(|| format!("expected a statement for `{source}`"))
}

fn diagnostic_texts(parser: &Parser) -> Vec<String> {
    parser.errors().iter().map(ToString::to_string).collect()
}

/// `else { fallback(); }` — perl -c: syntax error. Recovery must record the
/// orphaned-else diagnostic, wrap the block in a synthetic `If` whose
/// condition is the literal `1`, and leave no ERROR nodes.
#[test]
fn orphaned_else_recovers_synthetic_if_without_error_nodes() -> Result<(), String> {
    let source = "else { fallback(); }";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;

    let errors = diagnostic_texts(&parser);
    if !errors.iter().any(|e| e.contains("'else' without preceding 'if' or 'unless'")) {
        return Err(format!("expected orphaned-else diagnostic, got: {errors:?}"));
    }

    let stmt = first_statement(source, &ast)?;
    let NodeKind::If { condition, then_branch, elsif_branches, else_branch, keyword } = &stmt.kind
    else {
        return Err(format!(
            "expected synthetic If, got {} in {}",
            stmt.kind.kind_name(),
            ast.to_sexp()
        ));
    };
    match &condition.kind {
        NodeKind::Number { value } if value == "1" => {}
        other => {
            return Err(format!(
                "expected synthetic condition Number \"1\", got {}",
                other.kind_name()
            ));
        }
    }
    if !matches!(then_branch.kind, NodeKind::Block { .. }) {
        return Err(format!(
            "expected else block preserved as then_branch, got {}",
            then_branch.kind.kind_name()
        ));
    }
    if !elsif_branches.is_empty() {
        return Err("orphaned else must not grow an elsif chain".to_string());
    }
    if else_branch.is_some() {
        return Err("orphaned else must not nest a further else".to_string());
    }
    if keyword.is_some() {
        return Err(format!("expected no loop/unless keyword tag, got {keyword:?}"));
    }
    Ok(())
}

/// `elsif ($flag) { work(); } else { last_resort(); }` — perl -c: syntax
/// error. Recovery must keep the real condition, fold the trailing else, and
/// leave no ERROR nodes.
#[test]
fn orphaned_elsif_chain_recovers_condition_and_else_without_error_nodes() -> Result<(), String> {
    let source = "elsif ($flag) { work(); } else { last_resort(); }";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;

    let errors = diagnostic_texts(&parser);
    if !errors.iter().any(|e| e.contains("'elsif' without preceding 'if' or 'unless'")) {
        return Err(format!("expected orphaned-elsif diagnostic, got: {errors:?}"));
    }

    let stmt = first_statement(source, &ast)?;
    let NodeKind::If { condition, then_branch, elsif_branches, else_branch, keyword } = &stmt.kind
    else {
        return Err(format!(
            "expected recovered If, got {} in {}",
            stmt.kind.kind_name(),
            ast.to_sexp()
        ));
    };
    if matches!(&condition.kind, NodeKind::Number { value } if value == "1") {
        return Err(
            "elsif condition must not be replaced by the synthetic true constant".to_string()
        );
    }
    if !matches!(then_branch.kind, NodeKind::Block { .. }) {
        return Err(format!(
            "expected elsif block preserved as then_branch, got {}",
            then_branch.kind.kind_name()
        ));
    }
    if !elsif_branches.is_empty() {
        return Err("a single recovered elsif should occupy the If condition, not elsif_branches"
            .to_string());
    }
    let else_branch =
        must_some_with(else_branch.as_deref(), "trailing else folded into recovered If");
    if !matches!(else_branch.kind, NodeKind::Block { .. }) {
        return Err(format!("expected else block preserved, got {}", else_branch.kind.kind_name()));
    }
    if keyword.is_some() {
        return Err(format!("expected no loop/unless keyword tag, got {keyword:?}"));
    }
    Ok(())
}

/// `try($x);` — perl -c: syntax OK as a user call. Must not enter `parse_try`.
#[test]
fn try_paren_call_is_function_call_not_try_block() -> Result<(), String> {
    let source = "try($x);";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected diagnostics: {:?}", diagnostic_texts(&parser)));
    }

    let stmt = first_statement(source, &ast)?;
    let call = match &stmt.kind {
        NodeKind::ExpressionStatement { expression } => &expression.kind,
        other => other,
    };
    let NodeKind::FunctionCall { name, args } = call else {
        return Err(format!(
            "expected FunctionCall for try($x), got {} in {}",
            call.kind_name(),
            ast.to_sexp()
        ));
    };
    if name != "try" {
        return Err(format!("expected call name \"try\", got {name:?}"));
    }
    if args.len() != 1 {
        return Err(format!("expected one argument $x, got {}", args.len()));
    }
    if ast.to_sexp().contains("Try") {
        return Err(format!("try($x) must not parse as Try, got {}", ast.to_sexp()));
    }
    Ok(())
}

/// Opposite-direction control for the `try(` vs brace-form guard: the brace form
/// must still produce a `Try` node. (perl -c accepts this only under
/// `use experimental "try"`; the parser still owns the construct.)
#[test]
fn try_brace_still_parses_as_try_catch() -> Result<(), String> {
    let source = "try { risky(); } catch ($e) { handle($e); }";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected diagnostics: {:?}", diagnostic_texts(&parser)));
    }

    let stmt = first_statement(source, &ast)?;
    if !matches!(stmt.kind, NodeKind::Try { .. }) {
        return Err(format!(
            "expected Try for brace form, got {} in {}",
            stmt.kind.kind_name(),
            ast.to_sexp()
        ));
    }
    Ok(())
}

/// `CHECK: for (my $i = 0; $i < 3; $i++) { next; }` — perl -c: syntax OK.
/// Must be a statement label, never a phase block.
#[test]
fn phase_keyword_colon_is_statement_label_not_phase_block() -> Result<(), String> {
    let source = "CHECK: for (my $i = 0; $i < 3; $i++) { next; }";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected diagnostics: {:?}", diagnostic_texts(&parser)));
    }

    let stmt = first_statement(source, &ast)?;
    let NodeKind::LabeledStatement { label, statement: labeled } = &stmt.kind else {
        return Err(format!(
            "expected LabeledStatement for CHECK:, got {} in {}",
            stmt.kind.kind_name(),
            ast.to_sexp()
        ));
    };
    if label != "CHECK" {
        return Err(format!("expected label \"CHECK\", got {label:?}"));
    }
    if !matches!(labeled.kind, NodeKind::For { .. }) {
        return Err(format!("expected labeled C-style For, got {}", labeled.kind.kind_name()));
    }
    Ok(())
}

/// `CHECK: while (1) { last CHECK; }` — perl -c: syntax OK. Loop control
/// must attach the phase-keyword label.
#[test]
fn loop_control_attaches_phase_keyword_label() -> Result<(), String> {
    let source = "CHECK: while (1) { last CHECK; }";
    let (ast, parser) = parse_ast(source);
    assert_no_error_nodes(source, &ast)?;
    if !parser.errors().is_empty() {
        return Err(format!("unexpected diagnostics: {:?}", diagnostic_texts(&parser)));
    }

    let stmt = first_statement(source, &ast)?;
    let NodeKind::LabeledStatement { label, statement: labeled } = &stmt.kind else {
        return Err(format!(
            "expected LabeledStatement for CHECK:, got {} in {}",
            stmt.kind.kind_name(),
            ast.to_sexp()
        ));
    };
    if label != "CHECK" {
        return Err(format!("expected label \"CHECK\", got {label:?}"));
    }
    let NodeKind::While { body, .. } = &labeled.kind else {
        return Err(format!("expected labeled While, got {}", labeled.kind.kind_name()));
    };
    let NodeKind::Block { statements: body_stmts } = &body.kind else {
        return Err(format!("expected While body block, got {}", body.kind.kind_name()));
    };
    let ctrl = body_stmts
        .first()
        .ok_or_else(|| format!("expected loop-control statement in `{source}`"))?;
    let NodeKind::LoopControl { op, label: ctrl_label } = &ctrl.kind else {
        return Err(format!("expected LoopControl, got {}", ctrl.kind.kind_name()));
    };
    if op != "last" {
        return Err(format!("expected op \"last\", got {op:?}"));
    }
    if ctrl_label.as_deref() != Some("CHECK") {
        return Err(format!("expected control label CHECK, got {ctrl_label:?}"));
    }
    Ok(())
}
