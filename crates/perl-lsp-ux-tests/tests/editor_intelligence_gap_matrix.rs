use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

const HEADER: &str = include_str!("../fixtures/editor_intelligence_gap_matrix.json");
const OWNERS: &str = include_str!("../fixtures/editor_intelligence_owner_registry.json");
const CASES: &str = include_str!("../fixtures/editor_intelligence/case_manifest.pl");
const ROWS: &str = r#"
completion.constructor_assignment_control|positive_control|completion|workspace|wired|exact|representative_project|high|current|true|6604|completion_constructor_assignment_control|none|near_miss_control_required|frameworks/moo.pl
completion.method_return_chain.workspace|gap|completion|workspace|wired|bounded_fallback|multi_file|low|current|true|7439|completion_method_return_chain_workspace|callable_result_not_live|near_miss_control_required|method_return_chain/cases.pl
completion.method_return_chain.external_dependency|gap|completion|external_dependency|wired|gap|none|unknown|not_proven|false|7470|completion_method_return_chain_external_dependency|external_source_not_admitted|near_miss_control_required|external_dependency/lib/External/Client.pm
completion.returned_hash_shape.same_file|gap|completion|workspace|wired|bounded_fallback|unit|low|current|true|7436|completion_returned_hash_shape_same_file|returned_shape_not_propagated|near_miss_control_required|returned_hash/cases.pl
completion.returned_hash_shape.cross_module|gap|completion|workspace|wired|gap|multi_file|unknown|current|true|7436|completion_returned_hash_shape_cross_module|cross_module_shape_unavailable|near_miss_control_required|returned_hash/cases.pl
completion.typed_accessor_return|gap|completion|generated|wired|bounded_fallback|unit|medium|current|true|7438|completion_typed_accessor_return|accessor_return_not_promoted|near_miss_control_required|frameworks/moo.pl
completion.hashref_slot_receiver|gap|completion|workspace|wired|bounded_fallback|unit|low|current|true|7464|completion_hashref_slot_receiver|hashref_slot_not_promoted|near_miss_control_required|flow/hashref_slot.pl
completion.array_index_receiver|gap|completion|workspace|wired|bounded_fallback|unit|low|current|true|7464|completion_array_index_receiver|array_index_not_promoted|near_miss_control_required|flow/array_index.pl
completion.branch_join_receiver_union|gap|completion|workspace|wired|gap|unit|unknown|current|true|7465|completion_branch_join_receiver_union|branch_join_not_modeled|near_miss_control_required|flow/branch_join.pl
type_flow.ref_hash_narrowing|gap|type_flow|workspace|not_applicable|gap|unit|unknown|current|true|7426|type_flow_ref_hash_narrowing|ref_narrowing_missing|near_miss_control_required|flow/narrowing.pl
type_flow.isa_narrowing|gap|type_flow|workspace|not_applicable|gap|unit|unknown|current|true|7426|type_flow_isa_narrowing|isa_narrowing_missing|near_miss_control_required|flow/narrowing.pl
type_flow.defined_narrowing|gap|type_flow|workspace|not_applicable|gap|unit|unknown|current|true|7426|type_flow_defined_narrowing|defined_narrowing_missing|near_miss_control_required|flow/narrowing.pl
framework.mojo_base_accessors_and_parent|gap|framework|generated|wired|gap|none|unknown|not_proven|true|7441|framework_mojo_base_accessors_and_parent|mojo_base_adapter_missing|near_miss_control_required|frameworks/mojo_base.pl
framework.dbix_class_components|gap|framework|generated|wired|bounded_fallback|unit|medium|current|true|7443|framework_dbix_class_components|component_fact_missing|near_miss_control_required|frameworks/dbix_class.pl
framework.native_class_does|gap|framework|workspace|wired|gap|unit|unknown|current|true|7444|framework_native_class_does|native_role_fact_missing|near_miss_control_required|frameworks/native_class.pl
dispatch.literal_method_selector|gap|dispatch|workspace|wired|gap|unit|unknown|current|true|7449|dispatch_literal_method_selector|selector_fact_missing|near_miss_control_required|dispatch/cases.pl
dispatch.constant_method_selector|gap|dispatch|workspace|wired|gap|unit|unknown|current|true|7449|dispatch_constant_method_selector|constant_selector_missing|near_miss_control_required|dispatch/cases.pl
dispatch.finite_loop_selector|gap|dispatch|workspace|wired|gap|unit|unknown|current|true|7449|dispatch_finite_loop_selector|finite_selector_missing|near_miss_control_required|dispatch/cases.pl
rename.package_identity_plan|gap|rename|workspace|wired|safe_refusal|multi_file|high|current|true|7448|rename_package_identity_plan|package_plan_missing|near_miss_control_required|rename/cases.pl
rename.package_module_atomic_workspace_edit|gap|rename|workspace|wired|safe_refusal|none|high|current|true|7450|rename_package_module_atomic_workspace_edit|resource_edit_missing|near_miss_control_required|rename/cases.pl
rename.inherited_override_family|gap|rename|workspace|wired|safe_refusal|multi_file|high|current|true|7451|rename_inherited_override_family|override_family_missing|near_miss_control_required|rename/cases.pl
workspace.restart_warm_project_facts|gap|workspace|workspace|not_applicable|gap|none|unknown|not_proven|true|7454|workspace_restart_warm_project_facts|persistent_hydration_missing|near_miss_control_required|restart/cases.pl
framework.user_loaded_adapter|gap|framework|generated|absent|gap|none|unknown|not_proven|false|7478|framework_user_loaded_adapter|user_adapter_runtime_missing|near_miss_control_required|none
claims.protocol_semantic_proof_matrix|gap|claims|ambient_boundary|not_applicable|gap|none|unknown|not_proven|false|7460|claims_protocol_semantic_proof_matrix|generated_claim_matrix_missing|near_miss_control_required|none"#;

