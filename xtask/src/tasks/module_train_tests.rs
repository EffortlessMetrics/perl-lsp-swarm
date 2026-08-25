//! Focused proof for the C02 module-train slice (#11626).
//!
//! Layers:
//! * digest binding: the real manifest's canonical digest equals the pin,
//!   and every tampering mutation moves it (fail-closed drift);
//! * structural laws: successor/reverse-edge identity, fingerprints,
//!   uniqueness, role laws, cycles, import-class agreement;
//! * frontier derivation: known answers on the current tree, plus
//!   recompute-not-hardcode discriminators (mutating manifest data moves
//!   the frontier exactly as the typed edges dictate);
//! * typed effects: evidence never hard-blocks, bindings block consumers,
//!   external gates stay authorization-blocked, supersessions fail closed;
//! * determinism: two renders are byte-identical and insertion order does
//!   not move any byte.

use super::*;
use color_eyre::eyre::{Context, Result, bail};

fn real_value() -> Result<Value> {
    let root = crate::utils::project_root()?;
    let bytes = std::fs::read(root.join(MANIFEST_RELATIVE_PATH))
        .with_context(|| "failed to read the workspace manifest for tests")?;
    Ok(serde_json::from_slice(&bytes).with_context(|| "workspace manifest is not valid JSON")?)
}

fn parse_manifest(value: &Value) -> Result<Manifest> {
    serde_json::from_value(value.clone()).with_context(|| "strict manifest deserialization failed")
}

fn find_node_mut<'a>(value: &'a mut Value, node_id: &str) -> Result<&'a mut Value> {
    let nodes = value
        .get_mut("nodes")
        .and_then(|nodes| nodes.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest has no nodes array"))?;
    nodes
        .iter_mut()
        .find(|node| node.get("node_id").and_then(|id| id.as_str()) == Some(node_id))
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} not found"))
}

fn add_dep(value: &mut Value, from: &str, target: &str, class: &str) -> Result<()> {
    let provenance = "#11626 focused fixture mutation".to_string();
    let node = find_node_mut(value, from)?;
    let deps = node
        .get_mut("dependencies")
        .and_then(|deps| deps.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("node {from} has no dependencies array"))?;
    deps.push(serde_json::json!({ "target": target, "class": class, "provenance": provenance }));
    if !target.starts_with('#') {
        let target_node = find_node_mut(value, target)?;
        let successors = target_node
            .get_mut("successors")
            .and_then(|successors| successors.as_array_mut())
            .ok_or_else(|| color_eyre::eyre::eyre!("node {target} has no successors array"))?;
        successors.push(serde_json::Value::String(from.to_string()));
    }
    Ok(())
}

fn remove_dep(value: &mut Value, from: &str, target: &str) -> Result<()> {
    let node = find_node_mut(value, from)?;
    let deps = node
        .get_mut("dependencies")
        .and_then(|deps| deps.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("node {from} has no dependencies array"))?;
    deps.retain(|dep| dep.get("target").and_then(|t| t.as_str()) != Some(target));
    if !target.starts_with('#') {
        let target_node = find_node_mut(value, target)?;
        let successors = target_node
            .get_mut("successors")
            .and_then(|successors| successors.as_array_mut())
            .ok_or_else(|| color_eyre::eyre::eyre!("node {target} has no successors array"))?;
        successors.retain(|id| id.as_str() != Some(from));
    }
    Ok(())
}

fn ready_ids(statuses: &[NodeStatus]) -> Vec<String> {
    statuses
        .iter()
        .filter(|status| status.state == CurrentTreeState::Ready)
        .map(|status| status.node_id.clone())
        .collect()
}

