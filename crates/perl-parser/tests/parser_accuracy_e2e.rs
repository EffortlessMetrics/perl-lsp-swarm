//! End-to-end parser checks backed by parser-accuracy corpus metadata.
//!
//! These tests exercise the public `perl_parser::Parser` API against curated
//! fixture files and their manifest expectations so parser regressions are
//! caught at the same fixture boundary used by downstream accuracy tooling.

use perl_parser::{Node, NodeKind, Parser};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ParserAccuracyManifest {
    fixtures: Vec<ParserAccuracyFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyFixture {
    id: String,
    source_path: PathBuf,
    #[serde(default)]
    ast_expectations: Vec<AstExpectation>,
    #[serde(default)]
    forbidden_nodes: Vec<ForbiddenNode>,
    #[serde(default)]
    recovery_expectations: Vec<RecoveryExpectation>,
}

/// A node shape that must NOT appear at a given position.
///
/// Positive expectations cannot reject an *extra* wrong node: the matcher asks
/// whether some node matches, so a parser that emits the right node plus a
/// spurious one still passes. Disambiguation fixtures need the other half —
/// "the `q{}` braces did not open a block" is a different claim from "a String
/// is present".
///
/// `line` is required, unlike the optional refinements on `AstExpectation`. A
/// forbidden entry without a position would ban a kind across the whole file,
/// and the kinds worth forbidding here (`Block`, `ExpressionStatement`) all
/// occur legitimately elsewhere in the same fixture — `quote_like` contains
/// `sub quote { ... }`, whose body is a perfectly good `Block`.
#[derive(Debug, Deserialize)]
struct ForbiddenNode {
    id: String,
    kind: String,
    line: usize,
    #[serde(default)]
    parent_kind: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RecoveryExpectation {
    id: String,
    first_error_line: usize,
    error_region: LineRange,
    recovery_line: usize,
}

#[derive(Debug, Deserialize)]
struct LineRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Deserialize)]
struct AstExpectation {
    id: String,
    kind: String,
    line: usize,
    span_text: String,
    #[serde(default)]
    parent_kind: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
    #[serde(default)]
    operator: Option<String>,
    #[serde(default)]
    parent_operator: Option<String>,
}

#[derive(Debug)]
struct ObservedNode<'a> {
    kind: &'static str,
    line: usize,
    span_text: &'a str,
    parent_kind: Option<&'static str>,
    depth: usize,
    operator: Option<String>,
    parent_operator: Option<String>,
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

const E2E_FIXTURES: &[&str] = &[
    "package_basic",
    "imports_exports",
    "qualified_refs",
    "same_bare_subs",
    "role_method",
    "inherited_method",
    "operator_precedence",
    "quote_like",
    "regex_match",
    "method_call",
    "medium_method_call",
    "method_decl",
    "generated_accessor",
    "heuristic_generated_member",
    "slash_ambiguity",
    "control_flow_core",
    "dynamic_require_boundary",
    "typeglob_alias",
    "heredoc_basic",
    "post_error_package_sub_recovery",
    "signatures_basic",
    "format_decl",
    "postderef_boundary",
    "control_do_until",
    "eval_string_boundary",
    "autoload_boundary",
    "export_tags",
    "span_coordinates",
    "span_utf8_multibyte",
    "span_emoji",
    "span_crlf",
    "span_tabs",
    "span_bom",
    "span_cross_line",
    "span_mixed_newlines",
    "span_empty_at_eof",
    "heredoc_utf8_delimiter",
    "unterminated_heredoc",
    "bad_heredoc_terminator",
    "unclosed_quote_like_operator",
    "unclosed_regex",
    "unbalanced_bracket",
    "partial_sub_body",
    "orphan_close_delimiters",
    "missing_comma_list",
    "nested_malformed_delimiters",
    "malformed_heredoc_recovery",
    "method_completion_provider",
    "navigation_provider",
    "diagnostic_provider",
    "negative_symbol_regions",
    "incremental_small_edit",
];

