//! Issue #17000 — sweep of parser arms that fabricate zero-width scalar operands.
//!
//! The loop-condition repair landed in #17804; this file closes the remaining
//! acceptance item by recording every known fabrication arm on current `main` and
//! proving each disposition is distinguishable from a written operand without
//! inspecting source ranges (via `RecoveryConfidence` or `HirLiteral::synthesized`).
//!
//! | Parser arm | Witness | Fabricated AST | Consumer disposition |
//! | --- | --- | --- | --- |
//! | `while ()` empty condition | `while () { 42; }` | zero-width `Number("1")` | `RecoveryConfidence::Recovered` on the condition literal |
//! | orphaned `else` recovery | `else { 42; }` | zero-width `Number("1")` in recovered `If` | `RecoveryConfidence::Recovered` on the condition literal |
//! | missing unary operand | `!;` | zero-width `Undef` | `HirLiteral::synthesized` on the operand leaf |
//!
//! Non-goals: regex default-topic fabrication uses a distinct representation
//! (`is_synthesized_default_topic`); broader honesty work remains #6666.

use perl_parser_core::NodeKind;
use perl_parser_core::Parser;
use perl_parser_core::hir::{
    HirBody, HirExpr, HirFile, HirItem, HirKind, HirLiteralKind, LiteralKind, RecoveryConfidence,
    lower_ast, lower_body,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn root_body(source: &str) -> Result<HirBody, Box<dyn std::error::Error>> {
    let output = Parser::new(source).parse_with_recovery();
    Ok(lower_body(&output.ast))
}

fn zero_width_scalars(ast: &perl_parser_core::Node) -> Vec<(NodeKind, usize)> {
    let mut out = Vec::new();
    fn walk(node: &perl_parser_core::Node, out: &mut Vec<(NodeKind, usize)>) {
        if node.location.start == node.location.end {
            match &node.kind {
                NodeKind::Number { .. } | NodeKind::Undef => {
                    out.push((node.kind.clone(), node.location.start));
                }
                _ => {}
            }
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    walk(ast, &mut out);
    out
}

fn number_literals<'a>(file: &'a HirFile, value: &str) -> Vec<&'a HirItem> {
    file.items
        .iter()
        .filter(|item| {
            matches!(&item.kind, HirKind::LiteralExpr(lit)
                if lit.kind == LiteralKind::Number && lit.value.as_deref() == Some(value))
        })
        .collect()
}

#[test]
fn inventory_while_empty_condition_fabricates_zero_width_one() -> TestResult {
    let mut parser = Parser::new("while () { 42; }\n");
    let ast = parser.parse_with_recovery().ast;
    let fabricated = zero_width_scalars(&ast);
    assert_eq!(
        fabricated.len(),
        1,
        "expected exactly one zero-width scalar fabrication, got {fabricated:?}"
    );
    assert!(
        matches!(fabricated[0].0, NodeKind::Number { ref value } if value == "1"),
        "while () must fabricate Number(\"1\"), got {:?}",
        fabricated[0].0
    );

    let file = lower_source("while () { 42; }\n");
    let ones = number_literals(&file, "1");
    assert_eq!(ones.len(), 1);
    assert_eq!(ones[0].recovery_confidence, RecoveryConfidence::Recovered);
    Ok(())
}

#[test]
fn inventory_orphaned_else_fabricates_zero_width_one() -> TestResult {
    let mut parser = Parser::new("else { 42; }\n");
    let ast = parser.parse_with_recovery().ast;
    let fabricated = zero_width_scalars(&ast);
    assert_eq!(fabricated.len(), 1);
    assert!(
        matches!(fabricated[0].0, NodeKind::Number { ref value } if value == "1"),
        "orphaned else must fabricate Number(\"1\"), got {:?}",
        fabricated[0].0
    );

    let file = lower_source("else { 42; }\n");
    let ones = number_literals(&file, "1");
    assert_eq!(ones.len(), 1);
    assert_eq!(ones[0].recovery_confidence, RecoveryConfidence::Recovered);
    Ok(())
}

#[test]
fn inventory_missing_unary_operand_fabricates_zero_width_undef() -> TestResult {
    let mut parser = Parser::new("!;\n");
    let ast = parser.parse_with_recovery().ast;
    let fabricated = zero_width_scalars(&ast);
    assert_eq!(fabricated.len(), 1);
    assert!(
        matches!(fabricated[0].0, NodeKind::Undef),
        "missing operand must fabricate Undef, got {:?}",
        fabricated[0].0
    );

    let body = root_body("!;\n")?;
    assert!(
        body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Undef && lit.synthesized)),
        "zero-width Undef must be marked synthesized"
    );
    Ok(())
}

#[test]
fn inventory_written_operands_are_never_zero_width_fabrications() -> TestResult {
    for source in ["while (1) { 42; }\n", "if (1) { 42; }\n", "undef;\n"] {
        let mut parser = Parser::new(source);
        let ast = parser.parse_with_recovery().ast;
        assert!(
            zero_width_scalars(&ast).is_empty(),
            "written operand must not be zero-width fabricated, source: {source:?}"
        );
        let file = lower_source(source);
        for item in number_literals(&file, "1") {
            assert_eq!(item.recovery_confidence, RecoveryConfidence::Parsed);
        }
        let body = root_body(source)?;
        for expr in body.exprs.iter() {
            if let HirExpr::Literal(lit) = expr {
                assert!(!lit.synthesized, "written literal must not be synthesized");
            }
        }
    }
    Ok(())
}
