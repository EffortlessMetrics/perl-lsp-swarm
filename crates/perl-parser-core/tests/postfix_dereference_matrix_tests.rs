//! Exact parser and lowering contracts for Perl's non-interpolated postfix
//! dereference family (#13760).
//!
//! Clean parsing is not sufficient evidence here: the neighboring spellings use
//! three AST representations, so each row is pinned to its exact operator,
//! receiver, selector payload, byte span, child traversal, and canonical HIR
//! disposition.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind, Parser, SourceLocation};

type TestResult = Result<(), String>;

const MATRIX_SOURCE: &str = r#"
$sref->$*;
$aref->$#*;
$aref->@*;
$aref->@[0, 2];
$href->@{'alpha', $dynamic_key};
$href->%*;
$href->%{'alpha', $dynamic_key};
$cref->&*;
$gref->**;
"#;

#[derive(Clone, Copy)]
enum ExpectedShape<'a> {
    Unary {
        op: &'a str,
        receiver: &'a str,
    },
    Binary {
        op: &'a str,
        receiver: &'a str,
        selector: &'a str,
    },
    HashSlice {
        receiver: &'a str,
        selector: &'a str,
    },
}

#[derive(Clone, Copy)]
struct MatrixCase<'a> {
    text: &'a str,
    shape: ExpectedShape<'a>,
}

const MATRIX: &[MatrixCase<'static>] = &[
    MatrixCase {
        text: "$sref->$*",
        shape: ExpectedShape::Unary {
            op: "->$*",
            receiver: "$sref",
        },
    },
    MatrixCase {
        text: "$aref->$#*",
        shape: ExpectedShape::Unary {
            op: "->$#*",
            receiver: "$aref",
        },
    },
    MatrixCase {
        text: "$aref->@*",
        shape: ExpectedShape::Unary {
            op: "->@*",
            receiver: "$aref",
        },
    },
    MatrixCase {
        text: "$aref->@[0, 2]",
        shape: ExpectedShape::Binary {
            op: "->@[]",
            receiver: "$aref",
            selector: "0, 2",
        },
    },
    MatrixCase {
        text: "$href->@{'alpha', $dynamic_key}",
        shape: ExpectedShape::HashSlice {
            receiver: "$href",
            selector: "'alpha', $dynamic_key",
        },
    },
    MatrixCase {
        text: "$href->%*",
        shape: ExpectedShape::Unary {
            op: "->%*",
            receiver: "$href",
        },
    },
    MatrixCase {
        text: "$href->%{'alpha', $dynamic_key}",
        shape: ExpectedShape::Binary {
            op: "->%{}",
            receiver: "$href",
            selector: "'alpha', $dynamic_key",
        },
    },
    MatrixCase {
        text: "$cref->&*",
        shape: ExpectedShape::Unary {
            op: "->&*",
            receiver: "$cref",
        },
    },
    MatrixCase {
        text: "$gref->**",
        shape: ExpectedShape::Unary {
            op: "->**",
            receiver: "$gref",
        },
    },
];

fn source_text<'a>(source: &'a str, node: &Node) -> Result<&'a str, String> {
    source
        .get(node.location.start..node.location.end)
        .ok_or_else(|| {
            format!(
                "node span {}..{} is outside source of {} bytes",
                node.location.start,
                node.location.end,
                source.len()
            )
        })
}

fn collect_exact<'a>(
    node: &'a Node,
    source: &str,
    expected: &str,
    found: &mut Vec<&'a Node>,
) {
    if source.get(node.location.start..node.location.end) == Some(expected) {
        found.push(node);
    }
    for child in node.children() {
        collect_exact(child, source, expected, found);
    }
}

fn exact_node<'a>(ast: &'a Node, source: &str, expected: &str) -> Result<&'a Node, String> {
    let mut found = Vec::new();
    collect_exact(ast, source, expected, &mut found);
    if found.len() != 1 {
        return Err(format!(
            "expected one node spanning {expected:?}, found {}\n{}",
            found.len(),
            ast.to_sexp()
        ));
    }
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("the unique node spanning {expected:?} was not retained"))
}

fn assert_variable(node: &Node, expected_text: &str) -> TestResult {
    let expected_name = expected_text
        .strip_prefix('$')
        .ok_or_else(|| format!("expected scalar receiver, got {expected_text:?}"))?;
    if !matches!(
        &node.kind,
        NodeKind::Variable { sigil, name }
            if sigil == "$" && name == expected_name
    ) {
        return Err(format!(
            "expected receiver {expected_text}, got {}",
            node.kind.kind_name()
        ));
    }
    Ok(())
}

