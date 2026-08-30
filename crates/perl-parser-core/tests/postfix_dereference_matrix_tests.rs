//! Exact parser and lowering contracts for Perl's non-interpolated postfix
//! dereference family (#13760).
//!
//! The valid matrix uses three AST shapes: `Unary` for star forms, `Binary`
//! for delimiter-bearing array/hash forms, and `HashSlice` for postfix hash
//! slices. Delimiter-bearing rows pin the intended operator-inclusive span;
//! star rows pin current operand-only spans from the production defect tracked
//! in #13891. Recovery rows pin current diagnosed and silent-drop behavior,
//! including the span-containment defect tracked in #14174. This candidate
//! changes no production code.

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

const HASH_SLICE_HIR_ARGS: usize = 1 /* receiver */ + 2 /* selector keys in this fixture row */;

#[derive(Clone, Copy, Debug)]
enum ExpectedShape<'a> {
    Unary { op: &'a str, receiver: &'a str },
    Binary { op: &'a str, receiver: &'a str, selector: &'a str },
    HashSlice { receiver: &'a str, selector: &'a str },
}

#[derive(Clone, Copy, Debug)]
enum RowSpan<'a> {
    /// Operator-inclusive span, as an honest parser contract requires.
    Full(&'a str),
    /// Current behavior: arrow star-forms record only their operand's span
    /// (stale `last_end_position`); tracked in #13891. This pin must become
    /// `Full` in the PR that fixes it.
    OperandOnly(&'a str),
}

#[derive(Clone, Copy)]
struct MatrixCase<'a> {
    text: &'a str,
    shape: ExpectedShape<'a>,
    span: RowSpan<'a>,
}

const MATRIX: &[MatrixCase<'static>] = &[
    MatrixCase {
        text: "$sref->$*",
        shape: ExpectedShape::Unary { op: "->$*", receiver: "$sref" },
        span: RowSpan::OperandOnly("$sref"),
    },
    MatrixCase {
        text: "$aref->$#*",
        shape: ExpectedShape::Unary { op: "->$#*", receiver: "$aref" },
        span: RowSpan::OperandOnly("$aref"),
    },
    MatrixCase {
        text: "$aref->@*",
        shape: ExpectedShape::Unary { op: "->@*", receiver: "$aref" },
        span: RowSpan::OperandOnly("$aref"),
    },
    MatrixCase {
        text: "$aref->@[0, 2]",
        shape: ExpectedShape::Binary { op: "->@[]", receiver: "$aref", selector: "0, 2" },
        span: RowSpan::Full("$aref->@[0, 2]"),
    },
    MatrixCase {
        text: "$href->@{'alpha', $dynamic_key}",
        shape: ExpectedShape::HashSlice { receiver: "$href", selector: "'alpha', $dynamic_key" },
        span: RowSpan::Full("$href->@{'alpha', $dynamic_key}"),
    },
    MatrixCase {
        text: "$href->%*",
        shape: ExpectedShape::Unary { op: "->%*", receiver: "$href" },
        span: RowSpan::OperandOnly("$href"),
    },
    MatrixCase {
        text: "$href->%{'alpha', $dynamic_key}",
        shape: ExpectedShape::Binary {
            op: "->%{}",
            receiver: "$href",
            selector: "'alpha', $dynamic_key",
        },
        span: RowSpan::Full("$href->%{'alpha', $dynamic_key}"),
    },
    MatrixCase {
        text: "$cref->&*",
        shape: ExpectedShape::Unary { op: "->&*", receiver: "$cref" },
        span: RowSpan::OperandOnly("$cref"),
    },
    MatrixCase {
        text: "$gref->**",
        shape: ExpectedShape::Unary { op: "->**", receiver: "$gref" },
        span: RowSpan::OperandOnly("$gref"),
    },
];

