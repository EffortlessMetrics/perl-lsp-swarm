//! Discriminating tests for the `perl_corpus_train.v1` manifest check.
//!
//! Every shift-left rejection named on issue #10980 is proven against a real
//! mutation of the landed manifest that must fail with exactly the named
//! diagnostic; the canonical manifest, the shuffled control, and the
//! generated projections must pass and stay byte-deterministic.

use super::{
    INVALID_DIR, MANIFEST_PATH, SHUFFLED_PATH, canonical_form, invalid_fixture_names,
    render_explain_static, render_projections, title_fingerprint, validate_document,
};
use color_eyre::eyre::{Result, bail, eyre};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| eyre!("xtask must live one level below the repository root"))
}

fn load(rel: &str) -> Result<Value> {
    let text = std::fs::read_to_string(repo_root()?.join(rel))?;
    Ok(serde_json::from_str(&text)?)
}

fn codes(doc: &Value) -> Vec<String> {
    validate_document(doc).iter().map(|violation| violation.code.clone()).collect()
}

fn assert_code(doc: &Value, expected: &str) -> Result<()> {
    let actual = codes(doc);
    if !actual.iter().any(|code| code == expected) {
        bail!("expected {expected}, got {actual:?}");
    }
    Ok(())
}

fn node_mut<'a>(doc: &'a mut Value, node_id: &str) -> Result<&'a mut Map<String, Value>> {
    doc.get_mut("nodes")
        .and_then(Value::as_array_mut)
        .and_then(|nodes| {
            nodes
                .iter_mut()
                .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        })
        .and_then(Value::as_object_mut)
        .ok_or_else(|| eyre!("node {node_id} exists in the base manifest"))
}

fn first_node_with<'a>(
    doc: &'a mut Value,
    predicate: impl Fn(&Map<String, Value>) -> bool,
) -> Result<&'a mut Map<String, Value>> {
    doc.get_mut("nodes")
        .and_then(Value::as_array_mut)
        .and_then(|nodes| {
            nodes.iter_mut().filter_map(Value::as_object_mut).find(|node| predicate(node))
        })
        .ok_or_else(|| eyre!("a node matching the predicate exists in the base manifest"))
}

fn role_is(node: &Map<String, Value>, role: &str) -> bool {
    node.get("role").and_then(Value::as_str) == Some(role)
}

fn is_selectable(node: &Map<String, Value>) -> bool {
    node.get("selectable").and_then(Value::as_bool) == Some(true)
}

fn set(node: &mut Map<String, Value>, key: &str, value: Value) {
    node.insert(key.to_string(), value);
}

fn string_list(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|item| Value::String((*item).to_string())).collect())
}

