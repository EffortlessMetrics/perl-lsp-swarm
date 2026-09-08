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

/// The real repository tree, used wherever a test asserts current-tree truth.
fn real_tree() -> Result<RepoTreeSource> {
    RepoTreeSource::from_project_root()
}

/// An injectable tree whose files are exactly what a test declares.
///
/// Probes read the tree through this seam, so the negative direction — a tree
/// whose implementation or whose production consumer is missing — is testable
/// without mutating the repository.
struct FakeTree {
    files: std::collections::BTreeMap<String, String>,
}

impl FakeTree {
    /// Start from the real tree's probe-relevant files, so a test changes one
    /// fact at a time instead of reconstructing a whole repository.
    fn from_real() -> Result<Self> {
        let real = real_tree()?;
        let mut files = std::collections::BTreeMap::new();
        for path in [
            "xtask/src/main.rs",
            "xtask/src/tasks/module_train.rs",
            "xtask/src/tasks/module_train_live.rs",
        ] {
            if let Some(text) = real.read_text(path)? {
                files.insert(path.to_string(), text);
            }
        }
        Ok(Self { files })
    }

    /// Remove an anchor wherever it appears, simulating a tree where that
    /// exact semantic fact is absent.
    fn without_anchor(mut self, path: &str, anchor: &str) -> Result<Self> {
        let text = self
            .files
            .get(path)
            .ok_or_else(|| color_eyre::eyre::eyre!("fake tree has no {path}"))?;
        if !text.contains(anchor) {
            bail!("anchor {anchor} is not present in {path}; the fixture cannot falsify anything");
        }
        let stripped = text.replace(anchor, "__REMOVED_BY_FIXTURE__");
        self.files.insert(path.to_string(), stripped);
        Ok(self)
    }

    /// Add text to a file, simulating a tree where a residual component landed.
    fn with_added(mut self, path: &str, addition: &str) -> Result<Self> {
        let text = self
            .files
            .get(path)
            .ok_or_else(|| color_eyre::eyre::eyre!("fake tree has no {path}"))?;
        if text.contains(addition) {
            bail!(
                "addition is already present in {path}; the fixture cannot prove that landing it changes anything"
            );
        }
        let extended = format!("{text}\n{addition}\n");
        self.files.insert(path.to_string(), extended);
        Ok(self)
    }

    fn without_file(mut self, path: &str) -> Self {
        self.files.remove(path);
        self
    }
}

impl TreeSource for FakeTree {
    fn read_text(&self, relative: &str) -> Result<Option<String>> {
        Ok(self.files.get(relative).cloned())
    }
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
    let statuses = project_states(&manifest, &real_tree()?)?;
    let ready = ready_ids(&statuses);
    // C02 is no longer a ready start: its own implementation is partly on this
    // tree, so it reports `incomplete_current_tree` instead of inviting a
    // fresh agent to begin work that already exists.
    if ready != vec!["E00A".to_string(), "M01".to_string(), "M07A".to_string()] {
        bail!("unexpected ready frontier: {ready:?}");
    }

    let c01 = status_for(&statuses, "C01")?;
    if c01.state != CurrentTreeState::LandedCurrentTree
        || c01.implementation_presence != ProbeOutcome::Pass
    {
        bail!("C01 must be landed via its manifest probe");
    }

    // The train can now see itself: C02 partially, C03 fully.
    let c02 = status_for(&statuses, "C02")?;
    if c02.state != CurrentTreeState::IncompleteCurrentTree
        || c02.implementation_presence != ProbeOutcome::Partial
    {
        bail!("C02 must be incomplete on this tree, found {c02:?}");
    }
    if !c02
        .reasons
        .iter()
        .any(|reason| reason == "implementation_component_absent:static_packet_projection")
    {
        bail!("C02 must name its exact missing component: {:?}", c02.reasons);
    }

