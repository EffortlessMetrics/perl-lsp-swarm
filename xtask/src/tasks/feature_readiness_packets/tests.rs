//! Determinism, honesty, and negative-control proof for the FR-C05 packet
//! generator (#11286). Every mutation asserts its exact violation code so a
//! weakened validator cannot pass by becoming permissive.

use super::{build, denominator, nodes, render, validate};
use serde_json::Value;
use std::sync::OnceLock;

type TestResult<T = ()> = color_eyre::eyre::Result<T>;

fn registry() -> &'static Vec<super::model::NodeSpec> {
    static REGISTRY: OnceLock<Vec<super::model::NodeSpec>> = OnceLock::new();
    REGISTRY.get_or_init(nodes::all_nodes)
}

fn node(node_id: &str) -> &'static super::model::NodeSpec {
    registry()
        .iter()
        .find(|node| node.node_id == node_id)
        .unwrap_or_else(|| unreachable_node(node_id))
}

fn unreachable_node(node_id: &str) -> ! {
    // Test-only lookup failure is a programming error in this file; fail the
    // test binary through a panic-equivalent that names the bad id.
    panic!("unknown fixture node id in test: {node_id}")
}

fn builder_of(node_id: &str) -> (Value, String) {
    build::builder_document(node(node_id), None)
}

fn reviewer_of(node_id: &str) -> Value {
    build::reviewer_document(node(node_id), None).0
}

fn codes(violations: &[validate::Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

const ALL_NODE_IDS: &[&str] = &[
    "fr_1850_semantic_token_geometry",
    "fr_5108_navigation_truth_repair",
    "fr_6992_installed_critic_journey_proof",
    "fr_6997_critic_product_child",
    "fr_7122_support_registry_governance_row",
    "fr_7278_dap_release_ruling",
    "fr_8277_import_governed_operations_leaf",
    "fr_8301_deferred_npm_distribution",
    "fr_8305_import_containment_leaf",
    "fr_8336_import_claim_proof_row",
    "fr_8944_signature_semantic_cutover",
    "fr_9349_formatter_product_child",
    "fr_9415_dap_reliability_leaf",
    "fr_10724_formatting_currentness_proof",
    "fr_11250_semantic_token_shadow",
    "fr_11259_semantic_token_live_cutover",
    "fr_11261_object_facts_source_anchors",
    "fr_11263_application_framework_projection",
    "fr_11267_installed_vscode_proof",
    "fr_11271_zydeco_research",
];

#[test]
fn registry_is_complete_against_the_mandated_fixture_list() {
    let ids: Vec<_> = registry().iter().map(|node| node.node_id).collect();
    assert_eq!(ids.len(), ALL_NODE_IDS.len(), "registry size drifted");
    for expected in ALL_NODE_IDS {
        assert!(ids.contains(expected), "missing mandated node {expected}");
    }
}

#[test]
fn registry_matches_independent_actionable_denominator_and_dispositions() -> TestResult {
    let violations = validate::validate_registry_denominator(registry());
    assert!(violations.is_empty(), "denominator drift: {:?}", codes(&violations));
    let actionable = denominator::ENTRIES
        .iter()
        .filter(|entry| entry.disposition == super::model::DenominatorDisposition::Actionable)
        .count();
    assert_eq!(actionable, 18);
    assert_eq!(
        denominator::ENTRIES
            .iter()
            .filter(|entry| entry.disposition == super::model::DenominatorDisposition::Deferred)
            .count(),
        1
    );
    assert_eq!(
        denominator::ENTRIES
            .iter()
            .filter(|entry| entry.disposition == super::model::DenominatorDisposition::Excluded)
            .count(),
        4
    );
    Ok(())
}

#[test]
fn denominator_rejects_duplicate_registry_identity() {
    let mut duplicate = nodes::all_nodes();
    duplicate.push(duplicate[0].clone());
    let violations = validate::validate_registry_denominator(&duplicate);
    assert!(codes(&violations).contains(&"duplicate_node_identity"));
    assert!(codes(&violations).contains(&"duplicate_issue_identity"));
}

#[test]
fn denominator_rejects_membership_drift() {
    let mut missing = nodes::all_nodes();
    missing.remove(0);
    let violations = validate::validate_registry_denominator(&missing);
    assert!(codes(&violations).contains(&"denominator_missing"));
}

/// #11286's representative list pins exact issues; every one must appear.
#[test]
fn mandated_issue_numbers_are_represented() {
    let mandated: &[u32] = &[
        8305, 5108, 8944, 1850, 11250, 11259, 8277, 8336, 6997, 6992, 9349, 10724, 11271, 11261,
        11263, 11267, 8301, 7122, 9415, 7278,
    ];
    let present: Vec<u32> =
        registry().iter().flat_map(|node| node.issues.iter().copied()).collect();
    for issue in mandated {
        assert!(present.contains(issue), "mandated issue {issue} has no representative node");
    }
}

#[test]
fn every_node_emits_valid_builder_and_reviewer_packets() -> TestResult {
    for node in registry().iter() {
        let (builder, _) = build::builder_document(node, None);
        let violations = validate::validate_builder(&builder);
        assert!(
            violations.is_empty(),
            "{} builder invalid: {:?}",
            node.node_id,
            codes(&violations)
        );
        let reviewer = build::reviewer_document(node, None).0;
        let mut all = validate::validate_reviewer(&reviewer);
        all.extend(validate::validate_pair(&builder, &reviewer));
        assert!(all.is_empty(), "{} reviewer/pair invalid: {:?}", node.node_id, codes(&all));
    }
    Ok(())
}

#[test]
fn generation_is_deterministic_across_runs() {
    for node_id in ALL_NODE_IDS {
        let first = builder_of(node_id);
        let second = builder_of(node_id);
        assert_eq!(
            render::canonical_json(&first.0),
            render::canonical_json(&second.0),
            "{node_id} builder bytes differ between runs"
        );
        let first_review = reviewer_of(node_id);
        let second_review = reviewer_of(node_id);
        assert_eq!(
            render::canonical_json(&first_review),
            render::canonical_json(&second_review),
            "{node_id} reviewer bytes differ between runs"
        );
    }
}

/// Negative control: input order changes no canonical bytes.
#[test]
fn registry_input_order_never_changes_digest() {
    let forward = nodes::all_nodes();
    let mut reversed = nodes::all_nodes();
    reversed.reverse();
    let forward_digest = super::model::registry_digest(&forward);
    let reverse_digest = super::model::registry_digest(&reversed);
    assert_eq!(forward_digest, reverse_digest);
}

#[test]
fn registry_digest_covers_every_semantic_registry_section() {
    let baseline = nodes::all_nodes();
    let digest = |nodes: &[super::model::NodeSpec]| super::model::registry_digest(nodes);
    let mut cases = Vec::new();

    let mut changed = baseline.clone();
    changed[0].authorities[0].subject = "changed authority";
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].operations[0].policy_semantic = "changed operation policy";
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].allowed_surfaces.push("changed surface");
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].artifacts[0].claim_impact = "changed artifact impact";
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].controls[0].subject = "changed control";
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].commands[0].1 = "changed command";
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].lenses[0].applicable = !changed[0].lenses[0].applicable;
    cases.push(changed);
    let mut changed = baseline.clone();
    changed[0].old_paths.push(super::model::OldPathRow {
        seam: "changed old path",
        terminal_disposition: "removed",
    });
    cases.push(changed);

    let baseline_digest = digest(&baseline);
    assert!(cases.into_iter().all(|candidate| digest(&candidate) != baseline_digest));
}