#[test]
fn canonical_manifest_is_clean() -> Result<()> {
    let doc = load(MANIFEST_PATH)?;
    let violations = validate_document(&doc);
    if !violations.is_empty() {
        bail!(
            "the landed perl-corpus train manifest must validate: {:?}",
            violations.iter().map(|v| format!("{}: {}", v.code, v.detail)).collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn shuffled_control_canonizes_identically_validates_and_projects_identically() -> Result<()> {
    let base = load(MANIFEST_PATH)?;
    let shuffled = load(SHUFFLED_PATH)?;
    if canonical_form(&base) != canonical_form(&shuffled) {
        bail!("canonical form must be invariant under reordering");
    }
    if !validate_document(&shuffled).is_empty() {
        bail!("the shuffled control must validate: {:?}", codes(&shuffled));
    }
    if render_projections(&base)? != render_projections(&shuffled)? {
        bail!("projections must be byte-identical under reordering");
    }
    Ok(())
}

#[test]
fn projections_are_deterministic_across_renders() -> Result<()> {
    let base = load(MANIFEST_PATH)?;
    let first = render_projections(&base)?;
    let second = render_projections(&base)?;
    if first != second {
        bail!("two renders of one manifest must be byte-identical");
    }
    for (name, text) in &first {
        if text.contains("target/") || text.contains(env!("CARGO_MANIFEST_DIR")) {
            bail!("{name}: projection leaks an ambient host path");
        }
    }
    Ok(())
}

#[test]
fn every_expected_invalid_fixture_fails_with_named_code() -> Result<()> {
    let expected = load(&format!("{INVALID_DIR}/expected_errors.json"))?;
    let expected =
        expected.as_object().ok_or_else(|| eyre!("expected_errors.json is an object"))?;
    if expected.len() < 12 {
        bail!("all twelve #10980 rejection classes stay discriminated (found {})", expected.len());
    }
    let present = invalid_fixture_names(&repo_root()?)?;
    let listed: std::collections::BTreeSet<String> = expected.keys().cloned().collect();
    if present != listed {
        bail!(
            "fixture files and expected_errors.json keys must be the same set: {present:?} vs {listed:?}"
        );
    }
    for (filename, expected_code) in expected {
        let expected_code =
            expected_code.as_str().ok_or_else(|| eyre!("{filename}: string reason code"))?;
        if expected_code == "SCHEMA_VIOLATION" {
            // Schema-level fixtures are exercised by the gate command itself
            // (`run_check`), which applies the JSON Schema; the semantic layer
            // is not the discriminating instrument for them.
            continue;
        }
        let doc = load(&format!("{INVALID_DIR}/{filename}"))?;
        let actual: std::collections::BTreeSet<String> = codes(&doc).into_iter().collect();
        if actual.is_empty() {
            bail!("invalid/{filename} unexpectedly validated cleanly");
        }
        if actual.len() != 1 || !actual.contains(expected_code) {
            bail!("invalid/{filename}: expected exactly {expected_code}, got {actual:?}");
        }
    }
    Ok(())
}

#[test]
fn title_fingerprint_follows_the_shared_law() {
    // First 16 uppercase hex characters of SHA-256("").
    assert_eq!(title_fingerprint(""), "E3B0C44298FC1C14");
    assert_ne!(title_fingerprint("a"), title_fingerprint("b"));
}

// --- #10980 falsifiers, in the issue's order --------------------------------

#[test]
fn falsifier_1_controller_made_selectable_fails() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_ctrl_execution_8826")?;
    set(node, "selectable", Value::Bool(true));
    assert_code(&doc, "NON_LEAF_SELECTABLE")
}

#[test]
fn falsifier_2_two_nodes_owning_one_exclusive_authority_in_parallel_fail() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    // The opened-asset key belongs to #7693; giving it to an unrelated
    // selectable leaf (the C0 frontier tool) that neither precedes nor
    // follows it must fail.
    let node = node_mut(&mut doc, "pc_current_tree_frontier_10992")?;
    set(node, "exclusive_conflict_keys", string_list(&["perl_corpus.opened_asset"]));
    assert_code(&doc, "CONFLICT_KEY_PARALLEL_COLLISION")
}

#[test]
fn falsifier_2b_two_active_nodes_with_one_authority_after_fail() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let authority = node_mut(&mut doc, "pc_opened_asset_7693")?
        .get("authority_after")
        .cloned()
        .ok_or_else(|| eyre!("authority_after present"))?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "authority_after", authority);
    assert_code(&doc, "DUPLICATE_ACTIVE_AUTHORITY")
}

#[test]
fn falsifier_3_issue_closure_state_or_pull_number_as_stable_state_fails() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "issue_status", Value::String("closed".to_string()));
    assert_code(&doc, "MUTABLE_STATE_EMBEDDED")?;

    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "issue_ref", Value::String("PR #6745".to_string()));
    assert_code(&doc, "CANDIDATE_AS_ACTIVE_NODE")?;

    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(
        node,
        "candidate_reuse_policy",
        Value::String("resume main@ba8e2fbb00959d33848de647cfb76fc477f3c569".to_string()),
    );
    assert_code(&doc, "MUTABLE_STATE_EMBEDDED")
}

#[test]
fn falsifier_4_publication_as_ordinary_dependency_of_foundation_fails() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
    let deps = node
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("dependencies array"))?;
    deps.push(serde_json::json!({
        "target": "pc_repository_import_8850",
        "class": "hard",
        "provenance": "mutation"
    }));
    assert_code(&doc, "PUBLICATION_PROMOTED_INTO_FOUNDATION")?;

    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
    let deps = node
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("dependencies array"))?;
    deps.push(serde_json::json!({
        "target": "#EXPLICIT-AUTHORIZATION",
        "class": "authorization",
        "provenance": "mutation"
    }));
    assert_code(&doc, "AUTHORIZATION_ON_CODING_NODE")
}

