//! Canonical `NodeKind` vocabulary checks for parser-accuracy AST gold.
//!
//! Positive expectations eventually fail when their kind is misspelled because no
//! observed node can match. Forbidden-node rows are more dangerous: a misspelled
//! kind can pass vacuously and overstate negative coverage. Validate every AST kind
//! reference before parser output participates in the verdict.
//!
//! This test reads the authored manifest at compile time and inspects only the two
//! AST-expectation collections. It deliberately does not duplicate the parser-owned
//! manifest schema or create another runtime file-opening path.

use perl_parser::NodeKind;
use serde_json::Value;
use std::fmt;
use std::io;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MANIFEST_JSON: &str =
    include_str!("../../perl-corpus/fixtures/parser_accuracy/manifest.json");

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
    let manifest: Value = serde_json::from_str(MANIFEST_JSON)?;
    let fixtures = manifest.get("fixtures").and_then(Value::as_array).ok_or_else(|| {
        invalid_manifest("parser-accuracy manifest field `fixtures` must be an array")
    })?;

    let mut positive_references = 0usize;
    let mut forbidden_references = 0usize;

    for fixture in fixtures {
        let fixture_id = fixture.get("id").and_then(Value::as_str).ok_or_else(|| {
            invalid_manifest("every parser-accuracy fixture must have a string `id`")
        })?;

        positive_references += validate_reference_rows(
            fixture_id,
            fixture,
            "ast_expectations",
            "ast_expectations.kind",
            "ast_expectations.parent_kind",
        )?;
        forbidden_references += validate_reference_rows(
            fixture_id,
            fixture,
            "forbidden_nodes",
            "forbidden_nodes.kind",
            "forbidden_nodes.parent_kind",
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

fn validate_reference_rows(
    fixture_id: &str,
    fixture: &Value,
    collection: &'static str,
    kind_field: &'static str,
    parent_field: &'static str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let Some(raw_rows) = fixture.get(collection) else {
        return Ok(0);
    };
    let rows = raw_rows.as_array().ok_or_else(|| {
        invalid_manifest(format!("fixture `{fixture_id}` field `{collection}` must be an array"))
    })?;

    let mut checked = 0usize;
    for row in rows {
        let expectation_id = row.get("id").and_then(Value::as_str).ok_or_else(|| {
            invalid_manifest(format!(
                "fixture `{fixture_id}` field `{collection}` contains a row without a string `id`"
            ))
        })?;
        let kind = row.get("kind").and_then(Value::as_str).ok_or_else(|| {
            invalid_manifest(format!(
                "fixture `{fixture_id}` expectation `{expectation_id}` field `{kind_field}` must be a string"
            ))
        })?;

        validate_node_kind_reference(fixture_id, expectation_id, kind_field, kind)?;
        checked += 1;

        match row.get("parent_kind") {
            None | Some(Value::Null) => {}
            Some(Value::String(parent_kind)) => {
                validate_node_kind_reference(
                    fixture_id,
                    expectation_id,
                    parent_field,
                    parent_kind,
                )?;
                checked += 1;
            }
            Some(_) => {
                return Err(invalid_manifest(format!(
                    "fixture `{fixture_id}` expectation `{expectation_id}` field `{parent_field}` must be a string or null"
                ))
                .into());
            }
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

fn invalid_manifest(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
