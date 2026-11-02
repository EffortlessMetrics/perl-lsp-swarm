//! Schema-derived standard initialize `Capabilities` projection (#9534).
//!
//! The committed inventory at `.ci/dap/initialize-capability-schema.v1.json`
//! is the durable authority for standard wire fields, shapes, and upstream
//! request/event hints. Detailed presence/nullability semantics remain owned
//! by the #9404 inventory referenced from each row.

use serde::Deserialize;

const INVENTORY_JSON: &str = include_str!("../../../.ci/dap/initialize-capability-schema.v1.json");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct NullabilityRowRef {
    pub definition: String,
    pub field: String,
    pub class: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WireShape {
    pub kind: String,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub items: Option<Box<WireShape>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StandardCapabilitySchemaRow {
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
pub struct ExtensionCapabilityCandidateRow {
    pub row_id: String,
    pub classification: String,
    pub wire_name: String,
    pub rust_owner: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InitializeCapabilitySchemaInventory {
    pub schema_version: String,
    pub standard_row_count: usize,
    pub extension_candidate_count: usize,
    pub standard_rows: Vec<StandardCapabilitySchemaRow>,
    pub extension_candidates: Vec<ExtensionCapabilityCandidateRow>,
}

/// Parse the committed initialize capability schema inventory.
pub fn initialize_capability_schema_inventory()
-> Result<InitializeCapabilitySchemaInventory, serde_json::Error> {
    serde_json::from_str(INVENTORY_JSON)
}

/// Deterministic standard capability rows sorted by `row_id`.
pub fn standard_initialize_capability_rows()
-> Result<Vec<StandardCapabilitySchemaRow>, serde_json::Error> {
    Ok(initialize_capability_schema_inventory()?.standard_rows)
}

/// Repository custom initialize fields that are not part of the upstream standard projection.
pub fn extension_initialize_capability_candidates()
-> Result<Vec<ExtensionCapabilityCandidateRow>, serde_json::Error> {
    Ok(initialize_capability_schema_inventory()?.extension_candidates)
}