#[test]
fn manifest_sections_reveal_identity_values() -> Result<()> {
    // Discriminating per-section assertions over the typed model at the
    // pinned digest: each top-level section's identity is asserted, not just
    // parsed. (Values are facts of the pinned manifest revision; a classified
    // #11625 revision moves the digest pin and this test together.)
    let manifest = parse_manifest(&real_value()?)?;
    assert!(validate_manifest(&manifest).is_ok(), "pinned manifest must validate");
    assert_eq!(manifest.programme.parent_programme_issue, 8133, "parent programme issue");
    assert_eq!(manifest.programme.controller_issue, 4240, "programme controller issue");
    assert_eq!(manifest.programme.evidence_controller_issue, 8479, "evidence controller issue");
    assert_eq!(manifest.programme.home_programme, "module-programme", "home programme");
    assert_eq!(manifest.authority_planes.len(), 8, "authority plane count");
    assert!(
        manifest.authority_planes[0].plane.contains("durable module programme decisions"),
        "first plane identity"
    );
    let roles: Vec<&str> =
        manifest.train_role_vocabulary.iter().map(|entry| entry.role.as_str()).collect();
    assert_eq!(
        roles,
        vec![
            "controller",
            "spec",
            "evidence",
            "implementation",
            "cutover",
            "retirement",
            "proof",
            "fan_in",
            "claim",
            "external_gate"
        ],
        "train role vocabulary identity"
    );
    assert!(
        manifest.evidence_semantics.not_proven_law.contains("never pass"),
        "not-proven law wording"
    );
    assert!(
        manifest
            .external_authorities
            .iter()
            .any(|authority| authority.id == "#EXPLICIT-AUTHORIZATION"),
        "external authorization authority present"
    );
    assert!(
        manifest.open_decisions_routed_elsewhere.iter().any(|decision| decision.id == "OD1"),
        "OD1 routed to #10554"
    );
    assert_eq!(
        manifest.case_work_packet_bindings.consumers,
        vec!["M00S".to_string(), "P11A".to_string(), "P11F".to_string()],
        "binding consumers identity"
    );
    assert_eq!(
        manifest.case_work_packet_bindings.status, "structurally_pending",
        "binding status identity"
    );
    let profile_ids: Vec<&str> =
        manifest.claim_profiles.iter().map(|profile| profile.id.as_str()).collect();
    assert_eq!(
        profile_ids,
        vec![
            "module_contract_grounded",
            "module_static_resolution_core",
            "module_live_runtime_cutover",
            "module_exact_process_resolution_core",
            "module_exact_process_semantic_edit",
            "module_exact_process_full_closeout"
        ],
        "claim profile identity"
    );
    assert!(
        manifest
            .cross_programme_imports
            .iter()
            .any(|import| import.authority == "#10554" && import.relation == "consumed law"),
        "#10554 consumed-law import present"
    );
    assert_eq!(manifest.revision_governance.owner_node, "C01", "revision governance owner");
    assert_eq!(manifest.revision_governance.owner_issue, 11625, "revision governance issue");
    assert_eq!(manifest.nodes.len(), 52, "node count identity");
    let c01 = manifest
        .nodes
        .iter()
        .find(|node| node.node_id == "C01")
        .ok_or_else(|| color_eyre::eyre::eyre!("C01 missing"))?;
    assert_eq!(
        c01.review_forward.lenses,
        vec!["claim-ceiling honesty".to_string(), "dependency-type faithfulness".to_string()]
    );
    assert_eq!(c01.obligations.changelog, "none (internal contract)");
    Ok(())
}

#[test]
fn render_binding_reveals_both_worktree_states() -> Result<()> {
    let loaded = load_manifest()?;
    let clean = render_binding(
        &TreeBinding { tree_head: "B".repeat(40), dirty_paths: 0, manifest_dirty: false },
        &loaded,
    );
    assert!(clean.contains("worktree: clean"), "clean branch must render: {clean}");
    assert!(clean.contains("manifest_state: committed"), "committed branch must render: {clean}");
    let dirty = render_binding(
        &TreeBinding { tree_head: "C".repeat(40), dirty_paths: 7, manifest_dirty: true },
        &loaded,
    );
    assert!(dirty.contains("worktree: dirty:7paths"), "dirty branch must render exactly: {dirty}");
    assert!(dirty.contains("manifest_state: dirty"), "dirty manifest must render: {dirty}");
    Ok(())
}