fn assert_shape(source: &str, node: &Node, expected: ExpectedShape<'_>) -> TestResult {
    match expected {
        ExpectedShape::Unary { op, receiver } => {
            let NodeKind::Unary {
                op: actual_op,
                operand,
            } = &node.kind
            else {
                return Err(format!(
                    "expected Unary({op}), got {}",
                    node.kind.kind_name()
                ));
            };
            if actual_op != op {
                return Err(format!("expected unary op {op:?}, got {actual_op:?}"));
            }
            if source_text(source, operand)? != receiver {
                return Err(format!(
                    "expected receiver {receiver:?}, got {:?}",
                    source_text(source, operand)?
                ));
            }
            assert_variable(operand, receiver)?;
        }
        ExpectedShape::Binary {
            op,
            receiver,
            selector,
        } => {
            let NodeKind::Binary {
                op: actual_op,
                left,
                right,
            } = &node.kind
            else {
                return Err(format!(
                    "expected Binary({op}), got {}",
                    node.kind.kind_name()
                ));
            };
            if actual_op != op {
                return Err(format!("expected binary op {op:?}, got {actual_op:?}"));
            }
            if source_text(source, left)? != receiver || source_text(source, right)? != selector {
                return Err(format!(
                    "unexpected Binary({op}) children: receiver={:?}, selector={:?}",
                    source_text(source, left)?,
                    source_text(source, right)?
                ));
            }
            assert_variable(left, receiver)?;
        }
        ExpectedShape::HashSlice { receiver, selector } => {
            let NodeKind::HashSlice { target, keys } = &node.kind else {
                return Err(format!(
                    "expected HashSlice, got {}",
                    node.kind.kind_name()
                ));
            };
            if source_text(source, target)? != receiver || source_text(source, keys)? != selector {
                return Err(format!(
                    "unexpected HashSlice children: receiver={:?}, selector={:?}",
                    source_text(source, target)?,
                    source_text(source, keys)?
                ));
            }
            assert_variable(target, receiver)?;
        }
    }
    Ok(())
}

#[test]
fn complete_postfix_dereference_matrix_has_exact_shapes_and_spans() -> TestResult {
    assert_clean_parse(MATRIX_SOURCE);
    let ast = parse(MATRIX_SOURCE);

    for case in MATRIX {
        let node = exact_node(&ast, MATRIX_SOURCE, case.text)?;
        if source_text(MATRIX_SOURCE, node)? != case.text {
            return Err(format!("unexpected full span for {:?}", case.text));
        }
        assert_shape(MATRIX_SOURCE, node, case.shape)?;
    }

    Ok(())
}

#[test]
fn complete_postfix_dereference_matrix_exposes_receiver_and_selector_children() -> TestResult {
    let ast = parse(MATRIX_SOURCE);

    for case in MATRIX {
        let node = exact_node(&ast, MATRIX_SOURCE, case.text)?;
        let child_text = node
            .children()
            .into_iter()
            .map(|child| source_text(MATRIX_SOURCE, child))
            .collect::<Result<Vec<_>, _>>()?;
        let expected = match case.shape {
            ExpectedShape::Unary { receiver, .. } => vec![receiver],
            ExpectedShape::Binary {
                receiver,
                selector,
                ..
            }
            | ExpectedShape::HashSlice { receiver, selector } => vec![receiver, selector],
        };
        if child_text != expected {
            return Err(format!(
                "{} exposed child spans {child_text:?}, expected {expected:?}",
                case.text
            ));
        }
    }

    Ok(())
}

#[test]
fn postfix_dereference_matrix_has_explicit_canonical_hir_dispositions() -> TestResult {
    use perl_parser_core::hir::{BinaryOp, HirExpr, HirExprId, lower_ast};

    let ast = parse(MATRIX_SOURCE);
    let file = lower_ast(&ast);
    let body = file
        .root_body()
        .ok_or_else(|| "lower_ast did not expose a root body".to_string())?;

    for case in MATRIX {
        let ast_node = exact_node(&ast, MATRIX_SOURCE, case.text)?;
        let hir = body
            .source_map
            .expr_ranges
            .iter()
            .enumerate()
            .find(|(_, range)| **range == ast_node.location)
            .and_then(|(index, _)| body.expr(HirExprId(index as u32)))
            .ok_or_else(|| format!("{} has no canonical HIR expression", case.text))?;

        let matches_disposition = match (case.shape, hir) {
            (
                ExpectedShape::Unary { op, .. },
                HirExpr::Unary { op: actual_op, .. },
            ) => actual_op == op,
            (
                ExpectedShape::Binary { op, .. },
                HirExpr::Binary {
                    op: BinaryOp::Other(actual_op),
                    ..
                },
            ) => actual_op == op,
            (
                ExpectedShape::HashSlice { .. },
                HirExpr::Call { ast_kind, args, .. },
            ) => ast_kind == "HashSlice" && args.len() == 3,
            _ => false,
        };
        if !matches_disposition {
            return Err(format!(
                "{} lowered through an unexpected HIR disposition: {hir:?}",
                case.text
            ));
        }
    }

    Ok(())
}

