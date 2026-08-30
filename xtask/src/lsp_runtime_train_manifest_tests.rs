//! Focused proof for the G01 runtime-train contract (#11036).
//!
//! Shift-left order: the twelve falsifier shapes #11036 names come first, each
//! as an explicit mutation that must fail closed with its own reason, then the
//! positive coverage obligations, then determinism and pin binding. Every
//! mutation works on the real committed manifest bytes, so the proof
//! discriminates the actual artifact rather than a private toy copy.
//!
//! A wrong implementation these tests are built to catch: a validator that
//! accepts anything shaped like JSON and only checks the digest, which would
//! pass every happy-path assertion while rejecting none of the twelve.

use super::*;
use color_eyre::eyre::{Context, Result, bail};

fn repo_manifest_path() -> Result<std::path::PathBuf> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("xtask must live in a workspace subdirectory"))?;
    Ok(root.join(MANIFEST_RELATIVE_PATH))
}

fn real_value() -> Result<Value> {
    let bytes = std::fs::read(repo_manifest_path()?)
        .with_context(|| "failed to read the workspace runtime-train manifest for tests")?;
    serde_json::from_slice(&bytes).with_context(|| "workspace manifest is not valid JSON")
}

fn parse_strict(value: &Value) -> Result<Manifest> {
    serde_json::from_value(value.clone()).with_context(|| "strict deserialization failed")
}

/// Run the same law set `load_manifest_from` runs, minus the digest pin (each
/// mutation deliberately changes the bytes).
fn validate(value: &Value) -> Result<()> {
    let manifest = parse_strict(value)?;
    validate_manifest(&manifest)?;
    validate_no_mutable_live_facts(value, &manifest)?;
    validate_no_mutable_live_values(value, &manifest)
}

fn assert_rejected(value: &Value, needle: &str) -> Result<()> {
    match validate(value) {
        Ok(()) => bail!("expected rejection containing '{needle}', but validation passed"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains(needle),
                "rejection message mismatch\n  wanted substring: {needle}\n  actual: {rendered}"
            );
            Ok(())
        }
    }
}

fn node_mut<'a>(value: &'a mut Value, node_id: &str) -> Result<&'a mut Value> {
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest has no nodes array"))?;
    nodes
        .iter_mut()
        .find(|node| node.get("stable_node_id").and_then(Value::as_str) == Some(node_id))
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} not found"))
}

fn set_node_field(value: &mut Value, node_id: &str, field: &str, new_value: Value) -> Result<()> {
    let node = node_mut(value, node_id)?;
    *node
        .get_mut(field)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} lacks {field}"))? = new_value;
    Ok(())
}

fn push_node_string(value: &mut Value, node_id: &str, field: &str, item: &str) -> Result<()> {
    let node = node_mut(value, node_id)?;
    node.get_mut(field)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} lacks list {field}"))?
        .push(Value::String(item.to_string()));
    Ok(())
}

// ---------------------------------------------------------------------------
// The twelve required falsifiers.
// ---------------------------------------------------------------------------

// 1. A grouping node must never behave like an implementation leaf.
#[test]
fn lsp_runtime_train_controller_as_leaf_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(&mut value, "CTRL10360", "hard_dependencies", "SCHEMA11036")?;
    assert_rejected(&value, "carry no dependency edges")
}

#[test]
fn lsp_runtime_train_controller_with_a_proposition_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "CTRL10360",
        "one_pr_proposition",
        Value::String("implement the whole control plane".into()),
    )?;
    assert_rejected(&value, "never an implementation leaf")
}

#[test]
fn lsp_runtime_train_non_selectable_probe_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(&mut value, "DECIDE10554", "current_tree_probe_ids", "some_probe")?;
    assert_rejected(&value, "only a selectable leaf is integrated on a tree")
}

// 2. A selectable node states its full proposition, delta, rollback, and stop.
#[test]
fn lsp_runtime_train_missing_authority_delta_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "SCHEMA11036", "authority_after", Value::Array(vec![]))?;
    assert_rejected(&value, "omits its authority delta")
}

#[test]
fn lsp_runtime_train_missing_rollback_boundary_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "GRAPH11037", "rollback_boundary", Value::String("  ".into()))?;
    assert_rejected(&value, "omits its rollback boundary")
}

