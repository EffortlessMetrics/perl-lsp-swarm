//! Keep the checked-in receipt schema locked to the serialized UX contract.

use perl_lsp_ux_tests::{UxComponent, UxFailureClass};
use serde_json::Value;
use std::collections::BTreeSet;
use std::error::Error;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn receipt_schema() -> TestResult<Value> {
    Ok(serde_json::from_str(include_str!("../../../.ci/schemas/ux-scenario-run.schema.json"))?)
}

fn serialized_component(component: UxComponent) -> TestResult<String> {
    let value = serde_json::to_value(component)?;
    Ok(value.as_str().ok_or("serialized UX component must be a string")?.to_owned())
}

fn string_set(value: &Value, context: &str) -> TestResult<BTreeSet<String>> {
    let entries = value.as_array().ok_or_else(|| format!("{context} must be an array"))?;
    Ok(entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{context} entries must be strings"))
        })
        .collect::<Result<_, _>>()?)
}

#[test]
fn receipt_schema_accepts_every_ux_component() -> TestResult {
    let schema = receipt_schema()?;
    let schema_components =
        string_set(&schema["properties"]["component"]["enum"], "receipt component enum")?;

    // `UxComponent::ALL` is kept adjacent to the enum and guarded by an
    // exhaustive witness, so a new variant must be registered there before
    // this comparison can pass; a missing schema entry then fails here.
    let taxonomy_components: BTreeSet<String> =
        UxComponent::ALL.iter().copied().map(serialized_component).collect::<Result<_, _>>()?;

    assert_eq!(
        schema_components, taxonomy_components,
        "receipt schema component enum drifted from UxComponent"
    );
    Ok(())
}

#[test]
fn receipt_schema_preserves_explicit_null_measurement_states() -> TestResult {
    let schema = receipt_schema()?;
    let properties = &schema["properties"];

    let top_level_timing =
        string_set(&properties["time_to_first_useful_result_ms"]["type"], "top-level timing type")?;
    assert_eq!(
        top_level_timing,
        BTreeSet::from(["null".to_string(), "number".to_string()]),
        "top-level timing must admit measured numbers and explicit null"
    );

    let operation_timing = string_set(
        &properties["operation_timings"]["items"]["properties"]["time_to_first_useful_result_ms"]["type"],
        "operation timing type",
    )?;
    assert_eq!(
        operation_timing,
        BTreeSet::from(["null".to_string(), "number".to_string()]),
        "per-operation timing must admit measured numbers and explicit null"
    );

    let failure_class = &properties["failure_class"];
    let any_of = failure_class["anyOf"].as_array().ok_or("failure_class anyOf must be an array")?;
    assert!(
        any_of.iter().any(|branch| branch["type"] == "null"),
        "passing receipts may carry an explicit null failure_class"
    );
    assert!(
        any_of.iter().any(|branch| branch["$ref"] == "#/$defs/failure_class"),
        "non-null failure_class must reference the shared $defs enum"
    );

    Ok(())
}

#[test]
fn receipt_schema_failure_class_is_defined_once() -> TestResult {
    let schema = receipt_schema()?;

    // The shared definition is the only place a class list may appear.
    let shared_classes =
        string_set(&schema["$defs"]["failure_class"]["enum"], "$defs.failure_class enum")?;
    assert!(!shared_classes.is_empty(), "$defs.failure_class must enumerate the classes");

    let taxonomy_classes: BTreeSet<String> = [
        UxFailureClass::ProviderRegression,
        UxFailureClass::ServerCrash,
        UxFailureClass::Timeout,
        UxFailureClass::TestRace,
        UxFailureClass::Infra,
        UxFailureClass::MatrixDrift,
        UxFailureClass::BaselineDrift,
        UxFailureClass::NewTestBug,
        UxFailureClass::Unknown,
    ]
    .into_iter()
    .map(serialize_failure_class)
    .collect::<Result<_, _>>()?;
    assert_eq!(
        shared_classes, taxonomy_classes,
        "$defs.failure_class drifted from the UxFailureClass taxonomy"
    );

    // The top-level property admits null plus the shared reference only.
    let top_level = &schema["properties"]["failure_class"];
    assert!(
        top_level.get("enum").is_none(),
        "top-level failure_class must not re-declare the class list"
    );
    let top_level_any_of =
        top_level["anyOf"].as_array().ok_or("top-level failure_class must use anyOf")?;
    let expected_branches = serde_json::json!([
        { "$ref": "#/$defs/failure_class" },
        { "type": "null" }
    ]);
    assert_eq!(
        Value::Array(top_level_any_of.clone()),
        expected_branches,
        "top-level failure_class drifted from the shared definition"
    );

    // The fail/quarantined/skipped branch reuses the shared reference.
    let conditional_branch = &schema["allOf"][1]["then"]["properties"]["failure_class"];
    assert_eq!(
        conditional_branch,
        &serde_json::json!({ "$ref": "#/$defs/failure_class" }),
        "conditional failure_class must reference the shared definition"
    );

    Ok(())
}

fn serialize_failure_class(class: UxFailureClass) -> TestResult<String> {
    let value = serde_json::to_value(class)?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "serialized UX failure class must be a string".into())
}
