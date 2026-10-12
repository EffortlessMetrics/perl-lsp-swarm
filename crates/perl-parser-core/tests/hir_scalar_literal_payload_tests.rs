//! First-cohort scalar literal payloads through canonical body and PIR handoff (#17538).
//!
//! Admitted families: plain decimal numeric tokens, noninterpolating string forms,
//! and `undef`. Each leaf retains its exact source-derived payload plus a
//! fabrication marker; the PIR-A handoff resolves the same payload by an
//! operation-carried fact in the same lowering generation. Nothing here executes
//! Perl, folds constants, or claims EIR/profile support.

use std::error::Error;

use perl_parser_core::hir::{
    HirBody, HirExpr, HirLiteralKind, HirQuoteKind, HirStmt, lower_ast, lower_body,
};
use perl_parser_core::pir::{PirGraph, PirLiteralKind, PirOperation, lower_hir_bodies};
use perl_parser_core::{NodeKind, Parser};

type TestResult = Result<(), Box<dyn Error>>;

fn root_body(source: &str) -> Result<HirBody, Box<dyn Error>> {
    let output = Parser::new(source).parse_with_recovery();
    let body = lower_body(&output.ast);
    let file = lower_ast(&output.ast);
    let canonical = file.root_body().ok_or("canonical program root is missing")?;
    assert_eq!(&body, canonical, "public entry diverged for {source:?}");
    Ok(body)
}

fn first_expression(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or("root block is missing")?;
    let id = root.stmts.first().ok_or("root statement is missing")?;
    let HirStmt::Expr(expr) = body.stmt(*id).ok_or("statement is missing")? else {
        return Err("expected an expression statement".into());
    };
    body.expr(*expr).ok_or_else(|| "expression is missing".into())
}

fn literal_count(body: &HirBody) -> usize {
    body.exprs.iter().filter(|expr| matches!(expr, HirExpr::Literal(_))).count()
}

fn pir_literals(
    graph: &PirGraph,
) -> Vec<(PirLiteralKind, Option<String>, Option<HirQuoteKind>, bool)> {
    graph
        .nodes
        .iter()
        .filter_map(|node| match &node.operation {
            PirOperation::Literal { kind, value, quote, synthesized } => {
                Some((*kind, value.clone(), *quote, *synthesized))
            }
            _ => None,
        })
        .collect()
}

fn pir_graph(source: &str) -> PirGraph {
    let output = Parser::new(source).parse_with_recovery();
    let file = lower_ast(&output.ast);
    lower_hir_bodies(&file)
}

#[test]
fn leading_zero_lexemes_stay_opaque() -> TestResult {
    // Perl reads multi-digit leading-zero forms as octal (`010` is 8), so
    // they are outside the plain-decimal cohort and must stay Opaque.
    for source in ["my $x = 007;", "my $x = 010;"] {
        let body = root_body(source)?;
        assert_eq!(literal_count(&body), 0, "{source:?} must emit no leaf");
        assert!(
            body.exprs
                .iter()
                .any(|expr| matches!(expr, HirExpr::Opaque { ast_kind } if ast_kind == "Number")),
            "{source:?} must retain the Opaque fallback"
        );
    }
    Ok(())
}