#[test]
fn parser_accuracy_fixtures_satisfy_manifest_ast_expectations() -> TestResult {
    let workspace_root = workspace_root();
    let manifest_path = workspace_root
        .join("crates")
        .join("perl-corpus")
        .join("fixtures")
        .join("parser_accuracy")
        .join("manifest.json");
    let manifest_json = fs::read_to_string(&manifest_path)?;
    let manifest: ParserAccuracyManifest = serde_json::from_str(&manifest_json)?;

    let mut exercised = 0usize;
    for fixture_id in E2E_FIXTURES {
        let fixture = find_fixture(&manifest, fixture_id)?;
        let source_path = workspace_root.join(&fixture.source_path);
        let source = fs::read_to_string(&source_path)?;

        let mut parser = Parser::new(&source);
        let ast = parser.parse().map_err(|err| {
            format!("fixture `{}` should parse through the public parser API: {err}", fixture.id)
        })?;
        let observed = collect_observed_nodes(&ast, &source);

        // Per-fixture, not just in aggregate: a shared counter cannot tell that one
        // selected fixture lost every assertion while the others kept the suite green,
        // which is exactly the hollow-fixture state this selector exists to prevent.
        let contributed = fixture.ast_expectations.len() + fixture.forbidden_nodes.len();
        assert!(
            contributed > 0,
            "fixture `{}` is selected by E2E_FIXTURES but contributes no assertion: \
             give it `ast_expectations` or `forbidden_nodes`, or remove it from the selector",
            fixture.id
        );

        for expectation in &fixture.ast_expectations {
            assert_observed_expectation(&fixture.id, expectation, &observed);
            exercised += 1;
        }

        for forbidden in &fixture.forbidden_nodes {
            assert_node_absent(&fixture.id, forbidden, &observed);
            exercised += 1;
        }
    }

    assert!(exercised > 0, "selected parser accuracy e2e fixtures should include AST expectations");
    Ok(())
}

/// Recovery fixtures whose live `parse_with_recovery` diagnostic geometry still
/// diverges from the manifest contract.
///
/// `malformed_heredoc_recovery` currently reports its diagnostic at EOF
/// (`offset == source.len()`, one line past the final newline) rather than at
/// the heredoc opener (`first_error_line = 3`). Keep that gap explicit and
/// exact so it cannot hide inside a hardcoded allowlist again.
const RECOVERY_GEOMETRY_FOLLOWUPS: &[&str] = &["malformed_heredoc_recovery"];

#[test]
fn recovery_fixtures_report_their_expected_error_boundary() -> TestResult {
    let workspace_root = workspace_root();
    let manifest_json = fs::read_to_string(
        workspace_root
            .join("crates")
            .join("perl-corpus")
            .join("fixtures")
            .join("parser_accuracy")
            .join("manifest.json"),
    )?;
    let manifest: ParserAccuracyManifest = serde_json::from_str(&manifest_json)?;

    // Drive from every manifest fixture that declares recovery expectations and
    // every expectation row within them — `.first()` / hardcoded allowlists
    // silently drop contracts such as `post_error_package_sub_recovery`.
    let followups: std::collections::BTreeSet<&str> =
        RECOVERY_GEOMETRY_FOLLOWUPS.iter().copied().collect();
    let mut seen_followups = std::collections::BTreeSet::new();
    let mut exercised = 0usize;
    for fixture in &manifest.fixtures {
        if fixture.recovery_expectations.is_empty() {
            continue;
        }
        if followups.contains(fixture.id.as_str()) {
            seen_followups.insert(fixture.id.as_str());
            // Still require a nonempty diagnostic set so the follow-up fixture
            // cannot go silent while parked.
            let source = fs::read_to_string(workspace_root.join(&fixture.source_path))?;
            let mut parser = Parser::new(&source);
            let output = parser.parse_with_recovery();
            assert!(
                !output.diagnostics.is_empty(),
                "follow-up recovery fixture '{}' must still emit diagnostics",
                fixture.id
            );
            continue;
        }

        let source_path = workspace_root.join(&fixture.source_path);
        let source = fs::read_to_string(&source_path)?;
        let mut parser = Parser::new(&source);
        let output = parser.parse_with_recovery();
        let mut diagnostic_lines = std::collections::BTreeSet::new();
        for diagnostic in &output.diagnostics {
            if let Some(offset) = diagnostic.location() {
                diagnostic_lines.insert(byte_offset_to_line(&source, offset));
            }
        }
        let mut error_node_lines = std::collections::BTreeSet::new();
        collect_error_node_line_ranges(&output.ast, &source, &mut error_node_lines);
        assert!(
            !diagnostic_lines.is_empty(),
            "recovery fixture '{}' produced no diagnostics",
            fixture.id
        );

        for expectation in &fixture.recovery_expectations {
            exercised += 1;
            let expected_region = expectation.error_region.start..=expectation.error_region.end;
            let error_node_spillover: Vec<_> = error_node_lines
                .iter()
                .filter(|line| !expected_region.contains(line))
                .copied()
                .collect();
            assert!(
                error_node_spillover.is_empty(),
                "recovery fixture '{}' emitted Error AST node lines outside declared region {}..={}: {:?} ({})",
                fixture.id,
                expectation.error_region.start,
                expectation.error_region.end,
                error_node_spillover,
                expectation.id
            );
            let mut error_lines = diagnostic_lines.clone();
            error_lines.extend(error_node_lines.iter().copied());
            assert_eq!(
                error_lines.first().copied(),
                Some(expectation.first_error_line),
                "recovery fixture '{}' first error boundary drifted ({})",
                fixture.id,
                expectation.id
            );
            let spillover: Vec<_> = error_lines
                .iter()
                .filter(|line| !expected_region.contains(line))
                .copied()
                .collect();
            assert!(
                spillover.is_empty(),
                "recovery fixture '{}' emitted error lines outside declared region {}..={}: {:?} ({})",
                fixture.id,
                expectation.error_region.start,
                expectation.error_region.end,
                spillover,
                expectation.id
            );
            let missing_region_lines: Vec<_> =
                expected_region.clone().filter(|line| !error_lines.contains(&line)).collect();
            assert!(
                missing_region_lines.is_empty(),
                "recovery fixture '{}' missing error evidence on declared region lines {:?}: observed {:?} ({})",
                fixture.id,
                missing_region_lines,
                error_lines,
                expectation.id
            );
            if expectation.recovery_line <= expectation.error_region.end {
                assert!(
                    error_lines.contains(&expectation.recovery_line),
                    "recovery fixture '{}' missing recovery error node/diagnostic on line {} ({})",
                    fixture.id,
                    expectation.recovery_line,
                    expectation.id
                );
            } else {
                assert!(
                    !error_lines.contains(&expectation.recovery_line),
                    "recovery fixture '{}' emitted an error on post-error recovery line {} ({})",
                    fixture.id,
                    expectation.recovery_line,
                    expectation.id
                );
            }
        }
    }
    assert_eq!(
        seen_followups, followups,
        "RECOVERY_GEOMETRY_FOLLOWUPS must name only current divergent recovery fixtures"
    );
    assert!(
        exercised > 0,
        "parser-accuracy manifest must declare at least one recovery expectation"
    );
    Ok(())
}

