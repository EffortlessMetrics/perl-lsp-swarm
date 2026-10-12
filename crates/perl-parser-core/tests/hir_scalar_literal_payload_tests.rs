//! First-cohort scalar literal payloads through canonical body and PIR handoff (#17538).
//!
//! Admitted families: plain decimal numeric tokens, noninterpolating string forms,
//! and `undef`. Each leaf retains its exact source-derived payload plus a
//! fabrication marker; the PIR-A handoff resolves the same payload by an
//! operation-carried fact in the same lowering generation. Nothing here executes
//! Perl, folds constants, or claims EIR/profile support.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    HirBody, HirExpr, HirLiteralKind, HirQuoteKind, HirStmt, lower_ast, lower_body,
};
use perl_parser_core::pir::{PirGraph, PirOperation, lower_hir_bodies};

type TestResult = Result<(), Box<dyn Error>>;

fn root_body(source: &str) -> Result<HirBody, Box<dyn Error>> {
    let output = Parser::new(source).parse_with_recovery();
    let body = lower_body(&output.ast);
    let file = lower_ast(&output.ast);
    let canonical = file.root_body().ok_or("canonical program root is missing")?;
    assert_eq!(&body, canonical, "public entry diverged for {source:?}");
    Ok(body)
}

fn pir_graph(source: &str) -> PirGraph {
    let output = Parser::new(source).parse_with_recovery();
    let file = lower_ast(&output.ast);
    lower_hir_bodies(&file)
}

fn first_expression(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or("root block is missing")?;
    let id = root.stmts.first().ok_or("root statement is missing")?;
    let HirStmt::Expr(expr) = body.stmt(*id).ok_or("statement is missing")? else {
        return Err("expected an expression statement".into());
    };
    body.expr(*expr).ok_or_else(|| "expression is missing".into())
}

