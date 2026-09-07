//! Validate the differential real-Perl oracle receipt schema.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use xtask::schema_apply::validate_payload_against_schema;

const SCHEMA_PATH: &str = "schemas/oracle_receipt.v1.schema.json";
/// Canonical conforming receipt. Walking the schema proves its vocabulary
/// has not drifted; only applying it to an instance proves the published
/// contract binds anything at all (#14268).
const RECEIPT_PATH: &str = "fixtures/oracle_receipt/canonical_receipt.v1.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/oracle_receipt.v1.schema.json";
const SCHEMA_VERSION: &str = "oracle_receipt.v1";

const REQUIRED_TOP_LEVEL_FIELDS: &[&str] = &[
    "schema_version",
    "receipt_id",
    "comparison_class",
    "fixture_id",
    "source_snapshot",
    "rust_extractor",
    "perl_oracle",
    "module_path_authority",
    "environment",
    "ambient_inputs",
    "generated_inputs",
    "dynamic_boundaries",
    "stale_facts",
    "unsupported_effects",
    "normalized_facts",
    "comparisons",
    "provider_behavior_changed",
    "editor_runtime_dependency",
    "redaction",
    "claim_boundary",
];

const REQUIRED_COMPARISON_CLASSES: &[&str] = &[
    "PackageSubTable",
    "ImportExport",
    "IsaComposition",
    "ConstantPrototype",
    "FrameworkGeneratedMember",
    "CompileEffect",
];

const REQUIRED_RESULT_CLASSES: &[&str] = &[
    "oracle_agrees",
    "compiler_missing",
    "compiler_extra",
    "range_mismatch",
    "provenance_mismatch",
    "confidence_or_freshness_mismatch",
    "dynamic_or_unsupported",
    "oracle_ambient_unbounded",
    "stale_or_partial",
    "unknown",
];

const REQUIRED_PROMOTION_EFFECTS: &[&str] =
    &["supports_promotion", "blocks_promotion", "known_limitation", "unknown"];

const REQUIRED_FACT_PROVENANCE: &[&str] = &[
    "ExplicitSource",
    "SourceBackedGenerated",
    "GeneratedNoSource",
    "DynamicBoundary",
    "AmbientInput",
    "Unknown",
];

const REQUIRED_CLAIM_FIELDS: &[&str] =
    &["provider_behavior_changed", "editor_runtime_dependency", "claim_boundary"];

#[derive(Debug)]
struct ValidationStats {
    required_fields: usize,
    comparison_classes: usize,
    result_classes: usize,
    promotion_effects: usize,
    receipts_applied: usize,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "oracle receipt schema check passed: {} required fields, {} comparison classes, {} result classes, {} promotion effects, {} receipt(s) applied",
        stats.required_fields,
        stats.comparison_classes,
        stats.result_classes,
        stats.promotion_effects,
        stats.receipts_applied,
    );
    Ok(())
}

fn validate(root: &Path) -> Result<ValidationStats> {
    let (stats, violations) = collect(root)?;

    if !violations.is_empty() {
        eprintln!("oracle receipt schema violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!(
            "oracle receipt schema check failed with {} violation(s): {}",
            violations.len(),
            violations.join("; ")
        );
    }

    Ok(stats)
}

/// Collect every violation without reporting, so tests can assert on the
/// exact findings and on which layer produced them.
fn collect(root: &Path) -> Result<(ValidationStats, Vec<String>)> {
    let schema = read_schema(root, SCHEMA_PATH)?;
    let mut violations = Vec::new();

    validate_schema_shape(&schema, &mut violations);

    // The shape walk above only proves the schema still declares the
    // vocabulary this task pins. Applying the compiled schema to a real
    // receipt is what makes `additionalProperties`, `uniqueItems`,
    // `minItems`, `minLength`, `minimum` and every nested `$defs`
    // constraint load-bearing for schema-only consumers.
    let receipt = read_schema(root, RECEIPT_PATH)?;
    violations.extend(validate_payload_against_schema(
        &schema,
        SCHEMA_PATH,
        &receipt,
        RECEIPT_PATH,
    )?);

    let stats = ValidationStats {
        required_fields: REQUIRED_TOP_LEVEL_FIELDS.len(),
        comparison_classes: REQUIRED_COMPARISON_CLASSES.len(),
        result_classes: REQUIRED_RESULT_CLASSES.len(),
        promotion_effects: REQUIRED_PROMOTION_EFFECTS.len(),
        receipts_applied: 1,
    };
    Ok((stats, violations))
}