#[test]
fn same_width_values_stay_distinct() -> TestResult {
    for (source, payload) in [("0;", "0"), ("1;", "1"), ("2;", "2"), ("17;", "17"), ("19;", "19")] {
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
        assert_eq!(lit.kind, HirLiteralKind::String(HirQuoteKind::Single));
        assert_eq!(lit.payload, payload);
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
    assert_eq!(quoted.kind, HirLiteralKind::String(HirQuoteKind::Single));
    assert_eq!(quoted.payload, "42");
    assert!(!quoted.synthesized);
    let plain_body = root_body("42;")?;
    let HirExpr::Literal(plain) = first_expression(&plain_body)? else {
        return Err("plain 42 did not lower to a literal leaf".into());
    };
    assert_eq!(plain.kind, HirLiteralKind::Number);
    assert_eq!(plain.payload, "42");
    assert!(!plain.synthesized);
    Ok(())
}

#[test]
fn undef_leaf_is_not_a_bareword() -> TestResult {
    // Bare `undef;` parses as a function call (call resolution owns that shape);
    // `NodeKind::Undef` appears in value positions such as initializers.
    let undef_body = root_body("my $u = undef;")?;
    assert_eq!(literal_count(&undef_body), 1, "exactly one leaf expected");
    let found = undef_body.exprs.iter().any(|expr| {
        matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Undef && lit.payload.is_empty() && !lit.synthesized)
    });
    assert!(found, "undef initializer did not lower to a literal leaf");
    let bareword_body = root_body("foo;")?;
    assert!(
        !bareword_body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(_))),
        "bareword must not produce a literal leaf anywhere in the arena"
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
    // The sub body is a separate arena from the program root: `3` must live
    // in a non-root body and `4` in the root, not merely somewhere.
    let output = Parser::new("sub f { return 3; } f(4);").parse_with_recovery();
    let file = lower_ast(&output.ast);
    let root = file.root_body().ok_or("canonical program root is missing")?;
    let has_leaf = |body: &HirBody, payload: &str| {
        body.exprs.iter().any(|expr| {
            matches!(expr, HirExpr::Literal(lit)
                if lit.kind == HirLiteralKind::Number && lit.payload == payload)
        })
    };
    assert!(has_leaf(root, "4"), "root body must hold the argument leaf");
    assert!(!has_leaf(root, "3"), "return leaf must not leak into the root");
    assert!(
        file.bodies.iter().any(|body| body != root && has_leaf(body, "3")),
        "a non-root body must hold the return leaf"
    );
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
    assert_eq!(lit.kind, HirLiteralKind::String(HirQuoteKind::Single));
    assert_eq!(lit.payload, "a\\nb");
    assert_eq!(lit.payload.len(), 4);
    assert!(!lit.synthesized);
    Ok(())
}

#[test]
fn pir_handoff_resolves_exact_facts() -> TestResult {
    let graph = pir_graph("my $x = 42; 'hi';");
    let literals = pir_literals(&graph);
    for expected in [
        (PirLiteralKind::Number, Some("42".to_string()), None, false),
        (PirLiteralKind::String, Some("hi".to_string()), Some(HirQuoteKind::Single), false),
    ] {
        assert!(
            literals.contains(&expected),
            "missing exact PIR fact {expected:?} in {literals:?}"
        );
    }
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
fn valid_idiom_desugaring_is_marked_supplied() -> TestResult {
    // `while ()` is valid Perl with no diagnostic, but its empty condition is
    // still absent from the source text: the marker means "not written", not
    // "error recovery".
    let body = root_body("while () { last; }")?;
    assert!(
        body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(lit)
            if lit.kind == HirLiteralKind::Number && lit.synthesized)),
        "while-empty-condition leaf must be marked supplied"
    );
    Ok(())
}

#[test]
fn missing_operands_produce_flagged_undef_leaves() -> TestResult {
    // Unary recovery fabricates zero-width `Undef` operands (`!;`): they must
    // be flagged exactly like fabricated numbers, never labeled written.
    for source in ["!;", "my $x = !;"] {
        let body = root_body(source)?;
        assert!(
            body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(lit)
                if lit.kind == HirLiteralKind::Undef && lit.synthesized)),
            "{source:?} must produce a flagged Undef leaf"
        );
    }
    Ok(())
}

#[test]
fn pir_handoff_propagates_provenance() -> TestResult {
    let fabricated = pir_graph("else { 42; }");
    assert!(
        pir_literals(&fabricated)
            .iter()
            .any(|(kind, value, _, synthesized)| *kind == PirLiteralKind::Number
                && *value == Some("1".to_string())
                && *synthesized),
        "fabricated condition must stay flagged through PIR"
    );
    let written = pir_graph("if (1) { }");
    assert!(
        !pir_literals(&written).iter().any(|(_, _, _, synthesized)| *synthesized),
        "written constants must stay unflagged through PIR"
    );
    Ok(())
}

