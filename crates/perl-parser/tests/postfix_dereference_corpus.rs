//! Public-parser and governed-corpus proof for the complete postfix-dereference
//! family (#13763).
//!
//! The proof binds every matrix row to its exact public AST identity, payload,
//! receiver, and source span, and refuses first-substring binding through
//! whole-AST uniqueness and a repeated-marker control. Nearby generic forms
//! (ordinary and prefix slices, arrow element access) are pinned as controls so
//! they can never satisfy a dedicated postfix expectation.
//!
//! Span contracts are recorded per row. Slice forms span their full text. The
//! arrow star-form Unary nodes currently span exactly their operand because the
//! postfix parser reads a stale `last_end_position` for them; that defect is
//! pinned here as evidence and stays owned by a bounded parser-fix child, not
//! by this corpus claim.

use perl_parser::{Node, NodeKind, Parser};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// How the public AST spans a matrix row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanContract {
    /// The node spans the operator and its operand.
    Full,
    /// Current public contract: arrow star-form Unary nodes span exactly their
    /// operand, excluding the `->X*` operator. Pinned as evidence for the
    /// bounded parser-fix child; do not copy this expectation into new proofs.
    OperandOnly,
}

#[derive(Clone, Copy)]
enum ExpectedShape<'a> {
    Unary { op: &'a str, receiver: &'a str },
    Binary { op: &'a str, receiver: &'a str, selector: &'a str },
    HashSlice { receiver: &'a str, selector: &'a str },
}

#[derive(Clone, Copy)]
struct MatrixCase<'a> {
    text: &'a str,
    span: SpanContract,
    shape: ExpectedShape<'a>,
}

const MATRIX: &[MatrixCase<'static>] = &[
    MatrixCase {
        text: "$sref->$*",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->$*", receiver: "$sref" },
    },
    MatrixCase {
        text: "$aref->$#*",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->$#*", receiver: "$aref" },
    },
    MatrixCase {
        text: "$aref->@*",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->@*", receiver: "$aref" },
    },
    MatrixCase {
        text: "$aref->@[0, 2]",
        span: SpanContract::Full,
        shape: ExpectedShape::Binary { op: "->@[]", receiver: "$aref", selector: "0, 2" },
    },
    MatrixCase {
        text: "$href->@{'alpha', 'beta'}",
        span: SpanContract::Full,
        shape: ExpectedShape::HashSlice { receiver: "$href", selector: "'alpha', 'beta'" },
    },
    MatrixCase {
        text: "$href->%*",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->%*", receiver: "$href" },
    },
    MatrixCase {
        text: "$href->%{qw(alpha beta)}",
        span: SpanContract::Full,
        shape: ExpectedShape::Binary { op: "->%{}", receiver: "$href", selector: "qw(alpha beta)" },
    },
    MatrixCase {
        text: "$cref->&*",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->&*", receiver: "$cref" },
    },
    MatrixCase {
        text: "$gref->**",
        span: SpanContract::OperandOnly,
        shape: ExpectedShape::Unary { op: "->**", receiver: "$gref" },
    },
];

/// A repeated marker: the same postfix text must bind twice, never once.
const REPEATED_MARKER: &str = "$href->@{'beta', '東京'}";

