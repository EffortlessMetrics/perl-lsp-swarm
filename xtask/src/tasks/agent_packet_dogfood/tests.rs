//! Red-first proof seam for the shared dogfood core (#11024 family brief).
//!
//! Every mutated-packet negative control below is written BEFORE the
//! validating rules they exercise land, against synthetic fixtures only.
//! Each control pins one stable reason code; a green run proves the
//! validator rejects each mutant class instead of rendering plausible prose.

use super::*;
use serde_json::json;

const TREE_SHA_PLACEHOLDER: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";

/// (fixture name, pinned primary reason code) pairs — the complete
/// fail-closed mutant matrix committed under `fixtures/…​/invalid`.
const EXPECTED_INVALID: &[(&str, &str)] = &[
    ("missing_packet_digest.json", "missing_identity_field"),
    ("missing_subject.json", "missing_subject"),
    ("missing_model_identity.json", "missing_subject_field"),
    ("missing_scope_ceiling.json", "missing_scope_ceiling"),
    ("tampered_event_payload.json", "record_digest_mismatch"),
    ("tampered_packet_envelope.json", "packet_digest_mismatch"),
    ("unsorted_event_sequences.json", "event_seq_not_contiguous"),
    ("oversized_event_excerpt.json", "retention_bound_exceeded"),
    ("credential_in_payload.json", "credential_in_payload"),
    ("machine_local_path_in_payload.json", "local_path_in_payload"),
    ("chain_of_thought_in_payload.json", "cot_key_in_payload"),
    ("unknown_root_field.json", "unknown_field"),
    ("unknown_disposition.json", "unknown_disposition"),
    ("dangling_intervention_ref.json", "intervention_seq_unknown"),
];

fn sample_event(seq: u64, kind: &str, payload: Value) -> Value {
    let mut record = json!({
        "seq": seq,
        "kind": kind,
        "payload": payload,
        "digest": "0",
    });
    // Stamp computes over {domain, seq, kind, at_ms?, payload}; give the
    // placeholder a fixed at_ms so stamps stay reproducible across edits.
    record["at_ms"] = json!(seq * 10);
    record
}

fn base_manifest() -> Value {
    json!({
        "schema": SCHEMA_NAME,
        "schema_version": 1,
        "run_id": "synthetic-parser-p05-run-001",
        "identity": {
            "packet_id": "parser-p05-synthetic-001",
            "packet_digest": "0",
            "tree_sha": TREE_SHA_PLACEHOLDER,
            "spec_ref": "https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11024",
            "spec_digest": "2222222222222222222222222222222222222222222222222222222222222222",
        },
        "subject": {
            "agent": {"name": "synthetic-agent", "version": "0"},
            "model": {"id": "synthetic-model-x"},
            "tool": {"name": "perl-lsp-xtask", "version": "0"},
            "permissions": {
                "ceiling": ["workspace:read", "workspace:write"],
            },
        },
        "disposition": "completed",
        "events": [
            sample_event(0, "observation", json!({"note": "session opened"})),
            sample_event(1, "tool_call", json!({"tool": "perl-lsp", "op": "hover"})),
            sample_event(2, "tool_result", json!({"ok": true})),
        ],
        "results": [
            {
                "seq": 0,
                "kind": "check_result",
                "at_ms": 40,
                "payload": {"check": "hover-support", "outcome": "pass"},
                "digest": "0",
            },
        ],
        "human_intervention": [
            {
                "before_seq": 2,
                "role": "human_operator",
                "reason": "operator approved write scope mid-run",
            },
        ],
        "metadata": {"requested_by": "#11024 first slice"},
    })
}

fn stamped(base: Value) -> Value {
    let mut doc = base;
    stamp_manifest(&mut doc).expect("stamp succeeds");
    doc
}