fn read_schema(root: &Path, rel: &str) -> Result<Value> {
    let path = root.join(rel);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {rel}"))
}

fn validate_schema_shape(schema: &Value, violations: &mut Vec<String>) {
    require_string_at(schema, &["$id"], SCHEMA_ID, violations);
    require_const(schema, &["properties", "schema_version", "const"], SCHEMA_VERSION, violations);
    require_const(schema, &["properties", "editor_runtime_dependency", "const"], false, violations);
    require_string_at(
        schema,
        &["properties", "provider_behavior_changed", "type"],
        "boolean",
        violations,
    );
    require_string_at(
        schema,
        &["properties", "comparisons", "items", "$ref"],
        "#/$defs/comparison_result",
        violations,
    );
    require_required_set(schema, REQUIRED_TOP_LEVEL_FIELDS, violations);
    require_enum_set(schema, "comparison_class", REQUIRED_COMPARISON_CLASSES, violations);
    require_enum_set(schema, "result_class", REQUIRED_RESULT_CLASSES, violations);
    require_enum_set(schema, "promotion_effect", REQUIRED_PROMOTION_EFFECTS, violations);
    require_enum_set(schema, "fact_provenance", REQUIRED_FACT_PROVENANCE, violations);
    require_required_subset(
        schema,
        &["$defs", "comparison_result", "required"],
        &["result_class", "fact_id", "promotion_effect", "message"],
        violations,
    );
    require_required_subset(
        schema,
        &["$defs", "normalized_fact", "required"],
        &["fact_id", "name", "provenance", "confidence", "freshness", "fallback", "source_range"],
        violations,
    );
    require_required_subset(
        schema,
        &["$defs", "redaction", "required"],
        &["private_paths_redacted", "environment_values_redacted", "raw_launch_payloads_redacted"],
        violations,
    );
    require_required_subset(schema, &["required"], REQUIRED_CLAIM_FIELDS, violations);
}

fn require_string_at(schema: &Value, path: &[&str], expected: &str, violations: &mut Vec<String>) {
    match lookup(schema, path).and_then(Value::as_str) {
        Some(actual) if actual == expected => {}
        Some(actual) => violations.push(format!(
            "{}: {} is {:?}; expected {:?}",
            SCHEMA_PATH,
            path.join("."),
            actual,
            expected
        )),
        None => violations.push(format!("{SCHEMA_PATH}: missing string field {}", path.join("."))),
    }
}

fn require_const(
    schema: &Value,
    path: &[&str],
    expected: impl Into<ExpectedConst>,
    violations: &mut Vec<String>,
) {
    let expected = expected.into();
    match lookup(schema, path) {
        Some(value) if expected.matches(value) => {}
        Some(value) => violations.push(format!(
            "{SCHEMA_PATH}: {} is {}; expected {}",
            path.join("."),
            value,
            expected.label()
        )),
        None => violations.push(format!("{SCHEMA_PATH}: missing {}", path.join("."))),
    }
}

enum ExpectedConst {
    String(&'static str),
    Bool(bool),
}

impl ExpectedConst {
    fn matches(&self, value: &Value) -> bool {
        match self {
            Self::String(expected) => value.as_str() == Some(*expected),
            Self::Bool(expected) => value.as_bool() == Some(*expected),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::String(value) => format!("{value:?}"),
            Self::Bool(value) => value.to_string(),
        }
    }
}

impl From<&'static str> for ExpectedConst {
    fn from(value: &'static str) -> Self {
        Self::String(value)
    }
}