#[test]
fn falsifier_5_same_authority_nodes_declared_parallel_fail() -> Result<()> {
    // #10555 and #7693 share no key today; make #10555 claim the opened-asset
    // key too. #7693 hard-depends on #10555, so they are serialized: no
    // collision. Then remove that ordering edge and the collision appears.
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    let mut keys =
        node.get("exclusive_conflict_keys").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    if let Some(items) = keys.as_array_mut() {
        items.push(Value::String("perl_corpus.opened_asset".to_string()));
    }
    set(node, "exclusive_conflict_keys", keys);
    if codes(&doc).contains(&"CONFLICT_KEY_PARALLEL_COLLISION".to_string()) {
        bail!("a dependency-serialized shared key is not a parallel collision");
    }
    let opened = node_mut(&mut doc, "pc_opened_asset_7693")?;
    if let Some(deps) = opened.get_mut("dependencies").and_then(Value::as_array_mut) {
        deps.retain(|dep| dep.get("target").and_then(Value::as_str) != Some("pc_asset_path_10555"));
    }
    if let Some(consumers) = node_mut(&mut doc, "pc_asset_path_10555")?
        .get_mut("consumed_by")
        .and_then(Value::as_array_mut)
    {
        consumers.retain(|id| id.as_str() != Some("pc_opened_asset_7693"));
    }
    assert_code(&doc, "CONFLICT_KEY_PARALLEL_COLLISION")
}

#[test]
fn falsifier_6_missing_compatibility_exit_owner_fails() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
    set(node, "legacy_exit", serde_json::json!({ "owner": null, "condition": null }));
    assert_code(&doc, "MISSING_LEGACY_EXIT")
}

#[test]
fn falsifier_7_reactivated_superseded_candidate_fails() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = first_node_with(&mut doc, |node| {
        role_is(node, "historical") && node.get("superseded_by").and_then(Value::as_str).is_some()
    })?;
    set(node, "role", Value::String("implementation".to_string()));
    set(node, "selectable", Value::Bool(true));
    assert_code(&doc, "SUPERSEDED_REACTIVATED")
}

#[test]
fn falsifier_8_product_semantics_moved_into_generic_topology_fails() -> Result<()> {
    // A generic topology leaf that would own the product gold semantics
    // duplicates the gold owner's authority_after.
    let mut doc = load(MANIFEST_PATH)?;
    let authority = node_mut(&mut doc, "pc_gold_extensions_7006")?
        .get("authority_after")
        .cloned()
        .ok_or_else(|| eyre!("authority_after present"))?;
    let node = node_mut(&mut doc, "pc_topology_union_6994")?;
    set(node, "authority_after", authority);
    assert_code(&doc, "DUPLICATE_ACTIVE_AUTHORITY")
}

#[test]
fn falsifier_9_missing_falsifier_ceiling_or_stop_condition_fails() -> Result<()> {
    for field in ["first_falsifier", "claim_ceiling"] {
        let mut doc = load(MANIFEST_PATH)?;
        let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
        set(node, field, Value::String(String::new()));
        assert_code(&doc, "INCOMPLETE_ONE_PR_CONTRACT")?;
    }
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
    set(node, "stop_conditions", Value::Array(Vec::new()));
    assert_code(&doc, "INCOMPLETE_ONE_PR_CONTRACT")
}

#[test]
fn falsifier_10_omitting_a_current_expectation_leaf_fails() -> Result<()> {
    // Removing #11029 leaves its consumers pointing at a vanished node and
    // its predecessors carrying a stale consumed_by set.
    let mut doc = load(MANIFEST_PATH)?;
    let nodes =
        doc.get_mut("nodes").and_then(Value::as_array_mut).ok_or_else(|| eyre!("nodes array"))?;
    let before = nodes.len();
    nodes.retain(|node| {
        node.get("node_id").and_then(Value::as_str) != Some("pc_expectation_writers_11029")
    });
    if nodes.len() != before - 1 {
        bail!("#11029 must be encoded as a current leaf");
    }
    assert_code(&doc, "UNKNOWN_EDGE_TARGET")?;
    for required in [
        "pc_expectation_writers_11029",
        "pc_ctrl_expectation_consumers_11030",
        "pc_expectation_retirement_11031",
        "pc_ctrl_property_adoption_11032",
        "pc_property_suites_11580",
        "pc_fixture_promotion_11034",
    ] {
        let doc = load(MANIFEST_PATH)?;
        let present = doc.get("nodes").and_then(Value::as_array).is_some_and(|nodes| {
            nodes.iter().any(|node| node.get("node_id").and_then(Value::as_str) == Some(required))
        });
        if !present {
            bail!("{required} must be present in the current graph");
        }
    }
    Ok(())
}

