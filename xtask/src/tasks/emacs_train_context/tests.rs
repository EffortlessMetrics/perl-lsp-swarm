//! Falsifier suite for the exact-tree context engine (CTXENG `#11756`).
//!
//! Every test builds a synthetic tree (never the real repository) so the
//! laws are exercised against deliberately wrong inputs: each fixture must
//! fail closed with its law diagnostic, and the happy paths must render
//! deterministically. The real-tree denominator check runs in CI via
//! `cargo xtask integration emacs train contexts --check`, not here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use color_eyre::eyre::{Result, bail, eyre};
use serde_json::{Value, json};

use super::digest::title_fingerprint;
use super::model::*;
use super::render::render_json;
use super::resolve::{
    EngineInputs, LEDGER_RELATIVE_PATH, MANIFEST_RELATIVE_PATH, MAPPING_RELATIVE_PATH, Resolution,
    load_inputs, load_inputs_with_git, resolve_node, resolve_spec, validate_manifest,
    validate_mapping,
};

/// Deterministic fixture git identity: synthetic trees are not repositories,
/// so the binding law accepts an explicit identity in tests only.
fn load_fixture(root: &Path) -> Result<EngineInputs> {
    load_inputs_with_git(root, Some(("f".repeat(40), "7".repeat(40))))
}

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn fixture_tree(label: &str) -> Result<PathBuf> {
    let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let root =
        std::env::temp_dir().join(format!("emacs-ctx-{label}-{}-{unique}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(root.join(".spec/11716-emacs-support-architecture"))?;
    std::fs::create_dir_all(root.join(".spec/9001-sub"))?;
    std::fs::write(root.join("AGENTS.md"), "# fixture repository instructions\n")?;
    std::fs::write(
        root.join(".spec/11716-emacs-support-architecture/context.md"),
        "# fixture architecture\n",
    )?;
    std::fs::write(root.join(".spec/9001-sub/acceptance.md"), "# fixture bundle\n")?;
    Ok(root)
}

fn write_json(root: &Path, relative: &str, value: &Value) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_text(root: &Path, relative: &str, text: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

fn read_json(root: &Path, relative: &str) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(root.join(relative))?)?)
}

fn make_node(node_id: &str, issue: u64, lane: &str) -> Value {
    let title = format!("fixture node {node_id}");
    json!({
        "node_id": node_id,
        "issue": issue,
        "title": title,
        "title_fingerprint": title_fingerprint(&title),
        "aliases": [],
        "train_role": "implementation",
        "lane": lane,
        "chain": {"home": "emacs-support", "controller": "CTRL"},
        "one_pr_outcome": "fixture outcome",
        "authority_before": "none",
        "authority_after": "fixture",
        "buildable": true,
        "dependencies": [],
        "claim_ceiling": "fixture ceiling",
        "writer": {
            "conflict_key": format!("fixture.{node_id}"),
            "parallel_group": "A",
            "stack_relation": "none"
        },
        "consumed_authorities": [],
        "allowed_components": [],
        "forbidden_adjacent_owners": [],
        "spec": {
            "disposition": "ISSUE_PLAN_SUFFICIENT",
            "owner": node_id,
            "stale_policy": "E01R classifies movement",
            "spec_authority": "#11717"
        },
        "first_falsifier": "fixture falsifier",
        "controls": {
            "positive": "p", "opposite": "o", "stale": "s",
            "wrong_subject": "w", "fault": "f", "mutation": "m"
        },
        "proof": {"focused": "fixture focus", "routed": "fixture routing"},
        "review_forward": {"questions": [], "lenses": []},
        "obligations": {
            "schema": "none", "generated": "none", "docs": "d",
            "changelog": "none", "receipt": "none"
        },
        "exits": {
            "old_path": "none", "compatibility": "none",
            "supersession": "E01R", "transfer": "E01R"
        },
        "rollback": {
            "rollback": "r", "return_to_issue": "i",
            "not_proven": "np", "stop": "s"
        },
        "successors": [],
        "identity_fields": [],
        "limitations": []
    })
}