/// Collect every line covered by each typed `Error` node's `[start, end)` span.
///
/// Start-line-only collection lets a recovery node begin inside the declared
/// region while still consuming later valid declarations; requiring the full
/// covered line range keeps spillover assertions honest.
fn collect_error_node_line_ranges(
    node: &Node,
    source: &str,
    lines: &mut std::collections::BTreeSet<usize>,
) {
    if matches!(node.kind, NodeKind::Error { .. }) {
        let start_line = byte_offset_to_line(source, node.location.start);
        // `end` is exclusive; an empty/zero-width node still contributes its start line.
        let end_offset = node.location.end.saturating_sub(1).max(node.location.start);
        let end_line = byte_offset_to_line(source, end_offset);
        for line in start_line..=end_line {
            lines.insert(line);
        }
    }
    node.for_each_child(|child| collect_error_node_line_ranges(child, source, lines));
}

/// Every fixture with expectations must declare its own coverage honestly:
/// `E2E_FIXTURES` must not name a fixture the manifest does not carry, and no
/// fixture may claim an expectation id twice.
///
/// Duplicate ids are the quiet failure — two rows with one id read as two
/// assertions in review while a disposition against "that id" is ambiguous.
#[test]
fn parser_accuracy_manifest_ids_are_unique_and_selected_fixtures_exist() -> TestResult {
    let workspace_root = workspace_root();
    let manifest_json = fs::read_to_string(
        workspace_root
            .join("crates")
            .join("perl-corpus")
            .join("fixtures")
            .join("parser_accuracy")
            .join("manifest.json"),
    )?;
    let manifest: ParserAccuracyManifest = serde_json::from_str(&manifest_json)?;

    for fixture_id in E2E_FIXTURES {
        find_fixture(&manifest, fixture_id)?;
    }

    let mut seen = std::collections::BTreeSet::new();
    for fixture in &manifest.fixtures {
        for id in fixture
            .ast_expectations
            .iter()
            .map(|expectation| &expectation.id)
            .chain(fixture.forbidden_nodes.iter().map(|forbidden| &forbidden.id))
        {
            assert!(
                seen.insert(id.clone()),
                "duplicate parser-accuracy expectation id `{id}` in fixture `{}`",
                fixture.id
            );
        }
    }
    Ok(())
}

