//! Falsifier suite for the E06 actor-packet adapter (#11719).
//!
//! Every test builds a synthetic tree (never the real repository) so the
//! laws are exercised against deliberately wrong inputs: a missing required
//! input must refuse with the exact typed reason instead of rendering
//! plausible prose, and every happy path must render deterministically
//! through the shared #10872/#10881 contracts unchanged. The real-tree
//! denominator check runs in CI via
//! `cargo xtask integration emacs train packets --check`, not here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use color_eyre::eyre::{Context, Result};
use serde_json::{Value, json};

use super::test_support::{disposition_record, specs_ledger, write_json, write_text};
use super::*;
use crate::tasks::agent_implementation_packet::PacketProjection;
use crate::tasks::emacs_train_context::digest::title_fingerprint;
use crate::tasks::emacs_train_context::resolve::{
    LEDGER_RELATIVE_PATH, MANIFEST_RELATIVE_PATH, MAPPING_RELATIVE_PATH, load_inputs_with_git,
};
use crate::tasks::emacs_train_specs::DEFAULT_LEDGER_PATH as SPECS_LEDGER_RELATIVE_PATH;

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn fixture_tree(label: &str) -> Result<PathBuf> {
    let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let root =
        std::env::temp_dir().join(format!("emacs-pkt-{label}-{}-{unique}", std::process::id()));
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
    Ok(root)
}

/// Deterministic fixture git identity: synthetic trees are not repositories.
fn fixture_git() -> (String, String) {
    ("a".repeat(40), "5".repeat(40))
}

