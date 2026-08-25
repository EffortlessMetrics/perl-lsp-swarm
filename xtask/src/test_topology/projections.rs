//! Deterministic JSON, Markdown, and human-report projections.
//!
//! Scaffolding commit (#12125): projection signatures and their determinism
//! obligations are fixed here; real renderers land with the implementation
//! commit.

use super::model::TestTopologyInventoryV1;

/// Serializes the inventory as pretty JSON with a trailing newline.
///
/// Scaffolding stub: fails loudly instead of emitting partial artifacts.
pub fn render_json(_inventory: &TestTopologyInventoryV1) -> anyhow::Result<String> {
    Err(anyhow::anyhow!(
        "topology JSON projection lands with the #12125 implementation commit"
    ))
}

/// Renders the Markdown projection of the inventory.
///
/// Scaffolding stub: emits a placeholder header only.
pub fn render_markdown(_inventory: &TestTopologyInventoryV1) -> String {
    "# Compiler-Critical Test Topology Inventory\n\nprojection pending #12125\n".to_string()
}

/// Renders the deterministic human report.
///
/// Scaffolding stub: emits a placeholder line only.
pub fn render_report(inventory: &TestTopologyInventoryV1) -> String {
    format!(
        "test-topology report: cohort={} schema={} rows={}\n",
        inventory.cohort,
        inventory.schema_id,
        inventory.rows.len(),
    )
}