impl From<bool> for ExpectedConst {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

fn require_required_set(schema: &Value, expected: &[&str], violations: &mut Vec<String>) {
    require_exact_array_set(schema, &["required"], expected, violations);
}

fn require_enum_set(schema: &Value, def: &str, expected: &[&str], violations: &mut Vec<String>) {
    require_exact_array_set(schema, &["$defs", def, "enum"], expected, violations);
}

fn require_exact_array_set(
    schema: &Value,
    path: &[&str],
    expected: &[&str],
    violations: &mut Vec<String>,
) {
    let Some(actual) = string_array_at(schema, path, violations) else {
        return;
    };
    let actual_set = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let expected_set = expected.iter().copied().collect::<BTreeSet<_>>();
    for missing in expected_set.difference(&actual_set) {
        violations.push(format!(
            "{}: {} missing required entry {missing:?}",
            SCHEMA_PATH,
            path.join(".")
        ));
    }
    for unexpected in actual_set.difference(&expected_set) {
        violations.push(format!(
            "{}: {} contains unsupported entry {unexpected:?}",
            SCHEMA_PATH,
            path.join(".")
        ));
    }
}

fn require_required_subset(
    schema: &Value,
    path: &[&str],
    expected: &[&str],
    violations: &mut Vec<String>,
) {
    let Some(actual) = string_array_at(schema, path, violations) else {
        return;
    };
    let actual_set = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
    for missing in expected {
        if !actual_set.contains(*missing) {
            violations.push(format!(
                "{}: {} missing required entry {missing:?}",
                SCHEMA_PATH,
                path.join(".")
            ));
        }
    }
}

fn string_array_at(
    schema: &Value,
    path: &[&str],
    violations: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Some(value) = lookup(schema, path) else {
        violations.push(format!("{SCHEMA_PATH}: missing {}", path.join(".")));
        return None;
    };
    let Some(values) = value.as_array() else {
        violations.push(format!("{SCHEMA_PATH}: {} must be an array", path.join(".")));
        return None;
    };
    let mut out = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let Some(text) = value.as_str() else {
            violations.push(format!(
                "{}: {}[{index}] must be a string",
                SCHEMA_PATH,
                path.join(".")
            ));
            continue;
        };
        out.push(text.to_string());
    }
    Some(out)
}

