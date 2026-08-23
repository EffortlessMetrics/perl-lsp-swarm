//! Falsifier-first proof for the independent train validator (T02 `#11765`).
//!
//! Every fixture here is a mutation of the real stable manifest that must be
//! REJECTED (fail-closed). The fixtures mirror the fourteen falsifier classes
//! required by the issue: normalization acceptance, controller-in-frontier,
//! forced-serial parallel groups, duplicate writers, proof repair authority,
//! second home programmes, role collapse, class collapse, warning-only
//! contracts, active supersessions, order sensitivity, live-state bytes,
//! machine/human disagreement, and ownerless references.

use std::path::Path;

use color_eyre::eyre::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use super::TrainCommand;
use super::digest::canonical_digest;
use super::model::Manifest;
use super::run_train_at;
use super::validate::{GraphSummary, validate_static_bytes};

/// T01's independently recorded semantic digest for the landed manifest
/// (`#11764` closeout). Matching it proves this implementation reproduces
/// the reference canonicalization byte-for-byte. A T02R-classified semantic
/// revision changes it deliberately, together with this pin.
const PINNED_SEMANTIC_DIGEST: &str =
    "7FF4FF84343441C5AB64265818074AD8610038FDADD493F6224EAE883E7BCE7D";

fn manifest_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.spec/11764-controller-train-graph/train.manifest.json"
    ))
}

fn base_bytes() -> Result<Vec<u8>> {
    std::fs::read(manifest_path()).with_context(|| "reading the real stable manifest")
}

fn base_value() -> Result<Value> {
    serde_json::from_slice::<Value>(&base_bytes()?).with_context(|| "parsing the real manifest")
}

fn node_mut<'a>(value: &'a mut Value, node_id: &str) -> Result<&'a mut Value> {
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest has no nodes array"))?;
    nodes
        .iter_mut()
        .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} not found in fixture"))
}

/// Serialize a mutated document back to hygiene-clean manifest bytes.
fn serialize(value: &Value) -> Result<Vec<u8>> {
    let mut raw = serde_json::to_string(value).context("serializing mutated fixture")?;
    raw.push('\n');
    Ok(raw.into_bytes())
}

/// Apply a mutation to a fresh copy of the real manifest and require that
/// static validation rejects it, optionally matching a diagnostic fragment.
fn expect_reject_with(
    name: &str,
    fragment: &str,
    mutate: impl FnOnce(&mut Value) -> Result<()>,
) -> Result<()> {
    let mut value = base_value()?;
    mutate(&mut value)?;
    let raw = serialize(&value)?;
    let report = validate_static_bytes(&raw);
    if report.is_valid() {
        bail!("falsifier '{name}' was ACCEPTED by the validator — fail-closed violation");
    }
    let rendered: Vec<String> = report.diagnostics.iter().map(|d| d.render()).collect();
    ensure!(
        rendered.iter().any(|text| text.contains(fragment)),
        "falsifier '{name}' was rejected but no diagnostic matches '{fragment}':\n{}",
        rendered.join("\n")
    );
    Ok(())
}

fn expect_reject(name: &str, mutate: impl FnOnce(&mut Value) -> Result<()>) -> Result<()> {
    let mut value = base_value()?;
    mutate(&mut value)?;
    let raw = serialize(&value)?;
    let report = validate_static_bytes(&raw);
    ensure!(
        !report.is_valid(),
        "falsifier '{name}' was ACCEPTED by the validator — fail-closed violation"
    );
    Ok(())
}

/// Add a dependency edge consistently: dep on `node`, and fix the derived
/// successor set of the target so only the intended law fires.
fn add_edge(value: &mut Value, node: &str, target: &str, class: &str) -> Result<()> {
    let provenance = "#11681 dependency graph";
    {
        let node_value = node_mut(value, node)?;
        let deps = node_value
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("node {node} has no dependencies"))?;
        deps.push(json!({"target": target, "class": class, "provenance": provenance}));
    }
    let target_value = node_mut(value, target)?;
    let successors = target_value
        .get_mut("successors")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {target} has no successors"))?;
    let mut labels: Vec<String> =
        successors.iter().filter_map(Value::as_str).map(str::to_owned).collect();
    labels.push(node.to_owned());
    labels.sort();
    *successors = labels.into_iter().map(Value::String).collect();
    Ok(())
}