#[test]
fn project_fixture_is_discovered_and_emits_the_exact_postfix_matrix() -> TestResult {
    let explicit_root = fs::canonicalize(workspace_root())?;
    let resolved_paths = perl_corpus::CorpusPaths::try_from_root(&explicit_root)
        .map_err(|error| format!("explicit corpus root failed validation: {error:?}"))?;
    assert!(
        matches!(resolved_paths.root_source(), perl_corpus::CorpusRootSource::Explicit),
        "fixture discovery must keep explicit root authority"
    );
    let fixture_path =
        resolved_paths.root_authority().path().join("test_corpus/postfix_dereference_matrix.pl");
    let source = fs::read_to_string(&fixture_path)?;

    assert!(
        perl_corpus::files::get_test_files_from(resolved_paths.as_paths()).contains(&fixture_path),
        "the exact checkout fixture must belong to the explicitly rooted project corpus"
    );

    let ast = parse_clean(&source)?;
    for case in MATRIX {
        let node = exact_matrix_node(&ast, &source, case)?;
        assert_shape(node, &source, case.shape)?;
        pin_span_contract(node, &source, case)?;
        let expected_children = match case.shape {
            ExpectedShape::Unary { receiver, .. } => vec![receiver],
            ExpectedShape::Binary { receiver, selector, .. }
            | ExpectedShape::HashSlice { receiver, selector } => vec![receiver, selector],
        };
        let actual_children = node
            .children()
            .into_iter()
            .map(|child| node_source(child, &source))
            .collect::<Option<Vec<_>>>()
            .ok_or("a matrix child had an out-of-bounds source range")?;
        assert_eq!(actual_children, expected_children, "{} child traversal drifted", case.text);
    }

    assert_chained_unicode_hash_slice(&ast, &source)?;
    assert_slice_lvalues(&ast, &source)?;
    assert_repeated_hash_slice(&ast, &source)?;
    assert_generic_slice_controls(&ast, &source)?;
    Ok(())
}

#[test]
fn matrix_geometry_is_byte_exact_under_crlf_source() -> TestResult {
    let fixture_path = workspace_root().join("test_corpus/postfix_dereference_matrix.pl");
    let source = fs::read_to_string(fixture_path)?.replace('\n', "\r\n");
    let ast = parse_clean(&source)?;

    for case in MATRIX {
        let node = exact_matrix_node(&ast, &source, case)?;
        if case.span == SpanContract::Full {
            assert_eq!(node_source(node, &source), Some(case.text));
        }
        // OperandOnly rows prove their geometry through the exact receiver span
        // consumed by the binding matcher plus the pinned operand contract.
        pin_span_contract(node, &source, case)?;
    }

    let repeated = collect_all_spanning(&ast, &source, REPEATED_MARKER);
    assert_eq!(repeated.len(), 2, "the repeated marker must stay byte-exact twice");
    let chained = exact_node(&ast, &source, "$object->{payload}->@{'東京', 'alpha'}")?;
    assert_eq!(node_source(chained, &source), Some("$object->{payload}->@{'東京', 'alpha'}"));
    Ok(())
}

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if !output.diagnostics.is_empty() {
        return Err(format!(
            "postfix corpus retained {} diagnostic(s): {:?}",
            output.diagnostics.len(),
            output.diagnostics
        ));
    }
    Ok(output.ast)
}

/// Bind exactly one public AST node for a matrix row.
///
/// Slice rows bind by their full source text. Star rows bind by operator
/// variant plus the exact receiver span, because the current public span of an
/// arrow star-form Unary node excludes the operator.
fn exact_matrix_node<'a>(
    ast: &'a Node,
    source: &str,
    case: &MatrixCase<'_>,
) -> Result<&'a Node, String> {
    let found = match case.shape {
        ExpectedShape::Unary { op, receiver } => {
            let mut found = Vec::new();
            collect_star_unary(ast, source, op, receiver, &mut found);
            found
        }
        ExpectedShape::Binary { .. } | ExpectedShape::HashSlice { .. } => {
            collect_all_spanning(ast, source, case.text)
        }
    };
    if found.len() != 1 {
        return Err(format!(
            "row {:?} expected exactly one binding, found {}\n{}",
            case.text,
            found.len(),
            ast.to_sexp()
        ));
    }
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("the unique node for {:?} was not retained", case.text))
}

fn collect_all_spanning<'a>(node: &'a Node, source: &str, text: &str) -> Vec<&'a Node> {
    let mut found = Vec::new();
    collect_exact(node, source, text, &mut found);
    found
}

fn collect_exact<'a>(node: &'a Node, source: &str, expected: &str, found: &mut Vec<&'a Node>) {
    if node_source(node, source) == Some(expected) {
        found.push(node);
    }
    for child in node.children() {
        collect_exact(child, source, expected, found);
    }
}