fn lookup<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for segment in path {
        value = value.get(*segment)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    #[test]
    fn accepts_current_receipt_schema() -> TestResult {
        let tempdir = schema_workspace(current_schema_text())?;

        let stats = validate(tempdir.path())?;

        assert_eq!(stats.required_fields, REQUIRED_TOP_LEVEL_FIELDS.len());
        assert_eq!(stats.comparison_classes, REQUIRED_COMPARISON_CLASSES.len());
        assert_eq!(stats.result_classes, REQUIRED_RESULT_CLASSES.len());
        Ok(())
    }

    #[test]
    fn rejects_missing_result_class() -> TestResult {
        let tempdir =
            schema_workspace(current_schema_text().replace("\"unknown\"", "\"unknown_removed\""))?;

        let err = validation_error(tempdir.path())?;

        assert!(
            err.to_string().contains("oracle receipt schema check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_editor_runtime_dependency_not_false() -> TestResult {
        let tempdir = schema_workspace(current_schema_text().replace(
            r#""editor_runtime_dependency": {
      "const": false
    }"#,
            r#""editor_runtime_dependency": {
      "type": "boolean"
    }"#,
        ))?;

        let err = validation_error(tempdir.path())?;

        assert!(
            err.to_string().contains("oracle receipt schema check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_provider_behavior_changed_not_boolean() -> TestResult {
        let tempdir = schema_workspace(current_schema_text().replace(
            r#""provider_behavior_changed": {
      "type": "boolean"
    }"#,
            r#""provider_behavior_changed": {
      "const": false
    }"#,
        ))?;

        let err = validation_error(tempdir.path())?;

        assert!(
            err.to_string().contains("oracle receipt schema check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    fn schema_workspace(schema_text: String) -> TestResult<tempfile::TempDir> {
        workspace(schema_text, current_receipt_text())
    }

    fn receipt_workspace(receipt_text: String) -> TestResult<tempfile::TempDir> {
        workspace(current_schema_text(), receipt_text)
    }

    fn workspace(schema_text: String, receipt_text: String) -> TestResult<tempfile::TempDir> {
        let tempdir = tempfile::tempdir()?;
        fs::create_dir_all(tempdir.path().join("schemas"))?;
        fs::create_dir_all(tempdir.path().join("fixtures/oracle_receipt"))?;
        fs::write(tempdir.path().join(SCHEMA_PATH), schema_text)?;
        fs::write(tempdir.path().join(RECEIPT_PATH), receipt_text)?;
        Ok(tempdir)
    }

    fn current_receipt_text() -> String {
        include_str!("../../../fixtures/oracle_receipt/canonical_receipt.v1.json").to_string()
    }

    fn mutated_receipt(mutate: impl FnOnce(&mut Value)) -> TestResult<String> {
        let mut receipt: Value = serde_json::from_str(&current_receipt_text())?;
        mutate(&mut receipt);
        Ok(serde_json::to_string_pretty(&receipt)?)
    }

    /// Every mutation below is rejected by the applied schema, and every one
    /// of them is invisible to `validate_schema_shape`, which never reads a
    /// receipt. This is the discriminating proof for #14268: without the
    /// apply step the published contract binds no producer at all.
    fn assert_receipt_rejected(
        label: &str,
        mutate: impl FnOnce(&mut Value),
    ) -> TestResult<Vec<String>> {
        let tempdir = receipt_workspace(mutated_receipt(mutate)?)?;

        // Negative control: the schema itself is untouched, so the walk-only
        // layer reports nothing and would have passed this receipt.
        let mut shape_violations = Vec::new();
        validate_schema_shape(&read_schema(tempdir.path(), SCHEMA_PATH)?, &mut shape_violations);
        assert!(
            shape_violations.is_empty(),
            "{label}: the schema walk should be clean; only the applied schema catches this: {shape_violations:?}"
        );

        let (_, violations) = collect(tempdir.path())?;
        assert!(!violations.is_empty(), "{label}: the applied schema must reject this receipt");
        assert!(
            violations.iter().all(|violation| violation.starts_with(RECEIPT_PATH)),
            "{label}: every violation must name the receipt: {violations:?}"
        );

        // The reporting path must fail too, not merely collect findings.
        let err = validation_error(tempdir.path())?;
        assert!(
            err.to_string().contains("oracle receipt schema check failed"),
            "{label}: unexpected error: {err:?}"
        );
        Ok(violations)
    }

    #[test]
    fn accepts_the_canonical_receipt() -> TestResult {
        let tempdir = schema_workspace(current_schema_text())?;

        let stats = validate(tempdir.path())?;

        assert_eq!(stats.receipts_applied, 1);
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_unknown_top_level_field() -> TestResult {
        assert_receipt_rejected("unknown top-level field", |receipt| {
            receipt["surprise"] = Value::String("unexpected".to_string());
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_unknown_nested_field() -> TestResult {
        assert_receipt_rejected("unknown nested field", |receipt| {
            receipt["redaction"]["bonus"] = Value::Bool(true);
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_duplicate_denied_environment_entries() -> TestResult {
        assert_receipt_rejected("duplicate environment denial", |receipt| {
            receipt["environment"]["denied"] = serde_json::json!(["PERL5LIB", "PERL5LIB"]);
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_undeclared_environment_denial() -> TestResult {
        assert_receipt_rejected("undeclared environment denial", |receipt| {
            receipt["environment"]["denied"] = serde_json::json!(["PERL5SHLIB"]);
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_no_comparisons() -> TestResult {
        assert_receipt_rejected("empty comparisons", |receipt| {
            receipt["comparisons"] = Value::Array(Vec::new());
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_result_class_outside_the_closed_vocabulary() -> TestResult {
        assert_receipt_rejected("result_class outside enum", |receipt| {
            receipt["comparisons"][0]["result_class"] =
                Value::String("mostly_agrees".to_string());
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_empty_claim_boundary() -> TestResult {
        assert_receipt_rejected("empty claim boundary", |receipt| {
            receipt["claim_boundary"] = Value::String(String::new());
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_missing_a_nested_required_field() -> TestResult {
        assert_receipt_rejected("missing nested required field", |receipt| {
            if let Some(redaction) = receipt["redaction"].as_object_mut() {
                redaction.remove("raw_launch_payloads_redacted");
            }
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_claiming_an_editor_runtime_dependency() -> TestResult {
        assert_receipt_rejected("editor runtime dependency", |receipt| {
            receipt["editor_runtime_dependency"] = Value::Bool(true);
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_confidence_outside_the_closed_vocabulary() -> TestResult {
        assert_receipt_rejected("confidence outside enum", |receipt| {
            receipt["normalized_facts"]["rust"][0]["confidence"] =
                Value::String("certain".to_string());
        })?;
        Ok(())
    }

    #[test]
    fn rejects_receipt_with_negative_source_range() -> TestResult {
        assert_receipt_rejected("negative source range", |receipt| {
            receipt["normalized_facts"]["rust"][0]["source_range"]["start_line"] =
                serde_json::json!(-1);
        })?;
        Ok(())
    }

    /// The violation names the receipt and the offending location, so a
    /// failure is actionable without re-reading the document.
    #[test]
    fn receipt_violation_names_the_payload_and_instance_path() -> TestResult {
        let violations = assert_receipt_rejected("instance path", |receipt| {
            receipt["normalized_facts"]["rust"][0]["confidence"] =
                Value::String("certain".to_string());
        })?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("/normalized_facts/rust/0/confidence")),
            "violation must carry the instance path: {violations:?}"
        );
        Ok(())
    }

    fn validation_error(root: &Path) -> TestResult<color_eyre::Report> {
        match validate(root) {
            Ok(_) => bail!("schema mutation should fail validation"),
            Err(err) => Ok(err),
        }
    }

    fn current_schema_text() -> String {
        include_str!("../../../schemas/oracle_receipt.v1.schema.json").to_string()
    }
}