#[test]
fn falsifier_11_serializing_disjoint_or_parallelizing_conflicting_nodes_fails() -> Result<()> {
    // #11032 and #11034 are disjoint successors of #7020; a hard edge between
    // them would serialize disjoint authority. The manifest must carry none.
    let doc = load(MANIFEST_PATH)?;
    let nodes = doc.get("nodes").and_then(Value::as_array).ok_or_else(|| eyre!("nodes"))?;
    for (source, target) in [
        ("pc_property_suites_11580", "pc_fixture_promotion_11034"),
        ("pc_fixture_promotion_11034", "pc_property_suites_11580"),
    ] {
        let has_edge = nodes.iter().any(|node| {
            node.get("node_id").and_then(Value::as_str) == Some(source)
                && node.get("dependencies").and_then(Value::as_array).is_some_and(|deps| {
                    deps.iter().any(|dep| dep.get("target").and_then(Value::as_str) == Some(target))
                })
        });
        if has_edge {
            bail!("{source} -> {target} serializes disjoint authority");
        }
    }
    // Conversely, #6996 and #6999 share the expectation-relationship
    // authority and are serialized by #6999 -> #6996; declaring them
    // parallel (dropping the ordering edge) must collide on their key.
    let mut doc = load(MANIFEST_PATH)?;
    let identity = node_mut(&mut doc, "pc_fixture_expectation_identity_6999")?;
    if let Some(deps) = identity.get_mut("dependencies").and_then(Value::as_array_mut) {
        deps.retain(|dep| {
            dep.get("target").and_then(Value::as_str) != Some("pc_sidecar_concept_links_6996")
        });
    }
    if let Some(list) = node_mut(&mut doc, "pc_sidecar_concept_links_6996")?
        .get_mut("consumed_by")
        .and_then(Value::as_array_mut)
    {
        list.retain(|id| id.as_str() != Some("pc_fixture_expectation_identity_6999"));
    }
    assert_code(&doc, "CONFLICT_KEY_PARALLEL_COLLISION")
}

#[test]
fn falsifier_12_generated_output_is_invariant_under_shuffle_root_and_order() -> Result<()> {
    let base = load(MANIFEST_PATH)?;
    let mut reversed = base.clone();
    if let Some(nodes) = reversed.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.reverse();
        for node in nodes.iter_mut() {
            if let Some(deps) = node.get_mut("dependencies").and_then(Value::as_array_mut) {
                deps.reverse();
            }
            if let Some(list) = node.get_mut("consumed_by").and_then(Value::as_array_mut) {
                list.reverse();
            }
        }
    }
    if render_projections(&base)? != render_projections(&reversed)? {
        bail!("projection bytes changed under input reordering");
    }
    Ok(())
}

// --- additional laws --------------------------------------------------------

#[test]
fn evidence_edge_does_not_serialize_a_shared_exclusive_key() -> Result<()> {
    // #11583 carries only an evidence edge to #11579; an evidence dependency
    // lets #11583 land while #11579 is still not_proven, so sharing #11579's
    // exclusive key must be a parallel collision, not a serialized pair.
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_parser_accuracy_matrix_11583")?;
    set(
        node,
        "exclusive_conflict_keys",
        string_list(&[
            "perl_corpus.parser_accuracy_matrix",
            "perl_corpus.expectation_consumers_metrics",
        ]),
    );
    assert_code(&doc, "CONFLICT_KEY_PARALLEL_COLLISION")
}

#[test]
fn hard_cycle_is_rejected() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    let deps = node
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("dependencies array"))?;
    deps.push(serde_json::json!({
        "target": "pc_opened_asset_7693",
        "class": "hard",
        "provenance": "mutation"
    }));
    assert_code(&doc, "HARD_DEPENDENCY_CYCLE")
}

#[test]
fn dependency_on_a_controller_is_rejected() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_opened_asset_7693")?;
    let deps = node
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("dependencies array"))?;
    deps.push(serde_json::json!({
        "target": "pc_ctrl_execution_8826",
        "class": "hard",
        "provenance": "mutation"
    }));
    assert_code(&doc, "DEPENDENCY_ON_CONTROLLER")
}

#[test]
fn external_action_without_authorization_is_rejected() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = first_node_with(&mut doc, |node| role_is(node, "external_action"))?;
    if let Some(deps) = node.get_mut("dependencies").and_then(Value::as_array_mut) {
        deps.retain(|dep| dep.get("class").and_then(Value::as_str) != Some("authorization"));
    }
    assert_code(&doc, "AUTHORIZATION_MISSING")
}

#[test]
fn consumed_by_must_equal_the_derived_reverse_set() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "consumed_by", Value::Array(Vec::new()));
    assert_code(&doc, "CONSUMED_BY_MISMATCH")
}

#[test]
fn title_fingerprint_drift_is_rejected() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "title", Value::String("renamed without re-fingerprinting".to_string()));
    assert_code(&doc, "TITLE_FINGERPRINT_MISMATCH")
}

#[test]
fn unknown_conflict_key_is_rejected() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "exclusive_conflict_keys", string_list(&["perl_corpus.not_a_registered_key"]));
    assert_code(&doc, "CONFLICT_KEY_UNKNOWN")
}

