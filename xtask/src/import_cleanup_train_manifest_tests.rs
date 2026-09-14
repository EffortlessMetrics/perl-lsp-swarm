//! Focused proof for the ICT-C01 cleanup-train manifest (#11084).
//!
//! Layers, in the issue's shift-left order: malformed falsifiers first (each
//! of the twelve required shapes becomes an explicit mutation that must fail
//! closed with its named reason), then the happy graph's identity values, then
//! determinism and pin-binding guarantees. Every mutation works on the real
//! committed manifest bytes, so the proof discriminates the actual artifact
//! rather than a private toy copy.

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
        .with_context(|| "failed to read the workspace cleanup-train manifest for tests")?;
    serde_json::from_slice(&bytes).with_context(|| "workspace manifest is not valid JSON")
}

fn parse_strict(value: &Value) -> Result<Manifest> {
    serde_json::from_value(value.clone()).with_context(|| "strict deserialization failed")
}

fn validate(value: &Value) -> Result<()> {
    validate_manifest(&parse_strict(value)?)
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
        .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} not found"))
}

fn set_node_string(value: &mut Value, node_id: &str, field: &str, new_value: &str) -> Result<()> {
    let node = node_mut(value, node_id)?;
    *node
        .get_mut(field)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} lacks {field}"))? =
        Value::String(new_value.to_string());
    Ok(())
}

fn append_dep(value: &mut Value, from: &str, target: &str, class: &str) -> Result<()> {
    let deps = node_mut(value, from)?
        .get_mut("dependencies")
        .ok_or_else(|| color_eyre::eyre::eyre!("node {from} has no dependencies"))?
        .as_array_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("dependencies is not a list"))?;
    deps.push(serde_json::json!({
        "target": target,
        "class": class,
        "provenance": "#11084 falsifier mutation",
    }));
    Ok(())
}

// ---------------------------------------------------------------------------
// Malformed falsifiers first (the issue's shift-left review map).
// ---------------------------------------------------------------------------

#[test]
fn import_cleanup_train_manifest_controller_as_leaf_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    append_dep(&mut value, "CTRL1719", "GEO10647", "hard")?;
    assert_rejected(&value, "carry no dependency edges")
}

#[test]
fn import_cleanup_train_manifest_missing_containment_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.retain(|n| n.get("node_id").and_then(Value::as_str) != Some("CON11079"));
    }
    assert_rejected(&value, "four containment withdrawals")
}

#[test]
fn import_cleanup_train_manifest_operation_context_stage_transfer_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // DIAG10723 consumes the dispositions rail only as evidence; relabeling it
    // hard must fail because diagnostic rows do not sit above the contract on
    // the promotion lattice.
    let deps = node_mut(&mut value, "DIAG10723")?
        .get_mut("dependencies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no dependencies array"))?;
    for dep in deps.iter_mut() {
        if dep.get("target").and_then(Value::as_str) == Some("SCNT10710") {
            *dep.get_mut("class").ok_or_else(|| color_eyre::eyre::eyre!("no class"))? =
                Value::String("hard".into());
        }
    }
    assert_rejected(&value, "promotion lattice")
}

#[test]
fn import_cleanup_train_manifest_code_action_completion_collapse_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Dragging an internal plan row into a visible completion context gives
    // add_missing two completion_item consumers without touching any adapter:
    // exactly the collapse identity law 6 forbids.
    set_node_string(&mut value, "FACT11009", "product_context", "completion_item")?;
    assert_rejected(&value, "context collapse")
}

#[test]
fn import_cleanup_train_manifest_internal_plan_spine_role_reuse_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // The internal-plan spine stays stage-separated by role: the five active
    // add_missing internal_plan rows each hold a distinct role. Give DEC8948
    // the role ADM11169 already holds and the spine collapses to one stage.
    //
    // This is the negative control for the `spine_seen.insert` guard (#12910).
    // The insert is a side effect inside a match guard: a first sighting must
    // still be recorded and fall through, while a repeat must fail closed.
    // `import_cleanup_train_manifest_happy_graph_identity_values_hold`
    // covers the fall-through direction; this covers the bail.
    set_node_string(&mut value, "DEC8948", "role", "product_admission")?;
    assert_rejected(&value, "two internal-plan rows share role")
}

#[test]
fn import_cleanup_train_manifest_wire_confusion_between_adapters_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_string(&mut value, "CADP11184", "wire_kind", "workspace_edit")?;
    assert_rejected(&value, "exactly one governed completion_adapter")
}

