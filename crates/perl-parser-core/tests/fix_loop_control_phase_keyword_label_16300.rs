//! Loop control with phase-keyword labels (#16300).
//!
//! Phase keywords double as loop labels: `CHECK: while (1) { last CHECK; }`
//! is valid Perl. These tests pin the AST shape — the outer
//! `LabeledStatement`, the `While` beneath it, and the attached label on
//! the inner `LoopControl` — beyond the clean-parse coverage in
//! `fix_phase_keyword_as_label_2389.rs`.

use perl_parser_core::{NodeKind, Parser};

fn parse_one_statement(src: &str) -> perl_parser_core::Node {
    let mut parser = Parser::new(src);
    let ast = parser.parse().unwrap_or_else(|e| panic!("`{src}` must parse, got: {e:?}"));
    let sexp = ast.to_sexp();
    assert!(!sexp.contains("ERROR"), "Parse should not emit ERROR nodes: {sexp}\nsource: {src}");
    ast
}

#[test]
fn test_loop_control_with_phase_keyword_label() {
    for op in ["last", "next", "redo"] {
        let source = format!("CHECK: while (1) {{ {op} CHECK; }}");
        let ast = parse_one_statement(&source);

        let NodeKind::Program { statements } = &ast.kind else {
            panic!("expected Program, got {:?}", ast.kind);
        };
        let outer =
            statements.first().unwrap_or_else(|| panic!("expected one statement for `{source}`"));
        let NodeKind::LabeledStatement { label, statement } = &outer.kind else {
            panic!("expected LabeledStatement, got {:?}", outer.kind);
        };
        assert_eq!(label, "CHECK");
        let NodeKind::While { body, .. } = &statement.kind else {
            panic!("expected While under the label, got {:?}", statement.kind);
        };
        let NodeKind::Block { statements: body_stmts } = &body.kind else {
            panic!("expected Block loop body, got {:?}", body.kind);
        };
        let ctrl = body_stmts
            .first()
            .unwrap_or_else(|| panic!("expected a loop-control statement in `{source}`"));
        let NodeKind::LoopControl { op: found_op, label: found_label } = &ctrl.kind else {
            panic!("expected LoopControl in loop body, got {:?}", ctrl.kind);
        };
        assert_eq!(found_op, op);
        assert_eq!(found_label.as_deref(), Some("CHECK"));
    }
}
