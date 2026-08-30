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
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value, json};
use std::collections::BTreeSet;
use std::fmt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MANIFEST_JSON: &str =
    include_str!("../../perl-corpus/fixtures/parser_accuracy/manifest.json");
const AST_KIND_FIELD: &str = "ast_expectations.kind";
const AST_PARENT_KIND_FIELD: &str = "ast_expectations.parent_kind";
const FORBIDDEN_KIND_FIELD: &str = "forbidden_nodes.kind";
const FORBIDDEN_PARENT_KIND_FIELD: &str = "forbidden_nodes.parent_kind";

const PARSER_ACCURACY_FIXTURE_FIELDS: &[&str] = &[
    "id",
    "family",
    "label_mode",
    "source_path",
    "scored_lines",
    "scored_symbols",
    "fully_labeled_regions",
    "partial_labeled_regions",
    "unknown_regions",
    "negative_regions",
    "dynamic_boundaries",
    "unsupported_constructs",
    "real_project_file",
    "generated",
    "line_expectations",
    "ast_expectations",
    "symbol_expectations",
    "provider_expectations",
    "span_expectations",
    "symbol_safety_regions",
    "forbidden_nodes",
    "recovery_expectations",
    "incremental_expectations",
];
const AST_EXPECTATION_FIELDS: &[&str] =
    &["id", "kind", "line", "span_text", "parent_kind", "depth", "operator", "parent_operator"];
const FORBIDDEN_NODE_FIELDS: &[&str] = &["id", "kind", "line", "parent_kind", "depth"];

#[derive(Debug, Deserialize)]
struct ParserAccuracyNodeKindManifest {
    fixtures: Vec<ParserAccuracyNodeKindFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyNodeKindFixture {
    id: String,
    #[serde(default)]
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

#[derive(Debug)]
struct ManifestSchemaError(String);

impl fmt::Display for ManifestSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ManifestSchemaError {}

#[derive(Debug)]
struct StrictJsonValueSeed {
    path: String,
}

impl StrictJsonValueSeed {
    fn root() -> Self {
        Self { path: "manifest".to_string() }
    }
}

impl<'de> DeserializeSeed<'de> for StrictJsonValueSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonValueVisitor { path: self.path })
    }
}

#[derive(Debug)]
struct StrictJsonValueVisitor {
    path: String,
}

impl<'de> Visitor<'de> for StrictJsonValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom(format!("{} contains a non-finite number", self.path)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJsonValueSeed { path: self.path }.deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictJsonValueSeed {
            path: format!("{}[{}]", self.path, values.len()),
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object_access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        let mut seen = BTreeSet::new();
        while let Some(key) = object_access.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(A::Error::custom(format!(
                    "{} contains duplicate field `{key}`",
                    self.path
                )));
            }
            let value = object_access
                .next_value_seed(StrictJsonValueSeed { path: format!("{}.{}", self.path, key) })?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

#[test]
fn parser_accuracy_ast_nodekind_references_are_canonical() -> TestResult {
    let manifest = parse_manifest(MANIFEST_JSON)?;
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
fn manifest_rejects_duplicate_json_members_before_nodekind_validation() {
    let canonical = r#"{
        "fixtures": [{
            "id": "fixture",
            "ast_expectations": [{
                "id": "string_under_program",
                "kind": "String",
                "line": 1,
                "span_text": "value",
                "parent_kind": "Program"
            }],
            "forbidden_nodes": [{
                "id": "no_block",
                "kind": "Block",
                "line": 1,
                "parent_kind": "Program"
            }]
        }]
    }"#;
    assert!(
        parse_manifest(canonical).is_ok(),
        "the canonical duplicate-control manifest must remain admitted"
    );

    let cases = [
        (
            "manifest",
            "fixtures",
            r#"{
                "fixtures": [],
                "fixtures": []
            }"#,
        ),
        (
            "manifest.fixtures[0]",
            "id",
            r#"{
                "fixtures": [{
                    "id": "first",
                    "id": "second"
                }]
            }"#,
        ),
        (
            "manifest.fixtures[0].ast_expectations[0]",
            "kind",
            r#"{
                "fixtures": [{
                    "id": "fixture",
                    "ast_expectations": [{
                        "id": "forged_kind",
                        "kind": "Bloock",
                        "kind": "Block",
                        "line": 1,
                        "span_text": "value"
                    }]
                }]
            }"#,
        ),
        (
            "manifest.fixtures[0].forbidden_nodes[0]",
            "parent_kind",
            r#"{
                "fixtures": [{
                    "id": "fixture",
                    "forbidden_nodes": [{
                        "id": "forged_parent_kind",
                        "kind": "Block",
                        "line": 1,
                        "parent_kind": "ExpressionStatment",
                        "parent_kind": "Program"
                    }]
                }]
            }"#,
        ),
        (
            "manifest.fixtures[0].ast_expectations[0]",
            "line",
            r#"{
                "fixtures": [{
                    "id": "fixture",
                    "ast_expectations": [{
                        "id": "forged_line",
                        "kind": "Bloock",
                        "line": 1,
                        "line": 2
                    }]
                }]
            }"#,
        ),
    ];

    for (object_path, duplicate_key, json) in cases {
        let error = parse_manifest(json).expect_err("duplicate JSON member names must fail closed");
        let rendered = error.to_string();
        assert!(
            rendered.contains(object_path),
            "duplicate-key diagnostic `{rendered}` must identify object path `{object_path}`"
        );
        assert!(
            rendered.contains(&format!("duplicate field `{duplicate_key}`")),
            "duplicate-key diagnostic `{rendered}` must identify key `{duplicate_key}`"
        );
        assert!(
            !rendered.contains("non-canonical NodeKind"),
            "duplicate rejection must precede NodeKind validation; diagnostic `{rendered}` unexpectedly performed NodeKind validation"
        );
    }
}

