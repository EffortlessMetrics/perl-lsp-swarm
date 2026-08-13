use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde_json::Value;

const MATRIX: &str = include_str!("../fixtures/editor_intelligence_gap_matrix.json");
const FIXTURE_BANK: &str = concat!(
    include_str!("../fixtures/editor_intelligence/README.md"),
    include_str!("../fixtures/editor_intelligence/dispatch/computed_dispatch.pl"),
    include_str!("../fixtures/editor_intelligence/external_dependency/lib/External/Client.pm"),
    include_str!("../fixtures/editor_intelligence/flow/flow_cases.pl"),
    include_str!("../fixtures/editor_intelligence/frameworks/framework_cases.pl"),
    include_str!("../fixtures/editor_intelligence/method_return_chain/lib/Example/Response.pm"),
    include_str!("../fixtures/editor_intelligence/method_return_chain/lib/Example/Transaction.pm"),
    include_str!("../fixtures/editor_intelligence/method_return_chain/lib/Example/UserAgent.pm"),
    include_str!("../fixtures/editor_intelligence/method_return_chain/script/probe.pl"),
    include_str!("../fixtures/editor_intelligence/rename/lib/Old/Base.pm"),
    include_str!("../fixtures/editor_intelligence/rename/lib/Old/Child.pm"),
    include_str!("../fixtures/editor_intelligence/rename/lib/Old/Name.pm"),
    include_str!("../fixtures/editor_intelligence/rename/script/use_old_name.pl"),
    include_str!("../fixtures/editor_intelligence/restart/lib/Restart/Model.pm"),
    include_str!("../fixtures/editor_intelligence/returned_hash/lib/Example/Config.pm"),
    include_str!("../fixtures/editor_intelligence/returned_hash/script/probe.pl"),
);
const REQUIRED_IDS: &[&str] = &[
    "completion.constructor_assignment_control",
    "completion.method_return_chain.workspace",
    "completion.method_return_chain.external_dependency",
    "completion.returned_hash_shape.same_file",
    "completion.returned_hash_shape.cross_module",
    "completion.typed_accessor_return",
    "completion.hashref_slot_receiver",
    "completion.array_index_receiver",
    "completion.branch_join_receiver_union",
    "type_flow.ref_hash_narrowing",
    "type_flow.isa_narrowing",
    "type_flow.defined_narrowing",
    "framework.mojo_base_accessors_and_parent",
    "framework.dbix_class_components",
    "framework.native_class_does",
    "dispatch.literal_method_selector",
    "dispatch.constant_method_selector",
    "dispatch.finite_loop_selector",
    "rename.package_identity_plan",
    "rename.package_module_atomic_workspace_edit",
    "rename.inherited_override_family",
    "workspace.restart_warm_project_facts",
    "framework.user_loaded_adapter",
    "claims.protocol_semantic_proof_matrix",
];

fn string<'a>(value: &'a Value, key: &str, id: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("row `{id}` missing string `{key}`"))
}

fn strings<'a>(value: &'a Value, key: &str, id: &str) -> Result<Vec<&'a str>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .with_context(|| format!("row `{id}` missing array `{key}`"))?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .with_context(|| format!("row `{id}` has non-string `{key}` entry"))
        })
        .collect()
}

#[test]
fn editor_intelligence_gap_matrix_is_owned_and_discriminating() -> Result<()> {
    let matrix: Value = serde_json::from_str(MATRIX)?;
    anyhow::ensure!(matrix["schema_version"] == 1, "unexpected schema version");
    anyhow::ensure!(matrix["controller_issue"] == 7429, "controller drift");
    anyhow::ensure!(matrix["measurement_issue"] == 7430, "measurement issue drift");

    let rows = matrix["rows"].as_array().context("rows missing")?;
    let actual = rows
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<BTreeSet<_>>();
    let expected = REQUIRED_IDS.iter().copied().collect::<BTreeSet<_>>();
    anyhow::ensure!(actual == expected, "editor-intelligence denominator drifted");

    let mut exact_controls = 0_usize;
    for row in rows {
        let id = string(row, "id", "unknown")?;
        let kind = string(row, "row_kind", id)?;
        let result = string(row, "semantic_result", id)?;
        let owner = row["owner_issue"].as_u64().context("owner_issue missing")?;
        let markers = strings(row, "probe_markers", id)?;
        let sources = strings(row, "source_paths", id)?;
        let negatives = strings(row, "negative_controls", id)?;
        anyhow::ensure!(owner > 0, "row `{id}` is unowned");
        anyhow::ensure!(!sources.is_empty(), "row `{id}` has no source fixture");
        anyhow::ensure!(!markers.is_empty(), "row `{id}` has no source marker");
        anyhow::ensure!(!negatives.is_empty(), "row `{id}` has no negative control");
        for marker in markers {
            anyhow::ensure!(
                FIXTURE_BANK.contains(marker),
                "row `{id}` marker `{marker}` is absent from the fixture bank"
            );
        }
        if kind == "positive_control" {
            exact_controls += 1;
            anyhow::ensure!(result == "exact", "positive control `{id}` is not exact");
            anyhow::ensure!(
                row["confidence"] == "high",
                "positive control `{id}` is not high confidence"
            );
            anyhow::ensure!(
                row["freshness"] == "current",
                "positive control `{id}` is stale"
            );
        } else {
            anyhow::ensure!(kind == "gap", "row `{id}` has unknown kind `{kind}`");
            anyhow::ensure!(result != "exact", "gap row `{id}` claims exact behavior");
            anyhow::ensure!(
                !strings(row, "blockers", id)?.is_empty(),
                "gap row `{id}` has no blocker"
            );
        }
        if matches!(
            string(row, "source_tier", id)?,
            "external_dependency" | "ambient_boundary"
        ) {
            anyhow::ensure!(
                row["editable"] == false,
                "read-only row `{id}` implies edit authority"
            );
        }
    }
    anyhow::ensure!(exact_controls > 0, "matrix has no exact positive control");
    Ok(())
}