#[test]
fn lsp_runtime_train_missing_stop_boundary_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "GRAPH11037", "stop_conditions", Value::Array(vec![]))?;
    assert_rejected(&value, "omits its stop boundary")
}

// 3. Hard, evidence, authorization, and consumer edges stay distinct.
#[test]
fn lsp_runtime_train_edge_kind_conflation_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // GRAPH11037 is already a consumer of SCHEMA11036; also calling it a
    // prerequisite collapses the reader relation into an ordering one.
    push_node_string(&mut value, "SCHEMA11036", "hard_dependencies", "GRAPH11037")?;
    assert_rejected(&value, "must not be conflated")
}

#[test]
fn lsp_runtime_train_consumer_edge_used_as_prerequisite_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Claim a consumer that never reads this node back.
    push_node_string(&mut value, "DOGFOOD11311", "consumer_edges", "CTRL10360")?;
    assert_rejected(&value, "a consumer edge is a reader, not a prerequisite")
}

#[test]
fn lsp_runtime_train_evidence_edge_made_serializing_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let kinds = value
        .get_mut("edge_kinds")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no edge_kinds"))?;
    for kind in kinds.iter_mut() {
        if kind.get("kind").and_then(Value::as_str) == Some("evidence") {
            *kind
                .get_mut("serializes_implementation")
                .ok_or_else(|| color_eyre::eyre::eyre!("no flag"))? = Value::Bool(true);
        }
    }
    assert_rejected(&value, "only 'hard' edges order implementation")
}

// 4. External authorization is never inferred from dependency completion.
#[test]
fn lsp_runtime_train_external_action_without_authorization_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "EXTERNAL11311P",
        "authorization_dependencies",
        Value::Array(vec![]),
    )?;
    assert_rejected(&value, "can never be inferred from dependency completion")
}

#[test]
fn lsp_runtime_train_ordinary_node_claiming_authorization_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(
        &mut value,
        "SCHEMA11036",
        "authorization_dependencies",
        "#maintainer_submission",
    )?;
    assert_rejected(&value, "claims an authorization class")
}

// 5. A migration whose old path survives names its exit owner.
#[test]
fn lsp_runtime_train_cutover_without_old_path_exit_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "CUTOVER7384", "old_path_exit_owner", Value::Null)?;
    assert_rejected(&value, "must name who removes it")
}

#[test]
fn lsp_runtime_train_cutover_without_old_path_disposition_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "CUTOVER7384",
        "old_path_disposition",
        Value::String("none".into()),
    )?;
    set_node_field(&mut value, "CUTOVER7384", "old_path_exit_owner", Value::Null)?;
    assert_rejected(&value, "a cutover exists")
}

#[test]
fn lsp_runtime_train_exit_owner_without_a_surviving_path_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "SCHEMA11036",
        "old_path_exit_owner",
        Value::String("CTRL10360".into()),
    )?;
    assert_rejected(&value, "leaves no path to exit")
}

// 6. Two exclusive writers of one key are never both parallel-safe.
#[test]
fn lsp_runtime_train_parallel_exclusive_writers_are_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "FRONTIER11306",
        "parallel_disposition",
        Value::String("parallel_safe".into()),
    )?;
    set_node_field(
        &mut value,
        "CUTOVER7384",
        "parallel_disposition",
        Value::String("parallel_safe".into()),
    )?;
    assert_rejected(&value, "exclusive writers need an exact stack relation")
}

#[test]
fn lsp_runtime_train_conflict_serialized_node_without_stack_relation_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "CUTOVER7384",
        "stack_relation",
        Value::String("independent".into()),
    )?;
    assert_rejected(&value, "name the candidate it stacks on")
}

// 7. Mutable, live, observed, or released facts are not stable graph truth.
#[test]
fn lsp_runtime_train_mutable_live_fact_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let node = node_mut(&mut value, "SCHEMA11036")?;
    node.as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("node is not an object"))?
        .insert("head_sha".into(), Value::String("e78f2a2".into()));
    // Strict deserialization refuses the unknown field first; that is the same
    // fail-closed outcome, so assert on the parse rejection here and on the
    // scanner directly below.
    assert!(parse_strict(&value).is_err(), "an unknown live field must not deserialize");
    Ok(())
}