fn source_text<'a>(source: &'a str, node: &Node) -> Result<&'a str, String> {
    source.get(node.location.start..node.location.end).ok_or_else(|| {
        format!(
            "node span {}..{} is outside source of {} bytes",
            node.location.start,
            node.location.end,
            source.len()
        )
    })
}

fn collect_where<'a, F>(node: &'a Node, predicate: &F, found: &mut Vec<&'a Node>)
where
    F: Fn(&Node) -> bool,
{
    if predicate(node) {
        found.push(node);
    }
    for child in node.children() {
        collect_where(child, predicate, found);
    }
}

fn unique_node_where<F>(ast: &Node, predicate: F) -> Result<&Node, String>
where
    F: Fn(&Node) -> bool,
{
    let mut found = Vec::new();
    collect_where(ast, &predicate, &mut found);
    if found.len() != 1 {
        return Err(format!(
            "expected exactly one structurally matching node, found {}\n{}",
            found.len(),
            ast.to_sexp()
        ));
    }
    match found.into_iter().next() {
        Some(node) => Ok(node),
        None => Err(format!(
            "the unique structurally matching node was not retained\n{}",
            ast.to_sexp()
        )),
    }
}

fn assert_variable(node: &Node, expected_text: &str) -> TestResult {
    let expected_name = expected_text
        .strip_prefix('$')
        .ok_or_else(|| format!("expected scalar receiver, got {expected_text:?}"))?;
    if !matches!(
        &node.kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == expected_name
    ) {
        return Err(format!(
            "expected receiver {expected_text}, got {}: {}",
            node.kind.kind_name(),
            node.to_sexp()
        ));
    }
    Ok(())
}

fn assert_shape(source: &str, node: &Node, expected: ExpectedShape<'_>) -> TestResult {
    match expected {
        ExpectedShape::Unary { op, receiver } => {
            let NodeKind::Unary { op: actual_op, operand } = &node.kind else {
                return Err(format!(
                    "expected Unary({op}), got {}: {}",
                    node.kind.kind_name(),
                    node.to_sexp()
                ));
            };
            if actual_op != op {
                return Err(format!(
                    "expected unary op {op:?}, got {actual_op:?}: {}",
                    node.to_sexp()
                ));
            }
            if source_text(source, operand)? != receiver {
                return Err(format!(
                    "expected receiver {receiver:?}, got {:?}: {}",
                    source_text(source, operand)?,
                    node.to_sexp()
                ));
            }
            assert_variable(operand, receiver)?;
        }
        ExpectedShape::Binary { op, receiver, selector } => {
            let NodeKind::Binary { op: actual_op, left, right } = &node.kind else {
                return Err(format!(
                    "expected Binary({op}), got {}: {}",
                    node.kind.kind_name(),
                    node.to_sexp()
                ));
            };
            if actual_op != op {
                return Err(format!(
                    "expected binary op {op:?}, got {actual_op:?}: {}",
                    node.to_sexp()
                ));
            }
            if source_text(source, left)? != receiver || source_text(source, right)? != selector {
                return Err(format!(
                    "unexpected Binary({op}) children: receiver={:?}, selector={:?}\n{}",
                    source_text(source, left)?,
                    source_text(source, right)?,
                    node.to_sexp()
                ));
            }
            assert_variable(left, receiver)?;
        }
        ExpectedShape::HashSlice { receiver, selector } => {
            let NodeKind::HashSlice { target, keys } = &node.kind else {
                return Err(format!(
                    "expected HashSlice, got {}: {}",
                    node.kind.kind_name(),
                    node.to_sexp()
                ));
            };
            if source_text(source, target)? != receiver || source_text(source, keys)? != selector {
                return Err(format!(
                    "unexpected HashSlice children: receiver={:?}, selector={:?}\n{}",
                    source_text(source, target)?,
                    source_text(source, keys)?,
                    node.to_sexp()
                ));
            }
            assert_variable(target, receiver)?;
        }
    }
    Ok(())
}