fn make_manifest(nodes: &[Value]) -> Value {
    json!({
        "schema": "emacs_train.v1",
        "schema_version": 1,
        "programme": {
            "parent_programme_issue": 7979,
            "controller_issue": 8706,
            "home_programme": "emacs-support",
            "durable_architecture_issue": 11716,
            "durable_architecture_bundle": ".spec/11716-emacs-support-architecture",
            "method_authority": "#3983"
        },
        "authority_planes": [],
        "train_role_vocabulary": [],
        "evidence_semantics": {
            "not_proven_law": "np", "optional_visibility": "ov", "metadata_only_rule": "mo"
        },
        "external_authorities": [],
        "open_decisions_routed_elsewhere": [],
        "existing_candidate_adoption": {
            "node": "FIXT", "candidate_pull": 8026,
            "confirm_with": "#10930", "rule": "fixture"
        },
        "nodes": nodes,
        "supersessions": [],
        "revision_governance": {
            "owner_node": "E01R", "owner_issue": 11770,
            "invalidates": "i", "never": "n", "metadata_only": "m"
        },
        "limitations": []
    })
}

fn make_mapping(node_mappings: &[Value]) -> Value {
    json!({
        "schema": "emacs_train_context_mappings.v1",
        "schema_version": 1,
        "consumed_manifest": {
            "bundle": ".spec/10918-emacs-train-graph", "schema": "emacs_train.v1"
        },
        "consumed_ledger": {
            "bundle": ".spec/11770-emacs-train-revisions", "schema": "emacs_train_revision.v1"
        },
        "population_ownership": {
            "engine": 9000, "substrate_population": 9002, "projection_population": 9003
        },
        "bounds": {
            "max_components_per_node": 4, "max_tests_per_node": 4,
            "max_read_set": 4, "max_write_set": 4, "max_not_authority": 4
        },
        "nodes": node_mappings
    })
}

fn production_component(path: &str, symbol: &str) -> Value {
    json!({
        "component_id": format!("fixture.{symbol}"),
        "role": "production",
        "kind": "rust_source",
        "path": path,
        "symbol": symbol,
        "symbol_kind": "rust_item",
        "client_family": null,
        "notes": null
    })
}

fn mapped_node_entry(node_id: &str, components: &[Value]) -> Value {
    json!({
        "node_id": node_id,
        "status": "mapped",
        "components": components,
        "tests": [],
        "generated": [],
        "read_set": ["AGENTS.md"],
        "write_set": [".spec/9001-sub/acceptance.md"],
        "not_authority": [],
        "specs": [],
        "write_set_note": null,
        "no_production_component_reason": null,
        "blocker": null
    })
}

/// Build the standard fixture tree: SUB (adapter-eglot, mapped) and OTHER
/// (projection, unmapped), one revision entry referencing SUB.
fn load_fixture_inputs(root: &Path) -> Result<EngineInputs> {
    write_json(
        root,
        MANIFEST_RELATIVE_PATH,
        &make_manifest(&[
            make_node("SUB", 9001, "adapter-eglot"),
            make_node("OTHER", 9010, "projection"),
        ]),
    )?;
    write_json(
        root,
        LEDGER_RELATIVE_PATH,
        &json!({"schema": "emacs_train_revision.v1", "revisions": [
            {
                "entry_id": "REV-001", "sequence": 1, "revision_kind": "insert",
                "semantic_class": "node_add",
                "graph_effect": {"added_nodes": ["SUB"], "wiring": []},
                "successors": [],
                "invalidations": [
                    {"surface": "live_packet", "subjects": ["SUB"], "basis": "fixture"}
                ]
            }
        ]}),
    )?;
    write_text(root, "src/thing.rs", "pub fn owned_symbol() {}\n")?;
    write_json(
        root,
        MAPPING_RELATIVE_PATH,
        &make_mapping(&[mapped_node_entry(
            "SUB",
            &[production_component("src/thing.rs", "owned_symbol")],
        )]),
    )?;
    load_fixture(root)
}

fn fixture_node(inputs: &EngineInputs, node_id: &str) -> Result<TrainNode> {
    inputs.manifest.node(node_id).cloned().ok_or_else(|| eyre!("fixture node {node_id} missing"))
}

/// Assert that loading or resolving the fixture fails with the named law.
fn expect_law_failure(root: &Path, law: &str) -> Result<()> {
    let failure = match load_fixture(root) {
        Err(failure) => failure,
        Ok(inputs) => {
            let sub = fixture_node(&inputs, "SUB")?;
            match resolve_node(root, &inputs, &sub) {
                Ok(_) => bail!("expected law failure '{law}' but resolution succeeded"),
                Err(failure) => failure,
            }
        }
    };
    let rendered = format!("{failure:#}");
    assert!(rendered.contains(law), "expected failure containing '{law}', got: {rendered}");
    Ok(())
}