#[test]
fn delivery_routing_matches_declared_prerequisites_and_successors() {
    for (node_id, dependencies, unblocks) in [
        ("fr_1850_semantic_token_geometry", &[][..], &[11250, 11259][..]),
        ("fr_11250_semantic_token_shadow", &[1850][..], &[11259][..]),
        ("fr_11259_semantic_token_live_cutover", &[11250][..], &[][..]),
        ("fr_8305_import_containment_leaf", &[8277][..], &[8277][..]),
        ("fr_8277_import_governed_operations_leaf", &[8305][..], &[8336][..]),
        ("fr_11261_object_facts_source_anchors", &[11259][..], &[11263][..]),
        ("fr_11263_application_framework_projection", &[11261][..], &[][..]),
    ] {
        let (builder, _) = builder_of(node_id);
        assert_eq!(
            builder.pointer("/delivery/issues/dependencies"),
            Some(&serde_json::json!(dependencies))
        );
        assert_eq!(
            builder.pointer("/delivery/issues/unblocks"),
            Some(&serde_json::json!(unblocks))
        );
    }
}

#[test]
fn delivery_dependency_mutations_fail_closed() {
    let (builder, _) = builder_of("fr_11261_object_facts_source_anchors");
    let mutated = mutate(&builder, &|doc| {
        doc.pointer_mut("/delivery/issues/dependencies")
            .and_then(Value::as_array_mut)
            .map(|dependencies| dependencies.push(Value::from(11259)));
    });
    assert!(codes(&validate::validate_builder(&mutated)).contains(&"dependency_mismatch"));
}

#[test]
fn every_non_product_role_rejects_each_foreign_core_step() {
    let cases = [
        ("fr_8336_import_claim_proof_row", "execute_research_protocol"),
        ("fr_6992_installed_critic_journey_proof", "execute_registry_mapping"),
        ("fr_11271_zydeco_research", "execute_proof_protocol"),
        ("fr_7122_support_registry_governance_row", "execute_proof_protocol"),
    ];
    for (node_id, foreign_step) in cases {
        let (builder, _) = builder_of(node_id);
        let mutated = mutate(&builder, &|doc| {
            doc.get_mut("sequence")
                .and_then(Value::as_array_mut)
                .map(|steps| steps.push(Value::String(foreign_step.to_owned())));
        });
        assert!(
            codes(&validate::validate_builder(&mutated)).contains(&"wrong_role_sequence"),
            "{node_id} accepted {foreign_step}"
        );
    }
}

fn mutate(doc: &Value, path: &dyn Fn(&mut Value)) -> Value {
    let mut copy = doc.clone();
    path(&mut copy);
    copy
}

// Both directions of the role rule: a product packet stripped of its
// implementation step is equally invalid.
#[test]
fn product_role_without_implementation_step_fails() {
    let (product, _) = builder_of("fr_1850_semantic_token_geometry");
    let stripped = mutate(&product, &|doc| {
        if let Some(steps) = doc.get_mut("sequence").and_then(Value::as_array_mut) {
            steps.retain(|step| step.as_str() != Some("implement_proposition"));
        }
    });
    assert!(codes(&validate::validate_builder(&stripped)).contains(&"wrong_role_sequence"));
}