fn assert_span(source: &str, node: &Node, case: MatrixCase<'_>) -> TestResult {
    match (case.span, &case.shape) {
        (RowSpan::Full(expected), _) => {
            if source_text(source, node)? != expected {
                return Err(format!(
                    "{} must keep its full operator-inclusive span, got {:?}\n{}",
                    case.text,
                    source_text(source, node)?,
                    node.to_sexp()
                ));
            }
        }
        (RowSpan::OperandOnly(receiver), ExpectedShape::Unary { .. }) => {
            if source_text(source, node)? != receiver {
                return Err(format!(
                    "{} currently records only receiver text {receiver:?}, got {:?}\n{}",
                    case.text,
                    source_text(source, node)?,
                    node.to_sexp()
                ));
            }
            let NodeKind::Unary { operand, .. } = &node.kind else {
                return Err(format!(
                    "{} span pin reached non-Unary shape: {}",
                    case.text,
                    node.to_sexp()
                ));
            };
            if node.location != operand.location {
                return Err(format!(
                    "{} must currently share operand location {:?}, got {:?}\n{}",
                    case.text,
                    operand.location,
                    node.location,
                    node.to_sexp()
                ));
            }
        }
        (RowSpan::OperandOnly(receiver), shape) => {
            return Err(format!(
                "{} has OperandOnly span pin with incompatible shape {shape:?}",
                receiver
            ));
        }
    }
    Ok(())
}

fn matrix_node<'a>(ast: &'a Node, source: &str, case: MatrixCase<'_>) -> Result<&'a Node, String> {
    match case.shape {
        ExpectedShape::Unary { op, .. } => unique_node_where(
            ast,
            |node| matches!(&node.kind, NodeKind::Unary { op: actual, .. } if actual == op),
        ),
        ExpectedShape::Binary { op, .. } => unique_node_where(
            ast,
            |node| matches!(&node.kind, NodeKind::Binary { op: actual, .. } if actual == op),
        ),
        ExpectedShape::HashSlice { receiver, .. } => unique_node_where(ast, |node| {
            matches!(
                &node.kind,
                NodeKind::HashSlice { target, .. }
                    if source.get(target.location.start..target.location.end) == Some(receiver)
            )
        }),
    }
}

#[test]
fn complete_postfix_dereference_matrix_has_exact_shapes_and_spans() -> TestResult {
    assert_clean_parse(MATRIX_SOURCE);
    let ast = parse(MATRIX_SOURCE);

    for case in MATRIX {
        let node = matrix_node(&ast, MATRIX_SOURCE, *case)?;
        assert_shape(MATRIX_SOURCE, node, case.shape)?;
        assert_span(MATRIX_SOURCE, node, *case)?;
    }

    Ok(())
}

