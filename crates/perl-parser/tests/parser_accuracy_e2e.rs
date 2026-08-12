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
    "operator_precedence",
    "quote_like",
    "regex_match",
    "method_call",
    "slash_ambiguity",
    "control_flow_core",
    "dynamic_require_boundary",
    "typeglob_alias",
    "heredoc_basic",
    "post_error_package_sub_recovery",
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

        for expectation in &fixture.ast_expectations {
            assert_observed_expectation(&fixture.id, expectation, &observed);
            exercised += 1;
        }
    }

    assert!(exercised > 0, "selected parser accuracy e2e fixtures should include AST expectations");
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

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