#[test]
fn import_cleanup_train_manifest_missing_falsifier_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_string(&mut value, "ASM10757", "first_falsifier", "   ")?;
    assert_rejected(&value, "empty first_falsifier")
}

#[test]
fn import_cleanup_train_manifest_plausible_wrong_copy_paste_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let node = node_mut(&mut value, "DIAG11145")?;
    let source = node
        .get("plausible_wrong_implementation")
        .and_then(Value::as_str)
        .ok_or_else(|| color_eyre::eyre::eyre!("DIAG11145 lacks plausible_wrong"))?
        .to_string();
    set_node_string(&mut value, "DIAG10723", "plausible_wrong_implementation", &source)?;
    assert_rejected(&value, "must stay unique across nodes")
}

#[test]
fn import_cleanup_train_manifest_vague_spec_instruction_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let node = node_mut(&mut value, "IMPL10689")?;
    *node
        .get_mut("spec")
        .ok_or_else(|| color_eyre::eyre::eyre!("IMPL10689 has no spec block"))?
        .get_mut("requirement")
        .ok_or_else(|| color_eyre::eyre::eyre!("spec block has no requirement"))? =
        Value::String("none".into());
    assert_rejected(&value, "refuses to name its spec requirement")
}

#[test]
fn import_cleanup_train_manifest_duplicated_authority_slot_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let gov_key = node_mut(&mut value, "GOV11084")?
        .get("conflict_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    set_node_string(&mut value, "LIVE10696", "conflict_key", &gov_key)?;
    assert_rejected(&value, "duplicate writer conflict key")
}

#[test]
fn import_cleanup_train_manifest_absent_defect_owner_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let node = node_mut(&mut value, "ASM11138")?;
    let transfer = node
        .get_mut("adjacent_defect_transfer")
        .ok_or_else(|| color_eyre::eyre::eyre!("no defect transfer"))?;
    *transfer.get_mut("owner").ok_or_else(|| color_eyre::eyre::eyre!("no owner"))? =
        Value::String(" ".into());
    assert_rejected(&value, "empty adjacent_defect_transfer.owner")
}

#[test]
fn import_cleanup_train_manifest_external_as_native_readiness_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Even with a forged lattice pair, the explicit external-to-native law
    // fires: ECOM rows can never sit under client/closeout consumers.
    append_dep(&mut value, "CLNT10912", "ECOM10802", "hard")?;
    if let Some(lattice) = value.get_mut("edge_role_lattice").and_then(Value::as_array_mut) {
        lattice.push(
            serde_json::json!({"from_role": "external_compatibility", "to_role": "actual_client"}),
        );
    } else {
        bail!("manifest lost its lattice");
    }
    assert_rejected(&value, "external compatibility stage feeds native evidence")
}

#[test]
fn import_cleanup_train_manifest_reference_apply_claiming_client_stage_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_string(&mut value, "PRHF10877", "application_ceiling", "actual_client_applied")?;
    assert_rejected(&value, "exceeds role cap")
}

#[test]
fn import_cleanup_train_manifest_candidate_discovery_overclaim_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_string(&mut value, "DISC11185", "claim_ceiling", "client_verified")?;
    assert_rejected(&value, "exceeds the reviewed cap")
}

#[test]
fn import_cleanup_train_manifest_nondeterministic_order_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    if let Some(nodes) = value.get_mut("nodes").and_then(Value::as_array_mut) {
        if nodes.len() < 3 {
            bail!("manifest too small for reorder probe");
        }
        nodes.swap(0, 2);
    }
    assert_rejected(&value, "strictly ascending node_id order")
}

#[test]
fn import_cleanup_train_manifest_unknown_vocabulary_value_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    set_node_string(&mut value, "FACT10646", "role", "wizard_fact_projection")?;
    assert_rejected(&value, "unknown role")
}

#[test]
fn import_cleanup_train_manifest_dependency_cycle_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // LIVE10758 already depends on PURE10749; the reverse edge closes a cycle.
    append_dep(&mut value, "PURE10749", "LIVE10758", "hard")?;
    if let Some(lattice) = value.get_mut("edge_role_lattice").and_then(Value::as_array_mut) {
        lattice.push(
            serde_json::json!({"from_role": "code_action_adapter", "to_role": "pure_transform"}),
        );
    }
    assert_rejected(&value, "dependency cycle detected")
}