fn collect_star_unary<'a>(
    node: &'a Node,
    source: &str,
    op: &str,
    receiver: &str,
    found: &mut Vec<&'a Node>,
) {
    if let NodeKind::Unary { op: node_op, operand } = &node.kind
        && node_op == op
        && node_source(operand, source) == Some(receiver)
    {
        found.push(node);
    }
    for child in node.children() {
        collect_star_unary(child, source, op, receiver, found);
    }
}

fn pin_span_contract(node: &Node, source: &str, case: &MatrixCase<'_>) -> Result<(), String> {
    match (case.span, &node.kind) {
        (SpanContract::Full, _) => {
            if node_source(node, source) != Some(case.text) {
                return Err(format!("row {:?} lost its full-text span", case.text));
            }
        }
        (SpanContract::OperandOnly, NodeKind::Unary { operand, .. }) => {
            if node.location.start != operand.location.start
                || node.location.end != operand.location.end
            {
                return Err(format!(
                    "row {:?} drifted from the operand-only span contract",
                    case.text
                ));
            }
        }
        (SpanContract::OperandOnly, other) => {
            return Err(format!(
                "row {:?} expected a Unary node for the operand-only contract, got {}",
                case.text,
                other.kind_name()
            ));
        }
    }
    Ok(())
}

fn assert_shape(node: &Node, source: &str, expected: ExpectedShape<'_>) -> Result<(), String> {
    match expected {
        ExpectedShape::Unary { op, receiver } => {
            let NodeKind::Unary { op: actual_op, operand } = &node.kind else {
                return Err(format!("expected Unary({op}), got {}", node.kind.kind_name()));
            };
            if actual_op != op || node_source(operand, source) != Some(receiver) {
                return Err(format!(
                    "unexpected Unary row: op={actual_op:?}, receiver={:?}",
                    node_source(operand, source)
                ));
            }
        }
        ExpectedShape::Binary { op, receiver, selector } => {
            let NodeKind::Binary { op: actual_op, left, right } = &node.kind else {
                return Err(format!("expected Binary({op}), got {}", node.kind.kind_name()));
            };
            if actual_op != op
                || node_source(left, source) != Some(receiver)
                || node_source(right, source) != Some(selector)
            {
                return Err(format!(
                    "unexpected Binary row: op={actual_op:?}, receiver={:?}, selector={:?}",
                    node_source(left, source),
                    node_source(right, source)
                ));
            }
        }
        ExpectedShape::HashSlice { receiver, selector } => {
            let NodeKind::HashSlice { target, keys } = &node.kind else {
                return Err(format!("expected HashSlice, got {}", node.kind.kind_name()));
            };
            if node_source(target, source) != Some(receiver)
                || node_source(keys, source) != Some(selector)
            {
                return Err(format!(
                    "unexpected HashSlice row: receiver={:?}, selector={:?}",
                    node_source(target, source),
                    node_source(keys, source)
                ));
            }
        }
    }
    Ok(())
}

fn assert_chained_unicode_hash_slice(ast: &Node, source: &str) -> Result<(), String> {
    let node = exact_node(ast, source, "$object->{payload}->@{'東京', 'alpha'}")?;
    let NodeKind::HashSlice { target, keys } = &node.kind else {
        return Err(format!("expected chained HashSlice, got {}", node.kind.kind_name()));
    };
    if !matches!(&target.kind, NodeKind::Binary { op, .. } if op == "->{}")
        || node_source(target, source) != Some("$object->{payload}")
        || node_source(keys, source) != Some("'東京', 'alpha'")
    {
        return Err("chained Unicode HashSlice geometry drifted".to_string());
    }
    Ok(())
}

fn assert_slice_lvalues(ast: &Node, source: &str) -> Result<(), String> {
    for expected_lhs in ["$aref->@[1, 3]", "$href->@{qw(alpha beta)}"] {
        let node = exact_node(ast, source, expected_lhs)?;
        let mut is_assignment_lhs = false;
        find_parent_assignment(ast, node, &mut is_assignment_lhs);
        if !is_assignment_lhs {
            return Err(format!("{expected_lhs} was not retained as a plain `=` assignment lhs"));
        }
    }
    Ok(())
}