    let c03 = status_for(&statuses, "C03")?;
    if c03.state != CurrentTreeState::LandedCurrentTree
        || c03.implementation_presence != ProbeOutcome::Pass
    {
        bail!("C03 must be landed via its semantic probe, found {c03:?}");
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
    let statuses = project_states(&manifest, &real_tree()?)?;
    // Exactly the nodes with a written semantic probe may report anything but
    // `not_proven`. Every other node stays unguessed, so adding a probe is a
    // deliberate act rather than a side effect of a merge or a filename.
    for status in &statuses {
        if matches!(status.node_id.as_str(), "C01" | "C02" | "C03") {
            continue;
        }
        if status.implementation_presence != ProbeOutcome::Unprobed {
            bail!("node {} has a probe outcome this slice cannot have", status.node_id);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Semantic implementation probes (#11626 residual 1): the probe must read the
// tree, must require the production consumer, and must not be satisfiable by
// a neighbouring node's surface.
// ---------------------------------------------------------------------------

fn probe_for(node_id: &str, tree: &dyn TreeSource) -> Result<(ProbeOutcome, Vec<String>)> {
    let manifest = parse_manifest(&real_value()?)?;
    let node = manifest
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} not found"))?;
    probes::node_probe(node, tree)
}

/// Falsifier 2 of #11626: a module present on the tree while the production
/// consumer never dispatches to it must not read as landed.
#[test]
fn an_implementation_without_its_production_consumer_is_not_landed() -> Result<()> {
    // C03's modules stay exactly as they are; only the CLI dispatch to its
    // live refresh disappears.
    let tree = FakeTree::from_real()?
        .without_anchor("xtask/src/main.rs", "module_train_live::run_refresh")?;
    let (outcome, unmet) = probe_for("C03", &tree)?;
    if outcome != ProbeOutcome::Partial || !unmet.iter().any(|c| c == "live_snapshot_normalization")
    {
        bail!("an unwired C03 must not be landed: {outcome:?} unmet={unmet:?}");
    }
    Ok(())
}

/// A comment or string literal that repeats a selector must not satisfy the
/// production probe when the declaration and dispatch are absent.
#[test]
fn comments_and_literals_cannot_satisfy_dispatch_anchors() -> Result<()> {
    let real_tree = FakeTree::from_real()?;
    let (real_outcome, real_unmet) = probe_for("C02", &real_tree)?;
    if real_outcome != ProbeOutcome::Partial
        || real_unmet.iter().any(|component| component == "current_tree_probes")
    {
        bail!("real C02 dispatch probe is not positive: {real_outcome:?} unmet={real_unmet:?}");
    }

    let tree = real_tree
        .without_anchor("xtask/src/main.rs", "ModuleTrainCommand::Status")?
        .without_anchor("xtask/src/main.rs", "module_train::run_status")?
        .with_added(
            "xtask/src/main.rs",
            r#"
                // ModuleTrainCommand::Status
                const DOCUMENTATION: &str = "module_train::run_status";
                #[cfg(not(test))]
                fn unrelated_dispatch(command: ModuleTrainCommand) {
                    match command {
                        ModuleTrainCommand::Status { tree } => {
                            module_train::run_status(&tree);
                        }
                        _ => {}
                    }
                }
            "#,
        )?;
    let (outcome, unmet) = probe_for("C02", &tree)?;
    if outcome == ProbeOutcome::Pass
        || !unmet.iter().any(|component| component == "current_tree_probes")
    {
        bail!("comment-only dispatch anchors satisfied C02: {outcome:?} unmet={unmet:?}");
    }
    Ok(())
}

/// The mirror control: an implementation module removed while the CLI still
/// references it is equally not landed.
#[test]
fn a_consumer_without_its_implementation_is_not_landed() -> Result<()> {
    let tree = FakeTree::from_real()?.without_file("xtask/src/tasks/module_train_live.rs");
    let (outcome, unmet) = probe_for("C03", &tree)?;
    let expected = ["action_classification", "live_snapshot_normalization"];
    if outcome != ProbeOutcome::NotPresent || unmet != expected {
        bail!("C03 without its module must be wholly absent: {outcome:?} unmet={unmet:?}");
    }
    Ok(())
}

#[test]
fn adding_existing_fixture_content_is_rejected() -> Result<()> {
    let tree = FakeTree::from_real()?;
    if tree.with_added("xtask/src/main.rs", "ModuleTrainCommand::Status").is_ok() {
        bail!("fixture additions already present in the seed tree must be rejected");
    }
    Ok(())
}

#[test]
fn pinned_tree_source_ignores_worktree_edit_after_capture() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let path = repo.path().join("probe.rs");
    std::fs::write(&path, "fn captured() {}\n")?;
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "user.name", "test"],
        vec!["add", "probe.rs"],
        vec!["commit", "-qm", "capture"],
    ] {
        let output =
            std::process::Command::new("git").args(&args).current_dir(repo.path()).output()?;
        if !output.status.success() {
            bail!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
        }
    }
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.path())
        .output()?;
    if !head.status.success() {
        bail!("git rev-parse HEAD failed");
    }
    let head = String::from_utf8(head.stdout)?.trim().to_string();
    let tree_spec = format!("{head}^{{tree}}");
    let tree = std::process::Command::new("git")
        .args(["rev-parse", tree_spec.as_str()])
        .current_dir(repo.path())
        .output()?;
    if !tree.status.success() {
        bail!("git rev-parse HEAD^{{tree}} failed");
    }
    let tree = String::from_utf8(tree.stdout)?.trim().to_string();
    std::fs::write(&path, "fn edited() {}\n")?;
    let source = RepoTreeSource::from_root_at_revision(repo.path().to_path_buf(), tree)?;
    let captured = source
        .read_text("probe.rs")?
        .ok_or_else(|| color_eyre::eyre::eyre!("captured probe path must exist"))?;
    if captured != "fn captured() {}\n" {
        bail!("pinned source read the mutable worktree instead of the captured tree");
    }
    Ok(())
}