#[test]
fn import_cleanup_train_manifest_phantom_command_surface_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let templates = node_mut(&mut value, "GOV11084")?
        .get_mut("proof_command_templates")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("no command templates"))?;
    templates[0] = Value::String("cargo xtask check-architecture".into());
    assert_rejected(&value, "outside the proven verification surface")
}

#[test]
fn import_cleanup_train_manifest_superseded_generation_reentry_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    append_dep(&mut value, "SPEC11087", "OLD8310", "evidence")?;
    assert_rejected(&value, "depends on superseded generation")
}

// ---------------------------------------------------------------------------
// Happy graph identity + determinism + digest binding.
// ---------------------------------------------------------------------------

#[test]
fn import_cleanup_train_manifest_digest_pin_binds_committed_bytes() -> Result<()> {
    let loaded = load_manifest_from(repo_manifest_path()?.as_path())?;
    assert_eq!(loaded.canonical_digest(), PINNED_CANONICAL_DIGEST);
    Ok(())
}

#[test]
fn import_cleanup_train_manifest_digest_moves_on_any_semantic_tampering() -> Result<()> {
    let mut value = real_value()?;
    let before = canonical_digest(&value)?;
    set_node_string(&mut value, "SCNT10710", "title", "contract(imports): drifted title")?;
    let after = canonical_digest(&value)?;
    assert_ne!(before, after, "semantic tampering must move the canonical digest");
    // And the pinned loader refuses such bytes outright.
    assert_rejected_via_loader(&value).or_else(|err| {
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("digest drift"),
            "expected digest drift refusal, got: {rendered}"
        );
        Ok(())
    })
}

fn assert_rejected_via_loader(value: &Value) -> Result<()> {
    let path = std::env::temp_dir().join("import_cleanup_train_manifest_drift_probe.json");
    std::fs::write(&path, serde_json::to_vec_pretty(value)?)?;
    match load_manifest_from(&path) {
        Ok(_) => bail!("loader accepted tampered bytes"),
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            bail!("{err:#}")
        }
    }
}

#[test]
fn import_cleanup_train_manifest_digest_survives_array_order_only_changes() -> Result<()> {
    let mut value = real_value()?;
    // Reordering an order-free list (authority planes are content-addressed by
    // the walk) must not move the digest: order invariance is the
    // determinism guarantee downstream slices rely on.
    if let Some(planes) = value.get_mut("authority_planes").and_then(Value::as_array_mut) {
        planes.reverse();
    } else {
        bail!("manifest lost authority planes");
    }
    let reordered = canonical_digest(&value)?;
    let original = real_value()?;
    assert_eq!(canonical_digest(&original)?, reordered);
    // Node order, by contrast, is part of the deterministic surface.
    if let Some(nodes) = original.get("nodes").and_then(Value::as_array) {
        let ids: Vec<&str> =
            nodes.iter().filter_map(|n| n.get("node_id").and_then(Value::as_str)).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "committed nodes must be pre-sorted");
    }
    Ok(())
}

#[test]
fn import_cleanup_train_manifest_happy_graph_identity_values_hold() -> Result<()> {
    let loaded = load_manifest_from(repo_manifest_path()?.as_path())?;
    let root = repo_manifest_path()?;
    let value: Value = serde_json::from_slice(&std::fs::read(&root)?)?;
    let manifest = parse_strict(&value)?;
    assert!(validate(&value).is_ok(), "pinned manifest must satisfy every structural law");

    assert_eq!(manifest.programme.parent_programme_issue, 8277);
    assert_eq!(manifest.programme.controller_issue, 11081);
    assert_eq!(manifest.programme.evidence_controller_issue, 8336);

    assert_eq!(loaded.node_count(), 80, "complete reviewed graph population");
    let roles_of = |id: &str| {
        loaded
            .node_static_fact(id)
            .map(|f| f.role)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing node {id}"))
    };
    assert_eq!(roles_of("GOV11084")?, "semantic_contract");
    assert_eq!(roles_of("CON8305")?, "containment");
    assert_eq!(roles_of("GRD10865")?, "authority_guard");
    assert_eq!(roles_of("ADPT10667")?, "code_action_adapter");
    assert_eq!(roles_of("CADP11184")?, "completion_adapter");
    assert_eq!(roles_of("CLSE10945")?, "claim_closeout");

    // Four containment contexts stay independent.
    for id in ["CON8305", "CON10690", "CON11079", "CON11158"] {
        let fact =
            loaded.node_static_fact(id).ok_or_else(|| color_eyre::eyre::eyre!("missing {id}"))?;
        assert!(fact.dependencies.is_empty(), "containment {id} must stay independent");
    }

    // Adapter separation stays byte-visible.
    let action = loaded
        .node_static_fact("ADPT10667")
        .ok_or_else(|| color_eyre::eyre::eyre!("missing adapter ADPT10667"))?;
    let completion = loaded
        .node_static_fact("CADP11184")
        .ok_or_else(|| color_eyre::eyre::eyre!("missing adapter CADP11184"))?;
    assert_eq!(
        (action.wire_kind.as_str(), completion.wire_kind.as_str()),
        ("workspace_edit", "completion_item")
    );

    // The four immediate control-plane children exist beside this slice.
    for id in [
        "GOV11084",
        "GRPH11088",
        "SPECC11091",
        "STATE11094",
        "FRONT11098",
        "PKTS11101",
        "OBSV11105",
        "PRCT11113",
        "DOGF11122",
    ] {
        loaded
            .node_static_fact(id)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing control row {id}"))?;
    }

    // Superseded history keeps canonical successors and never carries edges.
    for id in ["OLD3080", "OLD8297", "OLD8303", "OLD8310", "OLD10743", "OLD10764", "OLD10778"] {
        let fact = loaded
            .node_static_fact(id)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing history row {id}"))?;
        assert_eq!(fact.status, "superseded");
        assert!(fact.dependencies.is_empty());
    }
    Ok(())
}