#[test]
fn lsp_runtime_train_mutable_live_fact_scanner_rejects_a_declared_key() -> Result<()> {
    let value = real_value()?;
    let manifest = parse_strict(&value)?;
    // Inject the forbidden key into a copy of the raw tree only, so the scanner
    // is exercised independently of the strict field set.
    let mut tampered = value.clone();
    tampered
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest is not an object"))?
        .insert("readiness".into(), Value::String("ready".into()));
    match validate_no_mutable_live_facts(&tampered, &manifest) {
        Ok(()) => bail!("the scanner accepted a declared mutable live fact"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains("never become stable"),
                "unexpected scanner message: {rendered}"
            );
            Ok(())
        }
    }
}

// 8. One authority does not imply one global actor or lock.
#[test]
fn lsp_runtime_train_global_conflict_key_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "SCHEMA11036",
        "exclusive_writer_conflict_keys",
        Value::Array(vec![Value::String("global".into())]),
    )?;
    assert_rejected(&value, "declares the global conflict key")
}

// 9. Product implementation types never enter the graph mechanics.
#[test]
fn lsp_runtime_train_product_type_in_generic_mechanics_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let roles = value
        .get_mut("role_vocabulary")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no role_vocabulary"))?;
    for role in roles.iter_mut() {
        if role.get("role").and_then(Value::as_str) == Some("proof") {
            *role.get_mut("owns").ok_or_else(|| color_eyre::eyre::eyre!("no owns"))? =
                Value::String("one obligation over a Perl parse result".into());
        }
    }
    assert_rejected(&value, "imports the implementation type")
}

// 10. Extraction is justified by landed reuse, never by structural symmetry.
#[test]
fn lsp_runtime_train_premature_generic_extraction_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let inventory = value
        .get_mut("shared_mechanics_ruling")
        .and_then(|r| r.get_mut("inventory"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no inventory"))?;
    for entry in inventory.iter_mut() {
        *entry.get_mut("landed").ok_or_else(|| color_eyre::eyre::eyre!("no landed"))? =
            Value::Bool(false);
    }
    assert_rejected(&value, "not extraction evidence")
}

#[test]
fn lsp_runtime_train_deferring_extraction_without_evidence_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    *value
        .get_mut("shared_mechanics_ruling")
        .and_then(|r| r.get_mut("duplication_evidence_for_10554"))
        .ok_or_else(|| color_eyre::eyre::eyre!("no duplication evidence"))? = Value::Array(vec![]);
    assert_rejected(&value, "judged on evidence rather than symmetry")
}

// 11. An unknown version or execution-significant field is never silently read.
#[test]
fn lsp_runtime_train_unknown_schema_version_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    *value
        .get_mut("schema_version")
        .ok_or_else(|| color_eyre::eyre::eyre!("no schema_version"))? =
        Value::Number(serde_json::Number::from(2u64));
    assert_rejected(&value, "unknown schema_version 2")
}

#[test]
fn lsp_runtime_train_unknown_field_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest is not an object"))?
        .insert("execution_significant_addition".into(), Value::Bool(true));
    assert!(
        parse_strict(&value).is_err(),
        "an unknown execution-significant field must fail closed, not be ignored"
    );
    Ok(())
}

// 12. Identical semantic inputs normalize to identical bytes.
#[test]
fn lsp_runtime_train_normalization_is_order_invariant() -> Result<()> {
    let value = real_value()?;
    let baseline = canonical_digest(&value)?;

    // Reordering object keys must not change the digest.
    let reserialized: Value = serde_json::from_str(&serde_json::to_string(&value)?)?;
    assert_eq!(canonical_digest(&reserialized)?, baseline, "key order changed the digest");

    // Reordering a semantically unordered list must not change the digest.
    let mut reordered = value.clone();
    if let Some(nodes) = reordered.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.reverse();
    }
    assert_eq!(canonical_digest(&reordered)?, baseline, "node order changed the digest");

    // A genuine semantic change must change it.
    let mut changed = value.clone();
    set_node_field(&mut changed, "SCHEMA11036", "lane", Value::String("other_lane".into()))?;
    assert_ne!(canonical_digest(&changed)?, baseline, "a semantic change left the digest unmoved");
    Ok(())
}

// ---------------------------------------------------------------------------
// Referential and topological integrity.
// ---------------------------------------------------------------------------

#[test]
fn lsp_runtime_train_unknown_node_reference_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(&mut value, "GRAPH11037", "hard_dependencies", "NO_SUCH_NODE")?;
    assert_rejected(&value, "references unknown node")
}