fn remove_edge(value: &mut Value, node: &str, target: &str) -> Result<()> {
    {
        let node_value = node_mut(value, node)?;
        let deps = node_value
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("node {node} has no dependencies"))?;
        deps.retain(|dep| dep.get("target").and_then(Value::as_str) != Some(target));
    }
    let target_value = node_mut(value, target)?;
    let successors = target_value
        .get_mut("successors")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {target} has no successors"))?;
    successors.retain(|successor| successor.as_str() != Some(node));
    Ok(())
}

/// Strip every edge around a node so it becomes fully isolated (no deps, no
/// dependents), letting orphan detection fire on a graph that is otherwise
/// internally consistent.
fn isolate_node(value: &mut Value, node_id: &str) -> Result<()> {
    let targets: Vec<String> = {
        let node_value = node_mut(value, node_id)?;
        node_value
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|deps| {
                deps.iter()
                    .filter_map(|dep| dep.get("target").and_then(Value::as_str))
                    .filter(|target| !target.starts_with('#'))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    for target in &targets {
        remove_edge(value, node_id, target)?;
    }
    let dependents: Vec<String> = {
        let nodes = value.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
        nodes
            .iter()
            .filter(|other| {
                other.get("node_id").and_then(Value::as_str) != Some(node_id)
                    && other.get("dependencies").and_then(Value::as_array).is_some_and(|deps| {
                        deps.iter()
                            .any(|dep| dep.get("target").and_then(Value::as_str) == Some(node_id))
                    })
            })
            .filter_map(|other| other.get("node_id").and_then(Value::as_str).map(str::to_owned))
            .collect()
    };
    for dependent in &dependents {
        remove_edge(value, dependent, node_id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The real manifest: pass + digest fidelity.
// ---------------------------------------------------------------------------

#[test]
fn real_manifest_validates_and_digest_matches_t01_pin() -> Result<()> {
    let raw = base_bytes()?;
    let report = validate_static_bytes(&raw);
    let rendered: Vec<String> = report.diagnostics.iter().map(|d| d.render()).collect();
    ensure!(
        report.is_valid(),
        "the landed T01 manifest must pass static validation:\n{}",
        rendered.join("\n")
    );
    let digest = report
        .semantic_digest
        .clone()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid report lacks a digest"))?;
    ensure!(
        digest == PINNED_SEMANTIC_DIGEST,
        "semantic digest {digest} does not match T01's recorded digest {PINNED_SEMANTIC_DIGEST}"
    );
    let summary = GraphSummary::of(
        report.manifest.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?,
    );
    ensure!(summary.node_count == 26, "expected 26 nodes, found {}", summary.node_count);
    ensure!(summary.edge_count == 155, "expected 155 edges, found {}", summary.edge_count);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 1: the validator must not accept malformed input by normalization.
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_bytes_are_rejected_not_normalized() {
    let raw = b"{\"schema\": \"issue_controller_train.v1\", \"schema_version\"";
    let report = validate_static_bytes(raw);
    assert!(report.diagnostics.iter().any(|d| d.check == "serialization"));
}

#[test]
fn unknown_top_level_key_is_rejected() -> Result<()> {
    expect_reject_with("unknown-top-key", "key set required", |value| {
        let map = value.as_object_mut().ok_or_else(|| color_eyre::eyre::eyre!("root"))?;
        map.insert("train_status".to_owned(), json!(true));
        Ok(())
    })
}

#[test]
fn float_numbers_are_rejected_as_schema_defects() -> Result<()> {
    expect_reject_with("float-number", "non-integer", |value| {
        value["schema_version"] = json!(1.5);
        Ok(())
    })
}

#[test]
fn byte_hygiene_rejects_bom_double_lf_and_tabs() {
    let raw = base_bytes().unwrap_or_else(|_| Vec::new());
    let with_bom = [b"\xEF\xBB\xBF".as_slice(), raw.as_slice()].concat();
    assert!(validate_static_bytes(&with_bom).diagnostics.iter().any(|d| d.check == "byte-hygiene"));

    let mut double_lf = raw.clone();
    double_lf.push(b'\n');
    assert!(
        validate_static_bytes(&double_lf)
            .diagnostics
            .iter()
            .any(|d| d.invariant.contains("trailing LF"))
    );

    let mut with_tab = Vec::new();
    with_tab.push(b'{');
    with_tab.push(b'\t');
    with_tab.extend_from_slice(&raw[1..]);
    assert!(
        validate_static_bytes(&with_tab)
            .diagnostics
            .iter()
            .any(|d| d.invariant.contains("no tab bytes"))
    );
}

// ---------------------------------------------------------------------------
// Falsifier 2: controllers never enter builder frontier eligibility.
// ---------------------------------------------------------------------------

#[test]
fn controller_never_enters_builder_frontier() -> Result<()> {
    expect_reject_with("controller-buildable", "assignability", |value| {
        node_mut(value, "CTRL")?["buildable"] = json!(true);
        Ok(())
    })
}

#[test]
fn fan_in_and_external_gate_stay_non_buildable() -> Result<()> {
    for (name, node_id) in [("fan-in", "P02"), ("external-gate", "R05B")] {
        expect_reject_with(name, "assignability", |value| {
            node_mut(value, node_id)?["buildable"] = json!(true);
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn ordinary_nodes_must_stay_buildable() -> Result<()> {
    expect_reject_with("ordinary-unbuildable", "assignability", |value| {
        node_mut(value, "T05")?["buildable"] = json!(false);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 3/4: parallelism and writer slots.
// ---------------------------------------------------------------------------

#[test]
fn two_nodes_cannot_advertise_one_semantic_writer_slot() -> Result<()> {
    expect_reject_with("duplicate-conflict-key", "conflict", |value| {
        let stolen = node_mut(value, "T02")?["writer"]["conflict_key"].clone();
        node_mut(value, "D01")?["writer"]["conflict_key"] = stolen;
        Ok(())
    })
}

#[test]
fn parallel_group_members_must_hard_depend_on_the_gate() -> Result<()> {
    expect_reject_with("parallel-gate-missing", "parallelism", |value| {
        node_mut(value, "C03")?["writer"]["parallel_group"] = json!("post-T04-parallel");
        Ok(())
    })
}

#[test]
fn serial_edge_inside_a_parallel_group_is_rejected() -> Result<()> {
    // R02 and R03 have disjoint writers and share the post-C01 parallel
    // group; a hard edge between them forces serial execution by issue order.
    expect_reject_with("forced-serial", "parallelism", |value| {
        add_edge(value, "C03", "C02", "hard")
    })
}

#[test]
fn stack_relation_and_parallel_group_are_exclusive() -> Result<()> {
    expect_reject_with("stack-parallel", "parallelism", |value| {
        node_mut(value, "C02")?["writer"]["stack_relation"] =
            json!("requires C05 tooling landed on protected main");
        Ok(())
    })
}

#[test]
fn conflict_keys_live_in_the_home_programme_namespace() -> Result<()> {
    expect_reject_with("foreign-writer-namespace", "conflict", |value| {
        node_mut(value, "C01")?["writer"]["conflict_key"] = json!("editor_clients.role_schema");
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 5: proof/fan-in may not repair product behavior.
// ---------------------------------------------------------------------------

#[test]
fn proof_and_fan_in_may_not_repair_product_behavior() -> Result<()> {
    for node_id in ["P01", "P02", "D01"] {
        expect_reject_with("proof-repair", "assignability", |value| {
            node_mut(value, node_id)?["claim_ceiling"] = json!("proves the composed denominator");
            Ok(())
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: one home programme; imports are edges, not second homes.
// ---------------------------------------------------------------------------

#[test]
fn second_home_programme_without_import_edge_is_rejected() -> Result<()> {
    expect_reject_with("second-home", "import", |value| {
        node_mut(value, "C01")?["chain"]["home"] = json!("editor-clients");
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 7: issue roles and train roles never collapse.
// ---------------------------------------------------------------------------

#[test]
fn unknown_train_role_is_rejected() -> Result<()> {
    expect_reject_with("unknown-role", "roles", |value| {
        node_mut(value, "T02")?["train_role"] = json!("ROLE_SCHEMA");
        Ok(())
    })
}

#[test]
fn issue_role_field_on_a_node_is_rejected() -> Result<()> {
    expect_reject_with("issue-role-collapse", "schema", |value| {
        node_mut(value, "T02")?["issue_role"] = json!("controller");
        Ok(())
    })
}

#[test]
fn role_vocabulary_order_is_frozen() -> Result<()> {
    expect_reject_with("role-order", "train role order broken", |value| {
        let roles = value
            .get_mut("train_role_vocabulary")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no vocabulary"))?;
        roles.swap(0, 1);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 8: typed edge classes never collapse.
// ---------------------------------------------------------------------------

#[test]
fn frozen_law_edge_reclassification_is_rejected() -> Result<()> {
    // Registry implementation (C02) satisfies reviewed role labels (C01):
    // the hard class is frozen; substituting optional erases the stage.
    expect_reject_with("law-edge-reclass", "law-edge", |value| {
        let deps = node_mut(value, "C02")?
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no deps"))?;
        for dep in deps {
            if dep.get("target").and_then(Value::as_str) == Some("C01") {
                dep["class"] = json!("optional");
            }
        }
        Ok(())
    })
}

#[test]
fn duplicate_edge_to_one_target_is_rejected() -> Result<()> {
    expect_reject_with("duplicate-edge", "edge-identity", |value| {
        let deps = node_mut(value, "C02")?
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no deps"))?;
        deps.push(
            json!({"target": "C01", "class": "optional", "provenance": "#11683 body references"}),
        );
        Ok(())
    })
}

#[test]
fn generic_depends_on_class_collapse_is_rejected() -> Result<()> {
    expect_reject_with("class-collapse", "edge-class", |value| {
        let deps = node_mut(value, "C04")?
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no deps"))?;
        for dep in deps {
            if dep.get("target").and_then(Value::as_str) == Some("C02") {
                dep["class"] = json!("depends_on");
            }
        }
        Ok(())
    })
}

#[test]
fn external_gate_requires_exactly_one_external_authorization_edge() -> Result<()> {
    expect_reject_with("gate-authorization", "external #EXPLICIT-AUTHORIZATION", |value| {
        let deps = node_mut(value, "R05B")?
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no deps"))?;
        for dep in deps {
            if dep.get("target").and_then(Value::as_str) == Some("#EXPLICIT-AUTHORIZATION") {
                dep["class"] = json!("hard");
            }
        }
        Ok(())
    })
}

#[test]
fn dependency_cycles_over_hard_edges_are_rejected() -> Result<()> {
    expect_reject_with("cycle", "cycle", |value| add_edge(value, "C01", "C02", "hard"))
}

// ---------------------------------------------------------------------------
// Falsifier 9: missing contracts are errors, never warnings.
// ---------------------------------------------------------------------------

#[test]
fn missing_first_falsifier_is_an_error_not_a_warning() -> Result<()> {
    expect_reject_with("missing-falsifier", "contract", |value| {
        node_mut(value, "T05")?["first_falsifier"] = json!(" ");
        Ok(())
    })
}

#[test]
fn missing_rollback_stop_contract_is_an_error() -> Result<()> {
    expect_reject_with("missing-stop", "rollback.stop", |value| {
        node_mut(value, "T06")?["rollback"]["stop"] = json!("");
        Ok(())
    })
}

#[test]
fn missing_spec_disposition_or_review_forward_is_an_error() -> Result<()> {
    expect_reject_with("missing-review", "review_forward", |value| {
        node_mut(value, "T07")?["review_forward"]["questions"] = json!([]);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 10: superseded nodes drain to their successor.
// ---------------------------------------------------------------------------

#[test]
fn superseded_node_staying_active_beside_successor_is_rejected() -> Result<()> {
    expect_reject_with("supersession-active", "supersession", |value| {
        value["supersessions"] = json!([{
            "superseded_node": "T08",
            "successor_issue": 11783,
            "reason": "replaced by the closeout rail"
        }]);
        Ok(())
    })
}

#[test]
fn malformed_supersession_without_successor_is_rejected() -> Result<()> {
    expect_reject_with("supersession-malformed", "schema", |value| {
        value["supersessions"] = json!([{"superseded_node": "T08", "reason": "replaced"}]);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Falsifier 11: input order never changes digest or projection bytes.
// ---------------------------------------------------------------------------

#[test]
fn input_order_does_not_change_digest_or_projection() -> Result<()> {
    let base = base_value()?;
    let mut shuffled = base.clone();

    let nodes = shuffled
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no nodes"))?;
    nodes.sort_by_key(|node| node.get("issue").and_then(Value::as_i64));
    for node in nodes {
        for key in ["dependencies", "successors", "identity_fields", "aliases", "limitations"] {
            if let Some(list) = node.get_mut(key).and_then(Value::as_array_mut) {
                list.reverse();
            }
        }
    }

    let base_digest = canonical_digest(&base)?;
    let shuffled_digest = canonical_digest(&shuffled)?;
    ensure!(
        base_digest == shuffled_digest,
        "canonical digest changed with input order: {base_digest} vs {shuffled_digest}"
    );

    let base_raw = serialize(&base)?;
    let shuffled_raw = serialize(&shuffled)?;
    let base_report = validate_static_bytes(&base_raw);
    let shuffled_report = validate_static_bytes(&shuffled_raw);
    ensure!(base_report.is_valid(), "base manifest stopped validating");
    ensure!(shuffled_report.is_valid(), "shuffled manifest must still validate");

    let base_manifest =
        base_report.manifest.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?;
    let shuffled_manifest = shuffled_report
        .manifest
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?;
    let base_projection = super::projection::render_projection(
        base_manifest,
        &base_digest,
        &GraphSummary::of(base_manifest),
    );
    let shuffled_projection = super::projection::render_projection(
        shuffled_manifest,
        &shuffled_digest,
        &GraphSummary::of(shuffled_manifest),
    );
    ensure!(
        base_projection == shuffled_projection,
        "generated projection changed with input order"
    );
    Ok(())
}

#[test]
fn projection_generation_is_deterministic_on_second_run() -> Result<()> {
    let raw = base_bytes()?;
    let report = validate_static_bytes(&raw);
    let manifest =
        report.manifest.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?;
    let digest = report
        .semantic_digest
        .as_deref()
        .ok_or_else(|| color_eyre::eyre::eyre!("digest missing"))?;
    let first = super::projection::render_projection(manifest, digest, &GraphSummary::of(manifest));
    let second =
        super::projection::render_projection(manifest, digest, &GraphSummary::of(manifest));
    ensure!(first == second, "second-run projection differs from the first run");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 12: no live/current state enters stable bytes or the projection.
// ---------------------------------------------------------------------------

#[test]
fn live_sha_tokens_in_manifest_values_are_rejected() -> Result<()> {
    expect_reject_with("live-sha", "live-state", |value| {
        let limitations = node_mut(value, "T03")?
            .get_mut("limitations")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no limitations"))?;
        limitations.push(json!(" rebased onto head 4f5bcb334 deadbeef 00ff11"));
        Ok(())
    })
}

#[test]
fn live_pr_and_timestamp_tokens_are_rejected() -> Result<()> {
    expect_reject_with("live-pr", "live-state", |value| {
        let limitations = node_mut(value, "T03")?
            .get_mut("limitations")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no limitations"))?;
        limitations.push(json!("merged via PR #12027 at 2026-08-20T10:00:00Z from origin/tooling"));
        Ok(())
    })
}

#[test]
fn generated_projection_carries_no_live_state() -> Result<()> {
    let raw = base_bytes()?;
    let report = validate_static_bytes(&raw);
    let manifest =
        report.manifest.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?;
    let digest = report
        .semantic_digest
        .as_deref()
        .ok_or_else(|| color_eyre::eyre::eyre!("digest missing"))?;
    let document =
        super::projection::render_projection(manifest, digest, &GraphSummary::of(manifest));
    for token in ["origin/", "refs/heads/", "pull/", "PR #", "merge-base", "worktrees/"] {
        ensure!(!document.contains(token), "projection contains live-state token '{token}'");
    }
    ensure!(!document.contains("2026-"), "projection contains a timestamp-like token");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 13: the human graph agrees exactly with the machine graph.
// ---------------------------------------------------------------------------

#[test]
fn projection_agrees_with_the_machine_graph() -> Result<()> {
    let raw = base_bytes()?;
    let report = validate_static_bytes(&raw);
    let manifest =
        report.manifest.as_ref().ok_or_else(|| color_eyre::eyre::eyre!("manifest missing"))?;
    let digest = report
        .semantic_digest
        .as_deref()
        .ok_or_else(|| color_eyre::eyre::eyre!("digest missing"))?;
    let summary = GraphSummary::of(manifest);
    let document = super::projection::render_projection(manifest, digest, &summary);

    for node in &manifest.nodes {
        ensure!(
            document.contains(&format!("### {} #{}", node.node_id, node.issue)),
            "projection is missing node {}",
            node.node_id
        );
    }
    for node in &manifest.nodes {
        for dep in &node.dependencies {
            let line = format!("- {} --{}--> {}", node.node_id, dep.class, dep.target);
            ensure!(document.contains(&line), "projection is missing edge {line}");
        }
    }
    ensure!(
        document.contains(&format!("- typed edges: {}", summary.edge_count)),
        "projection edge count disagrees with the machine graph"
    );
    ensure!(
        document.contains(&format!("semantic digest: {digest}")),
        "projection is not digest-bound to the manifest"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 14: references must have owners or explicit planned state.
// ---------------------------------------------------------------------------

#[test]
fn ownerless_consumed_authority_is_rejected() -> Result<()> {
    expect_reject_with("ownerless-authority", "authorities", |value| {
        let consumed = node_mut(value, "C01")?
            .get_mut("consumed_authorities")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no consumed authorities"))?;
        consumed.push(json!("#9999"));
        Ok(())
    })
}

#[test]
fn depending_on_an_unregistered_authority_is_rejected() -> Result<()> {
    expect_reject_with("unknown-edge-authority", "unknown external authority", |value| {
        let deps = node_mut(value, "T05")?
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no deps"))?;
        deps.push(
            json!({"target": "#9999", "class": "evidence", "provenance": "#11772 body references"}),
        );
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Structural uniqueness, node-set and route laws.
// ---------------------------------------------------------------------------

#[test]
fn duplicate_authority_after_proposition_is_rejected() -> Result<()> {
    expect_reject_with("duplicate-authority-after", "uniqueness", |value| {
        let stolen = node_mut(value, "T02")?["authority_after"].clone();
        node_mut(value, "T03")?["authority_after"] = stolen;
        Ok(())
    })
}

#[test]
fn duplicate_node_id_is_rejected() -> Result<()> {
    expect_reject_with("duplicate-node", "uniqueness", |value| {
        let clone = node_mut(value, "T02")?.clone();
        value.get_mut("nodes").and_then(Value::as_array_mut).map(|nodes| nodes.push(clone));
        Ok(())
    })
}

#[test]
fn unexpected_extra_node_is_rejected() -> Result<()> {
    expect_reject_with("unexpected-node", "node-set", |value| {
        let mut clone = node_mut(value, "T02")?.clone();
        clone["node_id"] = json!("Z99");
        clone["issue"] = json!(99999);
        clone["writer"]["conflict_key"] = json!("issue_controllers.z99");
        clone["authority_after"] = json!("a brand new authority");
        clone["title_fingerprint"] = json!("0123456789ABCDEF");
        value.get_mut("nodes").and_then(Value::as_array_mut).map(|nodes| nodes.push(clone));
        Ok(())
    })
}

#[test]
fn missing_expected_node_is_rejected() -> Result<()> {
    expect_reject_with("missing-node", "node-set", |value| {
        // Removing T02R breaks both the frozen node set and revision
        // governance ownership in one shot.
        if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
            nodes.retain(|node| node.get("node_id").and_then(Value::as_str) != Some("T02R"));
        }
        Ok(())
    })
}

#[test]
fn successor_drift_from_derived_reverse_edges_is_rejected() -> Result<()> {
    expect_reject_with("successor-drift", "successors", |value| {
        let successors = node_mut(value, "C01")?
            .get_mut("successors")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no successors"))?;
        successors.retain(|successor| successor.as_str() != Some("C02"));
        Ok(())
    })
}

#[test]
fn orphaned_node_is_rejected() -> Result<()> {
    expect_reject_with("orphan", "orphan", |value| {
        // Strip every edge around D01 so it has neither dependencies nor
        // dependents; orphan detection must fire.
        isolate_node(value, "D01")
    })
}

#[test]
fn authority_plane_order_is_frozen() -> Result<()> {
    expect_reject_with("plane-order", "authority plane order broken", |value| {
        let planes = value
            .get_mut("authority_planes")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("no planes"))?;
        planes.swap(0, 1);
        Ok(())
    })
}

#[test]
fn evidence_semantics_cannot_disappear() -> Result<()> {
    expect_reject_with("evidence-semantics-removed", "key set required", |value| {
        value.as_object_mut().map(|map| map.remove("evidence_semantics"));
        Ok(())
    })
}

#[test]
fn revision_governance_ownership_is_frozen() -> Result<()> {
    expect_reject_with("revision-owner", "revision", |value| {
        value["revision_governance"]["owner_node"] = json!("T03");
        Ok(())
    })
}

#[test]
fn title_fingerprint_must_recompute_from_the_title() -> Result<()> {
    expect_reject_with("fingerprint-drift", "fingerprint", |value| {
        node_mut(value, "T02")?["title_fingerprint"] = json!("0000000000000000");
        Ok(())
    })
}

#[test]
fn typed_manifest_parses_and_round_trips_the_node_count() -> Result<()> {
    let raw = base_bytes()?;
    let value: Value = serde_json::from_slice(&raw)?;
    let manifest: Manifest = serde_json::from_value(value)?;
    ensure!(manifest.nodes.len() == 26, "typed model must carry 26 nodes");
    ensure!(manifest.nodes.iter().any(|node| node.node_id == "P02" && node.train_role == "fan_in"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Command-level behavior: fail-closed routing, drift detection, explanation.
// ---------------------------------------------------------------------------

#[test]
fn check_without_static_fails_closed() -> Result<()> {
    // The static-plane guard fires before any path is touched, so temporary
    // paths exercise it deterministically.
    let manifest = Path::new("unused-manifest.json");
    let projection = Path::new("unused-projection.md");
    let error = run_train_at(TrainCommand::Check { static_plane: false }, manifest, projection)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("non-static check must fail closed"))?;
    assert!(error.to_string().contains("refusing to run a non-static check"));
    Ok(())
}

#[test]
fn graph_generate_then_check_round_trip_and_drift_detection() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let manifest_path = tmp.path().join("train.manifest.json");
    let projection_path = tmp.path().join("train.projection.md");
    std::fs::write(&manifest_path, base_bytes()?)?;

    run_train_at(TrainCommand::Graph { check: false }, &manifest_path, &projection_path)?;
    ensure!(projection_path.exists(), "graph generation must write the projection");

    run_train_at(TrainCommand::Graph { check: true }, &manifest_path, &projection_path)?;

    let original = std::fs::read_to_string(&projection_path)?;
    let tampered = original.replace("claim ceiling", "claim  ceiling");
    ensure!(tampered != original, "tamper must change the projection");
    std::fs::write(&projection_path, tampered)?;
    let error = run_train_at(TrainCommand::Graph { check: true }, &manifest_path, &projection_path)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("drifted projection must fail the check"))?;
    assert!(error.to_string().contains("drifted"));

    run_train_at(TrainCommand::Graph { check: false }, &manifest_path, &projection_path)?;
    run_train_at(TrainCommand::Graph { check: true }, &manifest_path, &projection_path)?;
    Ok(())
}

#[test]
fn graph_generation_refuses_an_invalid_manifest() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let manifest_path = tmp.path().join("train.manifest.json");
    let projection_path = tmp.path().join("train.projection.md");
    let mut value = base_value()?;
    node_mut(&mut value, "CTRL")?["buildable"] = json!(true);
    std::fs::write(&manifest_path, serialize(&value)?)?;
    let error =
        run_train_at(TrainCommand::Graph { check: false }, &manifest_path, &projection_path)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("invalid manifest must never be projected"))?;
    assert!(error.to_string().contains("refusing to project"));
    ensure!(!projection_path.exists(), "no projection may be written for an invalid manifest");
    Ok(())
}

#[test]
fn explain_static_renders_known_nodes_and_fails_closed_on_unknown() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let manifest_path = tmp.path().join("train.manifest.json");
    std::fs::write(&manifest_path, base_bytes()?)?;
    let projection_path = tmp.path().join("unused.md");
    let command = TrainCommand::ExplainStatic { node: "C05".to_owned() };
    run_train_at(command, &manifest_path, &projection_path)?;

    let error = run_train_at(
        TrainCommand::ExplainStatic { node: "Z99".to_owned() },
        &manifest_path,
        &projection_path,
    )
    .err()
    .ok_or_else(|| color_eyre::eyre::eyre!("unknown node must fail closed"))?;
    assert!(error.to_string().contains("unknown train node"));
    assert!(error.to_string().contains("C01"));
    Ok(())
}