/// Wrong-subject control: C03's live `explain` composes a live addendum and is
/// a different subject from C02's offline static packet. It must not satisfy
/// C02's residual component.
#[test]
fn the_live_explain_cannot_satisfy_the_offline_static_packet() -> Result<()> {
    let tree = FakeTree::from_real()?;
    // The real tree already ships `ModuleTrainLiveCommand::Explain` and
    // `module_train_live::run_explain`; if those satisfied C02, C02 would be
    // landed here rather than incomplete.
    let (outcome, unmet) = probe_for("C02", &tree)?;
    if outcome != ProbeOutcome::Partial || !unmet.iter().any(|c| c == "static_packet_projection") {
        bail!("C03's live explain must not satisfy C02's offline packet: {outcome:?} {unmet:?}");
    }
    Ok(())
}

/// Recompute, do not hardcode: landing the residual component in the tree must
/// flip C02 to landed without touching the probe registry.
#[test]
fn landing_the_residual_component_lands_c02() -> Result<()> {
    let tree = FakeTree::from_real()?.with_added(
        "xtask/src/main.rs",
        r#"
            fn run_cli(command: Commands) {
                match command {
                    Commands::ModuleTrain { command } => match command {
                        ModuleTrainCommand::Explain { node, tree } => {
                            module_train::run_explain(&node, &tree);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        "#,
    )?;
    let (outcome, unmet) = probe_for("C02", &tree)?;
    if outcome != ProbeOutcome::Pass || !unmet.is_empty() {
        bail!("completing C02's declared surface must land it: {outcome:?} unmet={unmet:?}");
    }

    // And a landed C02 satisfies its dependents' hard edge.
    let manifest = parse_manifest(&real_value()?)?;
    let statuses = project_states(&manifest, &tree)?;
    let c02 = status_for(&statuses, "C02")?;
    if c02.state != CurrentTreeState::LandedCurrentTree {
        bail!("C02 must project as landed once complete: {:?}", c02.state);
    }
    Ok(())
}

/// A probed node whose surface is wholly absent stays a legitimate start:
/// adding a probe must never turn an unbuilt node into a blocked one.
#[test]
fn a_wholly_absent_probed_node_still_reports_through_dependencies() -> Result<()> {
    let tree = FakeTree::from_real()?
        .without_file("xtask/src/tasks/module_train_live.rs")
        .without_anchor("xtask/src/main.rs", "ModuleTrainLiveCommand::Refresh")?
        .without_anchor("xtask/src/main.rs", "ModuleTrainLiveCommand::Next")?;
    let manifest = parse_manifest(&real_value()?)?;
    let statuses = project_states(&manifest, &tree)?;
    let c03 = status_for(&statuses, "C03")?;
    if c03.implementation_presence != ProbeOutcome::NotPresent {
        bail!("C03 must probe as absent here: {:?}", c03.implementation_presence);
    }
    // C02 is incomplete on this tree, so C03's hard edge is genuinely unmet.
    if c03.state != CurrentTreeState::BlockedHard
        || !c03.reasons.iter().any(|reason| reason == "hard_dep_not_landed:C02")
    {
        bail!("an absent C03 must fall through to dependency typing: {c03:?}");
    }
    Ok(())
}

/// A partially implemented node is not landed for its dependents.
#[test]
fn a_partial_node_does_not_satisfy_a_hard_dependent() -> Result<()> {
    let manifest = parse_manifest(&real_value()?)?;
    // C02 is Partial on the real tree; C03 passes its own probe, so drop C03's
    // probe surface to observe the edge itself.
    let tree = FakeTree::from_real()?.without_file("xtask/src/tasks/module_train_live.rs");
    let statuses = project_states(&manifest, &tree)?;
    let c03 = status_for(&statuses, "C03")?;
    if !c03.reasons.iter().any(|reason| reason == "hard_dep_not_landed:C02") {
        bail!("a partial C02 must not satisfy C03's hard edge: {:?}", c03.reasons);
    }
    Ok(())
}

/// Anti-vacuity, structural: no selector may target the module that declares
/// `PROBED_NODES`. If it did, the registry could become a self-targeting
/// source instead of an independent selector declaration.
#[test]
fn no_selector_targets_the_registry_that_declares_it() -> Result<()> {
    if real_tree()?.read_text(probes::REGISTRY_RELATIVE_PATH)?.is_none() {
        bail!(
            "recorded registry path {} does not exist; the anti-vacuity guard would compare against nothing",
            probes::REGISTRY_RELATIVE_PATH
        );
    }
    for path in probes::selector_paths() {
        if path == probes::REGISTRY_RELATIVE_PATH {
            bail!(
                "selector targets {path}, the file holding the anchor literals; \
                 it could be satisfied by its own declaration"
            );
        }
    }
    Ok(())
}

/// Negative direction for C02's `current_tree_probes` component. This is only
/// falsifiable because the anchor literals live in the probe registry module
/// while the anchors target `module_train.rs`: stripping the real code here
/// does not also strip the selector that names it.
#[test]
fn c02_current_tree_probes_component_fails_without_its_projection() -> Result<()> {
    let tree = FakeTree::from_real()?
        .without_anchor("xtask/src/tasks/module_train.rs", "fn project_states")?;
    let (outcome, unmet) = probe_for("C02", &tree)?;
    if !unmet.iter().any(|c| c == "current_tree_probes") {
        bail!("removing project_states must unmeet current_tree_probes: {outcome:?} {unmet:?}");
    }
    Ok(())
}

/// Negative direction for C02's `offline_frontier` component.
#[test]
fn c02_offline_frontier_component_fails_without_its_renderer() -> Result<()> {
    let tree = FakeTree::from_real()?
        .without_anchor("xtask/src/tasks/module_train.rs", "fn render_next")?;
    let (outcome, unmet) = probe_for("C02", &tree)?;
    if !unmet.iter().any(|c| c == "offline_frontier") {
        bail!("removing render_next must unmeet offline_frontier: {outcome:?} {unmet:?}");
    }
    Ok(())
}

/// A partially implemented node still records what its edges would block on,
/// rather than reporting only its missing component.
#[test]
fn a_partial_node_still_records_its_dependency_reasons() -> Result<()> {
    let mut value = real_value()?;
    // Give C02 an unmet hard edge on an unlanded node.
    add_dep(&mut value, "C02", "E00A", "hard")?;
    let manifest = parse_manifest(&value)?;
    let statuses = project_states(&manifest, &real_tree()?)?;
    let c02 = status_for(&statuses, "C02")?;
    if c02.state != CurrentTreeState::IncompleteCurrentTree {
        bail!("presence must still outrank the edge for the state: {:?}", c02.state);
    }
    if !c02.reasons.iter().any(|reason| reason == "hard_dep_not_landed:E00A") {
        bail!("a partial node must still show its unmet edge: {:?}", c02.reasons);
    }
    Ok(())
}

/// Anchor currency: on the real tree, exactly the anchors of C02's recorded
/// residual component may be missing. Any other missing anchor means a rename
/// silently drifted the registry away from the code — which would quietly
/// demote a landed node to `partial` rather than fail loudly.
#[test]
fn only_the_recorded_residual_anchors_are_missing_on_the_real_tree() -> Result<()> {
    let tree = real_tree()?;
    let residual = ["ModuleTrainCommand::Explain", "module_train::run_explain"];
    let mut missing: Vec<&str> = Vec::new();
    for (path, anchor) in probes::selector_anchors() {
        let present = tree.read_text(path)?.is_some_and(|text| text.contains(anchor));
        if !present {
            missing.push(anchor);
        }
    }
    missing.sort_unstable();
    let mut expected = residual;
    expected.sort_unstable();
    if missing != expected {
        bail!(
            "registry anchors drifted from the code: missing={missing:?}, \
             expected only the recorded residual {expected:?}"
        );
    }
    Ok(())
}

/// Instrument honesty: an unreadable selector target is an error, never a
/// silent absence that would render as a confident `not_proven`.
#[test]
fn a_fixture_that_cannot_falsify_is_rejected() -> Result<()> {
    // The helper refuses to "remove" an anchor that was never there, so a
    // negative control cannot silently degrade into a no-op.
    if FakeTree::from_real()?
        .without_anchor("xtask/src/main.rs", "ModuleTrainCommand::Explain")
        .is_ok()
    {
        bail!("removing an absent anchor must be rejected as a vacuous fixture");
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
    let statuses = project_states(&manifest, &real_tree()?)?;
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
    let statuses = project_states(&manifest, &real_tree()?)?;
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
    let statuses = project_states(&manifest, &real_tree()?)?;
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
    if project_states(&manifest, &real_tree()?).is_ok() {
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
    let statuses = project_states(&manifest, &real_tree()?)?;
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
    let first = render_status(&loaded, &binding, &real_tree()?)?;
    let second = render_status(&loaded, &binding, &real_tree()?)?;
    if first != second {
        bail!("status render is not deterministic");
    }
    let first_next = render_next(&loaded, &binding, &real_tree()?)?;
    let second_next = render_next(&loaded, &binding, &real_tree()?)?;
    if first_next != second_next {
        bail!("next render is not deterministic");
    }
    // Three, not four: C02 left the frontier when its own semantic probe
    // began reporting the implementation already on this tree.
    if !first_next.contains("ready_leaves: 3") {
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
    let baseline_statuses = project_states(&parse_manifest(&real_value()?)?, &real_tree()?)?;
    let shuffled_statuses = project_states(&parse_manifest(&value)?, &real_tree()?)?;
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