#[test]
fn lsp_runtime_train_hard_dependency_cycle_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // SCHEMA11036 <- GRAPH11037 already; closing the loop must fail. Remove the
    // consumer edge first so the conflation law does not mask the cycle law.
    set_node_field(&mut value, "SCHEMA11036", "consumer_edges", Value::Array(vec![]))?;
    push_node_string(&mut value, "SCHEMA11036", "hard_dependencies", "GRAPH11037")?;
    assert_rejected(&value, "hard dependency cycle")
}

#[test]
fn lsp_runtime_train_stale_supersession_half_edge_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "HISTORICAL9799", "superseded_by", Value::Array(vec![]))?;
    assert_rejected(&value, "stale supersession half-edge is rejected")
}

#[test]
fn lsp_runtime_train_claim_ceiling_above_role_cap_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "SCHEMA11036", "claim_ceiling", Value::String("programme".into()))?;
    assert_rejected(&value, "above the cap its role")
}

#[test]
fn lsp_runtime_train_controller_ref_to_a_non_controller_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "GRAPH11037",
        "controller_ref",
        Value::String("SCHEMA11036".into()),
    )?;
    assert_rejected(&value, "rather than controller")
}

#[test]
fn lsp_runtime_train_claiming_a_complete_population_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    *value
        .get_mut("population_status")
        .ok_or_else(|| color_eyre::eyre::eyre!("no population_status"))? =
        Value::String("complete".into());
    assert_rejected(&value, "completing the graph is the successor's authority")
}

// ---------------------------------------------------------------------------
// Gaps found by independent review on PR #13869 (Codex, three P2 findings).
// Each was confirmed against the law order before it was closed, so these are
// the controls proving the hole is actually shut.
// ---------------------------------------------------------------------------

#[test]
fn lsp_runtime_train_duplicate_role_claim_cap_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // A duplicate row survives the set-coverage check, and because
    // canonicalization sorts arrays the two orderings share one digest while
    // validating differently. Validation must not depend on input order.
    let caps = value
        .get_mut("role_claim_caps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no role_claim_caps"))?;
    caps.push(serde_json::json!({ "role": "implementation", "max_claim": "programme" }));
    assert_rejected(&value, "more than once")
}

#[test]
fn lsp_runtime_train_duplicate_role_claim_cap_is_rejected_in_either_order() -> Result<()> {
    // The reviewer's control: canonicalization sorts arrays, so the two
    // orderings of a conflicting duplicate share one digest. Both must be
    // rejected, or the effective cap depends on incidental input order.
    for reversed in [false, true] {
        let mut value = real_value()?;
        let caps = value
            .get_mut("role_claim_caps")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no role_claim_caps"))?;
        caps.push(serde_json::json!({ "role": "implementation", "max_claim": "programme" }));
        if reversed {
            caps.reverse();
        }
        assert_rejected(&value, "more than once")?;
    }
    Ok(())
}

#[test]
fn lsp_runtime_train_commit_sha_in_prose_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "SCHEMA11036",
        "one_pr_proposition",
        Value::String("define the contract, implemented on main at 0c07a9841c34ff3e".into()),
    )?;
    assert_rejected(&value, "a commit-shaped identifier")
}

#[test]
fn lsp_runtime_train_live_check_verdict_in_prose_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "GRAPH11037",
        "limitations",
        Value::Array(vec![Value::String("PR #13869 is green, so this is settled".into())]),
    )?;
    assert_rejected(&value, "a verdict about a live pull request or issue")
}

#[test]
fn lsp_runtime_train_writer_assignment_in_prose_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "PROBE11038",
        "rollback_boundary",
        Value::String("revert the probe module; assigned to the runtime lane writer".into()),
    )?;
    assert_rejected(&value, "a writer or agent assignment")
}

#[test]
fn lsp_runtime_train_readiness_verdict_in_prose_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Append rather than replace: clearing authority_after would trip the
    // consumed-authority law first and leave the value scan unexercised.
    push_node_string(&mut value, "FRONTIER11306", "authority_after", "ready = true")?;
    assert_rejected(&value, "a readiness verdict")
}

#[test]
fn lsp_runtime_train_value_scan_requires_declared_patterns() -> Result<()> {
    let mut value = real_value()?;
    *value
        .get_mut("forbidden_value_patterns")
        .ok_or_else(|| color_eyre::eyre::eyre!("no forbidden_value_patterns"))? =
        Value::Array(vec![]);
    assert_rejected(&value, "leaves state freely representable")
}