/// Mutate `doc` at pointer with `f`, returning violations of the mutant.
fn mutant<F: FnOnce(&mut Value)>(base: Value, f: F) -> Vec<Violation> {
    let mut doc = stamped(base);
    f(&mut doc);
    validate_manifest(&doc)
}

fn assert_contains(violations: &[Violation], expected: &str) {
    let codes = violation_codes(violations);
    assert!(codes.contains(&expected), "expected reason code {expected}, got {codes:?}");
}

// ---------------------------------------------------------------------------
// Fail-closed negative controls (the red-first entry proofs).
// ---------------------------------------------------------------------------

#[test]
fn negative_missing_packet_digest_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["identity"].as_object_mut().unwrap().remove("packet_digest");
    });
    assert_contains(&violations, "missing_identity_field");
}

#[test]
fn negative_missing_subject_metadata_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut().unwrap().remove("subject");
    });
    assert_contains(&violations, "missing_subject");
}

#[test]
fn negative_missing_model_identity_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["subject"].as_object_mut().unwrap().remove("model");
    });
    assert_contains(&violations, "missing_subject_field");
}

#[test]
fn negative_dropped_scope_ceiling_fails_closed() {
    let dropped = mutant(base_manifest(), |doc| {
        doc["subject"]["permissions"].as_object_mut().unwrap().remove("ceiling");
    });
    assert_contains(&dropped, "missing_scope_ceiling");

    let emptied = mutant(base_manifest(), |doc| {
        doc["subject"]["permissions"]["ceiling"] = json!([]);
    });
    assert_contains(&emptied, "missing_scope_ceiling");

    let no_permissions_at_all = mutant(base_manifest(), |doc| {
        doc["subject"].as_object_mut().unwrap().remove("permissions");
    });
    assert_contains(&no_permissions_at_all, "missing_scope_ceiling");
}

#[test]
fn negative_tampered_event_payload_mid_run_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][1]["payload"]["op"] = json!("didOpen-tampered-after-stamping");
    });
    assert_contains(&violations, "record_digest_mismatch");
}

#[test]
fn negative_tampered_packet_envelope_mid_run_fails_closed() {
    // Records restamp cleanly but the envelope recorded in the manifest was
    // captured before an attacker reordered the observable history: the
    // recomputed envelope must diverge from identity.packet_digest.
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][0].as_object_mut().unwrap().insert("kind".to_string(), json!("error"));
        let events = doc["events"].as_array().unwrap();
        let result_digests = vec![Some(doc["results"][0]["digest"].as_str().unwrap().to_string())];
        let run_id = doc["run_id"].as_str().unwrap().to_string();
        let mut recomputed_events = Vec::new();
        for event in events {
            recomputed_events.push(Some(
                record_digest(event.as_object().unwrap()).expect("record fields present"),
            ));
        }
        let honest_envelope =
            envelope_digest(&run_id, "completed", &recomputed_events, &result_digests);
        doc["identity"]["packet_digest"] = json!(honest_envelope);
        // Now mutate what the envelope covers WITHOUT restamping again:
        doc["disposition"] = json!("refused");
    });
    assert_contains(&violations, "packet_digest_mismatch");
}

#[test]
fn negative_unsorted_event_sequences_fail_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][2]["seq"] = json!(5);
    });
    assert_contains(&violations, "event_seq_not_contiguous");
}

#[test]
fn negative_oversized_excerpt_fails_closed() {
    let huge = "x".repeat(MAX_RECORD_BYTES * 3);
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][0]["payload"] = json!({"log": huge});
    });
    assert_contains(&violations, "retention_bound_exceeded");
}

#[test]
fn negative_credential_in_payload_fails_closed() {
    for leaked in ["api_key=hunter2", "-----BEGIN OPENSSH PRIVATE KEY-----"] {
        let violations = mutant(base_manifest(), |doc| {
            doc["events"][0]["payload"] = json!({"note": leaked});
        });
        assert_contains(&violations, "credential_in_payload");
    }
}