// Negative control: wrong role emitted as product implementation.
#[test]
fn non_product_roles_cannot_encode_product_implementation() {
    for node_id in [
        "fr_11271_zydeco_research",
        "fr_7122_support_registry_governance_row",
        "fr_8336_import_claim_proof_row",
        "fr_6992_installed_critic_journey_proof",
        "fr_7278_dap_release_ruling",
    ] {
        let (builder, _) = builder_of(node_id);
        let mutated = mutate(&builder, &|doc: &mut Value| {
            if let Some(sequence) = doc.get_mut("sequence").and_then(Value::as_array_mut) {
                for step in sequence.iter_mut() {
                    if matches!(
                        step.as_str(),
                        Some("execute_research_protocol")
                            | Some("execute_proof_protocol")
                            | Some("execute_registry_mapping")
                    ) {
                        *step = Value::String("implement_proposition".to_owned());
                    }
                }
            }
        });
        let violations = validate::validate_builder(&mutated);
        let violation_codes = codes(&violations);
        assert!(
            violation_codes.contains(&"wrong_role_sequence"),
            "{node_id}: implementation step must be rejected for non-product roles"
        );
    }
}

// Negative control: missing falsifier / artifact owner / claim rows.
#[test]
fn missing_required_cells_fail_closed() {
    let (builder, _) = builder_of("fr_5108_navigation_truth_repair");

    let no_falsifier = mutate(&builder.clone(), &|doc| {
        if let Some(proof) = doc.get_mut("proof").and_then(Value::as_object_mut) {
            proof.remove("first_falsifier");
        }
    });
    assert!(codes(&validate::validate_builder(&no_falsifier)).contains(&"missing_field"));

    let empty_artifacts = mutate(&builder.clone(), &|doc| {
        if let Some(object) = doc.as_object_mut() {
            object.insert("artifacts".to_owned(), Value::Array(Vec::new()));
        }
    });
    assert!(codes(&validate::validate_builder(&empty_artifacts)).contains(&"empty_field"));

    let no_ceiling = mutate(&builder.clone(), &|doc| {
        if let Some(claim) = doc.get_mut("claim_ceiling").and_then(Value::as_object_mut) {
            claim.insert("cannot_establish".to_owned(), Value::Array(Vec::new()));
        }
    });
    assert!(codes(&validate::validate_builder(&no_ceiling)).contains(&"empty_field"));

    let no_old_path_terminal = mutate(&builder, &|doc| {
        if let Some(delivery) = doc.get_mut("delivery").and_then(Value::as_object_mut) {
            delivery.insert(
                "old_path_dispositions".to_owned(),
                serde_json::json!([{ "seam": "masking return path" }]),
            );
        }
    });
    assert!(
        codes(&validate::validate_builder(&no_old_path_terminal))
            .contains(&"old_path_unterminated")
    );
}

// Negative control: generic verification language is banned.
#[test]
fn generic_verification_statements_are_rejected() {
    let (builder, _) = builder_of("fr_1850_semantic_token_geometry");
    let mutated = mutate(&builder, &|doc| {
        if let Some(proof) = doc.get_mut("proof").and_then(Value::as_object_mut) {
            proof.insert(
                "positive_discriminator".to_owned(),
                Value::String("add tests and run the workspace".to_owned()),
            );
        }
    });
    assert!(codes(&validate::validate_builder(&mutated)).contains(&"generic_verification"));
}

// Negative control: mutable collaboration state cannot hide in a packet.
#[test]
fn mutable_state_keys_are_rejected() {
    let (builder, _) = builder_of("fr_1850_semantic_token_geometry");
    let mutated = mutate(&builder, &|doc| {
        if let Some(work) = doc.get_mut("work").and_then(Value::as_object_mut) {
            work.insert("lease".to_owned(), Value::String("agent-7".to_owned()));
        }
    });
    assert!(codes(&validate::validate_builder(&mutated)).contains(&"mutable_state_key"));
}

// Negative control: content-addressing must bind identity to bytes.
#[test]
fn tampered_content_breaks_content_address() {
    let (builder, _) = builder_of("fr_8944_signature_semantic_cutover");
    let mutated = mutate(&builder.clone(), &|doc| {
        if let Some(work) = doc.get_mut("work").and_then(Value::as_object_mut) {
            work.insert(
                "objective_sentence".to_owned(),
                Value::String("a different objective that keeps the stale id".to_owned()),
            );
        }
    });
    assert!(codes(&validate::validate_builder(&mutated)).contains(&"content_address_mismatch"));
}

// Honesty: offline packets state unknown live state and require preflight.
#[test]
fn offline_packets_stay_honest_about_unknown_live_state() {
    let (builder, _) = builder_of("fr_6997_critic_product_child");
    let Some(live) = builder.pointer("/planes/live") else {
        panic!("generated packets always carry a live plane");
    };
    assert_eq!(live.get("state"), Some(&Value::String("unknown".to_owned())));
    assert_eq!(live.get("preflight_required"), Some(&Value::Bool(true)));
    assert_eq!(live.get("candidate_branch"), Some(&Value::Null));
    let base_head = builder.pointer("/delivery/base_head").and_then(Value::as_str).unwrap_or("");
    assert!(base_head.contains("preflight"), "offline packets must not invent a head: {base_head}");
}

// Negative control: an offline packet claiming observed knowledge fails.
#[test]
fn offline_packets_claiming_live_knowledge_fail() {
    let (builder, _) = builder_of("fr_6997_critic_product_child");
    let mutated = mutate(&builder, &|doc| {
        if let Some(live) = doc.pointer_mut("/planes/live").and_then(Value::as_object_mut) {
            live.insert(
                "candidate_branch".to_owned(),
                Value::String("agent/some-candidate".to_owned()),
            );
        }
    });
    assert!(
        codes(&validate::validate_builder(&mutated)).contains(&"offline_live_overspecification")
    );
}