#[test]
fn non_edge_import_cannot_carry_a_dependency_edge() -> Result<()> {
    let mut value = real_value()?;
    // #3982 is a consumed law, not an import edge: carrying a dependency on
    // it must be rejected by the import relation law.
    add_dep(&mut value, "M01", "#3982", "hard")?;
    let manifest = parse_manifest(&value)?;
    let outcome = validate_manifest(&manifest);
    assert!(outcome.is_err(), "non-edge import carrying a dependency must be rejected");
    let message = outcome.err().map(|error| error.to_string()).unwrap_or_default();
    assert!(
        message.contains("non-edge import"),
        "wrong structural failure for a non-edge import: {message}"
    );
    Ok(())
}

#[test]
fn load_errors_name_their_exact_cause() -> Result<()> {
    let dir = std::env::temp_dir().join("plsw-11626-module-train-tests");
    std::fs::create_dir_all(&dir).with_context(|| "failed to create test temp dir")?;

    let missing = dir.join("missing.manifest.json");
    let _ = std::fs::remove_file(&missing);
    let missing_outcome = load_manifest_from(&missing);
    assert!(missing_outcome.is_err(), "missing manifest must fail");
    let missing_message = missing_outcome.err().map(|error| error.to_string()).unwrap_or_default();
    assert!(
        missing_message.contains("failed to read"),
        "missing manifest failure must name the read: {missing_message}"
    );

    let garbage = dir.join("garbage.manifest.json");
    std::fs::write(&garbage, b"{ not json").with_context(|| "failed to write garbage fixture")?;
    let garbage_outcome = load_manifest_from(&garbage);
    let _ = std::fs::remove_file(&garbage);
    assert!(garbage_outcome.is_err(), "garbage manifest must fail");
    let garbage_message = garbage_outcome.err().map(|error| error.to_string()).unwrap_or_default();
    assert!(
        garbage_message.contains("not valid JSON"),
        "garbage manifest failure must name the parse: {garbage_message}"
    );
    Ok(())
}

fn status_for<'a>(statuses: &'a [NodeStatus], node_id: &str) -> Result<&'a NodeStatus> {
    statuses
        .iter()
        .find(|status| status.node_id == node_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} missing from projection"))
}

fn synthetic_binding() -> TreeBinding {
    TreeBinding { tree_head: "A".repeat(40), dirty_paths: 0, manifest_dirty: false }
}

// ---------------------------------------------------------------------------
// Digest binding (fail-closed on manifest drift).
// ---------------------------------------------------------------------------

#[test]
fn canonical_digest_of_current_manifest_matches_pin() -> Result<()> {
    let value = real_value()?;
    let digest = canonical_digest(&value)?;
    if digest != PINNED_CANONICAL_DIGEST {
        bail!("canonical digest {digest} does not match pin {PINNED_CANONICAL_DIGEST}");
    }
    Ok(())
}