#[test]
fn negative_machine_local_path_in_payload_fails_closed() {
    for leaked in [
        "C:\\Users\\dev\\secret.log",
        "F:\\Temp\\raw-dump.txt",
        "/home/dev/.ssh/id_rsa.pub",
        "%USERPROFILE%\\notes.md",
    ] {
        let violations = mutant(base_manifest(), |doc| {
            doc["events"][0]["payload"] = json!({"path_note": leaked});
        });
        assert_contains(&violations, "local_path_in_payload");
    }
}

#[test]
fn negative_chain_of_thought_in_payload_fails_closed() {
    for key in ["thinking", "chain_of_thought", "scratchpad"] {
        let violations = mutant(base_manifest(), |doc| {
            doc["events"][0]["payload"] = json!({ key: "hidden reasoning text" });
        });
        assert_contains(&violations, "cot_key_in_payload");
    }
}

#[test]
fn negative_unknown_root_field_domain_widening_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut()
            .unwrap()
            .insert("distribution_overlay".to_string(), json!({"extra": true}));
    });
    assert_contains(&violations, "unknown_field");
}

#[test]
fn negative_mutable_live_state_embedded_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["metadata"]
            .as_object_mut()
            .unwrap()
            .insert("lease".to_string(), json!({"owner": "runtime"}));
    });
    assert_contains(&violations, "mutable_state_embedded");
}

#[test]
fn negative_unknown_disposition_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["disposition"] = json!("auto_merged");
    });
    assert_contains(&violations, "unknown_disposition");
}

#[test]
fn negative_dangling_intervention_reference_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["human_intervention"][0]["before_seq"] = json!(99);
    });
    assert_contains(&violations, "intervention_seq_unknown");
}

#[test]
fn negative_result_sequence_must_be_contiguous() {
    let violations = mutant(base_manifest(), |doc| {
        doc["results"][0]["seq"] = json!(7);
    });
    assert_contains(&violations, "event_seq_not_contiguous");
}

// ---------------------------------------------------------------------------
// Positive paths and determinism.
// ---------------------------------------------------------------------------

#[test]
fn positive_well_formed_synthetic_packets_validate() {
    let completed = stamped(base_manifest());
    assert!(
        validate_manifest(&completed).is_empty(),
        "stamped completed packet must satisfy the closed core: {:?}",
        violation_codes(&validate_manifest(&completed))
    );

    // A refused transfer run with zero results still validates.
    let mut refused = base_manifest();
    refused["disposition"] = json!("transferred");
    refused["run_id"] = json!("synthetic-refused-transfer-002");
    refused.as_object_mut().unwrap().remove("results");
    let refused = stamped(refused);
    assert!(
        validate_manifest(&refused).is_empty(),
        "stamped transferred packet must satisfy the closed core: {:?}",
        violation_codes(&validate_manifest(&refused))
    );
}

#[test]
fn digests_recompute_stably_and_diverge_on_content_change() {
    let doc = stamped(base_manifest());
    let first = record_digest(doc["events"][0].as_object().unwrap()).expect("fields present");
    let second = record_digest(doc["events"][0].as_object().unwrap()).expect("fields present");
    assert_eq!(first, second, "recomputed digest must be stable");
    assert_eq!(first.len(), 64);
    assert!(
        first.bytes().all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase()),
        "digests are lowercase hex"
    );

    let mut changed = doc.clone();
    changed["events"][0]["payload"]["note"] = json!("different observation");
    let third =
        record_digest(changed["events"][0].as_object_mut().unwrap()).expect("fields present");
    assert_ne!(first, third, "content change must change the digest");
}

#[test]
fn stamping_is_idempotent() {
    let once = stamped(base_manifest());
    let mut twice = once.clone();
    stamp_manifest(&mut twice).expect("re-stamp succeeds");
    assert_eq!(canonical_form(&once), canonical_form(&twice));
}