#[test]
fn complete_postfix_dereference_matrix_exposes_receiver_and_selector_children() -> TestResult {
    let ast = parse(MATRIX_SOURCE);

    for case in MATRIX {
        let node = matrix_node(&ast, MATRIX_SOURCE, *case)?;
        let child_text = node
            .children()
            .into_iter()
            .map(|child| source_text(MATRIX_SOURCE, child))
            .collect::<Result<Vec<_>, _>>()?;
        let expected = match case.shape {
            ExpectedShape::Unary { receiver, .. } => vec![receiver],
            ExpectedShape::Binary { receiver, selector, .. }
            | ExpectedShape::HashSlice { receiver, selector } => vec![receiver, selector],
        };
        if child_text != expected {
            return Err(format!(
                "{} exposed child spans {child_text:?}, expected {expected:?}\n{}",
                case.text,
                node.to_sexp()
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
    let body =
        file.root_body().ok_or_else(|| "lower_ast did not expose a root body".to_string())?;

    for case in MATRIX {
        let ast_node = matrix_node(&ast, MATRIX_SOURCE, *case)?;
        let hir = body
            .source_map
            .expr_ranges
            .iter()
            .enumerate()
            .filter(|(_, range)| **range == ast_node.location)
            .filter_map(|(index, _)| body.expr(HirExprId(index as u32)))
            .find(|hir| match case.shape {
                ExpectedShape::Unary { op, .. } => {
                    matches!(hir, HirExpr::Unary { op: actual_op, .. } if actual_op == op)
                }
                ExpectedShape::Binary { op, .. } => matches!(
                    hir,
                    HirExpr::Binary { op: BinaryOp::Other(actual_op), .. } if actual_op == op
                ),
                ExpectedShape::HashSlice { .. } => matches!(hir, HirExpr::Call { .. }),
            })
            .ok_or_else(|| format!("{} has no canonical HIR expression", case.text))?;

        let matches_disposition = match (case.shape, hir) {
            (ExpectedShape::Unary { op, .. }, HirExpr::Unary { op: actual_op, .. }) => {
                actual_op == op
            }
            (
                ExpectedShape::Binary { op, .. },
                HirExpr::Binary { op: BinaryOp::Other(actual_op), .. },
            ) => actual_op == op,
            (ExpectedShape::HashSlice { .. }, HirExpr::Call { ast_kind, args, .. }) => {
                if ast_kind != ast_node.kind.kind_name() {
                    return Err(format!(
                        "{} carried HIR ast_kind {ast_kind:?}, expected {:?}\nAST: {}",
                        case.text,
                        ast_node.kind.kind_name(),
                        ast_node.to_sexp()
                    ));
                }
                if args.len() != HASH_SLICE_HIR_ARGS {
                    return Err(format!(
                        "{} HIR HashSlice args={}, expected {HASH_SLICE_HIR_ARGS}\nAST: {}",
                        case.text,
                        args.len(),
                        ast_node.to_sexp()
                    ));
                }
                let receiver_id = args
                    .first()
                    .ok_or_else(|| format!("{} HIR receiver argument was dropped", case.text))?;
                let receiver_range = body.source_map.expr_range(*receiver_id).ok_or_else(|| {
                    format!("{} HIR receiver argument has no source-map range", case.text)
                })?;
                let NodeKind::HashSlice { target, .. } = &ast_node.kind else {
                    return Err(format!(
                        "{} AST shape changed while checking HIR receiver\n{}",
                        case.text,
                        ast_node.to_sexp()
                    ));
                };
                if receiver_range != target.location {
                    return Err(format!(
                        "{} HIR receiver range {:?} does not equal AST target {:?}\nAST: {}",
                        case.text,
                        receiver_range,
                        target.location,
                        ast_node.to_sexp()
                    ));
                }
                true
            }
            _ => false,
        };
        if !matches_disposition {
            return Err(format!(
                "{} lowered through an unexpected HIR disposition: {hir:?}\nAST: {}",
                case.text,
                ast_node.to_sexp()
            ));
        }
    }

    Ok(())
}

#[test]
fn chained_receiver_and_utf8_selectors_keep_exact_geometry_and_hir_receiver() -> TestResult {
    use perl_parser_core::hir::{HirExpr, HirExprId, lower_ast};

    let source = "my @values = $object->{payload}->@{'naïve', '東京'};";
    let ast = parse(source);
    let slice = unique_node_where(&ast, |node| {
        matches!(
            &node.kind,
            NodeKind::HashSlice { target, .. }
                if source.get(target.location.start..target.location.end)
                    == Some("$object->{payload}")
        )
    })?;
    let NodeKind::HashSlice { target, keys } = &slice.kind else {
        return Err(format!(
            "expected HashSlice, got {}: {}",
            slice.kind.kind_name(),
            slice.to_sexp()
        ));
    };
    if source_text(source, slice)? != "$object->{payload}->@{'naïve', '東京'}" {
        return Err(format!(
            "chained UTF-8 slice span drifted: {:?}\n{}",
            source_text(source, slice)?,
            slice.to_sexp()
        ));
    }
    if source_text(source, target)? != "$object->{payload}"
        || source_text(source, keys)? != "'naïve', '東京'"
    {
        return Err(format!(
            "chained UTF-8 geometry drifted: target={:?}, keys={:?}\n{}",
            source_text(source, target)?,
            source_text(source, keys)?,
            slice.to_sexp()
        ));
    }
    if !matches!(&target.kind, NodeKind::Binary { op, .. } if op == "->{}") {
        return Err(format!(
            "chained receiver must retain Binary(->{{}}), got {}: {}",
            target.kind.kind_name(),
            target.to_sexp()
        ));
    }

    let file = lower_ast(&ast);
    let body =
        file.root_body().ok_or_else(|| "lower_ast did not expose a root body".to_string())?;
    let mut hir_matches = Vec::new();
    for (index, range) in body.source_map.expr_ranges.iter().enumerate() {
        if *range == slice.location {
            let id = HirExprId(index as u32);
            if let Some(expr) = body.expr(id) {
                hir_matches.push((id, expr));
            }
        }
    }
    if hir_matches.len() != 1 {
        return Err(format!(
            "expected one HIR expression for chained slice, found {}\nAST: {}",
            hir_matches.len(),
            slice.to_sexp()
        ));
    }
    let (_, hir) = match hir_matches.into_iter().next() {
        Some(match_item) => match_item,
        None => return Err("the chained slice HIR expression was not retained".to_string()),
    };
    let HirExpr::Call { ast_kind, args, .. } = hir else {
        return Err(format!(
            "chained slice must lower as Call, got {hir:?}\nAST: {}",
            slice.to_sexp()
        ));
    };
    if ast_kind != slice.kind.kind_name() {
        return Err(format!(
            "chained slice carried HIR ast_kind {ast_kind:?}, expected {:?}\nAST: {}",
            slice.kind.kind_name(),
            slice.to_sexp()
        ));
    }
    if args.len() != HASH_SLICE_HIR_ARGS {
        return Err(format!(
            "chained slice HIR args={}, expected {HASH_SLICE_HIR_ARGS}\nAST: {}",
            args.len(),
            slice.to_sexp()
        ));
    }
    let receiver_id = args
        .first()
        .ok_or_else(|| "chained slice HIR receiver argument was dropped".to_string())?;
    let receiver_range = body
        .source_map
        .expr_range(*receiver_id)
        .ok_or_else(|| "chained slice HIR receiver has no source-map range".to_string())?;
    if receiver_range != target.location {
        return Err(format!(
            "chained slice HIR receiver range {:?} does not equal AST target {:?}\nAST: {}",
            receiver_range,
            target.location,
            slice.to_sexp()
        ));
    }
    if !matches!(body.expr(*receiver_id), Some(HirExpr::Subscript(_))) {
        return Err(format!(
            "chained slice receiver must lower as Subscript, got {:?}\nAST: {}",
            body.expr(*receiver_id),
            slice.to_sexp()
        ));
    }
    Ok(())
}

fn collect_postfix_rows(node: &Node, source: &str, found: &mut Vec<String>) -> TestResult {
    let is_postfix = match &node.kind {
        NodeKind::Unary { op, .. } => {
            matches!(op.as_str(), "->$*" | "->$#*" | "->@*" | "->%*" | "->&*" | "->**")
        }
        NodeKind::Binary { op, .. } => matches!(op.as_str(), "->@[]" | "->%{}"),
        NodeKind::HashSlice { target, keys } => {
            // Shared HashSlice/KeyValueSlice nodes carry no postfix marker, so
            // this source-geometry check is unavoidable. A production
            // accessor is outside this test-only candidate's claim boundary
            // (#13760).
            source.get(target.location.end..keys.location.start).map(str::trim) == Some("->@{")
        }
        NodeKind::KeyValueSlice { target, keys } => {
            // Shared HashSlice/KeyValueSlice nodes carry no postfix marker, so
            // this source-geometry check is unavoidable. A production
            // accessor is outside this test-only candidate's claim boundary
            // (#13760).
            source.get(target.location.end..keys.location.start).map(str::trim) == Some("->%{")
        }
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
fn postfix_classifier_reports_matrix_rows_and_rejects_prefix_control() -> TestResult {
    let ast = parse(MATRIX_SOURCE);
    let mut postfix_rows = Vec::new();
    collect_postfix_rows(&ast, MATRIX_SOURCE, &mut postfix_rows)?;
    let expected = MATRIX
        .iter()
        .map(|case| match case.span {
            RowSpan::Full(text) | RowSpan::OperandOnly(text) => text.to_string(),
        })
        .collect::<Vec<_>>();
    if postfix_rows != expected {
        return Err(format!(
            "classifier found {postfix_rows:?}, expected exactly {expected:?}\n{}",
            ast.to_sexp()
        ));
    }

    let postfix_source = "$href->@{'only'};";
    let postfix_ast = parse(postfix_source);
    let mut postfix_only = Vec::new();
    collect_postfix_rows(&postfix_ast, postfix_source, &mut postfix_only)?;
    if postfix_only != vec!["$href->@{'only'}"] {
        return Err(format!(
            "classifier missed postfix-only row: {postfix_only:?}\n{}",
            postfix_ast.to_sexp()
        ));
    }

    let prefix_source = "@{$href}{'only'};";
    let prefix_ast = parse(prefix_source);
    let mut prefix_rows = Vec::new();
    collect_postfix_rows(&prefix_ast, prefix_source, &mut prefix_rows)?;
    if !prefix_rows.is_empty() {
        return Err(format!(
            "classifier misclassified prefix row as postfix: {prefix_rows:?}\n{}",
            prefix_ast.to_sexp()
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
@{$href}{'alpha', 'beta'};
%{$href}{'alpha', 'beta'};
$href->{'alpha'};
$aref->[0];
$cref->();
"#;
    let ast = parse(source);
    let mut postfix_rows = Vec::new();
    collect_postfix_rows(&ast, source, &mut postfix_rows)?;
    if !postfix_rows.is_empty() {
        return Err(format!(
            "legacy/ordinary controls were classified as postfix dereference: {postfix_rows:?}\n{}",
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RecoveryOutcome<'a> {
    /// A recovery diagnostic is retained, as an honest parser should.
    Diagnosed,
    /// Current behavior: the trailing operator is consumed with no
    /// diagnostic at all, leaving `retained` as the whole program text.
    /// Confirmed defect, tracked in #14174; this pin must flip when fixed.
    SilentlyDropsOperator { retained: &'a str },
}

#[derive(Clone, Copy)]
struct RecoveryCase<'a> {
    source: &'a str,
    outcome: RecoveryOutcome<'a>,
}

const RECOVERY_ROWS: &[RecoveryCase<'static>] = &[
    RecoveryCase { source: "$ref->", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase {
        source: "$ref->$",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref" },
    },
    RecoveryCase {
        source: "$ref->$#",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref->$#" },
    },
    RecoveryCase {
        source: "$ref->@",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref" },
    },
    RecoveryCase { source: "$ref->@[0, 2", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase { source: "$ref->@{'alpha'", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase {
        source: "$ref->%",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref" },
    },
    RecoveryCase { source: "$ref->%{'alpha'", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase {
        source: "$ref->&",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref" },
    },
    RecoveryCase {
        source: "$ref->*",
        outcome: RecoveryOutcome::SilentlyDropsOperator { retained: "$ref" },
    },
    // External Perl 5.34 oracle:
    // perl -Mstrict -e "use feature 'postderef'; my \$href={}; my @x = \$href->@{};"
    // prints `syntax error ... near "{}"`; empty selectors are not valid Perl
    // and therefore belong here rather than in the valid matrix.
    RecoveryCase { source: "$href->@{}", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase { source: "$href->%{}", outcome: RecoveryOutcome::Diagnosed },
    RecoveryCase { source: "$aref->@[]", outcome: RecoveryOutcome::Diagnosed },
];

fn assert_retained_ast_text(source: &str, ast: &Node, retained: &str) -> TestResult {
    if retained == "$ref" {
        let retained_node = unique_node_where(ast, |node| {
            source_text(source, node).ok() == Some(retained)
                && matches!(
                    &node.kind,
                    NodeKind::Variable { sigil, name } if sigil == "$" && name == "ref"
                )
        })?;
        if retained_node.location != (SourceLocation { start: 0, end: 4 }) {
            return Err(format!(
                "silent recovery retained {retained:?} at unexpected span {:?}\n{}",
                retained_node.location,
                ast.to_sexp()
            ));
        }
    } else if retained == "$ref->$#" {
        let retained_node = unique_node_where(ast, |node| {
            source_text(source, node).ok() == Some(retained)
                && matches!(&node.kind, NodeKind::MethodCall { .. })
        })?;
        if !matches!(&retained_node.kind, NodeKind::MethodCall { object, .. }
            if matches!(&object.kind, NodeKind::Variable { sigil, name }
                if sigil == "$" && name == "ref"))
        {
            return Err(format!(
                "silent recovery retained {retained:?} without MethodCall($ref, $#, ...): {}\n{}",
                retained_node.kind.kind_name(),
                ast.to_sexp()
            ));
        }
    }
    Ok(())
}

fn assert_surviving_declaration(
    source: &str,
    ast: &Node,
    expected_location: SourceLocation,
) -> TestResult {
    let declaration = unique_node_where(ast, |node| {
        matches!(
            &node.kind,
                NodeKind::VariableDeclaration { variable, .. }
                if source_text(source, node).ok() == Some("my $next = 1")
                    && node.location == expected_location
                    && matches!(&variable.kind, NodeKind::Variable { sigil, name }
                        if sigil == "$" && name == "next")
        )
    })?;
    if source_text(source, declaration)? != "my $next = 1" {
        return Err(format!(
            "surviving declaration text drifted: {:?}\n{}",
            source_text(source, declaration)?,
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn malformed_postfix_dereference_rows_pin_recovery_outcomes() -> TestResult {
    for case in RECOVERY_ROWS {
        let mut parser = Parser::new(case.source);
        let output = parser.parse_with_recovery();
        if !matches!(output.ast.kind, NodeKind::Program { .. }) {
            return Err(format!(
                "malformed row {:?} recovered as {}, not Program\n{}",
                case.source,
                output.ast.kind.kind_name(),
                output.ast.to_sexp()
            ));
        }
        match case.outcome {
            RecoveryOutcome::Diagnosed => {
                if output.diagnostics.is_empty() {
                    return Err(format!(
                        "malformed row {:?} retained no recovery diagnostic\n{}",
                        case.source,
                        output.ast.to_sexp()
                    ));
                }
            }
            RecoveryOutcome::SilentlyDropsOperator { retained } => {
                if !output.diagnostics.is_empty() {
                    return Err(format!(
                        "silent-drop row {:?} gained diagnostics {:?}\n{}",
                        case.source,
                        output.diagnostics,
                        output.ast.to_sexp()
                    ));
                }
                assert_retained_ast_text(case.source, &output.ast, retained)?;
            }
        }
    }

    let next_source = "$ref->$;\nmy $next = 1;\n";
    let mut next_parser = Parser::new(next_source);
    let next_output = next_parser.parse_with_recovery();
    if next_output.diagnostics.len() != 1 {
        return Err(format!(
            "following-statement row {:?} diagnostics={}, expected 1\n{}",
            next_source,
            next_output.diagnostics.len(),
            next_output.ast.to_sexp()
        ));
    }
    let _recovered_call = unique_node_where(&next_output.ast, |node| {
        node.location == (SourceLocation { start: 0, end: 8 })
            && matches!(&node.kind, NodeKind::MethodCall { .. })
    })?;
    assert_surviving_declaration(
        next_source,
        &next_output.ast,
        SourceLocation { start: 9, end: 21 },
    )?;

    let empty_source = "$href->@{};\nmy $next = 1;\n";
    let mut empty_parser = Parser::new(empty_source);
    let empty_output = empty_parser.parse_with_recovery();
    if empty_output.diagnostics.len() != 2 {
        return Err(format!(
            "empty-selector following-statement row diagnostics={}, expected 2\n{}",
            empty_output.diagnostics.len(),
            empty_output.ast.to_sexp()
        ));
    }
    assert_surviving_declaration(
        empty_source,
        &empty_output.ast,
        SourceLocation { start: 12, end: 24 },
    )?;
    Ok(())
}

#[test]
fn malformed_hash_slice_recovery_pins_span_violation_and_following_statement() -> TestResult {
    let source = "$ref->@{'alpha';\nmy $next = 1;\n";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if output.diagnostics.len() != 1 {
        return Err(format!(
            "malformed hash-slice row diagnostics={}, expected 1\n{}",
            output.diagnostics.len(),
            output.ast.to_sexp()
        ));
    }
    if !matches!(output.ast.kind, NodeKind::Program { .. }) {
        return Err(format!(
            "malformed hash-slice row root is {}, not Program\n{}",
            output.ast.kind.kind_name(),
            output.ast.to_sexp()
        ));
    }
    let slice =
        unique_node_where(&output.ast, |node| matches!(&node.kind, NodeKind::HashSlice { .. }))?;
    if slice.location != (SourceLocation { start: 0, end: 4 }) {
        return Err(format!(
            "current #14174 HashSlice span changed: got {:?}, expected 0..4\n{}",
            slice.location,
            output.ast.to_sexp()
        ));
    }
    let string = unique_node_where(slice, |node| {
        matches!(&node.kind, NodeKind::String { .. })
            && node.location == SourceLocation { start: 8, end: 15 }
    })?;
    let direct_children = slice.children();
    if !direct_children.iter().any(|child| std::ptr::eq(*child, string)) {
        return Err(format!(
            "current #14174 String child is not direct child of recovered HashSlice\n{}",
            output.ast.to_sexp()
        ));
    }
    if slice.location.start <= string.location.start && string.location.end <= slice.location.end {
        return Err(format!(
            "current #14174 span-containment violation disappeared: slice={:?}, child={:?}\n{}",
            slice.location,
            string.location,
            output.ast.to_sexp()
        ));
    }
    assert_surviving_declaration(source, &output.ast, SourceLocation { start: 17, end: 29 })?;
    Ok(())
}

#[test]
fn exact_source_ranges_remain_byte_based() -> TestResult {
    let source = "'é'; $href->@{'東京'};";
    let ast = parse(source);
    let node =
        unique_node_where(&ast, |candidate| matches!(&candidate.kind, NodeKind::HashSlice { .. }))?;
    let expected_start =
        source.find("$href").ok_or_else(|| "fixture lost $href marker".to_string())?;
    let expected = SourceLocation { start: expected_start, end: source.len() - 1 };
    if node.location != expected {
        return Err(format!(
            "UTF-8 prefix changed byte geometry: got {:?}, expected {expected:?}\n{}",
            node.location,
            ast.to_sexp()
        ));
    }
    if node.location != (SourceLocation { start: 6, end: 24 }) {
        return Err(format!(
            "UTF-8 postfix HashSlice location changed: got {:?}, expected 6..24\n{}",
            node.location,
            ast.to_sexp()
        ));
    }
    Ok(())
}