struct Row<'a> {
    id: &'a str,
    kind: &'a str,
    family: &'a str,
    tier: &'a str,
    protocol: &'a str,
    result: &'a str,
    proof: &'a str,
    confidence: &'a str,
    freshness: &'a str,
    editable: bool,
    owner: u64,
    marker: &'a str,
    blocker: &'a str,
    control: &'a str,
    fixture: &'a str,
}

impl<'a> Row<'a> {
    fn parse(line: &'a str) -> Result<Self> {
        let fields = line.split('|').collect::<Vec<_>>();
        anyhow::ensure!(fields.len() == 15, "row has {} fields", fields.len());
        let editable = match fields[9] {
            "true" => true,
            "false" => false,
            value => anyhow::bail!("invalid editable value `{value}`"),
        };
        let owner =
            fields[10].parse().with_context(|| format!("invalid owner issue `{}`", fields[10]))?;

        Ok(Self {
            id: fields[0],
            kind: fields[1],
            family: fields[2],
            tier: fields[3],
            protocol: fields[4],
            result: fields[5],
            proof: fields[6],
            confidence: fields[7],
            freshness: fields[8],
            editable,
            owner,
            marker: fields[11],
            blocker: fields[12],
            control: fields[13],
            fixture: fields[14],
        })
    }
}

fn is_allowed(vocabulary: &Value, key: &str, value: &str) -> bool {
    vocabulary[key]
        .as_array()
        .is_some_and(|entries| entries.iter().any(|entry| entry.as_str() == Some(value)))
}

fn active_owner_issues(registry: &Value) -> Result<BTreeSet<u64>> {
    let rows = registry["active_owners"].as_array().context("active_owners must be an array")?;
    let issues = rows
        .iter()
        .map(|row| row["issue"].as_u64().context("active owner is missing an issue number"))
        .collect::<Result<BTreeSet<_>>>()?;
    anyhow::ensure!(issues.len() == rows.len(), "active owner registry contains duplicates");
    Ok(issues)
}