fn literal_payloads(graph: &PirGraph) -> Vec<Option<String>> {
    graph
        .nodes
        .iter()
        .filter_map(|node| match &node.operation {
            PirOperation::Literal { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn decimal_leaf_preserves_exact_lexeme() -> TestResult {
    let body = root_body("my $x = 007;")?;
    let found = body.exprs.iter().any(|expr| {
        matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Number && lit.payload == "007" && !lit.synthesized)
    });
    assert!(found, "expected a Number leaf with payload \"007\"");
    Ok(())
}

#[test]
fn same_width_values_stay_distinct() -> TestResult {
    for (source, payload) in [("1;", "1"), ("2;", "2"), ("17;", "17"), ("19;", "19")] {
        let body = root_body(source)?;
        let HirExpr::Literal(lit) = first_expression(&body)? else {
            return Err(format!("{source:?} did not lower to a literal leaf").into());
        };
        assert_eq!(lit.kind, HirLiteralKind::Number);
        assert_eq!(lit.payload, payload);
        assert!(!lit.synthesized);
    }
    Ok(())
}

#[test]
fn empty_string_zero_and_double_zero_stay_distinct() -> TestResult {
    for (source, payload) in [("'';", ""), ("'0';", "0"), ("'00';", "00")] {
        let body = root_body(source)?;
        let HirExpr::Literal(lit) = first_expression(&body)? else {
            return Err(format!("{source:?} did not lower to a literal leaf").into());
        };
        assert_eq!(lit.kind, HirLiteralKind::String);
        assert_eq!(lit.payload, payload);
        assert_eq!(lit.quote, Some(HirQuoteKind::Single));
        assert!(!lit.synthesized);
    }
    Ok(())
}

#[test]
fn quoted_number_is_a_string_not_a_number() -> TestResult {
    let quoted_body = root_body("'42';")?;
    let HirExpr::Literal(quoted) = first_expression(&quoted_body)? else {
        return Err("quoted 42 did not lower to a literal leaf".into());
    };
    assert_eq!(quoted.kind, HirLiteralKind::String);
    assert_eq!(quoted.payload, "42");
    assert_eq!(quoted.quote, Some(HirQuoteKind::Single));
    let plain_body = root_body("42;")?;
    let HirExpr::Literal(plain) = first_expression(&plain_body)? else {
        return Err("plain 42 did not lower to a literal leaf".into());
    };
    assert_eq!(plain.kind, HirLiteralKind::Number);
    assert_eq!(plain.payload, "42");
    Ok(())
}

#[test]
fn undef_leaf_is_not_a_bareword() -> TestResult {
    // Bare `undef;` parses as a function call (call resolution owns that shape);
    // `NodeKind::Undef` appears in value positions such as initializers.
    let undef_body = root_body("my $u = undef;")?;
    let found = undef_body.exprs.iter().any(|expr| {
        matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Undef && lit.payload.is_empty() && !lit.synthesized)
    });
    assert!(found, "undef initializer did not lower to a literal leaf");
    let bareword_body = root_body("foo;")?;
    assert!(
        !matches!(first_expression(&bareword_body)?, HirExpr::Literal(_)),
        "bareword must not become a literal leaf"
    );
    Ok(())
}

#[test]
fn repeated_literals_are_not_deduplicated() -> TestResult {
    let body = root_body("1; 1;")?;
    let ones = body
        .exprs
        .iter()
        .filter(|expr| {
            matches!(expr, HirExpr::Literal(lit)
                if lit.kind == HirLiteralKind::Number && lit.payload == "1")
        })
        .count();
    assert_eq!(ones, 2, "each occurrence needs its own leaf");
    Ok(())
}

#[test]
fn arithmetic_keeps_literal_operands() -> TestResult {
    let body = root_body("1 + 2;")?;
    let HirExpr::Binary { lhs, rhs, .. } = first_expression(&body)? else {
        return Err("addition lost its binary structure".into());
    };
    for (id, payload) in [(*lhs, "1"), (*rhs, "2")] {
        let expr = body.expr(id).ok_or("operand is missing")?;
        assert!(
            matches!(expr, HirExpr::Literal(lit)
                if lit.kind == HirLiteralKind::Number && lit.payload == payload),
            "operand is not the {payload} leaf"
        );
    }
    Ok(())
}

#[test]
fn return_values_and_call_arguments_keep_leaves() -> TestResult {
    // The sub body is a separate arena from the program root; both positions
    // must keep exact leaves.
    let output = Parser::new("sub f { return 3; } f(4);").parse_with_recovery();
    let file = lower_ast(&output.ast);
    for payload in ["3", "4"] {
        assert!(
            file.bodies
                .iter()
                .flat_map(|body| body.exprs.iter())
                .any(|expr| matches!(expr, HirExpr::Literal(lit)
                if lit.kind == HirLiteralKind::Number && lit.payload == *payload)),
            "missing {payload} leaf in return/argument position"
        );
    }
    Ok(())
}

#[test]
fn escape_sequences_stay_verbatim() -> TestResult {
    // Single-quoted escapes pass through undecoded: the leaf records what the
    // parser produced, and decoding stays a later family. Backslash-n here is
    // two characters, not a newline.
    let body = root_body("'a\\nb';")?;
    let HirExpr::Literal(lit) = first_expression(&body)? else {
        return Err("escaped string did not lower to a literal leaf".into());
    };
    assert_eq!(lit.kind, HirLiteralKind::String);
    assert_eq!(lit.payload, "a\\nb");
    assert_eq!(lit.payload.len(), 4);
    assert_eq!(lit.quote, Some(HirQuoteKind::Single));
    Ok(())
}

#[test]
fn pir_handoff_resolves_exact_payloads() -> TestResult {
    let graph = pir_graph("my $x = 42; 'hi';");
    let payloads = literal_payloads(&graph);
    assert!(payloads.contains(&Some("42".to_string())), "missing 42 payload: {payloads:?}");
    assert!(payloads.contains(&Some("hi".to_string())), "missing hi payload: {payloads:?}");
    Ok(())
}

#[test]
fn fabricated_condition_is_flagged_written_constant_is_not() -> TestResult {
    let body = root_body("else { 42; }")?;
    let flagged = body.exprs.iter().any(|expr| {
        matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Number && lit.payload == "1" && lit.synthesized)
    });
    assert!(flagged, "orphaned-else fabrication needs a flagged leaf");
    let written = root_body("if (1) { }")?;
    assert!(
        !written.exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Literal(lit) if lit.synthesized
        )),
        "written constants must not be flagged"
    );
    Ok(())
}

#[test]
fn unsupported_forms_stay_outside_the_leaf() -> TestResult {
    // Plain `"x"` is deliberately outside too: the parser marks every
    // double-quoted form interpolated, and this slice does not second-guess
    // that authority by scanning for markers.
    for source in ["\"interp $x\";", "\"x\";", "0x1F;", "1.5;", "v1.2.3;"] {
        let body = root_body(source)?;
        assert!(
            !matches!(first_expression(&body)?, HirExpr::Literal(_)),
            "{source:?} is outside the admitted cohort"
        );
    }
    Ok(())
}

#[test]
fn every_body_leaf_has_one_matching_pir_operation() -> TestResult {
    let source = "my $a = 7; my $b = 's'; my $c = undef;";
    let output = Parser::new(source).parse_with_recovery();
    let file = lower_ast(&output.ast);
    let graph = lower_hir_bodies(&file);
    let mut expected: Vec<Option<String>> = file
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| match expr {
            HirExpr::Literal(lit) => Some(match lit.kind {
                HirLiteralKind::Undef => None,
                HirLiteralKind::Number | HirLiteralKind::String => Some(lit.payload.clone()),
                // Future kinds map explicitly when admitted; unknown kinds
                // must not silently claim a scalar payload.
                _ => None,
            }),
            _ => None,
        })
        .collect();
    expected.sort();
    let mut op_payloads = literal_payloads(&graph);
    op_payloads.sort();
    assert_eq!(op_payloads, expected, "leaf/operation payload join mismatch");
    Ok(())
}