#[test]
fn any_content_mutation_moves_the_digest() -> Result<()> {
    let base = canonical_digest(&real_value()?)?;
    let mutations: Vec<(&str, Box<dyn Fn(&mut Value) -> Result<()>>)> = vec![
        (
            "title text tampered",
            Box::new(|value: &mut Value| {
                find_node_mut(value, "C03")?["title"] =
                    serde_json::Value::String("tampered title".to_string());
                Ok(())
            }),
        ),
        (
            "edge class collapse evidence->hard",
            Box::new(|value: &mut Value| {
                let node = find_node_mut(value, "M01")?;
                let deps = node["dependencies"]
                    .as_array_mut()
                    .ok_or_else(|| color_eyre::eyre::eyre!("M01 dependencies missing"))?;
                for dep in deps.iter_mut() {
                    if dep["target"].as_str() == Some("E00A") {
                        dep["class"] = serde_json::Value::String("hard".to_string());
                    }
                }
                Ok(())
            }),
        ),
        (
            "binding promoted to bound",
            Box::new(|value: &mut Value| {
                value["case_work_packet_bindings"]["status"] =
                    serde_json::Value::String("bound".to_string());
                Ok(())
            }),
        ),
        (
            "live SHA smuggled into a limitation",
            Box::new(|value: &mut Value| {
                find_node_mut(value, "C03")?["limitations"]
                    .as_array_mut()
                    .ok_or_else(|| color_eyre::eyre::eyre!("limitations missing"))?
                    .push(serde_json::Value::String("rebased onto deadbeefdeadbeef".to_string()));
                Ok(())
            }),
        ),
    ];
    for (name, mutation) in mutations {
        let mut value = real_value()?;
        mutation(&mut value)?;
        let digest = canonical_digest(&value)?;
        if digest == base {
            bail!("mutation '{name}' failed to move the canonical digest");
        }
        if digest == PINNED_CANONICAL_DIGEST {
            bail!("mutation '{name}' still matches the pinned digest");
        }
    }
    Ok(())
}

#[test]
fn tampered_manifest_file_fails_closed_at_load() -> Result<()> {
    let mut value = real_value()?;
    find_node_mut(&mut value, "C03")?["claim_ceiling"] =
        serde_json::Value::String("widened ceiling".to_string());
    let dir = std::env::temp_dir().join("plsw-11626-module-train-tests");
    std::fs::create_dir_all(&dir).with_context(|| "failed to create test temp dir")?;
    let path = dir.join("tampered.manifest.json");
    std::fs::write(&path, serde_json::to_vec(&value)?)
        .with_context(|| "failed to write tampered manifest fixture")?;
    let outcome = load_manifest_from(&path);
    let _ = std::fs::remove_file(&path);
    if outcome.is_ok() {
        bail!("tampered manifest loaded successfully; the digest pin is not fail-closed");
    }
    let message = outcome.err().map(|error| error.to_string()).unwrap_or_default();
    if !message.contains("digest drift") {
        bail!("tampered manifest failure did not name digest drift: {message}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Structural laws.
// ---------------------------------------------------------------------------

#[test]
fn current_manifest_passes_structural_laws() -> Result<()> {
    let manifest = parse_manifest(&real_value()?)?;
    assert!(validate_manifest(&manifest).is_ok(), "pinned manifest must pass structural laws");
    Ok(())
}

#[test]
fn title_fingerprint_tamper_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    find_node_mut(&mut value, "C01")?["title_fingerprint"] =
        serde_json::Value::String("0000000000000000".to_string());
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("tampered title fingerprint passed structural validation");
    }
    Ok(())
}

#[test]
fn duplicate_conflict_key_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let stolen = find_node_mut(&mut value, "E00A")?["writer"]["conflict_key"].clone();
    find_node_mut(&mut value, "E00B")?["writer"]["conflict_key"] = stolen;
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("duplicate writer conflict key passed structural validation");
    }
    Ok(())
}

#[test]
fn duplicate_edge_to_one_target_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // C03 already hard-depends on C02; a second edge to the same target is a
    // conflicting identity.
    add_dep(&mut value, "C03", "C02", "optional")?;
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("duplicate dependency edge passed structural validation");
    }
    Ok(())
}

#[test]
fn self_dependency_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    remove_dep(&mut value, "C03", "C02")?;
    add_dep(&mut value, "C03", "C03", "hard")?;
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("self-dependency passed structural validation");
    }
    Ok(())
}

#[test]
fn unknown_dependency_target_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Retarget C03's hard edge in place; the unknown node target must be
    // rejected before any projection can treat it as satisfied.
    let c03 = find_node_mut(&mut value, "C03")?;
    let deps = c03
        .get_mut("dependencies")
        .and_then(|deps| deps.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("C03 dependencies missing"))?;
    for dep in deps.iter_mut() {
        if dep["target"].as_str() == Some("C02") {
            dep["target"] = serde_json::Value::String("ZZZ".to_string());
        }
    }
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("unknown dependency target passed structural validation");
    }
    Ok(())
}