#[test]
fn metadata_is_non_semantic_for_the_integrity_envelope() {
    let mut enriched = base_manifest();
    enriched["metadata"] = json!({
        "generated_at_utc": "2026-08-27T00:00:00Z",
        "extra_caller_note": {"nested": [1, 2, 3]},
    });
    let enriched = stamped(enriched);
    let plain = stamped(base_manifest());
    assert_eq!(
        enriched["identity"]["packet_digest"], plain["identity"]["packet_digest"],
        "metadata must not enter the canonical integrity envelope"
    );
}

#[test]
fn advisory_report_ordering_is_content_sorted_not_enumeration_sorted() {
    let mut entries = Vec::new();
    for index in 0..4 {
        let mut doc = base_manifest();
        doc["run_id"] = json!(format!("synthetic-run-{index:03}"));
        entries.push((format!("zz-source-{index}.json"), doc));
    }
    let forward = collect_rows(&entries);
    let mut reversed_entries = entries.clone();
    reversed_entries.reverse();

    let forward_rendered = render_report(&forward, DogfoodReportFormat::Markdown);
    let reverse_rendered =
        render_report(&collect_rows(&reversed_entries), DogfoodReportFormat::Markdown);
    assert_eq!(
        forward_rendered, reverse_rendered,
        "report must be sorted by content, not caller enumeration order"
    );
    let second_rendered = render_report(&forward, DogfoodReportFormat::Markdown);
    assert_eq!(forward_rendered, second_rendered, "rendering must be deterministic");

    let json_forward = render_report(&forward, DogfoodReportFormat::Json);
    let parsed: Value = serde_json::from_str(&json_forward).expect("valid JSON report");
    assert_eq!(parsed["report"], json!("agent-packet-dogfood.core.report.v1"));
    let runs = parsed["runs"].as_array().expect("runs array");
    let ids: Vec<&str> =
        runs.iter().filter_map(|r| r.get("run_id").and_then(Value::as_str)).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids, "JSON projection lists runs in sorted order");
}

#[test]
fn invalid_runs_render_as_invalid_without_panicking() {
    let mut broken = base_manifest();
    broken.as_object_mut().unwrap().remove("subject");
    let rows = collect_rows(vec![("broken.json".to_string(), broken)].as_slice());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].validity, "invalid");
}

// ---------------------------------------------------------------------------
// Repository-contract level proofs (fixtures + schema self-check).
// ---------------------------------------------------------------------------

#[test]
fn committed_schema_pins_module_vocabulary() {
    let root = project_root().expect("project root resolves");
    let violations = validate_schema_file(&root).expect("schema readable");
    assert!(
        violations.is_empty(),
        "committed schema drifted from the pinned closed vocabulary: {violations:?}"
    );
}

#[test]
fn every_pinned_mutant_has_a_committed_fixture_with_matching_expectation() {
    let root = project_root().expect("project root resolves");
    let expected_path = root.join(FIXTURE_DIR).join(INVALID_DIR).join("expected_errors.json");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&expected_path)
            .with_context(|| format!("reading {}", expected_path.display()))
            .expect("expected_errors.json readable"),
    )
    .expect("expected_errors.json parses");
    let map = expected.as_object().expect("object map");

    for (name, code) in EXPECTED_INVALID {
        let path = root.join(FIXTURE_DIR).join(INVALID_DIR).join(name);
        assert!(
            path.exists(),
            "mutant fixture {} is committed (red-first: matrix complete before rules land)",
            path.display()
        );
        let actual = map
            .get(*name)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name} is listed in expected_errors.json"));
        assert_eq!(actual, *code, "{name} expectation stays aligned with the pinned vocabulary");
        let doc = load_manifest(&path).expect("fixture parses").1;
        assert!(
            doc.get("schema").and_then(Value::as_str) == Some(SCHEMA_NAME),
            "{name} declares the shared core schema name"
        );
    }
}
