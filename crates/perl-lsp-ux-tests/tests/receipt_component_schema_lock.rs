//! Keep the receipt schema component enum locked to the serialized UX taxonomy.

use perl_lsp_ux_tests::UxComponent;
use std::collections::BTreeSet;

fn serialized_component(component: UxComponent) -> Result<String, Box<dyn std::error::Error>> {
    let value = serde_json::to_value(component)?;
    Ok(value
        .as_str()
        .ok_or("serialized UX component must be a string")?
        .to_owned())
}

#[test]
fn receipt_schema_accepts_every_ux_component() -> Result<(), Box<dyn std::error::Error>> {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../.ci/schemas/ux-scenario-run.schema.json"
    ))?;
    let schema_components: BTreeSet<String> = schema["properties"]["component"]["enum"]
        .as_array()
        .ok_or("receipt schema component enum must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or("receipt schema component entries must be strings")
        })
        .collect::<Result<_, _>>()?;

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