#[test]
fn unsupported_forms_stay_outside_the_leaf() -> TestResult {
    // Plain `"x"` is deliberately outside too: the parser marks every
    // double-quoted form interpolated, and this slice does not second-guess
    // that authority by scanning for markers.
    for source in ["\"interp $x\";", "\"x\";", "0x1F;", "1.5;", "1e3;", "1_000;", "v1.2.3;"] {
        let body = root_body(source)?;
        assert!(
            !body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(_))),
            "{source:?} must not produce a literal leaf anywhere in the arena"
        );
    }
    Ok(())
}

#[test]
fn legacy_fragments_keep_opaque_literals() -> TestResult {
    // Only the canonical constructor emits leaves: a non-Program fragment
    // takes the legacy path where the same literal stays Opaque.
    let output = Parser::new("my $x = 42;").parse_with_recovery();
    let NodeKind::Program { statements } = &output.ast.kind else {
        return Err("parser did not produce Program".into());
    };
    let fragment = statements.first().ok_or("missing declaration fragment")?;
    assert!(!matches!(&fragment.kind, NodeKind::Program { .. }));
    let body = lower_body(fragment);
    assert!(
        !body.exprs.iter().any(|expr| matches!(expr, HirExpr::Literal(_))),
        "legacy fragment must not emit literal leaves"
    );
    assert!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr, HirExpr::Opaque { ast_kind } if ast_kind == "Number")),
        "legacy fragment must retain the Opaque fallback"
    );
    Ok(())
}

#[test]
fn flat_path_shares_value_convention() -> TestResult {
    use perl_parser_core::pir::lower_hir;
    // The legacy flat path strips quotable token text to content exactly like
    // the body path; quote detection applies regardless of interpolation.
    let output = Parser::new("'x'; \"y\";").parse_with_recovery();
    let file = lower_ast(&output.ast);
    let graph = lower_hir(&file);
    for expected in [
        (PirLiteralKind::String, Some("x".to_string()), Some(HirQuoteKind::Single), false),
        (PirLiteralKind::String, Some("y".to_string()), Some(HirQuoteKind::Double), false),
    ] {
        assert!(pir_literals(&graph).contains(&expected), "missing flat-path fact {expected:?}");
    }
    Ok(())
}

#[test]
fn every_body_leaf_has_one_matching_pir_operation() -> TestResult {
    let source = "my $a = 7; my $b = 's'; my $c = undef;";
    let output = Parser::new(source).parse_with_recovery();
    let file = lower_ast(&output.ast);
    let graph = lower_hir_bodies(&file);
    let mut expected: Vec<(PirLiteralKind, Option<String>, Option<HirQuoteKind>, bool)> = file
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| match expr {
            HirExpr::Literal(lit) => Some(match lit.kind {
                HirLiteralKind::Number => {
                    (PirLiteralKind::Number, Some(lit.payload.clone()), None, lit.synthesized)
                }
                HirLiteralKind::String(quote) => (
                    PirLiteralKind::String,
                    Some(lit.payload.clone()),
                    Some(quote),
                    lit.synthesized,
                ),
                HirLiteralKind::Undef => (PirLiteralKind::Undef, None, None, lit.synthesized),
                // Future kinds map explicitly when admitted; unknown kinds
                // must not silently claim a scalar payload.
                _ => (PirLiteralKind::Undef, None, None, lit.synthesized),
            }),
            _ => None,
        })
        .collect();
    // A total leaf drop must fail here, not pass vacuously via `[] == []`.
    assert!(!expected.is_empty(), "expected leaves are missing from the fixture");
    let order = |facts: &mut [(PirLiteralKind, Option<String>, Option<HirQuoteKind>, bool)]| {
        facts.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    };
    order(&mut expected);
    let mut operations = pir_literals(&graph);
    order(&mut operations);
    assert_eq!(operations, expected, "leaf/operation fact join mismatch");
    Ok(())
}