fn find_parent_assignment(node: &Node, expected_lhs: &Node, found: &mut bool) {
    if let NodeKind::Assignment { lhs, op, .. } = &node.kind
        && op == "="
        && std::ptr::eq(lhs.as_ref(), expected_lhs)
    {
        *found = true;
    }
    for child in node.children() {
        if !*found {
            find_parent_assignment(child, expected_lhs, found);
        }
    }
}

/// The same postfix text occurs twice and must bind twice, with identical
/// HashSlice payloads at two distinct source occurrences.
fn assert_repeated_hash_slice(ast: &Node, source: &str) -> Result<(), String> {
    let found = collect_all_spanning(ast, source, REPEATED_MARKER);
    if found.len() != 2 {
        return Err(format!(
            "repeated marker {REPEATED_MARKER:?} must bind exactly twice, found {}",
            found.len()
        ));
    }
    for node in &found {
        let NodeKind::HashSlice { target, keys } = &node.kind else {
            return Err(format!(
                "repeated marker produced {}, expected HashSlice",
                node.kind.kind_name()
            ));
        };
        if node_source(target, source) != Some("$href")
            || node_source(keys, source) != Some("'beta', '東京'")
        {
            return Err("repeated marker payload drifted".to_string());
        }
    }
    if std::ptr::eq(found[0], found[1]) {
        return Err("the repeated marker matched one node twice".to_string());
    }
    if found[0].location.start == found[1].location.start
        && found[0].location.end == found[1].location.end
    {
        return Err("the repeated marker bound one source occurrence twice".to_string());
    }
    Ok(())
}

/// Ordinary and prefix slices are retained with exact spans and shapes that
/// cannot satisfy a dedicated postfix expectation.
fn assert_generic_slice_controls(ast: &Node, source: &str) -> Result<(), String> {
    for (text, expected_kind) in [
        ("@control_hash{'alpha', 'beta'}", "HashSlice"),
        ("%control_hash{qw(alpha beta)}", "KeyValueSlice"),
        ("@$href{'alpha', 'beta'}", "HashSlice"),
        ("%$href{qw(alpha beta)}", "KeyValueSlice"),
    ] {
        let node = exact_node(ast, source, text)?;
        let actual_kind = node.kind.kind_name();
        if actual_kind != expected_kind {
            return Err(format!(
                "control {text:?} produced {actual_kind}, expected {expected_kind}"
            ));
        }
    }

    // The prefix hash slice wraps a dereference rather than a plain variable,
    // so it cannot satisfy a postfix HashSlice expectation.
    let prefix = exact_node(ast, source, "@$href{'alpha', 'beta'}")?;
    let NodeKind::HashSlice { target, .. } = &prefix.kind else {
        return Err("prefix hash slice control lost its HashSlice shape".to_string());
    };
    if matches!(&target.kind, NodeKind::Variable { .. }) {
        return Err("prefix hash slice target must stay a dereference, not a plain variable".into());
    }

    // Ordinary arrow element access stays element access.
    let element = exact_node(ast, source, "$href->{alpha}")?;
    let NodeKind::Binary { op, .. } = &element.kind else {
        return Err(format!(
            "arrow element control produced {}, expected Binary",
            element.kind.kind_name()
        ));
    };
    if op != "->{}" {
        return Err(format!("arrow element control carried op {op:?}, expected \"->{{}}\""));
    }

    // Zero-match negative controls must use each control node's actual
    // receiver and selector spans.  Keeping this collection rooted at the
    // resolved control node also prevents a legitimate postfix occurrence
    // elsewhere in the fixture from making the negative control vacuous.
    for text in [
        "@control_hash{'alpha', 'beta'}",
        "%control_hash{qw(alpha beta)}",
        "@$href{'alpha', 'beta'}",
        "%$href{qw(alpha beta)}",
    ] {
        let control = exact_node(ast, source, text)?;
        let (receiver, selector) = match &control.kind {
            NodeKind::HashSlice { target, keys } | NodeKind::KeyValueSlice { target, keys } => (
                node_source(target, source)
                    .ok_or_else(|| format!("control {text:?} has an invalid receiver span"))?,
                node_source(keys, source)
                    .ok_or_else(|| format!("control {text:?} has an invalid selector span"))?,
            ),
            other => {
                return Err(format!(
                    "control {text:?} produced {}, expected a slice node",
                    other.kind_name()
                ));
            }
        };

        let mut found = Vec::new();
        collect_postfix_slice(control, source, receiver, selector, &mut found);
        if !found.is_empty() {
            return Err(format!(
                "control {text:?} ({receiver}, {selector}) satisfied a postfix slice expectation"
            ));
        }
        for star_op in ["->$*", "->$#*", "->@*", "->%*", "->&*", "->**"] {
            let mut found = Vec::new();
            collect_star_unary(control, source, star_op, receiver, &mut found);
            if !found.is_empty() {
                return Err(format!(
                    "control {text:?} receiver {receiver} satisfied the star-form postfix expectation {star_op}"
                ));
            }
        }
    }

    // No retained control node may itself satisfy a dedicated postfix
    // expectation.
    for text in [
        "@control_hash{'alpha', 'beta'}",
        "%control_hash{qw(alpha beta)}",
        "@$href{'alpha', 'beta'}",
        "%$href{qw(alpha beta)}",
        "$href->{alpha}",
    ] {
        let node = exact_node(ast, source, text)?;
        if satisfies_postfix_expectation(node, source) {
            return Err(format!("control {text:?} satisfied a dedicated postfix expectation"));
        }
    }
    Ok(())
}

