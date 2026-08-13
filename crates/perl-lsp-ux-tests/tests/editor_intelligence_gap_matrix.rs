use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::Value;

const HEADER: &str = include_str!("../fixtures/editor_intelligence_gap_matrix.json");
const CASES: &str = include_str!("../fixtures/editor_intelligence/case_manifest.pl");

#[derive(Clone, Copy)]
struct Row {
    id: &'static str,
    kind: &'static str,
    family: &'static str,
    tier: &'static str,
    protocol: &'static str,
    result: &'static str,
    proof: &'static str,
    confidence: &'static str,
    freshness: &'static str,
    editable: bool,
    owner: u64,
    marker: &'static str,
    blocker: &'static str,
}

const ROWS: &[Row] = &[
    Row { id: "completion.constructor_assignment_control", kind: "positive_control", family: "completion", tier: "workspace", protocol: "wired", result: "exact", proof: "representative_project", confidence: "high", freshness: "current", editable: true, owner: 6604, marker: "completion_constructor_assignment_control", blocker: "" },
    Row { id: "completion.method_return_chain.workspace", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "bounded_fallback", proof: "multi_file", confidence: "low", freshness: "current", editable: true, owner: 7439, marker: "completion_method_return_chain_workspace", blocker: "callable_result_not_live" },
    Row { id: "completion.method_return_chain.external_dependency", kind: "gap", family: "completion", tier: "external_dependency", protocol: "wired", result: "gap", proof: "none", confidence: "unknown", freshness: "not_proven", editable: false, owner: 7470, marker: "completion_method_return_chain_external_dependency", blocker: "external_source_not_admitted" },
    Row { id: "completion.returned_hash_shape.same_file", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "bounded_fallback", proof: "unit", confidence: "low", freshness: "current", editable: true, owner: 7440, marker: "completion_returned_hash_shape_same_file", blocker: "returned_shape_not_propagated" },
    Row { id: "completion.returned_hash_shape.cross_module", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "gap", proof: "multi_file", confidence: "unknown", freshness: "current", editable: true, owner: 7440, marker: "completion_returned_hash_shape_cross_module", blocker: "cross_module_shape_unavailable" },
    Row { id: "completion.typed_accessor_return", kind: "gap", family: "completion", tier: "generated", protocol: "wired", result: "bounded_fallback", proof: "unit", confidence: "medium", freshness: "current", editable: true, owner: 7438, marker: "completion_typed_accessor_return", blocker: "accessor_return_not_promoted" },
    Row { id: "completion.hashref_slot_receiver", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "bounded_fallback", proof: "unit", confidence: "low", freshness: "current", editable: true, owner: 7464, marker: "completion_hashref_slot_receiver", blocker: "hashref_slot_not_promoted" },
    Row { id: "completion.array_index_receiver", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "bounded_fallback", proof: "unit", confidence: "low", freshness: "current", editable: true, owner: 7464, marker: "completion_array_index_receiver", blocker: "array_index_not_promoted" },
    Row { id: "completion.branch_join_receiver_union", kind: "gap", family: "completion", tier: "workspace", protocol: "wired", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7465, marker: "completion_branch_join_receiver_union", blocker: "branch_join_not_modeled" },
    Row { id: "type_flow.ref_hash_narrowing", kind: "gap", family: "type_flow", tier: "workspace", protocol: "not_applicable", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7437, marker: "type_flow_ref_hash_narrowing", blocker: "ref_narrowing_missing" },
    Row { id: "type_flow.isa_narrowing", kind: "gap", family: "type_flow", tier: "workspace", protocol: "not_applicable", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7437, marker: "type_flow_isa_narrowing", blocker: "isa_narrowing_missing" },
    Row { id: "type_flow.defined_narrowing", kind: "gap", family: "type_flow", tier: "workspace", protocol: "not_applicable", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7437, marker: "type_flow_defined_narrowing", blocker: "defined_narrowing_missing" },
    Row { id: "framework.mojo_base_accessors_and_parent", kind: "gap", family: "framework", tier: "generated", protocol: "wired", result: "gap", proof: "none", confidence: "unknown", freshness: "not_proven", editable: true, owner: 7441, marker: "framework_mojo_base_accessors_and_parent", blocker: "mojo_base_adapter_missing" },
    Row { id: "framework.dbix_class_components", kind: "gap", family: "framework", tier: "generated", protocol: "wired", result: "bounded_fallback", proof: "unit", confidence: "medium", freshness: "current", editable: true, owner: 7443, marker: "framework_dbix_class_components", blocker: "component_fact_missing" },
    Row { id: "framework.native_class_does", kind: "gap", family: "framework", tier: "workspace", protocol: "wired", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7444, marker: "framework_native_class_does", blocker: "native_role_fact_missing" },
    Row { id: "dispatch.literal_method_selector", kind: "gap", family: "dispatch", tier: "workspace", protocol: "wired", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7446, marker: "dispatch_literal_method_selector", blocker: "selector_fact_missing" },
    Row { id: "dispatch.constant_method_selector", kind: "gap", family: "dispatch", tier: "workspace", protocol: "wired", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7446, marker: "dispatch_constant_method_selector", blocker: "constant_selector_missing" },
    Row { id: "dispatch.finite_loop_selector", kind: "gap", family: "dispatch", tier: "workspace", protocol: "wired", result: "gap", proof: "unit", confidence: "unknown", freshness: "current", editable: true, owner: 7446, marker: "dispatch_finite_loop_selector", blocker: "finite_selector_missing" },
    Row { id: "rename.package_identity_plan", kind: "gap", family: "rename", tier: "workspace", protocol: "wired", result: "safe_refusal", proof: "multi_file", confidence: "high", freshness: "current", editable: true, owner: 7448, marker: "rename_package_identity_plan", blocker: "package_plan_missing" },
    Row { id: "rename.package_module_atomic_workspace_edit", kind: "gap", family: "rename", tier: "workspace", protocol: "wired", result: "safe_refusal", proof: "none", confidence: "high", freshness: "current", editable: true, owner: 7450, marker: "rename_package_module_atomic_workspace_edit", blocker: "resource_edit_missing" },
    Row { id: "rename.inherited_override_family", kind: "gap", family: "rename", tier: "workspace", protocol: "wired", result: "safe_refusal", proof: "multi_file", confidence: "high", freshness: "current", editable: true, owner: 7451, marker: "rename_inherited_override_family", blocker: "override_family_missing" },
    Row { id: "workspace.restart_warm_project_facts", kind: "gap", family: "workspace", tier: "workspace", protocol: "not_applicable", result: "gap", proof: "none", confidence: "unknown", freshness: "not_proven", editable: true, owner: 7454, marker: "workspace_restart_warm_project_facts", blocker: "persistent_hydration_missing" },
    Row { id: "framework.user_loaded_adapter", kind: "gap", family: "framework", tier: "generated", protocol: "absent", result: "gap", proof: "none", confidence: "unknown", freshness: "not_proven", editable: false, owner: 7459, marker: "framework_user_loaded_adapter", blocker: "user_adapter_runtime_missing" },
    Row { id: "claims.protocol_semantic_proof_matrix", kind: "gap", family: "claims", tier: "ambient_boundary", protocol: "not_applicable", result: "gap", proof: "none", confidence: "unknown", freshness: "not_proven", editable: false, owner: 7460, marker: "claims_protocol_semantic_proof_matrix", blocker: "generated_claim_matrix_missing" },
];