fn mutate_mapping(root: &Path, mutate: impl Fn(&mut Value)) -> Result<()> {
    let mut mapping = read_json(root, MAPPING_RELATIVE_PATH)?;
    mutate(&mut mapping);
    write_json(root, MAPPING_RELATIVE_PATH, &mapping)
}

fn mutate_manifest(root: &Path, mutate: impl Fn(&mut Value)) -> Result<()> {
    let mut manifest = read_json(root, MANIFEST_RELATIVE_PATH)?;
    mutate(&mut manifest);
    write_json(root, MANIFEST_RELATIVE_PATH, &manifest)
}

#[test]
fn happy_path_resolves_and_binds_digests() -> Result<()> {
    let root = fixture_tree("happy")?;
    let inputs = load_fixture_inputs(&root)?;
    let sub = fixture_node(&inputs, "SUB")?;
    let packet = resolve_node(&root, &inputs, &sub)?.packet().clone();
    assert_eq!(packet.status, "ok");
    assert_eq!(packet.node.node_id, "SUB");
    assert_eq!(packet.components.len(), 1);
    assert_eq!(packet.components[0].symbol.as_deref(), Some("owned_symbol"));
    assert_eq!(packet.revision_currency.latest_entry_id.as_deref(), Some("REV-001"));
    assert_eq!(packet.instructions.len(), 1);
    assert_eq!(packet.instructions[0].path, "AGENTS.md");
    assert_eq!(packet.checked_specs.len(), 1);
    assert_eq!(packet.checked_specs[0].bundle, "9001-sub");
    assert_eq!(packet.binding.git_commit.len(), 40);
    assert_eq!(packet.binding.input_digest.len(), 64);
    assert!(packet.write_set.iter().all(|path| path.contains('/')));
    Ok(())
}

#[test]
fn determinism_two_renders_are_identical() -> Result<()> {
    let root = fixture_tree("determinism")?;
    let inputs = load_fixture_inputs(&root)?;
    let sub = fixture_node(&inputs, "SUB")?;
    let first = render_json(resolve_node(&root, &inputs, &sub)?.packet())?;
    let second = render_json(resolve_node(&root, &inputs, &sub)?.packet())?;
    assert_eq!(first, second);
    let round_tripped = super::render::parse_json(&first)?;
    assert_eq!(round_tripped.status, "ok");
    assert_eq!(round_tripped.node.issue, 9001);
    Ok(())
}

#[test]
fn resolve_accepts_issue_number() -> Result<()> {
    let root = fixture_tree("issue-lookup")?;
    let inputs = load_fixture_inputs(&root)?;
    let by_issue = resolve_spec(&root, &inputs, "#9001")?;
    assert_eq!(by_issue.packet().node.node_id, "SUB");
    assert!(resolve_spec(&root, &inputs, "NOPE").is_err());
    Ok(())
}

#[test]
fn unmapped_node_yields_precise_blocker() -> Result<()> {
    let root = fixture_tree("gap")?;
    let inputs = load_fixture_inputs(&root)?;
    let other = fixture_node(&inputs, "OTHER")?;
    let resolution = resolve_node(&root, &inputs, &other)?;
    assert!(resolution.is_gap());
    let packet = resolution.packet();
    assert_eq!(packet.status, "mapping_gap");
    let gap = packet.gaps.first().ok_or_else(|| eyre!("gap packet must carry its gap entry"))?;
    assert_eq!(gap.action, "return_to_issue");
    assert_eq!(gap.owner_issue, 9003, "projection lane routes to the projection population leaf");
    assert_eq!(packet.components.len(), 0);
    Ok(())
}

#[test]
fn unmapped_without_blocker_fails_closed() -> Result<()> {
    let root = fixture_tree("l08")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        if let Some(nodes) = mapping["nodes"].as_array_mut() {
            nodes.push(json!({
                "node_id": "OTHER", "status": "unmapped", "components": [], "tests": [],
                "generated": [], "read_set": [], "write_set": [], "not_authority": [],
                "specs": []
            }));
        }
    })?;
    expect_law_failure(&root, "L08")
}

#[test]
fn wrong_manifest_schema_fails_closed() -> Result<()> {
    let root = fixture_tree("l01")?;
    load_fixture_inputs(&root)?;
    mutate_manifest(&root, |manifest| {
        manifest["schema"] = json!("some_other_train.v1");
    })?;
    expect_law_failure(&root, "L01")
}