#[test]
fn lsp_runtime_train_reverse_supersession_half_edge_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // The forward direction was already checked; drop the forward edge and keep
    // the reverse one, which referential integrity alone would accept.
    set_node_field(&mut value, "CUTOVER7384", "supersedes", Value::Array(vec![]))?;
    assert_rejected(&value, "claims to be superseded by")
}

#[test]
fn lsp_runtime_train_conflicting_writers_without_a_hard_path_are_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Both nodes claim a serialized disposition, but removing the hard edge
    // leaves nothing that actually orders them. A label is not an ordering.
    set_node_field(
        &mut value,
        "CUTOVER7384",
        "parallel_disposition",
        Value::String("serialized_by_hard_dependency".into()),
    )?;
    set_node_field(&mut value, "CUTOVER7384", "hard_dependencies", Value::Array(vec![]))?;
    set_node_field(&mut value, "CUTOVER7384", "consumed_authorities", Value::Array(vec![]))?;
    assert_rejected(&value, "must be backed by a real edge")
}

// ---------------------------------------------------------------------------
// Authority, artifact, and obligation laws.
// ---------------------------------------------------------------------------

#[test]
fn lsp_runtime_train_consuming_an_unproduced_authority_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(
        &mut value,
        "GRAPH11037",
        "consumed_authorities",
        "an authority no dependency establishes",
    )?;
    assert_rejected(&value, "never asserted")
}

#[test]
fn lsp_runtime_train_consuming_authority_from_a_dropped_dependency_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // CUTOVER7384 consumes FRONTIER11306's authority but is not named as one of
    // its consumers, so dropping the edge reaches the consumed-authority law
    // instead of the consumer-reciprocity law.
    set_node_field(&mut value, "CUTOVER7384", "hard_dependencies", Value::Array(vec![]))?;
    // Assert on wording unique to the consumed-authority law: the shorter
    // "no hard or evidence" prefix is shared with the consumer-reciprocity law,
    // so it would stop discriminating if CUTOVER7384 ever became a listed
    // consumer of FRONTIER11306.
    assert_rejected(&value, "dependency produces")
}

#[test]
fn lsp_runtime_train_shared_artifact_without_one_writer_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // OBSERVE11040 and PACKET11042 legitimately share an artifact while holding
    // distinct writer keys. Collapsing them onto one key removes the single
    // explicit writer the parallel disposition depends on.
    set_node_field(
        &mut value,
        "PACKET11042",
        "exclusive_writer_conflict_keys",
        Value::Array(vec![Value::String("lsp_runtime_train.observation".into())]),
    )?;
    assert_rejected(&value, "a shared artifact needs one explicit writer")
}

#[test]
fn lsp_runtime_train_external_action_without_transfer_owner_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "EXTERNAL11311P", "transfer_owner", Value::Null)?;
    assert_rejected(&value, "must name who performs it")
}

#[test]
fn lsp_runtime_train_selectable_node_without_a_falsifier_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "PROOF11033", "required_falsifier_ids", Value::Array(vec![]))?;
    assert_rejected(&value, "cannot be falsified cannot be reviewed")
}

#[test]
fn lsp_runtime_train_falsifier_reused_as_positive_obligation_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    push_node_string(&mut value, "GRAPH11037", "positive_proof_obligation_ids", "cycle_rejected")?;
    assert_rejected(&value, "different proofs")
}

#[test]
fn lsp_runtime_train_non_selectable_node_requiring_artifacts_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "DECIDE10554", "artifact_map_required", Value::Bool(true))?;
    assert_rejected(&value, "only a leaf that lands a candidate owns artifacts")
}

#[test]
fn lsp_runtime_train_checked_spec_without_artifact_map_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // DOGFOOD11311 is a selectable leaf that requires no artifact map, so
    // demanding a checked spec there has no artifact set to belong to.
    set_node_field(
        &mut value,
        "DOGFOOD11311",
        "checked_spec_disposition_required",
        Value::Bool(true),
    )?;
    assert_rejected(&value, "not a parallel authority")
}

#[test]
fn lsp_runtime_train_missing_return_to_issue_conditions_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "PROBE11038", "return_to_issue_conditions", Value::Array(vec![]))?;
    assert_rejected(&value, "omits its return-to-issue conditions")
}

