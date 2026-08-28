//! Canonical `NodeKind` vocabulary checks for parser-accuracy AST gold.
//!
//! Positive expectations eventually fail when their kind is misspelled because no
//! observed node can match. Forbidden-node rows are more dangerous: a misspelled
//! kind can pass vacuously and overstate negative coverage. Validate every AST kind
//! reference before parser output participates in the verdict.
//!
//! This test deserializes a narrow projection of the authored manifest at compile
//! time. Parser, scorer, span, and recovery fields remain owned by their existing
//! harnesses; this projection owns only the NodeKind reference identity it checks.

use perl_parser::NodeKind;
use serde::Deserialize;
use std::fmt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MANIFEST_JSON: &str =
    include_str!("../../perl-corpus/fixtures/parser_accuracy/manifest.json");
const AST_KIND_FIELD: &str = "ast_expectations.kind";
const AST_PARENT_KIND_FIELD: &str = "ast_expectations.parent_kind";
const FORBIDDEN_KIND_FIELD: &str = "forbidden_nodes.kind";
const FORBIDDEN_PARENT_KIND_FIELD: &str = "forbidden_nodes.parent_kind";

#[derive(Debug, Deserialize)]
struct ParserAccuracyNodeKindManifest {
    fixtures: Vec<ParserAccuracyNodeKindFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyNodeKindFixture {
    id: String,
    ast_expectations: Vec<NodeKindReference>,
    #[serde(default)]
    forbidden_nodes: Vec<NodeKindReference>,
}

#[derive(Debug, Deserialize)]
struct NodeKindReference {
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
    let manifest: ParserAccuracyNodeKindManifest = serde_json::from_str(MANIFEST_JSON)?;
    let mut positive_references = 0usize;
    let mut forbidden_references = 0usize;

    for fixture in &manifest.fixtures {
        positive_references += validate_reference_rows(
            &fixture.id,
            &fixture.ast_expectations,
            AST_KIND_FIELD,
            AST_PARENT_KIND_FIELD,
        )?;
        forbidden_references += validate_reference_rows(
            &fixture.id,
            &fixture.forbidden_nodes,
            FORBIDDEN_KIND_FIELD,
            FORBIDDEN_PARENT_KIND_FIELD,
        )?;
    }

    assert!(
        positive_references > 0,
        "parser-accuracy manifest must expose at least one positive AST NodeKind reference"
    );
    assert!(
        forbidden_references > 0,
        "parser-accuracy manifest must expose at least one forbidden AST NodeKind reference"
    );

    Ok(())
}

#[test]
fn manifest_requires_ast_nodekind_collection() {
    let without_ast_collection = r#"{
        "fixtures": [{
            "id": "fixture",
            "ast_expectation": [],
            "forbidden_nodes": []
        }]
    }"#;
    assert!(
        serde_json::from_str::<ParserAccuracyNodeKindManifest>(without_ast_collection).is_err(),
        "a misspelled or missing ast_expectations collection must not default to empty"
    );

    let optional_forbidden_collection = r#"{
        "fixtures": [{
            "id": "fixture",
            "ast_expectations": []
        }]
    }"#;
    assert!(
        serde_json::from_str::<ParserAccuracyNodeKindManifest>(optional_forbidden_collection)
            .is_ok(),
        "the existing schema permits fixtures without forbidden_nodes"
    );
}

#[test]
fn non_canonical_kind_controls_are_rejected_before_absence_matching() {
    let canonical = [NodeKindReference {
        id: "quote_braces_not_block".to_string(),
        kind: "Block".to_string(),
        parent_kind: None,
    }];
    assert_eq!(
        validate_reference_rows(
            "quote_like",
            &canonical,
            FORBIDDEN_KIND_FIELD,
            FORBIDDEN_PARENT_KIND_FIELD,
        ),
        Ok(1),
        "the canonical forbidden-node control must remain admitted"
    );

    let canonical = [NodeKindReference {
        id: "string_under_statement".to_string(),
        kind: "String".to_string(),
        parent_kind: Some("ExpressionStatement".to_string()),
    }];
    assert_eq!(
        validate_reference_rows("quote_like", &canonical, AST_KIND_FIELD, AST_PARENT_KIND_FIELD,),
        Ok(2),
        "the canonical kind and parent-kind control must remain admitted"
    );

    let controls = [
        (FORBIDDEN_KIND_FIELD, FORBIDDEN_PARENT_KIND_FIELD, "Bloock"),
        (FORBIDDEN_KIND_FIELD, FORBIDDEN_PARENT_KIND_FIELD, "Strng"),
        (AST_KIND_FIELD, AST_PARENT_KIND_FIELD, "FunctionCal"),
        (AST_PARENT_KIND_FIELD, AST_KIND_FIELD, "ExpressionStatment"),
    ];
    for (kind_field, parent_field, non_canonical) in controls {
        let rows = [NodeKindReference {
            id: "varied_non_canonical_control".to_string(),
            kind: if kind_field == AST_PARENT_KIND_FIELD
                || kind_field == FORBIDDEN_PARENT_KIND_FIELD
            {
                "String".to_string()
            } else {
                non_canonical.to_string()
            },
            parent_kind: if parent_field == AST_KIND_FIELD || parent_field == FORBIDDEN_KIND_FIELD {
                Some(non_canonical.to_string())
            } else {
                None
            },
        }];

        assert!(
            validate_reference_rows("varied_controls", &rows, kind_field, parent_field).is_err(),
            "non-canonical control `{non_canonical}` in `{kind_field}` must be rejected"
        );
    }
}

fn validate_reference_rows(
    fixture_id: &str,
    rows: &[NodeKindReference],
    kind_field: &'static str,
    parent_field: &'static str,
) -> Result<usize, InvalidNodeKindReference> {
    let mut checked = 0usize;
    for row in rows {
        validate_node_kind_reference(fixture_id, &row.id, kind_field, &row.kind)?;
        checked += 1;

        if let Some(parent_kind) = row.parent_kind.as_deref() {
            validate_node_kind_reference(fixture_id, &row.id, parent_field, parent_kind)?;
            checked += 1;
        }
    }
    Ok(checked)
}

fn validate_node_kind_reference(
    fixture_id: &str,
    expectation_id: &str,
    field: &'static str,
    kind: &str,
) -> Result<(), InvalidNodeKindReference> {
    if NodeKind::ALL_KIND_NAMES.contains(&kind) {
        return Ok(());
    }

    Err(InvalidNodeKindReference {
        fixture_id: fixture_id.to_string(),
        expectation_id: expectation_id.to_string(),
        field,
        kind: kind.to_string(),
    })
}