fn make_node(node_id: &str, issue: u64, role: &str, disposition: &str) -> Value {
    json!({
        "node_id": node_id,
        "issue": issue,
        "title": format!("fixture node {node_id}"),
        "title_fingerprint": title_fingerprint(&format!("fixture node {node_id}")),
        "aliases": [],
        "train_role": role,
        "lane": "adapter-eglot",
        "chain": {"home": "emacs-support", "controller": "CTRL"},
        "one_pr_outcome": "fixture bounded outcome for the packet adapter",
        "authority_before": "none",
        "authority_after": "fixture authority after",
        "buildable": true,
        "dependencies": [],
        "claim_ceiling": "fixture ceiling; no Emacs packet schema, no model invocation",
        "writer": {
            "conflict_key": format!("emacs.fixture.{node_id}"),
            "parallel_group": "A",
            "stack_relation": "none"
        },
        "consumed_authorities": ["#10872", "#10881"],
        "allowed_components": [],
        "forbidden_adjacent_owners": ["leaf execution (covered trains)"],
        "spec": {
            "disposition": disposition,
            "owner": node_id,
            "stale_policy": "E01R classifies movement",
            "spec_authority": "#11717"
        },
        "first_falsifier": "An Emacs-local packet schema duplicates the shared contracts.",
        "controls": {
            "positive": "packets satisfy the shared schemas unchanged",
            "opposite": "an Emacs-local packet schema",
            "stale": "packets stale after contract revision",
            "wrong_subject": "a packet embedding live state",
            "fault": "rendering rendered as pass on fault",
            "mutation": "a packet field fabricated"
        },
        "proof": {"focused": "focused adapter contract test", "routed": "routed proof per POLICY"},
        "review_forward": {"questions": [], "lenses": []},
        "obligations": {
            "schema": "adapter contract over #10872/#10881 payloads",
            "generated": "none",
            "docs": "contract notes",
            "changelog": "none (internal contract)",
            "receipt": "none"
        },
        "exits": {
            "old_path": "none (new surface)",
            "compatibility": "none",
            "supersession": "supersession via E01R",
            "transfer": "transfer with manifest revision"
        },
        "rollback": {
            "rollback": "revert adapter; actors re-derive packets",
            "return_to_issue": "return here when shared contracts move",
            "not_proven": "missing or partial evidence stays not_proven, never pass",
            "stop": "stop before invocation, mutation or scheduling"
        },
        "successors": [],
        "identity_fields": [],
        "limitations": ["fixture limitation"]
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

fn mapped_node_entry(node_id: &str, with_test: bool) -> Value {
    json!({
        "node_id": node_id,
        "status": "mapped",
        "components": [{
            "component_id": format!("fixture.adapter.{node_id}"),
            "role": "production",
            "kind": "rust_source",
            "path": format!("src/{}.rs", node_id.to_lowercase()),
            "symbol": format!("compose_fixture_{node_id}"),
            "symbol_kind": "rust_item",
            "client_family": null,
            "notes": null
        }],
        "tests": if with_test {
            json!([{"path": "tests/adapter_contract.rs", "selector": "fixture_packet_law", "kind": "falsifier"}])
        } else {
            json!([])
        },
        "generated": [],
        "read_set": ["AGENTS.md"],
        "write_set": [format!("src/{}.rs", node_id.to_lowercase())],
        "not_authority": [],
        "specs": [],
        "write_set_note": null,
        "no_production_component_reason": null,
        "blocker": null
    })
}

fn unmapped_node_entry(node_id: &str) -> Value {
    json!({
        "node_id": node_id,
        "status": "unmapped",
        "components": [],
        "tests": [],
        "generated": [],
        "read_set": [],
        "write_set": [],
        "not_authority": [],
        "specs": [],
        "write_set_note": null,
        "no_production_component_reason": null,
        "blocker": {
            "reason": "fixture mapping gap owned elsewhere",
            "owner_issue": 9001,
            "action": "return_to_issue"
        }
    })
}

/// Build the standard fixture tree: SUB (implementation leaf, mapped with a
/// test surface) and OTHER (unmapped). Writes the E02 ledger with records
/// for the given nodes.
fn load_fixture_inputs(
    root: &Path,
    manifest_nodes: &[Value],
    mapping_nodes: &[Value],
    ledger_records: &[Value],
) -> Result<AdapterInputs> {
    write_json(root, MANIFEST_RELATIVE_PATH, &make_manifest(manifest_nodes))?;
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
    for node_id in
        manifest_nodes.iter().filter_map(|node| node.get("node_id")).filter_map(Value::as_str)
    {
        let lower = node_id.to_lowercase();
        write_text(
            root,
            &format!("src/{lower}.rs"),
            &format!("pub fn compose_fixture_{node_id}() {{}}\n"),
        )?;
    }
    write_text(root, "tests/adapter_contract.rs", "#[test] fn fixture_packet_law() {}\n")?;
    write_json(root, MAPPING_RELATIVE_PATH, &make_mapping(mapping_nodes))?;
    write_json(root, SPECS_LEDGER_RELATIVE_PATH, &specs_ledger(ledger_records))?;
    let engine = load_inputs_with_git(root, Some(fixture_git()))
        .with_context(|| "loading fixture engine inputs")?;
    complete_adapter_inputs(root, engine)
}

fn default_fixture(root: &Path) -> Result<AdapterInputs> {
    load_fixture_inputs(
        root,
        &[
            make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT"),
            make_node("OTHER", 9010, "implementation", "ISSUE_PLAN_SUFFICIENT"),
        ],
        &[mapped_node_entry("SUB", true), unmapped_node_entry("OTHER")],
        &[
            disposition_record("SUB", 9001, "ISSUE_PLAN_SUFFICIENT"),
            disposition_record("OTHER", 9010, "ISSUE_PLAN_SUFFICIENT"),
        ],
    )
}

/// An explicit complete observation that no candidate owns this claim.
///
/// #11719 admits a coding packet only against a current observation. Vacancy
/// must be *observed and supplied*, never inferred from a missing flag, so the
/// happy-path tests state it here instead of leaving `live` as `None`.
///
/// The shared #10872 vocabulary is `not_observed | observed`, and `observed`
/// requires a non-empty `candidate_identity` -- so an observed vacancy is
/// recorded as the caller's exact statement of what the sweep found, not as an
/// absent field.
fn observed_vacant() -> LiveObservation {
    LiveObservation {
        candidate_state: "observed".to_string(),
        digest: "sha256:0000000000000000".to_string(),
        candidate_identity: Some(
            "no candidate: no open PR or dirty checkout owns this claim".to_string(),
        ),
        collision_state: None,
    }
}

// ---------------------------------------------------------------------------
// #11719: "No live observation means no coding packet assuming vacancy."
// A coding packet admits a repository writer, so it must not be composable
// against an unobserved claim -- and `not_observed` records that nobody
// looked, which is not the same as looking and finding nothing.
// ---------------------------------------------------------------------------

#[test]
fn coding_packet_without_live_observation_refuses_fail_closed() -> Result<()> {
    let root = fixture_tree("no-live")?;
    let inputs = default_fixture(&root)?;
    for profile in ["coding_agent_bounded", "coding_agent_strong"] {
        let refusal = compose_builder_packet(&root, &inputs, "SUB", profile, None)
            .err()
            .unwrap_or_else(|| panic!("{profile} must not assume the claim is vacant"));
        assert_eq!(refusal.code, "NO_LIVE_OBSERVATION", "{}", refusal.line());
    }
    Ok(())
}

#[test]
fn not_observed_live_state_is_not_evidence_of_vacancy() -> Result<()> {
    let root = fixture_tree("not-observed")?;
    let inputs = default_fixture(&root)?;
    let live = LiveObservation {
        candidate_state: "not_observed".to_string(),
        digest: "sha256:0000000000000000".to_string(),
        candidate_identity: None,
        collision_state: None,
    };
    let refusal =
        compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", Some(&live))
            .err()
            .expect("not_observed must never admit a coding packet");
    assert_eq!(refusal.code, "NO_LIVE_OBSERVATION", "{}", refusal.line());
    assert!(refusal.detail.contains("absence of knowledge is never vacancy"), "{}", refusal.detail);
    Ok(())
}

#[test]
fn review_and_reconcile_routes_are_not_blocked_by_the_live_gate() -> Result<()> {
    // Neither route emits a coding packet or a repository write boundary, so
    // the live gate must not refuse a read-only review for want of an
    // observation it never acts on.
    let root = fixture_tree("anchored")?;
    let inputs = default_fixture(&root)?;
    let candidates = json!([
        {"identity": "PR #8800", "state": "open_stale_base", "facts": "dirty unique work"}
    ]);
    let doc = compose_reconcile_packet(&root, &inputs, "SUB", Some(&candidates))
        .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(doc["frontier"]["decision"], "blocked");
    Ok(())
}

// ---------------------------------------------------------------------------
// The reconciliation adjudication is over the candidates' facts, so the facts
// must reach the packet and its frontier identity. Two sets differing only in
// facts must never render the same bytes.
// ---------------------------------------------------------------------------

#[test]
fn reconcile_binds_the_supplied_candidate_facts_into_the_packet_and_digest() -> Result<()> {
    let root = fixture_tree("reconcile-facts")?;
    let inputs = default_fixture(&root)?;

    let render = |facts: &str| -> Result<(String, String, String)> {
        let candidates = json!([
            {"identity": "PR #8800 (tooling/sub-claim)", "state": "open_stale_base", "facts": facts}
        ]);
        let doc = compose_reconcile_packet(&root, &inputs, "SUB", Some(&candidates))
            .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
        Ok((
            render_builder_packet(&doc, PacketProjection::Machine)?,
            doc["frontier"]["digest"].as_str().unwrap_or_default().to_string(),
            doc["frontier"]["blocking_edges"][0]["reason"].as_str().unwrap_or_default().to_string(),
        ))
    };

    let (bytes_a, digest_a, reason_a) = render("dirty unique work, unpushed")?;
    let (bytes_b, digest_b, _) = render("clean, fully pushed, superseded")?;

    assert!(reason_a.contains("dirty unique work, unpushed"), "{reason_a}");
    assert_ne!(bytes_a, bytes_b, "candidate facts must change the rendered packet");
    assert_ne!(digest_a, digest_b, "candidate facts must change the frontier identity");
    Ok(())
}

#[test]
fn reconcile_refuses_incomplete_or_duplicate_candidate_facts() -> Result<()> {
    let root = fixture_tree("reconcile-malformed")?;
    let inputs = default_fixture(&root)?;
    let cases = [
        json!([]),
        json!([{"identity": "PR #8800", "state": "open"}]),
        json!([
            {"identity": "PR #8800", "state": "open", "facts": "a"},
            {"identity": "PR #8800", "state": "open", "facts": "b"}
        ]),
    ];
    for candidates in cases {
        let refusal = compose_reconcile_packet(&root, &inputs, "SUB", Some(&candidates))
            .err()
            .unwrap_or_else(|| panic!("incomplete candidate facts must refuse: {candidates}"));
        assert_eq!(refusal.code, "MALFORMED_CANDIDATE_FACTS", "{}", refusal.line());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 1 + 3: a complete-input node renders a shared-contract packet
// (zero drift) and identical inputs produce identical canonical bytes.
// ---------------------------------------------------------------------------

#[test]
fn complete_node_renders_shared_contract_packet_deterministically() -> Result<()> {
    let root = fixture_tree("happy")?;
    let inputs = default_fixture(&root)?;
    let live = observed_vacant();
    let doc = compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", Some(&live))
        .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(doc["schema"], BUILDER_CONTRACT, "the payload must be the shared #10872 contract");
    let first = render_builder_packet(&doc, PacketProjection::Machine)?;
    let second = render_builder_packet(&doc, PacketProjection::Machine)?;
    assert_eq!(first, second, "identical inputs must produce byte-identical packets");
    // Re-composition from the same inputs is byte-stable too.
    let doc_again =
        compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", Some(&live))
            .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(first, render_builder_packet(&doc_again, PacketProjection::Machine)?);
    // Emacs supplies fields only: no Emacs-local schema identity anywhere.
    assert!(first.find("emacs_packet").is_none());
    assert!(first.find("emacs-local").is_none());
    // Honest offline observation: the packet carries exactly the observation
    // the caller supplied and never invents one of its own.
    assert_eq!(doc["live_observation"]["candidate_state"], "observed");
    assert_eq!(
        doc["live_observation"]["candidate_identity"],
        live.candidate_identity.clone().unwrap_or_default()
    );
    assert_eq!(doc["work"]["profile_decision"]["selected_value"], "ISSUE_PLAN_SUFFICIENT");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 2: every missing-input class refuses with the exact reason.
// ---------------------------------------------------------------------------

#[test]
fn missing_spec_disposition_fails_the_load_with_the_exact_node() -> Result<()> {
    let root = fixture_tree("no-spec")?;
    let failure = match load_fixture_inputs(
        &root,
        &[make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT")],
        &[mapped_node_entry("SUB", true)],
        &[], // no E02 record for SUB
    ) {
        Err(failure) => failure,
        Ok(inputs) => {
            match compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", None) {
                // Defense-in-depth: a hand-constructed ledger hole still refuses
                // with the typed per-node reason instead of prose.
                Err(refusal) => panic!("unexpected typed refusal path: {}", refusal.line()),
                Ok(_) => panic!("a missing checked disposition must never compose a packet"),
            }
        }
    };
    let rendered = format!("{failure:#}");
    assert!(rendered.contains(SPECS_LEDGER_RELATIVE_PATH), "{rendered}");
    assert!(rendered.contains("does not cover manifest node SUB (#9001)"), "{rendered}");
    Ok(())
}

#[test]
fn controller_node_never_receives_a_coding_packet() -> Result<()> {
    let root = fixture_tree("controller")?;
    let inputs = load_fixture_inputs(
        &root,
        &[make_node("CTRL", 9005, "controller", "CONTROLLER_NO_CODING_SPEC")],
        &[mapped_node_entry("CTRL", true)],
        &[disposition_record("CTRL", 9005, "CONTROLLER_NO_CODING_SPEC")],
    )?;
    for profile in ["coding_agent_bounded", "coding_agent_strong"] {
        let refusal = compose_builder_packet(&root, &inputs, "CTRL", profile, None)
            .expect_err("a controller/fan-in node must never receive a coding packet");
        assert_eq!(refusal.code, "PROFILE_NOT_PERMITTED", "{profile}");
        assert!(refusal.detail.contains("controller"));
    }
    Ok(())
}

#[test]
fn mapping_gap_context_refuses_with_typed_blocker() -> Result<()> {
    let root = fixture_tree("gap")?;
    let inputs = default_fixture(&root)?;
    let refusal = compose_builder_packet(&root, &inputs, "OTHER", "coding_agent_bounded", None)
        .expect_err("an unmapped exact-tree context must refuse");
    assert_eq!(refusal.code, "CONTEXT_MAPPING_GAP");
    assert!(refusal.detail.contains("fixture mapping gap owned elsewhere"));
    assert!(refusal.detail.contains("#9001"));
    Ok(())
}

#[test]
fn external_hard_dependency_refuses_rather_than_assuming_currency() -> Result<()> {
    let root = fixture_tree("ext-dep")?;
    let mut node = make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT");
    node["dependencies"] = json!([
        {"target": "#9999", "class": "hard", "provenance": "fixture"}
    ]);
    let inputs = load_fixture_inputs(
        &root,
        &[node],
        &[mapped_node_entry("SUB", true)],
        &[disposition_record("SUB", 9001, "ISSUE_PLAN_SUFFICIENT")],
    )?;
    let refusal = compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", None)
        .expect_err("an unverifiable external hard dependency must refuse");
    assert_eq!(refusal.code, "HARD_DEPENDENCY_NOT_CURRENT");
    assert!(refusal.detail.contains("#9999"));
    assert!(refusal.detail.contains("#10923/#10930"));
    Ok(())
}

#[test]
fn blocking_disposition_refuses_coding() -> Result<()> {
    let root = fixture_tree("not-proven")?;
    let inputs = load_fixture_inputs(
        &root,
        &[make_node("SUB", 9001, "implementation", "NOT_PROVEN")],
        &[mapped_node_entry("SUB", true)],
        &[disposition_record("SUB", 9001, "NOT_PROVEN")],
    )?;
    let refusal = compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", None)
        .expect_err("a NOT_PROVEN disposition must refuse a coding packet");
    assert_eq!(refusal.code, "SPEC_DISPOSITION_NOT_BUILDER");
    assert!(refusal.detail.contains("NOT_PROVEN"));
    Ok(())
}

#[test]
fn maintainer_profile_is_not_permitted_for_ordinary_coding_nodes() -> Result<()> {
    let root = fixture_tree("maintainer")?;
    let inputs = default_fixture(&root)?;
    let refusal = compose_builder_packet(&root, &inputs, "SUB", "maintainer_external_action", None)
        .expect_err("a coding leaf with no declared external action must refuse the profile");
    assert_eq!(refusal.code, "PROFILE_NOT_PERMITTED");
    Ok(())
}

// ---------------------------------------------------------------------------
// Review packet: independent challenge surface, supplied facts only.
// ---------------------------------------------------------------------------

fn fixture_head() -> String {
    fixture_git().0
}

fn complete_controls() -> BTreeMap<String, BTreeMap<String, Value>> {
    let criteria = [
        "exists",
        "red_before_or_mutation_evidence",
        "passes_only_intended_implementation",
        "correct_subject_and_generation",
        "independent_expectation_source",
        "alternate_subject_exclusion",
    ];
    let mut controls = BTreeMap::new();
    for falsifier in ["F_first", "F_opposite", "F_stale", "F_wrong_subject", "F_fault"] {
        let mut map = BTreeMap::new();
        for criterion in criteria {
            map.insert(
                criterion.to_string(),
                json!({
                    "status": "established",
                    "evidence": format!("fixture evidence for {falsifier}/{criterion}")
                }),
            );
        }
        controls.insert(falsifier.to_string(), map);
    }
    controls
}

#[test]
fn review_packet_requires_supplied_candidate_identity_and_controls() -> Result<()> {
    let root = fixture_tree("review-missing")?;
    let inputs = default_fixture(&root)?;
    let empty = ReviewFacts {
        base: String::new(),
        head: String::new(),
        diff: String::new(),
        controls: BTreeMap::new(),
    };
    let refusal = compose_review_packet(&root, &inputs, "SUB", &empty)
        .expect_err("offline review must refuse without facts");
    assert_eq!(refusal.code, "MISSING_CANDIDATE_IDENTITY");

    let partial = ReviewFacts {
        base: "base0".into(),
        head: fixture_head(),
        diff: "sha256:dd".into(),
        controls: BTreeMap::new(),
    };
    let refusal = compose_review_packet(&root, &inputs, "SUB", &partial)
        .expect_err("missing controls must refuse");
    assert_eq!(refusal.code, "MISSING_NEGATIVE_CONTROL_EVIDENCE");

    let uncovered = ReviewFacts {
        base: "base0".into(),
        head: fixture_head(),
        diff: "sha256:dd".into(),
        controls: {
            let mut controls = complete_controls();
            controls.remove("F_stale");
            controls
        },
    };
    let refusal = compose_review_packet(&root, &inputs, "SUB", &uncovered)
        .expect_err("an uncovered falsifier must refuse");
    assert_eq!(refusal.code, "CONTROL_FALSIFIER_UNCOVERED");
    assert!(refusal.detail.contains("F_stale"));
    Ok(())
}

#[test]
fn review_packet_refuses_unestablished_control_evidence() -> Result<()> {
    let root = fixture_tree("review-not-established")?;
    let inputs = default_fixture(&root)?;
    let mut controls = complete_controls();
    controls
        .get_mut("F_first")
        .expect("F_first row")
        .insert("exists".to_string(), json!({"status": "not_established", "evidence": "gap"}));
    let facts = ReviewFacts {
        base: "base0".into(),
        head: fixture_head(),
        diff: "sha256:dd".into(),
        controls,
    };
    let refusal = compose_review_packet(&root, &inputs, "SUB", &facts)
        .expect_err("unestablished evidence is a finding, never a pass");
    assert_eq!(refusal.code, "CONTROL_NOT_ESTABLISHED");
    assert!(refusal.detail.contains("exists"));
    Ok(())
}

#[test]
fn review_packet_renders_shared_review_contract_deterministically() -> Result<()> {
    let root = fixture_tree("review-happy")?;
    let inputs = default_fixture(&root)?;
    let facts = ReviewFacts {
        base: "base0".into(),
        head: fixture_head(),
        diff: "sha256:dd".into(),
        controls: complete_controls(),
    };
    let doc = compose_review_packet(&root, &inputs, "SUB", &facts)
        .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(doc["schema"], REVIEW_CONTRACT);
    let first =
        render_review_packet(&doc, crate::tasks::agent_review_packet::ReviewProjection::Machine)?;
    let second =
        render_review_packet(&doc, crate::tasks::agent_review_packet::ReviewProjection::Machine)?;
    assert_eq!(first, second);
    // The reviewer surface challenges; it does not mirror the builder.
    assert!(
        doc["challenge"]["primary_proposition"]
            .as_str()
            .expect("proposition")
            .contains("authority")
    );
    assert!(first.contains("Q_one_authority"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Reconciliation packet: no live observation means a typed refusal.
// ---------------------------------------------------------------------------

#[test]
fn reconcile_refuses_without_supplied_candidates_and_blocks_with_them() -> Result<()> {
    let root = fixture_tree("reconcile")?;
    let inputs = default_fixture(&root)?;
    let refusal = compose_reconcile_packet(&root, &inputs, "SUB", None)
        .expect_err("no live observation means no reconcile packet assuming vacancy");
    assert_eq!(refusal.code, "NO_LIVE_OBSERVATION");

    let candidates = json!([
        {"identity": "PR #8800 (tooling/sub-claim)", "state": "open_stale_base", "facts": "dirty unique work"}
    ]);
    let doc = compose_reconcile_packet(&root, &inputs, "SUB", Some(&candidates))
        .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(doc["schema"], BUILDER_CONTRACT);
    assert_eq!(
        doc["actor"]["write_boundary"], "none",
        "reconciliation carries no coding authority"
    );
    assert_eq!(doc["frontier"]["decision"], "blocked");
    assert_eq!(doc["frontier"]["blocking_edges"][0]["edge"], "PR #8800 (tooling/sub-claim)");
    render_builder_packet(&doc, PacketProjection::Machine)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Review-finding regressions.
// ---------------------------------------------------------------------------

#[test]
fn spec_only_hard_dependency_does_not_count_as_landed() -> Result<()> {
    let root = fixture_tree("spec-dep")?;
    // DEP holds only ISSUE_PLAN_SUFFICIENT (specability, not landing) and
    // has a full exact-tree context; SUB hard-depends on it.
    let mut dep = make_node("DEP", 9101, "implementation", "ISSUE_PLAN_SUFFICIENT");
    dep["train_role"] = json!("stable_contract");
    dep["successors"] = json!(["SUB"]);
    let mut sub = make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT");
    sub["dependencies"] = json!([{"target": "DEP", "class": "hard", "provenance": "fixture"}]);
    let inputs = load_fixture_inputs(
        &root,
        &[sub, dep],
        &[mapped_node_entry("SUB", true), unmapped_node_entry("DEP")],
        &[
            disposition_record("SUB", 9001, "ISSUE_PLAN_SUFFICIENT"),
            disposition_record("DEP", 9101, "ISSUE_PLAN_SUFFICIENT"),
        ],
    )?;
    let refusal = compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", None)
        .expect_err("a spec-only disposition on a hard dependency must not count as landed");
    assert_eq!(refusal.code, "HARD_DEPENDENCY_NOT_CURRENT");
    assert!(refusal.detail.contains("specability"), "detail: {}", refusal.detail);
    Ok(())
}

#[test]
fn landed_contract_hard_dependency_admits_the_packet() -> Result<()> {
    let root = fixture_tree("landed-dep")?;
    let mut dep = make_node("DEP", 9101, "implementation", "EXISTING_CONTRACT_SUFFICIENT");
    dep["train_role"] = json!("stable_contract");
    dep["successors"] = json!(["SUB"]);
    let mut sub = make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT");
    sub["dependencies"] = json!([{"target": "DEP", "class": "hard", "provenance": "fixture"}]);
    let inputs = load_fixture_inputs(
        &root,
        &[sub, dep],
        &[mapped_node_entry("SUB", true), mapped_node_entry("DEP", true)],
        &[
            disposition_record("SUB", 9001, "ISSUE_PLAN_SUFFICIENT"),
            disposition_record("DEP", 9101, "EXISTING_CONTRACT_SUFFICIENT"),
        ],
    )?;
    let live = observed_vacant();
    let doc = compose_builder_packet(&root, &inputs, "SUB", "coding_agent_bounded", Some(&live))
        .map_err(|refusal| color_eyre::eyre::eyre!("unexpected refusal: {}", refusal.line()))?;
    assert_eq!(doc["frontier"]["decision"], "ready");
    Ok(())
}

#[test]
fn duplicate_or_mismatched_ledger_records_fail_the_load() -> Result<()> {
    let root = fixture_tree("dup-ledger")?;
    let mut inputs = load_fixture_inputs(
        &root,
        &[make_node("SUB", 9001, "implementation", "ISSUE_PLAN_SUFFICIENT")],
        &[mapped_node_entry("SUB", true)],
        &[disposition_record("SUB", 9001, "ISSUE_PLAN_SUFFICIENT")],
    )?;
    // A second, stale record for the same node must not be silently trusted.
    inputs.specs.records.push(inputs.specs.records[0].clone());
    let ledger_bytes = serde_json::to_vec(&inputs.specs)?;
    write_json(
        &root,
        SPECS_LEDGER_RELATIVE_PATH,
        &serde_json::from_slice::<Value>(&ledger_bytes)?,
    )?;
    let engine = load_inputs_with_git(&root, Some(fixture_git()))?;
    let failure = match complete_adapter_inputs(&root, engine) {
        Err(failure) => failure,
        Ok(_) => panic!("duplicate E02 records must fail the adapter load"),
    };
    assert!(format!("{failure:#}").contains("duplicate records"));
    Ok(())
}

#[test]
fn review_head_must_bind_the_observed_checkout() -> Result<()> {
    let root = fixture_tree("head-mismatch")?;
    let inputs = default_fixture(&root)?;
    let facts = ReviewFacts {
        base: "base0".into(),
        head: "deadbeef".to_string(),
        diff: "sha256:dd".into(),
        controls: complete_controls(),
    };
    let refusal = compose_review_packet(&root, &inputs, "SUB", &facts)
        .expect_err("a head from another tree must refuse");
    assert_eq!(refusal.code, "HEAD_TREE_MISMATCH");
    Ok(())
}

#[test]
fn eligibility_refusals_are_distinct_from_instrument_failures() -> Result<()> {
    for code in [
        "MISSING_SPEC_DISPOSITION",
        "PROFILE_NOT_PERMITTED",
        "SPEC_DISPOSITION_NOT_BUILDER",
        "CONTEXT_MAPPING_GAP",
        "HARD_DEPENDENCY_NOT_CURRENT",
        "NO_WRITE_SURFACE",
    ] {
        assert!(is_eligibility_refusal(code), "{code} must be a typed eligibility refusal");
    }
    for code in [
        "SHARED_CONTRACT_VALIDATION_FAILED",
        "NODE_RESOLUTION_FAILED",
        "CONTEXT_RESOLUTION_FAILED",
        "BUILDER_PACKET_INVALID",
    ] {
        assert!(!is_eligibility_refusal(code), "{code} is an instrument failure, not eligibility");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 4: the #12181 E06-consumer fixture round-trips through the same
// shared validation/rendering entry this adapter consumes.
// ---------------------------------------------------------------------------

#[test]
fn e06_consumer_fixture_round_trips_through_the_adapter_entry() -> Result<()> {
    let root = crate::utils::project_root()?;
    let fixture =
        root.join("fixtures/agent_implementation_packet/consumer_emacs_e06_shape.v1.json");
    let bytes =
        std::fs::read(&fixture).with_context(|| format!("reading {}", fixture.display()))?;
    let doc: Value = serde_json::from_slice(&bytes)?;
    let machine = render_builder_packet(&doc, PacketProjection::Machine)
        .with_context(|| "the E06 consumer fixture must satisfy the shared contract unchanged")?;
    let markdown = render_builder_packet(&doc, PacketProjection::Markdown)?;
    let compact = render_builder_packet(&doc, PacketProjection::Compact)?;
    assert!(machine.contains("agent_implementation_packet.v1"));
    assert!(!markdown.is_empty() && !compact.is_empty());
    Ok(())
}