#[test]
fn every_selectable_leaf_has_a_complete_contract_and_every_controller_is_unselectable() -> Result<()>
{
    let doc = load(MANIFEST_PATH)?;
    let nodes = doc.get("nodes").and_then(Value::as_array).ok_or_else(|| eyre!("nodes"))?;
    let mut controllers = 0usize;
    let mut selectable = 0usize;
    for node in nodes.iter().filter_map(Value::as_object) {
        if role_is(node, "controller") {
            controllers += 1;
            if is_selectable(node) {
                bail!("controller marked selectable");
            }
        }
        if is_selectable(node) {
            selectable += 1;
        }
    }
    if controllers < 13 {
        bail!("every #8826 programme controller is encoded (found {controllers})");
    }
    if selectable < 60 {
        bail!("every current concrete leaf is encoded (found {selectable} selectable leaves)");
    }
    Ok(())
}

#[test]
fn explain_static_renders_a_bounded_packet_without_readiness() -> Result<()> {
    let doc = load(MANIFEST_PATH)?;
    let text = render_explain_static(&doc, "pc_opened_asset_7693")?;
    if !text.contains("issue_ref: #7693") || !text.contains("hard -> pc_asset_path_10555") {
        bail!("packet must carry the node's subject and typed predecessors:\n{text}");
    }
    if !text.contains("readiness: not evaluated here") {
        bail!("packet must disclaim readiness");
    }
    if render_explain_static(&doc, "pc_missing").is_ok() {
        bail!("unknown node must fail");
    }
    Ok(())
}

/// The gate command itself applies the JSON Schema, every law, the fixtures,
/// and projection freshness end to end.
#[test]
fn gate_command_run_check_is_green_on_the_landed_tree() -> Result<()> {
    super::run_check()
}

#[test]
fn uppercase_commit_hash_is_still_a_mutable_coordinate() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(
        node,
        "candidate_reuse_policy",
        Value::String("resume main@BA8E2FBB00959D33848DE647CFB76FC477F3C569".to_string()),
    );
    assert_code(&doc, "MUTABLE_STATE_EMBEDDED")?;

    // A 16-digit title fingerprint is not a commit coordinate.
    let doc = load(MANIFEST_PATH)?;
    if codes(&doc).iter().any(|code| code == "MUTABLE_STATE_EMBEDDED") {
        bail!("uppercase title fingerprints must not read as commit hashes");
    }
    Ok(())
}

#[test]
fn dependency_class_vocabulary_must_be_the_closed_unique_set() -> Result<()> {
    // Duplicate `hard` in place of `authorization`: schema-valid (three enum
    // entries) while the authorization edges keep using the omitted class.
    let mut doc = load(MANIFEST_PATH)?;
    let classes = doc
        .get_mut("dependency_classes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("dependency_classes is an array"))?;
    let hard = classes
        .iter()
        .find(|entry| entry.get("class").and_then(Value::as_str) == Some("hard"))
        .cloned()
        .ok_or_else(|| eyre!("hard is declared"))?;
    classes.retain(|entry| entry.get("class").and_then(Value::as_str) != Some("authorization"));
    classes.push(hard);
    assert_code(&doc, "VOCABULARY_DRIFT")?;

    // A missing role with the count kept by a duplicate is the same drift.
    let mut doc = load(MANIFEST_PATH)?;
    let roles = doc
        .get_mut("role_vocabulary")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("role_vocabulary is an array"))?;
    let proof = roles
        .iter()
        .find(|entry| entry.get("role").and_then(Value::as_str) == Some("proof"))
        .cloned()
        .ok_or_else(|| eyre!("proof is declared"))?;
    roles.retain(|entry| entry.get("role").and_then(Value::as_str) != Some("decision"));
    roles.push(proof);
    assert_code(&doc, "VOCABULARY_DRIFT")
}

#[test]
fn numeric_semantic_authority_must_resolve_to_a_node_or_declared_authority() -> Result<()> {
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "semantic_authority_refs", string_list(&["#8826", "#99999"]));
    assert_code(&doc, "UNKNOWN_EDGE_TARGET")?;

    // A node subject that is not an external authority still resolves.
    let mut doc = load(MANIFEST_PATH)?;
    let node = node_mut(&mut doc, "pc_asset_path_10555")?;
    set(node, "semantic_authority_refs", string_list(&["#8826", "#7705"]));
    if codes(&doc).iter().any(|code| code == "UNKNOWN_EDGE_TARGET") {
        bail!("a node subject is a resolvable semantic authority");
    }
    Ok(())
}