#[test]
fn editor_intelligence_gap_matrix_is_owned_and_discriminating() -> Result<()> {
    let header: Value = serde_json::from_str(HEADER)?;
    let owner_registry: Value = serde_json::from_str(OWNERS)?;
    let rows = ROWS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(Row::parse)
        .collect::<Result<Vec<_>>>()?;

    anyhow::ensure!(header["schema_version"] == 1, "unexpected schema version");
    anyhow::ensure!(header["controller_issue"] == 7429, "controller drift");
    anyhow::ensure!(header["measurement_issue"] == 7430, "measurement issue drift");
    anyhow::ensure!(header["row_count"].as_u64() == Some(rows.len() as u64), "row count drift");

    anyhow::ensure!(owner_registry["schema_version"] == 1, "unexpected owner schema version");
    anyhow::ensure!(owner_registry["controller_issue"] == 7460, "owner controller drift");
    let active_owners = active_owner_issues(&owner_registry)?;
    let superseded_owners = owner_registry["superseded_owners"]
        .as_object()
        .context("superseded_owners must be an object")?;
    for (legacy, successor) in superseded_owners {
        let legacy = legacy.parse::<u64>().context("superseded owner key is not an issue number")?;
        let successor = successor.as_u64().context("superseded owner successor is not an issue")?;
        anyhow::ensure!(!active_owners.contains(&legacy), "superseded owner #{legacy} is still active");
        anyhow::ensure!(
            active_owners.contains(&successor),
            "successor #{successor} for superseded owner #{legacy} is not active"
        );
    }

    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/editor_intelligence");
    for row in &rows {
        if row.fixture == "none" {
            continue;
        }
        let path = fixture_root.join(row.fixture);
        let body = std::fs::read_to_string(&path).with_context(|| {
            format!("matrix row `{}` fixture `{}` is missing", row.id, row.fixture)
        })?;
        anyhow::ensure!(!body.trim().is_empty(), "fixture `{}` is empty", row.fixture);
        anyhow::ensure!(
            body.contains(row.marker),
            "fixture `{}` does not carry marker `{}`",
            row.fixture,
            row.marker
        );
    }

    let vocabulary = &header["vocabulary"];
    let mut ids = BTreeSet::new();
    let mut exact_controls = 0_usize;

    for row in rows {
        anyhow::ensure!(ids.insert(row.id), "duplicate row `{}`", row.id);
        anyhow::ensure!(row.owner > 0, "unowned row `{}`", row.id);
        anyhow::ensure!(
            active_owners.contains(&row.owner),
            "matrix row `{}` names inactive owner #{}",
            row.id,
            row.owner
        );
        anyhow::ensure!(
            !superseded_owners.contains_key(&row.owner.to_string()),
            "matrix row `{}` still names superseded owner #{}",
            row.id,
            row.owner
        );
        anyhow::ensure!(CASES.contains(row.marker), "missing marker `{}`", row.marker);
        anyhow::ensure!(CASES.contains(row.control), "missing negative control `{}`", row.control);
        anyhow::ensure!(is_allowed(vocabulary, "row_kind", row.kind));
        anyhow::ensure!(is_allowed(vocabulary, "family", row.family));
        anyhow::ensure!(is_allowed(vocabulary, "source_tier", row.tier));
        anyhow::ensure!(is_allowed(vocabulary, "protocol_handler", row.protocol));
        anyhow::ensure!(is_allowed(vocabulary, "semantic_result", row.result));
        anyhow::ensure!(is_allowed(vocabulary, "proof_breadth", row.proof));
        anyhow::ensure!(is_allowed(vocabulary, "confidence", row.confidence));
        anyhow::ensure!(is_allowed(vocabulary, "freshness", row.freshness));

        if row.kind == "positive_control" {
            exact_controls += 1;
            anyhow::ensure!(row.result == "exact", "positive control is not exact");
            anyhow::ensure!(row.confidence == "high", "positive control lacks confidence");
            anyhow::ensure!(row.freshness == "current", "positive control is stale");
            anyhow::ensure!(row.proof != "none", "positive control lacks proof");
        } else {
            anyhow::ensure!(row.kind == "gap", "unexpected row kind");
            anyhow::ensure!(row.result != "exact", "gap row claims exact behavior");
            anyhow::ensure!(row.blocker != "none", "gap row has no blocker");
        }

        if matches!(row.tier, "external_dependency" | "ambient_boundary") {
            anyhow::ensure!(!row.editable, "read-only tier implies edit authority");
        }
    }

    anyhow::ensure!(exact_controls > 0, "no exact positive control");
    Ok(())
}