/// Whether a node satisfies a dedicated postfix-dereference expectation: an
/// arrow star-form Unary, an arrow-slice Binary (`->@[]` / `->%{}`), or a
/// HashSlice whose own source text contains the arrow. Ordinary/prefix slices
/// and arrow element access never qualify.
fn satisfies_postfix_expectation(node: &Node, source: &str) -> bool {
    match &node.kind {
        NodeKind::Unary { op, .. } => op.starts_with("->") && op.ends_with('*'),
        NodeKind::Binary { op, .. } => op == "->@[]" || op == "->%{}",
        NodeKind::HashSlice { .. } => {
            node_source(node, source).is_some_and(|text| text.contains("->"))
        }
        _ => false,
    }
}

/// Collect the dedicated arrow-slice postfix bindings for a receiver/selector
/// payload: arrow-slice Binary rows and HashSlice rows whose own source text
/// contains the arrow (excluding ordinary and prefix slices).
fn collect_postfix_slice<'a>(
    node: &'a Node,
    source: &str,
    receiver: &str,
    selector: &str,
    found: &mut Vec<&'a Node>,
) {
    let matches = match &node.kind {
        NodeKind::Binary { op, left, right } => {
            (op == "->@[]" || op == "->%{}")
                && node_source(left, source) == Some(receiver)
                && node_source(right, source) == Some(selector)
        }
        NodeKind::HashSlice { target, keys } => {
            node_source(target, source) == Some(receiver)
                && node_source(keys, source) == Some(selector)
                && node_source(node, source).is_some_and(|text| text.contains("->"))
        }
        _ => false,
    };
    if matches {
        found.push(node);
    }
    for child in node.children() {
        collect_postfix_slice(child, source, receiver, selector, found);
    }
}

fn exact_node<'a>(ast: &'a Node, source: &str, expected: &str) -> Result<&'a Node, String> {
    let found = collect_all_spanning(ast, source, expected);
    if found.len() != 1 {
        return Err(format!(
            "expected one public AST node spanning {expected:?}, found {}\n{}",
            found.len(),
            ast.to_sexp()
        ));
    }
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("the unique node for {expected:?} was not retained"))
}

fn node_source<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