#[test]
fn tampered_title_fingerprint_fails_closed() -> Result<()> {
    let root = fixture_tree("l03")?;
    load_fixture_inputs(&root)?;
    mutate_manifest(&root, |manifest| {
        manifest["nodes"][0]["title"] = json!("silently edited title");
    })?;
    expect_law_failure(&root, "L03")
}

#[test]
fn asymmetric_edge_fails_closed() -> Result<()> {
    let root = fixture_tree("l04")?;
    load_fixture_inputs(&root)?;
    mutate_manifest(&root, |manifest| {
        // SUB gains a hard dependency on OTHER, but OTHER does not list SUB
        // as a successor: a silently rewritten edge must fail, not normalize.
        manifest["nodes"][0]["dependencies"] =
            json!([{"target": "OTHER", "class": "hard", "provenance": "fixture"}]);
    })?;
    expect_law_failure(&root, "L04")
}

#[test]
fn helper_kind_as_production_fails_closed() -> Result<()> {
    let root = fixture_tree("l18")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["components"][0]["kind"] = json!("rust_test");
    })?;
    expect_law_failure(&root, "L18")
}

#[test]
fn stale_symbol_anchor_fails_closed() -> Result<()> {
    let root = fixture_tree("l16-symbol")?;
    load_fixture_inputs(&root)?;
    // The symbol is removed from the exact file: a same-named symbol in any
    // other file must not satisfy the mapping.
    write_text(&root, "src/thing.rs", "pub fn something_else() {}\n")?;
    expect_law_failure(&root, "L16")
}

#[test]
fn missing_component_file_fails_closed() -> Result<()> {
    let root = fixture_tree("l16-file")?;
    load_fixture_inputs(&root)?;
    std::fs::remove_file(root.join("src/thing.rs"))?;
    expect_law_failure(&root, "L16")
}

#[test]
fn cross_node_symbol_collision_fails_closed() -> Result<()> {
    let root = fixture_tree("l12")?;
    load_fixture_inputs(&root)?;
    write_text(&root, "src/other.rs", "pub fn owned_symbol() {}\n")?;
    mutate_mapping(&root, |mapping| {
        if let Some(nodes) = mapping["nodes"].as_array_mut() {
            nodes.push(mapped_node_entry(
                "OTHER",
                &[production_component("src/other.rs", "owned_symbol")],
            ));
        }
    })?;
    expect_law_failure(&root, "L12")
}

#[test]
fn cross_client_family_fails_closed() -> Result<()> {
    let root = fixture_tree("l15")?;
    load_fixture_inputs(&root)?;
    // SUB's lane is adapter-eglot; an lsp-mode family claim inside it is the
    // Eglot/lsp cross-satisfaction falsifier.
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["components"][0]["client_family"] = json!("lsp");
    })?;
    expect_law_failure(&root, "L15")
}

#[test]
fn broad_write_set_fails_closed() -> Result<()> {
    let root = fixture_tree("l14")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["write_set"] = json!(["xtask/"]);
    })?;
    expect_law_failure(&root, "L14")
}

#[test]
fn missing_root_agents_fails_closed() -> Result<()> {
    let root = fixture_tree("l19")?;
    load_fixture_inputs(&root)?;
    // The read set is cleared first so the instruction-chain law is the law
    // under test, not read-set existence.
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["read_set"] = json!(["src/thing.rs"]);
    })?;
    std::fs::remove_file(root.join("AGENTS.md"))?;
    expect_law_failure(&root, "L19")
}

#[test]
fn mapped_without_production_or_reason_fails_closed() -> Result<()> {
    let root = fixture_tree("l13")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        // Replace the production component with a doc component and drop the
        // explanation: a packet hiding its missing production seam is invalid.
        mapping["nodes"][0]["components"] = json!([{
            "component_id": "fixture.doc", "role": "doc", "kind": "doc",
            "path": ".spec/9001-sub/acceptance.md"
        }]);
    })?;
    expect_law_failure(&root, "L13")
}

#[test]
fn bounds_violation_fails_closed() -> Result<()> {
    let root = fixture_tree("l07-bounds")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        // Five read-set entries against the fixture maximum of four; bounds
        // are checked during mapping validation, before tree resolution.
        mapping["nodes"][0]["read_set"] = json!(["AGENTS.md", "a.rs", "b.rs", "c.rs", "d.rs"]);
    })?;
    expect_law_failure(&root, "L07")
}