#[test]
fn one_sided_edge_removal_breaks_successor_identity() -> Result<()> {
    let mut value = real_value()?;
    // Remove the successor mention without removing the edge: successors
    // must be exactly the derived reverse-edge set.
    let c02 = find_node_mut(&mut value, "C02")?;
    let successors = c02
        .get_mut("successors")
        .and_then(|successors| successors.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("C02 successors missing"))?;
    successors.retain(|id| id.as_str() != Some("C03"));
    let manifest = parse_manifest(&value)?;
    let outcome = validate_manifest(&manifest);
    if outcome.is_ok() {
        bail!("successor set mismatch passed structural validation");
    }
    let message = outcome.err().map(|error| error.to_string()).unwrap_or_default();
    if !message.contains("successor set mismatch") {
        bail!("wrong structural failure for successor tampering: {message}");
    }
    Ok(())
}

#[test]
fn hard_cycle_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // M07A has no dependencies; a hard edge M07A -> M07C closes a cycle
    // through M07C -> M07B -> ... -> M07A while keeping successors exact.
    add_dep(&mut value, "M07A", "M07C", "hard")?;
    let manifest = parse_manifest(&value)?;
    let outcome = validate_manifest(&manifest);
    if outcome.is_ok() {
        bail!("hard cycle passed structural validation");
    }
    let message = outcome.err().map(|error| error.to_string()).unwrap_or_default();
    if !message.contains("cycle") {
        bail!("wrong structural failure for a cycle: {message}");
    }
    Ok(())
}

#[test]
fn controller_made_buildable_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    find_node_mut(&mut value, "CTRL")?["buildable"] = serde_json::Value::Bool(true);
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("buildable controller passed the role/buildable law");
    }
    Ok(())
}

#[test]
fn evidence_import_edge_must_stay_evidence_class() -> Result<()> {
    let mut value = real_value()?;
    // #11114 is an evidence import; hardening C02's edge to it violates the
    // import relation law.
    let c02 = find_node_mut(&mut value, "C02")?;
    let deps = c02
        .get_mut("dependencies")
        .and_then(|deps| deps.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("C02 dependencies missing"))?;
    for dep in deps.iter_mut() {
        if dep["target"].as_str() == Some("#11114") {
            dep["class"] = serde_json::Value::String("hard".to_string());
        }
    }
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("hardened evidence import edge passed the import class law");
    }
    Ok(())
}