#[test]
fn manifest_preserves_optional_nodekind_collections_and_rejects_misspelled_keys() {
    let forbidden_only = r#"{
        "fixtures": [{
            "id": "fixture",
            "forbidden_nodes": [{
                "id": "no_block",
                "kind": "Block",
                "line": 1
            }]
        }]
    }"#;
    let manifest = parse_manifest(forbidden_only).expect("forbidden-only fixture is valid");
    assert!(manifest.fixtures[0].ast_expectations.is_empty());

    let misspelled_parent_kind = r#"{
        "fixtures": [{
            "id": "fixture",
            "ast_expectations": [{
                "id": "expectation",
                "kind": "String",
                "line": 1,
                "span_text": "value",
                "parent_knd": "Program"
            }]
        }]
    }"#;
    assert!(
        parse_manifest(misspelled_parent_kind).is_err(),
        "a misspelled parent_kind key must not be silently ignored"
    );

    let misspelled_forbidden_nodes = r#"{
        "fixtures": [{
            "id": "fixture",
            "forbidden_nodez": []
        }]
    }"#;
    assert!(
        parse_manifest(misspelled_forbidden_nodes).is_err(),
        "a misspelled forbidden_nodes key must not be silently ignored"
    );

    let misspelled_forbidden_parent_kind = r#"{
        "fixtures": [{
            "id": "fixture",
            "forbidden_nodes": [{
                "id": "no_block",
                "kind": "Block",
                "line": 1,
                "parent_knd": "Program"
            }]
        }]
    }"#;
    assert!(
        parse_manifest(misspelled_forbidden_parent_kind).is_err(),
        "a misspelled forbidden_nodes parent_kind key must not be silently ignored"
    );
}

