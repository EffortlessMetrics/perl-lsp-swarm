//! Schema-derived standard initialize `Capabilities` projection (#9534).
//!
//! The committed inventory at `.ci/dap/initialize-capability-schema.v1.json`
//! is the durable authority for standard wire fields, shapes, and upstream
//! request/event hints. Detailed presence/nullability semantics remain owned
//! by the #9404 inventory referenced from each row.

use serde::Deserialize;

const INVENTORY_JSON: &str = include_str!("../../../.ci/dap/initialize-capability-schema.v1.json");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct NullabilityRowRef {
    pub definition: String,
    pub field: String,
    pub class: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct WireShape {
    pub kind: String,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub items: Option<Box<WireShape>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct StandardCapabilitySchemaRow {
    pub row_id: String,
    pub classification: String,
    pub wire_name: String,
    pub upstream_definition: String,
    pub upstream_property: String,
    pub schema_required: bool,
    pub wire_shape: WireShape,
    pub nullability_row: NullabilityRowRef,
    pub related_requests: Vec<String>,
    pub related_events: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ExtensionCapabilityCandidateRow {
    pub row_id: String,
    pub classification: String,
    pub wire_name: String,
    pub rust_owner: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct InitializeCapabilitySchemaInventory {
    pub schema_version: String,
    pub standard_row_count: usize,
    pub extension_candidate_count: usize,
    pub standard_rows: Vec<StandardCapabilitySchemaRow>,
    pub extension_candidates: Vec<ExtensionCapabilityCandidateRow>,
}

/// Parse the committed initialize capability schema inventory.
pub(crate) fn initialize_capability_schema_inventory()
-> Result<InitializeCapabilitySchemaInventory, serde_json::Error> {
    serde_json::from_str(INVENTORY_JSON)
}

/// Deterministic standard capability rows sorted by `row_id`.
pub(crate) fn standard_initialize_capability_rows()
-> Result<Vec<StandardCapabilitySchemaRow>, serde_json::Error> {
    Ok(initialize_capability_schema_inventory()?.standard_rows)
}

/// Repository custom initialize fields that are not part of the upstream standard projection.
pub(crate) fn extension_initialize_capability_candidates()
-> Result<Vec<ExtensionCapabilityCandidateRow>, serde_json::Error> {
    Ok(initialize_capability_schema_inventory()?.extension_candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn standard_row_set_matches_pinned_upstream_capabilities_properties() -> TestResult {
        let rows = standard_initialize_capability_rows()?;
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../.ci/dap/upstream-debugAdapterProtocol.json"
        ))?;
        let properties = schema["definitions"]["Capabilities"]["properties"]
            .as_object()
            .ok_or("upstream Capabilities.properties missing")?;
        let upstream: BTreeSet<String> = properties.keys().cloned().collect();
        let projected: BTreeSet<String> = rows.iter().map(|row| row.wire_name.clone()).collect();
        assert_eq!(projected, upstream);
        Ok(())
    }

    #[test]
    fn every_standard_row_links_to_a_nullability_inventory_row() -> TestResult {
        let nullability: serde_json::Value =
            serde_json::from_str(include_str!("../../../.ci/dap/nullability-inventory.v1.json"))?;
        let indexed: BTreeMap<(String, String), &serde_json::Value> = nullability["rows"]
            .as_array()
            .ok_or("nullability rows missing")?
            .iter()
            .filter_map(|row| {
                let definition = row.get("definition")?.as_str()?.to_owned();
                let field = row.get("field")?.as_str()?.to_owned();
                Some(((definition, field), row))
            })
            .collect();
        for row in standard_initialize_capability_rows()? {
            let key = (row.nullability_row.definition.clone(), row.nullability_row.field.clone());
            let null_row =
                indexed.get(&key).ok_or_else(|| format!("missing nullability row for {key:?}"))?;
            assert_eq!(
                null_row.get("class").and_then(|value| value.as_str()),
                Some(row.nullability_row.class.as_str())
            );
        }
        Ok(())
    }

    #[test]
    fn extension_candidates_remain_outside_the_standard_projection() -> TestResult {
        let standard: BTreeSet<String> = standard_initialize_capability_rows()?
            .iter()
            .map(|row| row.wire_name.clone())
            .collect();
        let extensions = extension_initialize_capability_candidates()?;
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].wire_name, "supportsInlineValues");
        assert!(!standard.contains("supportsInlineValues"));
        Ok(())
    }

    #[test]
    fn multi_request_descriptions_preserve_full_request_families() -> TestResult {
        let rows = standard_initialize_capability_rows()?;
        let step_back = rows
            .iter()
            .find(|row| row.wire_name == "supportsStepBack")
            .ok_or("missing supportsStepBack row")?;
        assert_eq!(
            step_back.related_requests,
            vec!["stepBack".to_owned(), "reverseContinue".to_owned()]
        );
        let formatting = rows
            .iter()
            .find(|row| row.wire_name == "supportsValueFormattingOptions")
            .ok_or("missing supportsValueFormattingOptions row")?;
        assert_eq!(
            formatting.related_requests,
            vec!["stackTrace".to_owned(), "variables".to_owned(), "evaluate".to_owned(),]
        );
        Ok(())
    }
}