#[test]
fn external_authorization_edge_requires_gate_role_and_class() -> Result<()> {
    let mut value = real_value()?;
    add_dep(&mut value, "M01", "#EXPLICIT-AUTHORIZATION", "hard")?;
    let manifest = parse_manifest(&value)?;
    if validate_manifest(&manifest).is_ok() {
        bail!("#EXPLICIT-AUTHORIZATION as a hard edge passed the gate law");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Frontier derivation: known answers on the current tree.
// ---------------------------------------------------------------------------

#[test]
fn frontier_known_answers_on_current_tree() -> Result<()> {
    let manifest = parse_manifest(&real_value()?)?;
    let statuses = project_states(&manifest)?;
    let ready = ready_ids(&statuses);
    if ready != vec!["C02".to_string(), "E00A".to_string(), "M01".to_string(), "M07A".to_string()] {
        bail!("unexpected ready frontier: {ready:?}");
    }

    let c01 = status_for(&statuses, "C01")?;
    if c01.state != CurrentTreeState::LandedCurrentTree
        || c01.implementation_presence != ProbeOutcome::Pass
    {
        bail!("C01 must be landed via its manifest probe");
    }

    let c03 = status_for(&statuses, "C03")?;
    if c03.state != CurrentTreeState::BlockedHard {
        bail!("C03 must be hard-blocked on C02, found {:?}", c03.state);
    }
    if !c03.reasons.iter().any(|reason| reason == "hard_dep_not_landed:C02") {
        bail!("C03 reasons must name the exact unlanded hard dep: {:?}", c03.reasons);
    }

    let ctrl = status_for(&statuses, "CTRL")?;
    if ctrl.state != CurrentTreeState::NotProven
        || !ctrl.reasons.iter().any(|reason| reason == "role_never_implementation_start:controller")
    {
        bail!("controllers must never enter the frontier: {ctrl:?}");
    }

    let fan_in = status_for(&statuses, "P11F")?;
    if fan_in.state != CurrentTreeState::NotProven
        || !fan_in.reasons.iter().any(|reason| reason == "role_never_implementation_start:fan_in")
    {
        bail!("fan-in must never enter the frontier: {fan_in:?}");
    }

    // Case/work-packet binding stays structurally pending for consumers.
    let m00s = status_for(&statuses, "M00S")?;
    if m00s.state != CurrentTreeState::BlockedHard
        || !m00s
            .reasons
            .iter()
            .any(|reason| reason == "case_work_packet_binding:structurally_pending")
    {
        bail!("M00S must carry the pending binding as a typed reason: {m00s:?}");
    }

    // Retirement cannot precede its admitted cutovers.
    let l09g = status_for(&statuses, "L09G")?;
    if l09g.state != CurrentTreeState::BlockedHard {
        bail!("L09G must be hard-blocked, found {:?}", l09g.state);
    }
    for cutover in ["L09A", "L09B", "L09C", "L09D", "L09E", "L09F"] {
        let reason = format!("hard_dep_not_landed:{cutover}");
        if !l09g.reasons.iter().any(|candidate| candidate == &reason) {
            bail!("L09G reasons must name {reason}: {:?}", l09g.reasons);
        }
    }

    // Cross-programme hard imports stay honestly unestablished offline.
    let m02 = status_for(&statuses, "M02")?;
    if !m02
        .reasons
        .iter()
        .any(|reason| reason == "hard_dep_cross_programme_state_not_establishable:#7622")
    {
        bail!("M02 must record the cross-programme hard import: {:?}", m02.reasons);
    }

    // Evidence-class deps stay visible limitations, never hard blockers:
    // M01 is ready while E00A/E00B remain unlanded evidence deps.
    let m01 = status_for(&statuses, "M01")?;
    if !m01.reasons.iter().any(|reason| reason == "evidence_dep_not_current:E00A") {
        bail!("M01 must keep its evidence limitation visible: {:?}", m01.reasons);
    }
    Ok(())
}

#[test]
fn unprobed_nodes_stay_not_proven_never_guessed() -> Result<()> {
    let manifest = parse_manifest(&real_value()?)?;
    let statuses = project_states(&manifest)?;
    for status in &statuses {
        if status.node_id == "C01" {
            continue;
        }
        if status.implementation_presence != ProbeOutcome::Absent {
            bail!("node {} has a probe outcome this slice cannot have", status.node_id);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recompute, do not hardcode: manifest data moves the frontier.
// ---------------------------------------------------------------------------

#[test]
fn landing_a_node_by_data_unblocks_its_hard_dependents() -> Result<()> {
    let mut value = real_value()?;
    // Simulate a landed world purely through manifest data: retarget E00B's
    // and E00C's hard edges from the unlanded E00A to the landed C01
    // (keeping the successor law exact). The projection must recompute the
    // frontier from the data: E00B/E00C become hard-ready, while M00S stays
    // blocked on E00D/E00E.
    for node in ["E00B", "E00C"] {
        remove_dep(&mut value, node, "E00A")?;
        add_dep(&mut value, node, "C01", "hard")?;
    }
    let manifest = parse_manifest(&value)?;
    validate_manifest(&manifest)?;
    let statuses = project_states(&manifest)?;
    let ready = ready_ids(&statuses);
    for node in ["E00B", "E00C"] {
        if !ready.contains(&node.to_string()) {
            bail!("landed hard dep by data must unblock {node}: ready={ready:?}");
        }
    }
    let m00s = status_for(&statuses, "M00S")?;
    if m00s.state != CurrentTreeState::BlockedHard {
        bail!("M00S must stay blocked on the remaining E00 family: {:?}", m00s.state);
    }
    Ok(())
}

#[test]
fn class_collapse_reintroduces_the_hard_block() -> Result<()> {
    let mut value = real_value()?;
    let m01 = find_node_mut(&mut value, "M01")?;
    let deps = m01
        .get_mut("dependencies")
        .and_then(|deps| deps.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("M01 dependencies missing"))?;
    for dep in deps.iter_mut() {
        if dep["target"].as_str() == Some("E00A") {
            dep["class"] = serde_json::Value::String("hard".to_string());
        }
    }
    let manifest = parse_manifest(&value)?;
    let statuses = project_states(&manifest)?;
    let m01 = status_for(&statuses, "M01")?;
    if m01.state != CurrentTreeState::BlockedHard {
        bail!("hardening M01's E00A edge must hard-block M01, found {:?}", m01.state);
    }
    Ok(())
}

#[test]
fn controller_edges_never_gate_builders() -> Result<()> {
    let mut value = real_value()?;
    // E00A's only hard dep is the EVID controller; dropping a controller edge
    // must not change E00A's readiness, while dropping its (nonexistent) node
    // deps would. Prove the controller law by adding a second controller hard
    // edge: E00A stays ready.
    add_dep(&mut value, "E00A", "CTRL", "hard")?;
    let manifest = parse_manifest(&value)?;
    let statuses = project_states(&manifest)?;
    let e00a = status_for(&statuses, "E00A")?;
    if e00a.state != CurrentTreeState::Ready {
        bail!("controller hard edge must not gate E00A: {:?}", e00a.state);
    }
    Ok(())
}

#[test]
fn populated_supersessions_fail_closed() -> Result<()> {
    let mut value = real_value()?;
    value["supersessions"] = serde_json::json!([{ "node": "C01", "by": "C99", "note": "fixture" }]);
    let manifest = parse_manifest(&value)?;
    if project_states(&manifest).is_ok() {
        bail!("populated supersessions must fail closed in this slice");
    }
    Ok(())
}

#[test]
fn binding_consumer_without_hard_blocks_is_blocked_evidence() -> Result<()> {
    let mut value = real_value()?;
    // Reduce M00S to only its controller hard edge so the pending binding is
    // the strongest typed block left.
    for target in ["E00A", "E00B", "E00C", "E00D", "E00E"] {
        remove_dep(&mut value, "M00S", target)?;
    }
    let manifest = parse_manifest(&value)?;
    let statuses = project_states(&manifest)?;
    let m00s = status_for(&statuses, "M00S")?;
    if m00s.state != CurrentTreeState::BlockedEvidence {
        bail!("M00S must be blocked_evidence once hard deps clear: {:?}", m00s.state);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Determinism.
// ---------------------------------------------------------------------------

#[test]
fn renders_are_byte_identical_across_runs() -> Result<()> {
    let loaded = load_manifest()?;
    let binding = synthetic_binding();
    let first = render_status(&loaded, &binding)?;
    let second = render_status(&loaded, &binding)?;
    if first != second {
        bail!("status render is not deterministic");
    }
    let first_next = render_next(&loaded, &binding)?;
    let second_next = render_next(&loaded, &binding)?;
    if first_next != second_next {
        bail!("next render is not deterministic");
    }
    if !first_next.contains("ready_leaves: 4") {
        bail!("next render lost the known frontier size:\n{first_next}");
    }
    Ok(())
}

#[test]
fn insertion_order_does_not_move_any_byte() -> Result<()> {
    let mut value = real_value()?;
    let nodes = value
        .get_mut("nodes")
        .and_then(|nodes| nodes.as_array_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("nodes array missing"))?;
    nodes.reverse();
    for node in nodes.iter_mut() {
        if let Some(deps) = node.get_mut("dependencies").and_then(|deps| deps.as_array_mut()) {
            deps.reverse();
        }
        if let Some(successors) =
            node.get_mut("successors").and_then(|successors| successors.as_array_mut())
        {
            successors.reverse();
        }
        if let Some(fields) =
            node.get_mut("identity_fields").and_then(|fields| fields.as_array_mut())
        {
            fields.reverse();
        }
    }
    let baseline_digest = canonical_digest(&real_value()?)?;
    let shuffled_digest = canonical_digest(&value)?;
    if baseline_digest != shuffled_digest {
        bail!("canonical digest moved with insertion order");
    }
    let baseline_statuses = project_states(&parse_manifest(&real_value()?)?)?;
    let shuffled_statuses = project_states(&parse_manifest(&value)?)?;
    let baseline_lines: Vec<String> = baseline_statuses
        .iter()
        .map(|status| format!("{}|{:?}|{:?}", status.node_id, status.state, status.reasons))
        .collect();
    let shuffled_lines: Vec<String> = shuffled_statuses
        .iter()
        .map(|status| format!("{}|{:?}|{:?}", status.node_id, status.state, status.reasons))
        .collect();
    if baseline_lines != shuffled_lines {
        bail!("state projection moved with insertion order");
    }
    Ok(())
}

#[test]
fn tree_binding_rejects_non_head_trees() -> Result<()> {
    // Assert the rejection reason, not merely failure: a tree_binding broken
    // elsewhere (project-root or git spawn faults) must not satisfy this test.
    let failure = match tree_binding("origin/main") {
        Err(failure) => failure,
        Ok(binding) => {
            bail!("non-HEAD tree binding must fail closed in this slice, got {binding:?}")
        }
    };
    let message = format!("{failure:#}");
    if !message.contains("binds --tree HEAD only") {
        bail!("non-HEAD rejection failed for the wrong reason: {message}");
    }
    Ok(())
}

#[test]
fn git_facts_follow_the_loaded_repository_not_the_process_cwd() -> Result<()> {
    // Regression for the foreign-repo binding defect: git output must be
    // resolved inside the passed root. A scratch repository with one known
    // commit must report its own HEAD even though the test process runs in
    // the worktree.
    let dir = std::env::temp_dir().join("plsw-11626-module-train-scratch-repo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).with_context(|| "failed to create scratch repo dir")?;
    fn scratch_git(dir: &std::path::Path, args: &[&str]) -> Result<()> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", std::path::Path::new("NUL"))
            .env("GIT_CONFIG_SYSTEM", std::path::Path::new("NUL"))
            .output()
            .with_context(|| format!("failed to spawn git {}", args.join(" ")))?;
        if !output.status.success() {
            bail!("git {} failed in scratch repo", args.join(" "));
        }
        Ok(())
    }
    scratch_git(&dir, &["init", "--quiet"])?;
    scratch_git(&dir, &["config", "user.email", "scratch@example.invalid"])?;
    scratch_git(&dir, &["config", "user.name", "scratch"])?;
    std::fs::write(dir.join("marker.txt"), "scratch").with_context(|| "scratch marker write")?;
    scratch_git(&dir, &["add", "marker.txt"])?;
    scratch_git(&dir, &["commit", "--quiet", "-m", "scratch"])?;
    let scratch_head = git_output(&dir, &["rev-parse", "HEAD"])?;
    let worktree_root = crate::utils::project_root()?;
    let worktree_head = git_output(&worktree_root, &["rev-parse", "HEAD"])?;
    if scratch_head == worktree_head {
        bail!("scratch discrimination failed: both repositories report the same HEAD");
    }
    // The property under test: same binary, same process, different roots —
    // each root's HEAD is reported for that root.
    let again = git_output(&dir, &["rev-parse", "HEAD"])?;
    if again != scratch_head {
        bail!("git output does not follow the passed repository root");
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