#[test]
fn lsp_runtime_train_missing_node_limitations_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "PROBE11038", "limitations", Value::Array(vec![]))?;
    assert_rejected(&value, "omits its limitations")
}

// ---------------------------------------------------------------------------
// Positive coverage obligations: the fixtures must exercise the contract.
// ---------------------------------------------------------------------------

#[test]
fn lsp_runtime_train_manifest_loads_with_its_pinned_digest() -> Result<()> {
    let loaded = load_manifest()?;
    assert_eq!(loaded.canonical_digest(), PINNED_CANONICAL_DIGEST);
    assert_eq!(loaded.population_status(), REQUIRED_POPULATION_STATUS);
    assert!(loaded.node_count() >= 12, "fixture set too small to exercise the contract");
    Ok(())
}

#[test]
fn lsp_runtime_train_every_role_horizon_and_disposition_is_exercised() -> Result<()> {
    // validate_coverage enforces this on load; assert the positive shape too so
    // a future contraction of the fixture set fails here with a clear reason.
    let value = real_value()?;
    let manifest = parse_strict(&value)?;
    let roles: BTreeSet<&str> = manifest.role_vocabulary.iter().map(|r| r.role.as_str()).collect();
    let used: BTreeSet<&str> = manifest.nodes.iter().map(|n| n.role.as_str()).collect();
    assert_eq!(roles, used, "fixture nodes must exercise exactly the declared roles");

    let horizons: BTreeSet<&str> =
        manifest.release_horizon_ladder.iter().map(|h| h.value.as_str()).collect();
    let used_horizons: BTreeSet<&str> =
        manifest.nodes.iter().map(|n| n.release_horizon.as_str()).collect();
    assert_eq!(horizons, used_horizons, "every release horizon must be exercised");

    let dispositions: BTreeSet<&str> =
        manifest.old_path_dispositions.iter().map(|d| d.value.as_str()).collect();
    let used_dispositions: BTreeSet<&str> =
        manifest.nodes.iter().map(|n| n.old_path_disposition.as_str()).collect();
    assert_eq!(dispositions, used_dispositions, "every old-path disposition must be exercised");
    Ok(())
}

#[test]
fn lsp_runtime_train_removing_a_role_fixture_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.retain(|n| n.get("stable_node_id").and_then(Value::as_str) != Some("HISTORICAL9799"));
    }
    // Dropping the only historical fixture must fail closed rather than quietly
    // shrinking what the contract is proven to express. Referential integrity
    // happens to fire first here (CUTOVER7384 supersedes it), which is the same
    // fail-closed outcome. `validate_coverage` itself is proven to reject by
    // `lsp_runtime_train_dropping_the_only_external_action_fails_coverage`,
    // which removes a fixture nothing references.
    match validate(&value) {
        Ok(()) => bail!("expected a rejection after removing the historical fixture"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains("roles")
                    || rendered.contains("old-path dispositions")
                    || rendered.contains("references unknown node"),
                "unexpected message: {rendered}"
            );
            Ok(())
        }
    }
}

#[test]
fn lsp_runtime_train_dropping_the_only_external_action_fails_coverage() -> Result<()> {
    let mut value = real_value()?;
    // EXTERNAL11311P is referenced by no other node, so removing it reaches the
    // coverage law directly instead of tripping referential integrity first.
    if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.retain(|n| n.get("stable_node_id").and_then(Value::as_str) != Some("EXTERNAL11311P"));
    }
    assert_rejected(&value, "no fixture node exercises these roles")
}

#[test]
fn lsp_runtime_train_selectable_nodes_are_the_leaf_roles() -> Result<()> {
    let loaded = load_manifest()?;
    let selectable = loaded.selectable_node_ids();
    assert!(!selectable.is_empty(), "a contract with no selectable leaf cannot route work");
    assert!(
        !selectable.iter().any(|id| id == "CTRL10360"),
        "a controller must never be selectable"
    );
    assert!(
        !selectable.iter().any(|id| id == "EXTERNAL11311P"),
        "an external action must never be selectable"
    );
    assert!(selectable.iter().any(|id| id == "SCHEMA11036"), "the schema leaf must be selectable");
    Ok(())
}

