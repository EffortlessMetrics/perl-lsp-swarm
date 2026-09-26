//! #16300: direct unit shape coverage for parser arms whose behavior is
//! already proven by corpus/differential testing (ground-truthed against
//! Strawberry perl 5.42 `perl -c`) but had no unit test pinning the AST.
//! Each test asserts the proven node shape so a refactor cannot silently
//! reroute the arm.

use super::*;
use perl_tdd_support::must_some;

#[test]
fn test_try_paren_call_is_function_call_not_try_block() {
    // statements.rs guard: only `try {` (brace form) enters parse_try;
    // `try(...)` is an ordinary call of a user-defined sub named `try`.
    let code = "try($x);";
    let mut parser = Parser::new(code);
    let ast = must_some(parser.parse().ok());
    assert!(parser.errors().is_empty(), "unexpected errors: {:?}", parser.errors());

    let NodeKind::Program { statements } = &ast.kind else {
        unreachable!("Expected Program node, got {:?}", ast.kind);
    };
    assert_eq!(statements.len(), 1, "got: {}", ast.to_sexp());

    let sexp = ast.to_sexp();
    assert!(
        !sexp.contains("Try"),
        "try($x) must not parse as the try/catch construct, got: {sexp}"
    );
    let call = match &statements[0].kind {
        NodeKind::ExpressionStatement { expression } => &expression.kind,
        other => other,
    };
    let NodeKind::FunctionCall { name, args } = call else {
        unreachable!("expected FunctionCall for try($x), got {:?}", call);
    };
    assert_eq!(name, "try");
    assert_eq!(args.len(), 1, "expected the single argument $x");
}

#[test]
fn test_try_brace_still_parses_as_try_catch() {
    // Positive control for the guard: the brace form must keep producing a
    // Try node, so the two arms stay discriminated.
    let code = "try { risky(); } catch ($e) { handle($e); }";
    let mut parser = Parser::new(code);
    let ast = must_some(parser.parse().ok());
    assert!(parser.errors().is_empty(), "unexpected errors: {:?}", parser.errors());
    let NodeKind::Program { statements } = &ast.kind else {
        unreachable!("Expected Program node, got {:?}", ast.kind);
    };
    assert!(
        matches!(statements[0].kind, NodeKind::Try { .. }),
        "expected Try node for block form, got: {}",
        ast.to_sexp()
    );
}

#[test]
fn test_phase_keyword_colon_is_statement_label_not_phase_block() {
    // statements.rs guard: `CHECK:` followed by `:` is a loop label, so
    // `CHECK: for (...)` must yield a LabeledStatement, never a phase block.
    let code = "CHECK: for (my $i = 0; $i < 3; $i++) { next; }";
    let mut parser = Parser::new(code);
    let ast = must_some(parser.parse().ok());
    assert!(parser.errors().is_empty(), "unexpected errors: {:?}", parser.errors());

    let NodeKind::Program { statements } = &ast.kind else {
        unreachable!("Expected Program node, got {:?}", ast.kind);
    };
    assert_eq!(statements.len(), 1, "got: {}", ast.to_sexp());
    let NodeKind::LabeledStatement { label, statement: labeled } = &statements[0].kind else {
        unreachable!("Expected LabeledStatement for CHECK:, got {:?}", statements[0].kind);
    };
    assert_eq!(label, "CHECK");
    assert!(
        matches!(labeled.kind, NodeKind::For { .. }),
        "expected labeled For loop, got {:?}",
        labeled.kind
    );
}

#[test]
fn test_loop_control_attaches_phase_keyword_label() {
    // parse_loop_control label arm: `last CHECK` must attach the
    // phase-keyword label like any other label.
    let code = "CHECK: while (1) { last CHECK; }";
    let mut parser = Parser::new(code);
    let ast = must_some(parser.parse().ok());
    assert!(parser.errors().is_empty(), "unexpected errors: {:?}", parser.errors());

    let NodeKind::Program { statements } = &ast.kind else {
        unreachable!("Expected Program node, got {:?}", ast.kind);
    };
    let NodeKind::LabeledStatement { label, statement: labeled } = &statements[0].kind else {
        unreachable!("Expected LabeledStatement for CHECK:, got {:?}", statements[0].kind);
    };
    assert_eq!(label, "CHECK");
    let NodeKind::While { body, .. } = &labeled.kind else {
        unreachable!("expected labeled While loop, got {:?}", labeled.kind);
    };
    let NodeKind::Block { statements: body_stmts } = &body.kind else {
        unreachable!("expected While body block, got {:?}", body.kind);
    };
    assert_eq!(body_stmts.len(), 1, "got: {}", ast.to_sexp());
    let NodeKind::LoopControl { op, label: ctrl_label } = &body_stmts[0].kind else {
        unreachable!("expected LoopControl, got {:?}", body_stmts[0].kind);
    };
    assert_eq!(op, "last");
    assert_eq!(ctrl_label.as_deref(), Some("CHECK"));
}