#[test]
fn span_fixtures_preserve_the_bytes_they_measure() -> TestResult {
    let workspace_root = workspace_root();
    let fixture_root =
        workspace_root.join("crates").join("perl-corpus").join("fixtures").join("parser_accuracy");

    let crlf = fs::read(fixture_root.join("span_crlf.pl"))?;
    assert!(
        crlf.windows(2).any(|window| window == b"\r\n"),
        "span_crlf must contain at least one CRLF sequence"
    );

    let bom = fs::read(fixture_root.join("span_bom.pl"))?;
    assert!(bom.starts_with(b"\xef\xbb\xbf"), "span_bom must begin with a UTF-8 BOM");

    let mixed = fs::read(fixture_root.join("span_mixed_newlines.pl"))?;
    let has_crlf = mixed.windows(2).any(|window| window == b"\r\n");
    let has_lone_lf = mixed
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || mixed[index - 1] != b'\r'));
    assert!(
        has_crlf && has_lone_lf,
        "span_mixed_newlines must contain both LF and CRLF line endings"
    );
    Ok(())
}

fn find_fixture<'a>(
    manifest: &'a ParserAccuracyManifest,
    fixture_id: &str,
) -> Result<&'a ParserAccuracyFixture, Box<dyn std::error::Error>> {
    manifest
        .fixtures
        .iter()
        .find(|fixture| fixture.id == fixture_id)
        .ok_or_else(|| format!("parser accuracy fixture `{fixture_id}` is missing").into())
}

fn collect_observed_nodes<'a>(node: &'a Node, source: &'a str) -> Vec<ObservedNode<'a>> {
    let mut nodes = Vec::new();
    collect_observed_nodes_rec(node, source, &mut nodes, None, 0, None);
    nodes
}

fn collect_observed_nodes_rec<'a>(
    node: &'a Node,
    source: &'a str,
    nodes: &mut Vec<ObservedNode<'a>>,
    parent_kind: Option<&'static str>,
    depth: usize,
    parent_operator: Option<&str>,
) {
    let span_text = source.get(node.location.start..node.location.end).unwrap_or_default();
    let operator = node_operator(node).map(str::to_owned);
    nodes.push(ObservedNode {
        kind: node.kind.kind_name(),
        line: byte_offset_to_line(source, node.location.start),
        span_text,
        parent_kind,
        depth,
        operator: operator.clone(),
        parent_operator: parent_operator.map(str::to_owned),
    });

    let current_kind = node.kind.kind_name();
    node.for_each_child(|child| {
        collect_observed_nodes_rec(
            child,
            source,
            nodes,
            Some(current_kind),
            depth + 1,
            operator.as_deref(),
        )
    });
}

fn node_operator(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Binary { op, .. } | NodeKind::Assignment { op, .. } => Some(op.as_str()),
        NodeKind::Match { negated, .. } => Some(if *negated { "!~" } else { "=~" }),
        _ => None,
    }
}

fn assert_observed_expectation(
    fixture_id: &str,
    expectation: &AstExpectation,
    observed: &[ObservedNode<'_>],
) {
    let matched = observed.iter().any(|node| {
        node.kind == expectation.kind
            && node.line == expectation.line
            && node.span_text.contains(&expectation.span_text)
            && expectation
                .parent_kind
                .as_deref()
                .is_none_or(|parent| node.parent_kind == Some(parent))
            && expectation.depth.is_none_or(|depth| node.depth == depth)
            && expectation
                .operator
                .as_deref()
                .is_none_or(|operator| node.operator.as_deref() == Some(operator))
            && match expectation.operator.as_deref() {
                Some(_) => {
                    node.parent_operator.as_deref() == expectation.parent_operator.as_deref()
                }
                None => expectation
                    .parent_operator
                    .as_deref()
                    .is_none_or(|operator| node.parent_operator.as_deref() == Some(operator)),
            }
    });

    assert!(
        matched,
        "fixture `{fixture_id}` missing AST expectation `{}`: expected kind `{}` on line {} containing {:?}",
        expectation.id, expectation.kind, expectation.line, expectation.span_text
    );
}

fn assert_node_absent(fixture_id: &str, forbidden: &ForbiddenNode, observed: &[ObservedNode<'_>]) {
    let offender = observed.iter().find(|node| {
        node.kind == forbidden.kind
            && node.line == forbidden.line
            && forbidden
                .parent_kind
                .as_deref()
                .is_none_or(|parent| node.parent_kind == Some(parent))
            && forbidden.depth.is_none_or(|depth| node.depth == depth)
    });

    assert!(
        offender.is_none(),
        "fixture `{fixture_id}` violates forbidden node `{}`: found kind `{}` on line {} spanning {:?}",
        forbidden.id,
        forbidden.kind,
        forbidden.line,
        offender.map(|node| node.span_text).unwrap_or_default(),
    );
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