#[test]
fn unknown_role_fails_closed() -> Result<()> {
    let root = fixture_tree("l07-role")?;
    load_fixture_inputs(&root)?;
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["components"][0]["role"] = json!("meta_production");
    })?;
    expect_law_failure(&root, "L07")
}

#[test]
fn nested_agents_chain_is_discovered_and_digest_bound() -> Result<()> {
    let root = fixture_tree("instructions")?;
    load_fixture_inputs(&root)?;
    write_text(&root, "src/AGENTS.md", "# fixture package instructions\n")?;
    let inputs = load_fixture(&root)?;
    let sub = fixture_node(&inputs, "SUB")?;
    let packet = resolve_node(&root, &inputs, &sub)?.packet().clone();
    let paths: Vec<&str> = packet.instructions.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, vec!["AGENTS.md", "src/AGENTS.md"]);
    assert_eq!(packet.instructions[1].sha256.len(), 64);
    Ok(())
}

#[test]
fn dangling_successor_fails_structural_validation() -> Result<()> {
    let mut single = make_manifest(&[make_node("SOLO", 9001, "foundation")]);
    single["nodes"][0]["successors"] = json!(["GHOST"]);
    let typed: TrainManifest = serde_json::from_value(single)?;
    let failure =
        validate_manifest(&typed).err().ok_or_else(|| eyre!("dangling successor must fail"))?;
    assert!(format!("{failure:#}").contains("L04"));
    Ok(())
}

#[test]
fn mapping_validation_rejects_unknown_node() -> Result<()> {
    let manifest: TrainManifest =
        serde_json::from_value(make_manifest(&[make_node("SUB", 9001, "foundation")]))?;
    let mapping: MappingDocument = serde_json::from_value(make_mapping(&[mapped_node_entry(
        "GHOST",
        &[production_component("src/thing.rs", "x")],
    )]))?;
    let failure = validate_mapping(&manifest, &mapping)
        .err()
        .ok_or_else(|| eyre!("unknown node must fail"))?;
    assert!(format!("{failure:#}").contains("L06"));
    Ok(())
}

#[test]
fn revision_currency_binds_ledger_digest() -> Result<()> {
    let root = fixture_tree("revision")?;
    let inputs = load_fixture_inputs(&root)?;
    let sub = fixture_node(&inputs, "SUB")?;
    let packet = resolve_node(&root, &inputs, &sub)?.packet().clone();
    assert_eq!(packet.revision_currency.ledger_schema, "emacs_train_revision.v1");
    assert_eq!(packet.revision_currency.ledger_digest, inputs.ledger_digest);
    assert_eq!(packet.revision_currency.latest_semantic_class.as_deref(), Some("node_add"));
    let other = fixture_node(&inputs, "OTHER")?;
    let other_packet = resolve_node(&root, &inputs, &other)?.packet().clone();
    assert!(other_packet.revision_currency.latest_entry_id.is_none());
    Ok(())
}

#[test]
fn resolution_type_distinguishes_packet_and_gap() -> Result<()> {
    let root = fixture_tree("types")?;
    let inputs = load_fixture_inputs(&root)?;
    let sub = fixture_node(&inputs, "SUB")?;
    assert!(matches!(resolve_node(&root, &inputs, &sub)?, Resolution::Packet(_)));
    let other = fixture_node(&inputs, "OTHER")?;
    assert!(matches!(resolve_node(&root, &inputs, &other)?, Resolution::Gap(_)));
    Ok(())
}

#[test]
fn missing_git_identity_fails_closed() -> Result<()> {
    // A synthetic tree loaded through the production path is not a
    // repository: the engine must refuse to emit an unbound packet (L10).
    let root = fixture_tree("l10")?;
    load_fixture_inputs(&root)?;
    let failure =
        load_inputs(&root).err().ok_or_else(|| eyre!("a non-repository tree must fail closed"))?;
    assert!(format!("{failure:#}").contains("L10"));
    Ok(())
}

#[test]
fn oversized_path_bytes_fail_closed() -> Result<()> {
    // Bounded means bytes too, not just counts: a mapping entry with an
    // oversized path is a defect, never a larger valid packet (L11).
    let root = fixture_tree("l11-bytes")?;
    load_fixture_inputs(&root)?;
    let oversized = format!("src/{}.rs", "x".repeat(600));
    mutate_mapping(&root, |mapping| {
        mapping["nodes"][0]["read_set"] = json!([oversized]);
    })?;
    expect_law_failure(&root, "L11")
}