#[test]
fn import_cleanup_train_manifest_every_active_row_names_discriminators_and_boundaries() -> Result<()>
{
    let path = repo_manifest_path()?;
    let value: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let manifest = parse_strict(&value)?;
    let mut falsifiers: Vec<&str> = Vec::new();
    let mut wrongs: Vec<&str> = Vec::new();
    for node in &manifest.nodes {
        if node.status != "active" || CONTROL_ROLES.contains(&node.role.as_str()) {
            continue;
        }
        assert!(!node.first_falsifier.trim().is_empty(), "{} lacks a falsifier", node.node_id);
        falsifiers.push(node.first_falsifier.trim());
        wrongs.push(node.plausible_wrong_implementation.trim());
        assert!(!node.stop_conditions.is_empty(), "{} lacks stop conditions", node.node_id);
        assert!(
            !node.review_forward_questions.is_empty(),
            "{} lacks review-forward questions",
            node.node_id
        );
    }
    let dedup = |items: &[&str]| items.iter().collect::<BTreeSet<_>>().len();
    assert_eq!(
        dedup(&falsifiers),
        falsifiers.len(),
        "first falsifiers must be unique across nodes"
    );
    assert_eq!(
        dedup(&wrongs),
        wrongs.len(),
        "plausible-wrong implementations must be unique across nodes"
    );
    Ok(())
}

// Review-repair falsifiers (#12825 review round one): each locks a verified
// reviewer finding so the failure mode cannot regress silently.

#[test]
fn import_cleanup_train_manifest_dependency_class_order_is_contract() -> Result<()> {
    let mut value = real_value()?;
    // The digest is order-invariant by design, so this reordering must be
    // caught by structural law instead: swapping hard/evidence changes the
    // published schema's ordered const without moving the pin.
    if let Some(classes) = value.get_mut("dependency_classes").and_then(Value::as_array_mut) {
        classes.swap(0, 1);
    } else {
        bail!("manifest lost dependency_classes");
    }
    assert_rejected(&value, "order-significant")
}

#[test]
fn import_cleanup_train_manifest_external_inside_indirect_closure_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    // Two-hop laundering: the immediate-edge guard passes because CLNT10912
    // reaches ECOM only through PROC10893; the closure walk must not.
    append_dep(&mut value, "PROC10893", "ECOM10786", "hard")?;
    assert_rejected(&value, "native evidence closure")
}

#[test]
fn import_cleanup_train_manifest_unknown_claim_cap_is_rejected() -> Result<()> {
    let mut value = real_value()?;
    let caps = value
        .get_mut("role_claim_caps")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("manifest lost role_claim_caps"))?;
    for cap in caps.iter_mut() {
        if cap.get("role").and_then(Value::as_str) == Some("candidate_discovery") {
            *cap.get_mut("max_claim").ok_or_else(|| color_eyre::eyre::eyre!("no max_claim"))? =
                Value::String("client_verified_bogus".into());
        }
    }
    assert_rejected(&value, "invents max_claim")
}