// Live snapshots: closed vocabulary, section isolation, action variants.
#[test]
fn live_snapshots_change_only_live_sections() -> TestResult {
    fn snapshot(writer_active: bool, action: &str) -> build::LiveSnapshot {
        build::LiveSnapshot {
            head_sha: "a".repeat(40),
            candidate_branch: Some("agent/candidate".to_owned()),
            writer_active,
            required_action: super::model::LiveAction::parse(action)
                .unwrap_or(super::model::LiveAction::None),
            source_digest: "b".repeat(64),
        }
    }
    let node = node("fr_10724_formatting_currentness_proof");
    let quiet = build::builder_document(node, Some(&snapshot(false, "none"))).0;
    let busy = build::builder_document(node, Some(&snapshot(true, "resume"))).0;

    fn strip_live(doc: &Value) -> String {
        let mut copy = doc.clone();
        // The content-addressed id covers the whole document including the
        // live plane, so it is excluded with the live sections themselves.
        if let Some(object) = copy.as_object_mut() {
            object.remove("packet_id");
        }
        if let Some(planes) = copy.pointer_mut("/planes").and_then(Value::as_object_mut) {
            planes.remove("live");
        }
        if let Some(delivery) = copy.pointer_mut("/delivery").and_then(Value::as_object_mut) {
            delivery.remove("base_head");
        }
        render::canonical_json(&copy)
    }
    assert_eq!(strip_live(&quiet), strip_live(&busy), "live refresh rewrote stable sections");
    assert_eq!(quiet.pointer("/planes/live/state").and_then(Value::as_str), Some("observed"));
    assert_eq!(quiet.pointer("/planes/live/required_action").and_then(Value::as_str), Some("none"));

    let parsed = build::LiveSnapshot::parse(
        br#"{"head_sha": "c3ab8ff13720e8ad9047dd39466b3c8974e592c2", "candidate_branch": null, "required_action": "repair", "writer_active": false}"#,
    )?;
    assert_eq!(parsed.required_action.as_str(), "repair");
    Ok(())
}

#[test]
fn live_snapshot_parsing_fails_closed() {
    let missing_head = build::LiveSnapshot::parse(br#"{"candidate_branch": "x"}"#);
    assert!(missing_head.is_err());
    let unknown_key = build::LiveSnapshot::parse(
        br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "lease": "agent-7"}"#,
    );
    assert!(unknown_key.is_err());
    let unknown_action = build::LiveSnapshot::parse(
        br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "required_action": "merge"}"#,
    );
    assert!(unknown_action.is_err());
    let short_head = build::LiveSnapshot::parse(br#"{"head_sha": "abc"}"#);
    assert!(short_head.is_err());
}

