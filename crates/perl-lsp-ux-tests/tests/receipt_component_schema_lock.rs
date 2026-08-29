//! Keep the checked-in receipt schema locked to the serialized UX contract.

use perl_lsp_ux_tests::UxComponent;
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

    let taxonomy_components: BTreeSet<String> = [
        UxComponent::Completion,
        UxComponent::Diagnostics,
        UxComponent::ModuleResolution,
        UxComponent::WorkspaceSymbols,
        UxComponent::Rename,
        UxComponent::SafeDelete,
        UxComponent::Hover,
        UxComponent::GotoDefinition,
        UxComponent::SignatureHelp,
        UxComponent::CodeLens,
        UxComponent::FoldingRange,
        UxComponent::SemanticTokens,
        UxComponent::CodeActions,
        UxComponent::Infra,
        UxComponent::AiCompletion,
    ]
    .into_iter()
    .map(serialized_component)
    .collect::<Result<_, _>>()?;

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

    let failure_classes = properties["failure_class"]["enum"]
        .as_array()
        .ok_or("failure_class enum must be an array")?;
    assert!(
        failure_classes.iter().any(Value::is_null),
        "passing receipts may carry an explicit null failure_class"
    );

    Ok(())
}
