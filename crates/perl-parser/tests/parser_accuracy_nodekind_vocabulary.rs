//! Canonical `NodeKind` vocabulary checks for parser-accuracy AST gold.
//!
//! Positive expectations eventually fail when their kind is misspelled because no
//! observed node can match. Forbidden-node rows are more dangerous: a misspelled
//! kind can pass vacuously and overstate negative coverage. Validate every AST kind
//! reference before parser output participates in the verdict.

use perl_parser::NodeKind;
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize)]
struct ParserAccuracyManifest {
    fixtures: Vec<ParserAccuracyFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyFixture {
    id: String,
    #[serde(default)]
    ast_expectations: Vec<AstExpectation>,
    #[serde(default)]
    forbidden_nodes: Vec<ForbiddenNode>,
}

#[derive(Debug, Deserialize)]
struct AstExpectation {
    id: String,
    kind: String,
    #[serde(default)]
    parent_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForbiddenNode {
    id: String,
    kind: String,
    #[serde(default)]
    parent_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InvalidNodeKindReference {
    fixture_id: String,
    expectation_id: String,
    field: &'static str,
    kind: String,
}

impl fmt::Display for InvalidNodeKindReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "fixture `{}` expectation `{}` field `{}` references non-canonical NodeKind `{}`; expected one of: {}",
            self.fixture_id,
            self.expectation_id,
            self.field,
            self.kind,
            NodeKind::ALL_KIND_NAMES.join(", ")
        )
    }
}

impl std::error::Error for InvalidNodeKindReference {}

#[test]
fn parser_accuracy_ast_nodekind_references_are_canonical() -> TestResult {
    let manifest = load_manifest()?;

    for fixture in &manifest.fixtures {
        for expectation in &fixture.ast_expectations {
            validate_node_kind_reference(
                &fixture.id,
                &expectation.id,
                "ast_expectations.kind",
                &expectation.kind,
            )?;
            if let Some(parent_kind) = expectation.parent_kind.as_deref() {
                validate_node_kind_reference(
                    &fixture.id,
                    &expectation.id,
                    "ast_expectations.parent_kind",
                    parent_kind,
                )?;
            }
        }

        for forbidden in &fixture.forbidden_nodes {
            validate_node_kind_reference(
                &fixture.id,
                &forbidden.id,
                "forbidden_nodes.kind",
                &forbidden.kind,
            )?;
            if let Some(parent_kind) = forbidden.parent_kind.as_deref() {
                validate_node_kind_reference(
                    &fixture.id,
                    &forbidden.id,
                    "forbidden_nodes.parent_kind",
                    parent_kind,
                )?;
            }
        }
    }

    Ok(())
}

#[test]
fn misspelled_forbidden_kind_is_rejected_before_absence_matching() {
    assert!(
        validate_node_kind_reference(
            "quote_like",
            "quote_braces_not_block",
            "forbidden_nodes.kind",
            "Block",
        )
        .is_ok(),
        "the canonical control must remain admitted"
    );

    assert_eq!(
        validate_node_kind_reference(
            "quote_like",
            "quote_braces_not_block",
            "forbidden_nodes.kind",
            "Bloock",
        ),
        Err(InvalidNodeKindReference {
            fixture_id: "quote_like".to_string(),
            expectation_id: "quote_braces_not_block".to_string(),
            field: "forbidden_nodes.kind",
            kind: "Bloock".to_string(),
        })
    );
}

fn validate_node_kind_reference(
    fixture_id: &str,
    expectation_id: &str,
    field: &'static str,
    kind: &str,
) -> Result<(), InvalidNodeKindReference> {
    if NodeKind::ALL_KIND_NAMES.iter().any(|canonical| *canonical == kind) {
        return Ok(());
    }

    Err(InvalidNodeKindReference {
        fixture_id: fixture_id.to_string(),
        expectation_id: expectation_id.to_string(),
        field,
        kind: kind.to_string(),
    })
}

fn load_manifest() -> Result<ParserAccuracyManifest, Box<dyn std::error::Error>> {
    let manifest_path = workspace_root()
        .join("crates")
        .join("perl-corpus")
        .join("fixtures")
        .join("parser_accuracy")
        .join("manifest.json");
    let manifest_json = fs::read_to_string(manifest_path)?;
    Ok(serde_json::from_str(&manifest_json)?)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