// Reviewer independence: full lens coverage with typed questions.
#[test]
fn reviewer_packets_cover_all_lenses_and_stage_examples() {
    for node_id in ALL_NODE_IDS {
        let reviewer = reviewer_of(node_id);
        let applicable: Vec<&str> = reviewer
            .get("lenses")
            .and_then(Value::as_array)
            .map(|lenses| {
                lenses
                    .iter()
                    .filter_map(Value::as_object)
                    .filter(|lens| lens.get("applicable").and_then(Value::as_bool).unwrap_or(false))
                    .filter_map(|lens| lens.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        assert!(!applicable.is_empty(), "{node_id} reviewer has no applicable lens");
        let examples = reviewer
            .get("stage_falsification_examples")
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or_default();
        assert!(examples >= 1, "{node_id} reviewer lacks stage falsification examples");
    }
}

// Stale references: a reviewer bound to another builder/head is stale.
#[test]
fn stale_builder_reference_and_head_are_detected() {
    let vscode = node("fr_11267_installed_vscode_proof");
    let (builder, _) = build::builder_document(vscode, None);
    let (_, other_digest) = build::builder_document(node("fr_1850_semantic_token_geometry"), None);
    let (reviewer, _) = build::reviewer_document(vscode, None);

    let stale_ref = mutate(&reviewer.clone(), &|doc| {
        if let Some(reference) = doc.get_mut("builder_ref").and_then(Value::as_object_mut) {
            reference.insert("digest".to_owned(), Value::String(other_digest.clone()));
        }
    });
    assert!(codes(&validate::validate_pair(&builder, &stale_ref)).contains(&"stale_builder_ref"));

    let stale_head = mutate(&reviewer, &|doc| {
        if let Some(currentness) = doc.get_mut("currentness").and_then(Value::as_object_mut) {
            currentness.insert("base_head".to_owned(), Value::String("main@deadbeef".to_owned()));
        }
    });
    assert!(codes(&validate::validate_pair(&builder, &stale_head)).contains(&"stale_head"));
}

// Compact renderings retain load-bearing constraints.
#[test]
fn compact_projections_stay_lossless_for_every_node() {
    for node_id in ALL_NODE_IDS {
        let (builder, _) = builder_of(node_id);
        let compact_text = render::compact(&builder);
        let loss = render::validate_compact_lossless(&builder, &compact_text);
        assert!(loss.is_empty(), "{node_id} compact lost constraints: {:?}", codes(&loss));

        let reviewer = reviewer_of(node_id);
        let review_compact = render::compact(&reviewer);
        let review_loss = render::validate_reviewer_compact_lossless(&reviewer, &review_compact);
        assert!(
            review_loss.is_empty(),
            "{node_id} reviewer compact lost constraints: {:?}",
            codes(&review_loss)
        );
        assert!(
            review_compact.contains("FALSIFY[") || review_compact.contains("AUDIT "),
            "{node_id} review compact dropped falsifiers"
        );
    }
}

#[test]
fn reviewer_compact_mutations_are_non_vacuous_for_every_audit_cell() {
    let reviewer = reviewer_of("fr_8305_import_containment_leaf");
    let compact = render::compact(&reviewer);

    let assert_missing = |path: String, value: Value| {
        let mutated = mutate(&reviewer, &|doc| {
            if let Some(slot) = doc.pointer_mut(&path) {
                *slot = value.clone();
            }
        });
        assert!(
            codes(&render::validate_reviewer_compact_lossless(&mutated, &compact))
                .contains(&"compact_loss"),
            "reviewer compact loss was not detected at {path}"
        );
    };

    assert_missing("/subject/issues".to_owned(), serde_json::json!([424242]));
    for (index, lens) in reviewer["lenses"].as_array().into_iter().flatten().enumerate() {
        for field in ["reason", "questions/0"] {
            let Some(original) = lens.pointer(&format!("/{field}")) else { continue };
            let Some(original) = original.as_str() else { continue };
            assert_missing(
                format!("/lenses/{index}/{field}"),
                Value::String(format!("unique reviewer mutation {index} {field} {original}")),
            );
        }
    }
    for (section, fields) in [
        ("stage_falsification_examples", ["question"].as_slice()),
        ("negative_control_audit", ["subject", "requirement"].as_slice()),
        ("old_path_audit", ["seam", "terminal_disposition"].as_slice()),
    ] {
        for (index, row) in reviewer[section].as_array().into_iter().flatten().enumerate() {
            for field in fields {
                let Some(original) = row.get(*field).and_then(Value::as_str) else { continue };
                assert_missing(
                    format!("/{section}/{index}/{field}"),
                    Value::String(format!(
                        "unique reviewer mutation {section} {index} {field} {original}"
                    )),
                );
            }
        }
    }
}

#[test]
fn generated_packets_validate_against_both_checked_schemas() {
    for node_id in ALL_NODE_IDS {
        let (builder, _) = builder_of(node_id);
        assert!(
            validate::validate_schema_instance(&builder).is_empty(),
            "builder schema drift for {node_id}"
        );
        let reviewer = reviewer_of(node_id);
        assert!(
            validate::validate_schema_instance(&reviewer).is_empty(),
            "reviewer schema drift for {node_id}"
        );
    }
}

#[test]
fn reviewer_identity_fields_are_bound_to_the_builder_subject() {
    let (builder, _) = builder_of("fr_8305_import_containment_leaf");
    let reviewer = reviewer_of("fr_8305_import_containment_leaf");
    for field in ["issues", "role", "profile", "claim_ceiling_sentence"] {
        let mut changed = reviewer.clone();
        let path = format!("/subject/{field}");
        if let Some(value) = changed.pointer_mut(&path) {
            *value = if value.is_array() {
                serde_json::json!([99999])
            } else {
                serde_json::json!("foreign")
            };
        }
        assert!(
            codes(&validate::validate_pair(&builder, &changed)).contains(&"subject_mismatch"),
            "{field} was not bound"
        );
    }
}

#[test]
fn equivalent_main_movement_preserves_packet_identity() {
    let node = node("fr_8305_import_containment_leaf");
    let snapshot = |head: &str| build::LiveSnapshot {
        head_sha: head.to_owned(),
        candidate_branch: None,
        writer_active: false,
        required_action: super::model::LiveAction::None,
        source_digest: "d".repeat(64),
    };
    let first = build::builder_document(node, Some(&snapshot(&"a".repeat(40))));
    let second = build::builder_document(node, Some(&snapshot(&"b".repeat(40))));
    assert_eq!(first.0["packet_id"], second.0["packet_id"]);
    assert_eq!(first.1, second.1);
    let mut semantic = snapshot(&"b".repeat(40));
    semantic.writer_active = true;
    let changed = build::builder_document(node, Some(&semantic));
    assert_ne!(first.0["packet_id"], changed.0["packet_id"]);
}

#[test]
fn parsed_snapshot_head_movement_preserves_packet_identity() -> TestResult {
    let first = build::LiveSnapshot::parse(
        br#"{"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","candidate_branch":null,"writer_active":false,"required_action":"none"}"#,
    )?;
    let second = build::LiveSnapshot::parse(
        br#"{
          "required_action": "none",
          "writer_active": false,
          "candidate_branch": null,
          "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }"#,
    )?;
    let node = node("fr_8305_import_containment_leaf");
    let first_packet = build::builder_document(node, Some(&first));
    let second_packet = build::builder_document(node, Some(&second));
    assert_eq!(first.source_digest, second.source_digest);
    assert_eq!(first_packet.0["packet_id"], second_packet.0["packet_id"]);
    assert_eq!(first_packet.1, second_packet.1);
    Ok(())
}

#[test]
fn pair_rejects_unbound_or_malformed_main_heads() {
    let (builder, _) = builder_of("fr_8305_import_containment_leaf");
    let reviewer = reviewer_of("fr_8305_import_containment_leaf");
    for head in ["main@stale", "main@", "other@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] {
        let mut changed = reviewer.clone();
        changed["currentness"]["base_head"] = Value::String(head.to_owned());
        assert!(
            codes(&validate::validate_pair(&builder, &changed)).contains(&"invalid_base_head"),
            "unbound head {head:?} was accepted"
        );
    }
}

#[test]
fn differing_snapshot_source_changes_packet_identity() {
    let node = node("fr_8305_import_containment_leaf");
    let snapshot = |digest: &str| build::LiveSnapshot {
        head_sha: "a".repeat(40),
        candidate_branch: None,
        writer_active: false,
        required_action: super::model::LiveAction::None,
        source_digest: digest.to_owned(),
    };
    let first = build::builder_document(node, Some(&snapshot(&"b".repeat(64))));
    let second = build::builder_document(node, Some(&snapshot(&"c".repeat(64))));
    assert_ne!(first.0["packet_id"], second.0["packet_id"]);
    assert_ne!(first.1, second.1);
}

// Every documented artifact cell must survive the dense projection: an
// id(mode)-only artifact row drops owner/proof/check/lens/impact constraints.
#[test]
fn compact_projection_detects_dropped_artifact_cells() {
    let (builder, _) = builder_of("fr_9415_dap_reliability_leaf");
    let compact_text = render::compact(&builder);
    let mut stripped = builder.clone();
    if let Some(items) = stripped.pointer_mut("/artifacts").and_then(Value::as_array_mut) {
        for item in items.iter_mut().filter_map(Value::as_object_mut) {
            for field in [
                "owner",
                "required_change_or_proof",
                "check_command",
                "review_lens",
                "claim_impact",
            ] {
                item.insert(field.to_owned(), Value::String("(dropped cell)".to_owned()));
            }
        }
    }
    let loss = render::validate_compact_lossless(&stripped, &compact_text);
    assert!(
        codes(&loss).iter().all(|code| *code == "compact_loss"),
        "artifact-cell loss must be reported as compact_loss: {:?}",
        codes(&loss)
    );
    assert!(
        !loss.is_empty(),
        "an artifact reduced to cells absent from the compact projection must be detected"
    );
}

#[test]
fn compact_projection_keeps_duplicate_artifact_cells_local() {
    let (builder, _) = builder_of("fr_8305_import_containment_leaf");
    let compact_text = render::compact(&builder);
    let mut drifted = builder.clone();
    let first_owner = drifted
        .pointer("/artifacts/0/owner")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Some(second) = drifted.pointer_mut("/artifacts/1").and_then(Value::as_object_mut) {
        second.insert("owner".to_owned(), Value::String(first_owner));
    }
    let loss = render::validate_compact_lossless(&drifted, &compact_text);
    assert!(codes(&loss).contains(&"compact_loss"));
}

#[test]
fn reviewer_compact_rejects_cross_lens_satisfaction() {
    let reviewer = reviewer_of("fr_8305_import_containment_leaf");
    let compact_text = render::compact(&reviewer);
    let reason = reviewer["lenses"][0]["reason"].as_str().unwrap_or_default();
    assert!(!reason.is_empty());
    let without_first_reason = compact_text.replacen(&format!(" reason={reason}"), "", 1);
    let loss = render::validate_reviewer_compact_lossless(&reviewer, &without_first_reason);
    assert!(codes(&loss).contains(&"compact_loss"));
}

// Dropped stop conditions are a rendering regression, not a style choice.
#[test]
fn compact_projection_detects_dropped_constraints() {
    let (builder, _) = builder_of("fr_7122_support_registry_governance_row");
    let compact_text = render::compact(&builder);
    let first_condition = builder
        .pointer("/stop/conditions")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    assert!(!first_condition.is_empty());
    let without_stop = compact_text.replace(&first_condition, "");
    let loss = render::validate_compact_lossless(&builder, &without_stop);
    assert!(codes(&loss).contains(&"compact_loss"));
}

#[test]
fn compact_projection_detects_each_required_packet_section_loss() {
    let (builder, _) = builder_of("fr_8305_import_containment_leaf");
    let compact_text = render::compact(&builder);
    for (section, replacement) in [
        ("operations", serde_json::json!([{"feature": "dropped"}])),
        (
            "claim_ceiling",
            serde_json::json!({"prerequisite_disposition": "dropped", "successors": [], "remaining_not_proven": []}),
        ),
        ("sequence", serde_json::json!(["dropped"])),
        (
            "delivery",
            serde_json::json!({"issues": {"controller": 0, "dependencies": [], "unblocks": []}}),
        ),
        (
            "durable_spec",
            serde_json::json!({"disposition": "dropped", "owner": "dropped", "note": "dropped"}),
        ),
        (
            "live",
            serde_json::json!({"state": "dropped", "snapshot_digest": null, "head_sha": null, "candidate_branch": null, "writer_active": null, "required_action": "none", "preflight_required": true}),
        ),
    ] {
        let mut mutated = builder.clone();
        if section == "delivery" {
            if let Some(delivery) = mutated.get_mut("delivery").and_then(Value::as_object_mut) {
                delivery.insert("issues".to_owned(), replacement["issues"].clone());
            }
        } else if section == "claim_ceiling" {
            if let Some(target) = mutated.get_mut(section) {
                *target = replacement;
            }
        } else if section == "live" {
            if let Some(target) = mutated.pointer_mut("/planes/live") {
                *target = replacement;
            }
        } else if let Some(target) = mutated.get_mut(section) {
            *target = replacement;
        }
        let loss = render::validate_compact_lossless(&mutated, &compact_text);
        assert!(codes(&loss).contains(&"compact_loss"), "{section} loss was not detected");
    }
}

// Schema files stay pinned to the Rust closed vocabularies.
#[test]
fn schema_files_match_closed_vocabularies() -> TestResult {
    use super::validate as v;
    let root = crate::utils::project_root()?;
    for (file, defs) in [
        (
            "schemas/feature_readiness_builder_packet.v1.schema.json",
            vec![
                ("role", v::ROLES),
                ("node_disposition", v::DISPOSITIONS),
                ("profile", v::PROFILES),
                ("domain", v::DOMAINS),
                ("authority_group", v::AUTHORITY_GROUPS),
                ("artifact_mode", v::ARTIFACT_MODES),
                ("durable_spec_disposition", v::DURABLE_SPEC_DISPOSITIONS),
                ("sequence_step", v::SEQUENCE_STEPS),
                ("control_class", v::CONTROL_CLASSES),
                ("terminal_old_path_disposition", v::TERMINAL_OLD_PATH_DISPOSITIONS),
                ("forbidden_action", v::FORBIDDEN_ACTIONS),
                ("live_action", v::LIVE_ACTIONS),
            ],
        ),
        (
            "schemas/feature_readiness_reviewer_packet.v1.schema.json",
            vec![
                ("role", v::ROLES),
                ("review_lens", v::REVIEW_LENSES),
                ("example_stage", v::EXAMPLE_STAGES),
                ("terminal_old_path_disposition", v::TERMINAL_OLD_PATH_DISPOSITIONS),
            ],
        ),
    ] {
        let text = std::fs::read_to_string(root.join(file))
            .map_err(|error| color_eyre::eyre::eyre!("reading {file}: {error}"))?;
        let schema: Value = serde_json::from_str(&text)
            .map_err(|error| color_eyre::eyre::eyre!("parsing {file}: {error}"))?;
        for (name, vocabulary) in &defs {
            let enum_values: Vec<&str> = schema
                .pointer(&format!("/$defs/{name}/enum"))
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            assert_eq!(
                enum_values.as_slice(),
                *vocabulary,
                "{file}: $defs.{name}.enum drifted from the pinned vocabulary"
            );
        }
    }
    Ok(())
}

// Finding: the builder schema pinned delivery.old_path_dispositions items to
// the bare terminal-disposition string while the generator, the Rust
// validator, and the reviewer schema all treat every row as
// {seam, terminal_disposition}. The schema must describe the emitted shape.
#[test]
fn builder_schema_describes_emitted_old_path_rows() -> TestResult {
    let schema = read_schema_file("schemas/feature_readiness_builder_packet.v1.schema.json")?;
    let delivery_required: Vec<&str> = schema
        .pointer("/properties/delivery/required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        delivery_required.contains(&"changed_surfaces"),
        "builder schema must require runtime-validated delivery.changed_surfaces"
    );
    let items = schema.pointer("/properties/delivery/properties/old_path_dispositions/items");
    let Some(items) = items else {
        panic!("builder schema must constrain delivery.old_path_dispositions items");
    };
    assert_eq!(items.get("type"), Some(&Value::String("object".to_owned())));
    let required: Vec<&str> = items
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        required.as_slice(),
        &["seam", "terminal_disposition"],
        "old-path rows must carry seam identity beside the terminal disposition"
    );
    assert_eq!(
        items.pointer("/properties/terminal_disposition/$ref"),
        Some(&Value::String("#/$defs/terminal_old_path_disposition".to_owned())),
        "the terminal vocabulary stays pinned through $defs"
    );

    // Discriminating consumer control: a string-form row still fails closed.
    let (builder, _) = builder_of("fr_5108_navigation_truth_repair");
    let string_form = mutate(&builder, &|doc| {
        if let Some(delivery) = doc.get_mut("delivery").and_then(Value::as_object_mut) {
            delivery.insert("old_path_dispositions".to_owned(), serde_json::json!(["removed"]));
        }
    });
    assert!(
        codes(&validate::validate_builder(&string_form)).contains(&"old_path_unterminated"),
        "string-form old-path rows must never satisfy the delivery audit"
    );
    Ok(())
}

// Both new schema files are closed surfaces: unknown keys must be rejected by
// standard JSON Schema validation exactly as the Rust validator rejects them.
#[test]
fn schema_files_reject_unknown_fields_everywhere() -> TestResult {
    for file in [
        "schemas/feature_readiness_builder_packet.v1.schema.json",
        "schemas/feature_readiness_reviewer_packet.v1.schema.json",
    ] {
        let schema = read_schema_file(file)?;
        let mut open_objects: Vec<String> = Vec::new();
        assert_every_object_is_closed(&schema, &mut String::from("$"), &mut open_objects);
        assert!(
            open_objects.is_empty(),
            "{file}: object schemas without additionalProperties:false: {open_objects:?}"
        );
    }
    Ok(())
}

fn read_schema_file(file: &str) -> TestResult<Value> {
    let root = crate::utils::project_root()?;
    let text = std::fs::read_to_string(root.join(file))
        .map_err(|error| color_eyre::eyre::eyre!("reading {file}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| color_eyre::eyre::eyre!("parsing {file}: {error}"))
}

fn assert_every_object_is_closed(value: &Value, path: &mut String, open: &mut Vec<String>) {
    if let Some(object) = value.as_object() {
        if object.get("type").and_then(Value::as_str) == Some("object")
            && object.get("additionalProperties") != Some(&Value::Bool(false))
        {
            open.push(path.clone());
        }
        for (key, child) in object {
            let length = path.len();
            path.push('/');
            path.push_str(key);
            assert_every_object_is_closed(child, path, open);
            path.truncate(length);
        }
    } else if let Some(items) = value.as_array() {
        for (index, child) in items.iter().enumerate() {
            let length = path.len();
            path.push_str(&format!("/{index}"));
            assert_every_object_is_closed(child, path, open);
            path.truncate(length);
        }
    }
}

// A `--all --check` denominator gate consumes `--live-snapshot`: an
// unreadable or malformed snapshot fails closed instead of silently running
// the offline denominator.
#[test]
fn all_check_consumes_the_live_snapshot() -> TestResult {
    use super::FeatureReadinessTrainCommand;
    let command = |snapshot: Option<std::path::PathBuf>| FeatureReadinessTrainCommand::Packet {
        node: None,
        reviewer: false,
        markdown: false,
        compact: false,
        check: true,
        live_snapshot: snapshot,
        all: true,
    };
    let Err(error) =
        super::run(command(Some(std::path::PathBuf::from("definitely-absent/snapshot.json"))))
    else {
        panic!("--all --check must fail closed when --live-snapshot cannot be read");
    };
    assert!(
        error.to_string().contains("reading live snapshot"),
        "the failure must name the unreadable snapshot input: {error}"
    );

    let mut complete = std::env::temp_dir();
    complete.push(format!("fr-live-complete-{}.json", std::process::id()));
    std::fs::write(
        &complete,
        br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "candidate_branch": null, "writer_active": false, "required_action": "none"}"#,
    )?;
    let ran = super::run(command(Some(complete.clone())));
    let _ = std::fs::remove_file(complete);
    ran?;

    let incomplete =
        std::env::temp_dir().join(format!("fr-live-partial-{}.json", std::process::id()));
    std::fs::write(&incomplete, br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#)?;
    let result = super::run(command(Some(incomplete.clone())));
    let _ = std::fs::remove_file(incomplete);
    let Err(error) = result else {
        panic!("--all --check must reject an incomplete live snapshot");
    };
    assert!(
        error.to_string().contains("incomplete live snapshot"),
        "the failure must name the incomplete observation set: {error}"
    );
    Ok(())
}

// A snapshot supplies observations, not defaults: omitting writer/action
// state fails closed naming the missing cells instead of implying
// writer_active=false under an `observed` banner.
#[test]
fn live_snapshot_parsing_requires_complete_observations() -> TestResult {
    let head_only =
        build::LiveSnapshot::parse(br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#);
    let Err(error) = head_only else {
        panic!("a head-only snapshot hides unknown writer/action state; it must fail closed");
    };
    let message = error.to_string();
    for missing in ["candidate_branch", "writer_active", "required_action"] {
        assert!(
            message.contains(missing),
            "the diagnostic must name every missing observation ({missing}): {message}"
        );
    }

    let complete = build::LiveSnapshot::parse(
        br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "candidate_branch": null, "writer_active": true, "required_action": "repair"}"#,
    )?;
    assert!(complete.writer_active);
    assert_eq!(complete.candidate_branch, None);
    assert_eq!(complete.required_action.as_str(), "repair");

    let typed_wrong_shape = build::LiveSnapshot::parse(
        br#"{"head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "candidate_branch": 7, "writer_active": false, "required_action": "none"}"#,
    );
    assert!(typed_wrong_shape.is_err());
    Ok(())
}

// A reviewer-mode check binds its verdict to the reviewer bytes: both digests
// stay on the receipt with labeled roles, never one digest wearing two hats.
#[test]
fn check_receipt_reports_both_labeled_digests() {
    let line = super::check_receipt_line(
        "fr_8305_import_containment_leaf",
        "reviewer",
        "a".repeat(64).as_str(),
        "b".repeat(64).as_str(),
    );
    assert!(line.starts_with("FR_PACKET_CHECK node=fr_8305_import_containment_leaf"), "{line}");
    assert!(line.contains("packet=reviewer"), "selected role must be named: {line}");
    let builder = format!("builder_digest={}", "a".repeat(64));
    let review = format!("reviewer_digest={}", "b".repeat(64));
    assert!(line.contains(&builder), "builder digest must carry its role label: {line}");
    assert!(line.contains(&review), "reviewer digest must be reported, not discarded: {line}");

    let builder_mode = super::check_receipt_line(
        "fr_8305_import_containment_leaf",
        "builder",
        "a".repeat(64).as_str(),
        "b".repeat(64).as_str(),
    );
    assert!(builder_mode.contains("packet=builder"));
}

// Role/disposition separation across the mandated representative fixtures.
#[test]
fn roles_and_claim_ceilings_remain_distinct() {
    let product = ["fr_1850_semantic_token_geometry", "fr_9415_dap_reliability_leaf"];
    let proof = ["fr_8336_import_claim_proof_row", "fr_10724_formatting_currentness_proof"];
    let installed = ["fr_6992_installed_critic_journey_proof", "fr_11267_installed_vscode_proof"];
    let research = ["fr_11271_zydeco_research"];
    let governance = ["fr_7122_support_registry_governance_row", "fr_7278_dap_release_ruling"];

    for id in product {
        assert!(node(id).role.allows_product_implementation(), "{id} must be product");
    }
    for id in proof.iter().chain(installed.iter()) {
        assert_eq!(
            node(id).profile.as_str(),
            if installed.contains(&id) { "installed_client" } else { "proof_only" }
        );
        assert!(!node(id).role.allows_product_implementation(), "{id} must not be product");
    }
    for id in research {
        assert_eq!(node(id).durable_spec.0.as_str(), "RETURN_TO_ISSUE_FOR_UNSETTLED_DECISION");
    }
    for id in governance {
        assert!(!node(id).role.allows_product_implementation(), "{id} must not be product");
    }
    // Deferred/blocked nodes cannot emit coding work.
    for id in ["fr_8301_deferred_npm_distribution", "fr_7278_dap_release_ruling"] {
        let (builder, _) = builder_of(id);
        let steps: Vec<&str> = builder
            .get("sequence")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        assert!(!steps.contains(&"implement_proposition"), "{id} emits coding work");
    }
}