#[test]
fn manifest_rejects_arbitrary_unknown_json_keys_at_each_validated_level() {
    let mut root = json!({ "fixtures": [] });
    root.as_object_mut()
        .expect("test manifest root is an object")
        .insert("unknown_root_field_generated".to_string(), json!("unknown_root_value"));

    let mut fixture = json!({ "id": "fixture" });
    fixture
        .as_object_mut()
        .expect("test fixture is an object")
        .insert("unknown_fixture_field_generated".to_string(), json!("unknown_fixture_value"));
    let fixture_manifest = json!({ "fixtures": [fixture] });

    let mut ast_row = json!({ "id": "expectation", "kind": "String", "line": 1 });
    ast_row
        .as_object_mut()
        .expect("test AST row is an object")
        .insert("unknown_ast_row_field_generated".to_string(), json!("unknown_ast_row_value"));
    let ast_fixture = json!({ "id": "fixture", "ast_expectations": [ast_row] });
    let ast_manifest = json!({ "fixtures": [ast_fixture] });

    let mut forbidden_row = json!({ "id": "forbidden", "kind": "Block", "line": 1 });
    forbidden_row.as_object_mut().expect("test forbidden row is an object").insert(
        "unknown_forbidden_row_field_generated".to_string(),
        json!("unknown_forbidden_row_value"),
    );
    let forbidden_fixture = json!({ "id": "fixture", "forbidden_nodes": [forbidden_row] });
    let forbidden_manifest = json!({ "fixtures": [forbidden_fixture] });

    let cases = [
        ("root", root, "unknown_root_field_generated"),
        ("fixture", fixture_manifest, "unknown_fixture_field_generated"),
        ("AST row", ast_manifest, "unknown_ast_row_field_generated"),
        ("forbidden row", forbidden_manifest, "unknown_forbidden_row_field_generated"),
    ];
    for (location, manifest, unknown_key) in cases {
        let rendered = manifest.to_string();
        let error = parse_manifest(&rendered)
            .expect_err("an arbitrary unknown JSON key must be rejected by schema validation");
        assert!(
            error.to_string().contains(unknown_key),
            "{location} validation diagnostic must identify generated key `{unknown_key}`: {error}"
        );
    }
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

#[test]
fn invalid_nodekind_diagnostic_identifies_the_reference() {
    let error = validate_reference_rows(
        "quote_like",
        &[NodeKindReference {
            id: "string_under_statement".to_string(),
            kind: "ExpressionStatment".to_string(),
            parent_kind: None,
        }],
        AST_KIND_FIELD,
        AST_PARENT_KIND_FIELD,
    )
    .expect_err("the misspelled NodeKind must be rejected");

    let rendered = error.to_string();
    for expected in ["quote_like", "string_under_statement", AST_KIND_FIELD, "ExpressionStatment"] {
        assert!(rendered.contains(expected), "diagnostic `{rendered}` must contain `{expected}`");
    }
}

fn parse_manifest(
    json: &str,
) -> Result<ParserAccuracyNodeKindManifest, Box<dyn std::error::Error>> {
    let value = parse_strict_json_value(json)?;
    validate_manifest_schema(&value)?;
    Ok(serde_json::from_value(value)?)
}

fn parse_strict_json_value(json: &str) -> Result<Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let value = StrictJsonValueSeed::root().deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

fn validate_manifest_schema(value: &Value) -> Result<(), ManifestSchemaError> {
    let root = value.as_object().ok_or_else(|| {
        ManifestSchemaError("parser-accuracy manifest must be a JSON object".to_string())
    })?;
    reject_unknown_fields(root, &["schema_version", "fixtures"], "manifest")?;

    let fixtures = root
        .get("fixtures")
        .and_then(Value::as_array)
        .ok_or_else(|| ManifestSchemaError("manifest.fixtures must be an array".to_string()))?;

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let fixture_path = format!("manifest.fixtures[{fixture_index}]");
        let fixture_object = fixture
            .as_object()
            .ok_or_else(|| ManifestSchemaError(format!("{fixture_path} must be a JSON object")))?;
        reject_unknown_fields(fixture_object, PARSER_ACCURACY_FIXTURE_FIELDS, &fixture_path)?;
        validate_reference_collection(
            fixture_object,
            "ast_expectations",
            AST_EXPECTATION_FIELDS,
            &fixture_path,
        )?;
        validate_reference_collection(
            fixture_object,
            "forbidden_nodes",
            FORBIDDEN_NODE_FIELDS,
            &fixture_path,
        )?;
    }

    Ok(())
}

fn validate_reference_collection(
    fixture: &Map<String, Value>,
    field: &str,
    allowed_fields: &[&str],
    fixture_path: &str,
) -> Result<(), ManifestSchemaError> {
    let Some(collection) = fixture.get(field) else {
        return Ok(());
    };
    let collection = collection
        .as_array()
        .ok_or_else(|| ManifestSchemaError(format!("{fixture_path}.{field} must be an array")))?;
    for (row_index, row) in collection.iter().enumerate() {
        let row_path = format!("{fixture_path}.{field}[{row_index}]");
        let row_object = row
            .as_object()
            .ok_or_else(|| ManifestSchemaError(format!("{row_path} must be a JSON object")))?;
        reject_unknown_fields(row_object, allowed_fields, &row_path)?;
    }
    Ok(())
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed_fields: &[&str],
    object_path: &str,
) -> Result<(), ManifestSchemaError> {
    if let Some(unknown) = object.keys().find(|key| !allowed_fields.contains(&key.as_str())) {
        return Err(ManifestSchemaError(format!(
            "{object_path} contains unknown field `{unknown}`"
        )));
    }
    Ok(())
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