#[test]
fn lsp_runtime_train_node_static_fact_is_bounded() -> Result<()> {
    let loaded = load_manifest()?;
    let fact = loaded
        .node_static_fact("FRONTIER11306")
        .ok_or_else(|| color_eyre::eyre::eyre!("FRONTIER11306 missing"))?;
    assert_eq!(fact.issue_ref, 11306);
    assert_eq!(fact.role, "implementation");
    assert_eq!(fact.old_path_disposition, "forwarding_with_exit");
    assert_eq!(fact.old_path_exit_owner.as_deref(), Some("CUTOVER7384"));
    assert!(loaded.node_static_fact("NO_SUCH_NODE").is_none());
    Ok(())
}

// ---------------------------------------------------------------------------
// Claim-ceiling guard: this module owns a data contract, nothing more.
// ---------------------------------------------------------------------------

#[test]
fn no_state_or_command_surface_is_added() -> Result<()> {
    let source = include_str!("lsp_runtime_train_manifest.rs");

    // Substrings for surfaces that cannot be spelled around: any subprocess or
    // network use has to name one of these.
    for forbidden in ["std::process", "Command::new", "reqwest", "octocrab", "ureq", "clap::"] {
        assert!(
            !source.contains(forbidden),
            "#11036 stops before commands and remote access; found '{forbidden}'"
        );
    }

    // Function names are matched by pattern, not by exact spelling: a literal
    // "fn readiness" scan is defeated by `fn compute_readiness`, so the guard
    // would bind a naming convention rather than the claim ceiling.
    let banned_fn = regex::Regex::new(r"fn\s+\w*(readiness|frontier|observe|probe|packet)\w*")
        .map_err(|e| color_eyre::eyre::eyre!("guard regex failed to compile: {e}"))?;
    if let Some(found) = banned_fn.find(source) {
        bail!(
            "#11036 stops before readiness, frontier, observation, probe, and packet surfaces; \
             found '{}'",
            found.as_str()
        );
    }
    Ok(())
}

/// The digest pin's rejection branch, which every mutation test deliberately
/// skips: without this, inverting or misrouting the comparison would go unseen.
#[test]
fn lsp_runtime_train_digest_drift_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(&mut value, "SCHEMA11036", "lane", Value::String("tampered_lane".into()))?;

    let dir = tempfile::tempdir().map_err(|e| color_eyre::eyre::eyre!("tempdir: {e}"))?;
    let path = dir.path().join("lsp_runtime_train.v1.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&value)?)
        .map_err(|e| color_eyre::eyre::eyre!("write: {e}"))?;

    match load_manifest_from(&path) {
        Ok(_) => bail!("a tampered manifest loaded despite the pinned digest"),
        Err(err) => {
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains("digest drift"),
                "expected a digest-drift rejection, got: {rendered}"
            );
            Ok(())
        }
    }
}

/// The pin's accept branch must also be exercised against an on-disk copy, so
/// the drift test above cannot pass merely because loading always fails.
#[test]
fn lsp_runtime_train_untampered_copy_loads_from_an_explicit_path() -> Result<()> {
    let dir = tempfile::tempdir().map_err(|e| color_eyre::eyre::eyre!("tempdir: {e}"))?;
    let path = dir.path().join("lsp_runtime_train.v1.json");
    std::fs::copy(repo_manifest_path()?, &path)
        .map_err(|e| color_eyre::eyre::eyre!("copy: {e}"))?;
    let loaded = load_manifest_from(&path)?;
    assert_eq!(loaded.canonical_digest(), PINNED_CANONICAL_DIGEST);
    Ok(())
}

#[test]
fn lsp_runtime_train_self_duplicate_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_field(
        &mut value,
        "HISTORICAL9799",
        "duplicate_of",
        Value::String("HISTORICAL9799".into()),
    )?;
    assert_rejected(&value, "its own duplicate")
}

#[test]
fn lsp_runtime_train_unexercised_stack_relation_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value
        .get_mut("stack_relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no stack_relations"))?
        .push(serde_json::json!({
            "value": "restacked_from",
            "owns": "declared but exercised by no fixture",
        }));
    assert_rejected(&value, "no fixture node exercises these stack relations")
}

#[test]
fn lsp_runtime_train_unexercised_parallel_disposition_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    value
        .get_mut("parallel_dispositions")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no parallel_dispositions"))?
        .push(serde_json::json!({
            "value": "serialized_by_authorization",
            "owns": "declared but exercised by no fixture",
        }));
    assert_rejected(&value, "no fixture node exercises these parallel dispositions")
}