#[test]
fn editor_intelligence_gap_matrix_is_owned_and_discriminating() -> Result<()> {
    let header: Value = serde_json::from_str(HEADER)?;
    anyhow::ensure!(header["schema_version"] == 1, "unexpected schema version");
    anyhow::ensure!(header["controller_issue"] == 7429, "controller drift");
    anyhow::ensure!(header["measurement_issue"] == 7430, "measurement issue drift");
    anyhow::ensure!(header["row_count"] == ROWS.len(), "row count drift");

    let vocabulary = &header["vocabulary"];
    let allowed = |key: &str, value: &str| {
        vocabulary[key]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry == value))
    };

    let mut ids = BTreeSet::new();
    let mut exact_controls = 0_usize;
    for row in ROWS {
        anyhow::ensure!(ids.insert(row.id), "duplicate row `{}`", row.id);
        anyhow::ensure!(row.owner > 0, "unowned row `{}`", row.id);
        anyhow::ensure!(CASES.contains(row.marker), "missing marker `{}`", row.marker);
        anyhow::ensure!(allowed("row_kind", row.kind), "unknown row kind");
        anyhow::ensure!(allowed("family", row.family), "unknown family");
        anyhow::ensure!(allowed("source_tier", row.tier), "unknown source tier");
        anyhow::ensure!(allowed("protocol_handler", row.protocol), "unknown protocol state");
        anyhow::ensure!(allowed("semantic_result", row.result), "unknown result state");
        anyhow::ensure!(allowed("proof_breadth", row.proof), "unknown proof state");
        anyhow::ensure!(allowed("confidence", row.confidence), "unknown confidence");
        anyhow::ensure!(allowed("freshness", row.freshness), "unknown freshness");

        if row.kind == "positive_control" {
            exact_controls += 1;
            anyhow::ensure!(row.result == "exact", "positive control is not exact");
            anyhow::ensure!(row.confidence == "high", "positive control lacks confidence");
            anyhow::ensure!(row.freshness == "current", "positive control is stale");
            anyhow::ensure!(row.proof != "none", "positive control lacks proof");
        } else {
            anyhow::ensure!(row.kind == "gap", "unexpected row kind");
            anyhow::ensure!(row.result != "exact", "gap row claims exact behavior");
            anyhow::ensure!(!row.blocker.is_empty(), "gap row has no blocker");
        }
        if matches!(row.tier, "external_dependency" | "ambient_boundary") {
            anyhow::ensure!(!row.editable, "read-only tier implies edit authority");
        }
    }
    anyhow::ensure!(exact_controls > 0, "no exact positive control");
    Ok(())
}