#[test]
fn chained_receiver_and_utf8_selectors_keep_exact_geometry() -> TestResult {
    let source = "my @values = $object->{payload}->@{'naïve', '東京'};";
    let ast = parse(source);
    let slice = exact_node(&ast, source, "$object->{payload}->@{'naïve', '東京'}")?;
    let NodeKind::HashSlice { target, keys } = &slice.kind else {
        return Err(format!(
            "expected HashSlice, got {}",
            slice.kind.kind_name()
        ));
    };
    if source_text(source, target)? != "$object->{payload}"
        || source_text(source, keys)? != "'naïve', '東京'"
    {
        return Err(format!(
            "chained UTF-8 geometry drifted: target={:?}, keys={:?}",
            source_text(source, target)?,
            source_text(source, keys)?
        ));
    }
    if !matches!(&target.kind, NodeKind::Binary { op, .. } if op == "->{}") {
        return Err(format!(
            "chained receiver must retain Binary(->{{}}), got {}",
            target.kind.kind_name()
        ));
    }
    Ok(())
}

#[test]
fn legacy_slices_and_ordinary_arrow_forms_are_not_postfix_dereference_rows() -> TestResult {
    let source = r#"
@hash{'alpha', 'beta'};
%hash{'alpha', 'beta'};
@$href{'alpha', 'beta'};
%$href{'alpha', 'beta'};
$href->{'alpha'};
$aref->[0];
$cref->();
"#;
    let ast = parse(source);
    let mut postfix_rows = Vec::new();
    collect_postfix_rows(&ast, source, &mut postfix_rows)?;
    if !postfix_rows.is_empty() {
        return Err(format!(
            "legacy/ordinary controls were classified as postfix dereference: {postfix_rows:?}"
        ));
    }
    Ok(())
}

fn collect_postfix_rows(node: &Node, source: &str, found: &mut Vec<String>) -> TestResult {
    let is_postfix = match &node.kind {
        NodeKind::Unary { op, .. } => matches!(
            op.as_str(),
            "->$*" | "->$#*" | "->@*" | "->%*" | "->&*" | "->**"
        ),
        NodeKind::Binary { op, .. } => matches!(op.as_str(), "->@[]" | "->%{}"),
        NodeKind::HashSlice { target, .. } => source
            .get(target.location.end..node.location.end)
            .is_some_and(|suffix| suffix.contains("->@{")),
        _ => false,
    };
    if is_postfix {
        found.push(source_text(source, node)?.to_string());
    }
    for child in node.children() {
        collect_postfix_rows(child, source, found)?;
    }
    Ok(())
}

#[test]
fn malformed_postfix_dereference_rows_recover_without_panicking() -> TestResult {
    for source in [
        "$ref->",
        "$ref->$",
        "$ref->$#",
        "$ref->@",
        "$ref->@[0, 2",
        "$ref->@{'alpha'",
        "$ref->%",
        "$ref->%{'alpha'",
        "$ref->&",
        "$ref->*",
    ] {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        if output.diagnostics.is_empty() {
            return Err(format!(
                "malformed row {source:?} retained no recovery diagnostic"
            ));
        }
        if !matches!(output.ast.kind, NodeKind::Program { .. }) {
            return Err(format!(
                "malformed row {source:?} recovered as {}, not Program",
                output.ast.kind.kind_name()
            ));
        }
    }
    Ok(())
}

#[test]
fn exact_source_ranges_remain_byte_based() -> TestResult {
    let source = "'é'; $href->@{'東京'};";
    let ast = parse(source);
    let node = exact_node(&ast, source, "$href->@{'東京'}")?;
    let expected_start = source
        .find("$href")
        .ok_or_else(|| "fixture lost $href marker".to_string())?;
    let expected = SourceLocation {
        start: expected_start,
        end: source.len() - 1,
    };
    if node.location != expected {
        return Err(format!(
            "UTF-8 prefix changed byte geometry: got {:?}, expected {expected:?}",
            node.location
        ));
    }
    Ok(())
}
