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
    ("missing_model_revision.json", "missing_subject_field"),
    ("missing_scope_ceiling.json", "missing_scope_ceiling"),
    ("tampered_event_payload.json", "record_digest_mismatch"),
    ("tampered_packet_envelope.json", "packet_digest_mismatch"),
    ("tampered_subject_ceiling.json", "packet_digest_mismatch"),
    ("tampered_tree_sha.json", "packet_digest_mismatch"),
    ("tampered_model_identity.json", "packet_digest_mismatch"),
    ("unsorted_event_sequences.json", "event_seq_not_contiguous"),
    ("unsorted_result_sequences.json", "result_seq_not_contiguous"),
    ("empty_events.json", "missing_events"),
    ("results_not_array.json", "not_an_object"),
    ("oversized_event_excerpt.json", "retention_bound_exceeded"),
    ("credential_in_payload.json", "credential_in_payload"),
    ("credential_in_metadata.json", "credential_in_payload"),
    ("structured_credential_keys.json", "credential_in_payload"),
    ("chain_of_thought_in_metadata.json", "cot_key_in_payload"),
    ("mutable_state_camel_case.json", "mutable_state_embedded"),
    ("invalid_run_id.json", "credential_in_payload"),
    ("metadata_not_object.json", "not_an_object"),
    ("machine_local_path_in_payload.json", "local_path_in_payload"),
    ("machine_local_path_in_subject.json", "local_path_in_payload"),
    ("chain_of_thought_in_payload.json", "cot_key_in_payload"),
    ("nested_chain_of_thought_in_payload.json", "cot_key_in_payload"),
    ("malformed_packet_digest.json", "malformed_digest"),
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
            "model": {"id": "synthetic-model-x", "revision": "r0"},
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
        let identity = doc["identity"].as_object().expect("identity object");
        let honest_envelope = envelope_digest(
            &envelope_identity(identity),
            &doc["subject"],
            &run_id,
            "completed",
            &recomputed_events,
            &result_digests,
            &doc["human_intervention"],
        );
        doc["identity"]["packet_digest"] = json!(honest_envelope);
        // Now mutate what the envelope covers WITHOUT restamping again:
        doc["disposition"] = json!("refused");
    });
    assert_contains(&violations, "packet_digest_mismatch");
}

#[test]
fn negative_tampered_subject_ceiling_mid_run_fails_closed() {
    // The envelope binds the complete subject metadata: widening the
    // permission scope ceiling after stamping must invalidate the packet.
    let violations = mutant(base_manifest(), |doc| {
        doc["subject"]["permissions"]["ceiling"] =
            json!(["workspace:read", "workspace:write", "network:any"]);
    });
    assert_contains(&violations, "packet_digest_mismatch");
}

#[test]
fn negative_tampered_tree_sha_mid_run_fails_closed() {
    // The envelope binds packet/tree/spec identity: swapping the source tree
    // after stamping must invalidate the packet.
    let violations = mutant(base_manifest(), |doc| {
        doc["identity"]["tree_sha"] =
            json!("3333333333333333333333333333333333333333333333333333333333333333");
    });
    assert_contains(&violations, "packet_digest_mismatch");
}

#[test]
fn negative_tampered_model_identity_mid_run_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["subject"]["model"]["id"] = json!("shadow-model-y");
    });
    assert_contains(&violations, "packet_digest_mismatch");
}

#[test]
fn negative_tampered_intervention_is_bound_to_packet_digest() {
    let violations = mutant(base_manifest(), |doc| {
        doc["human_intervention"][0]["reason"] = json!("operator approved a different scope");
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
fn negative_result_sequence_must_be_contiguous() {
    // Result records carry their own sequence space and their own reason
    // code: a consumer reading the code alone can tell which space failed.
    let violations = mutant(base_manifest(), |doc| {
        doc["results"][0]["seq"] = json!(7);
    });
    assert_contains(&violations, "result_seq_not_contiguous");
}

#[test]
fn negative_empty_required_events_fail_closed() {
    // `events: []` is a purported observable run with no observations: it
    // contradicts the schema's minItems: 1 and must fail closed.
    let violations = mutant(base_manifest(), |doc| {
        doc["events"] = json!([]);
    });
    assert_contains(&violations, "missing_events");
}

#[test]
fn negative_present_non_array_results_fail_closed() {
    // A present `results` field with a non-array value must not pass as an
    // absent optional field.
    let violations = mutant(base_manifest(), |doc| {
        doc["results"] = json!({"seq": 0});
    });
    assert_contains(&violations, "not_an_object");

    let non_array_events = mutant(base_manifest(), |doc| {
        doc["events"] = json!({"seq": 0});
    });
    assert_contains(&non_array_events, "not_an_object");
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
fn negative_credential_outside_payload_fails_closed() {
    // Hygiene scans the complete retained document: a credential in allowed
    // metadata no longer survives stamping or validation.
    let violations = mutant(base_manifest(), |doc| {
        doc["metadata"] = json!({"debug": "api_key=hunter2"});
    });
    assert_contains(&violations, "credential_in_payload");
}

#[test]
fn negative_metadata_must_match_the_schema_object_boundary() {
    let violations = mutant(base_manifest(), |doc| {
        doc["metadata"] = json!("caller metadata");
    });
    assert_contains(&violations, "not_an_object");
}

#[test]
fn negative_structured_credential_key_fails_closed() {
    for key in [
        "api_key",
        "accessToken",
        "clientSecret",
        "credentials",
        "tokenValue",
        "clientSecretValue",
        "apiKeyId",
        "APIKeyId",
        "nestedTokenValue",
        "credentialRef",
        "api.key",
        "api/key",
        "client.secret",
        "private/key",
    ] {
        let violations = mutant(base_manifest(), |doc| {
            doc["metadata"] = json!({"nested": [{ key: "hunter2" }]});
        });
        assert_contains(&violations, "credential_in_payload");
    }
}

#[test]
fn normalized_credential_keys_require_segment_boundaries() {
    for key in [
        "tokenValue",
        "clientSecretValue",
        "apiKeyId",
        "outer_token_value",
        "api.key",
        "api/key",
        "API.Key",
    ] {
        assert!(is_credential_key(key), "{key} must be rejected as a credential key");
    }
    for key in ["tokenized", "secretary", "credentialish"] {
        assert!(!is_credential_key(key), "{key} is not a credential-key segment");
    }
}

#[test]
fn negative_missing_model_revision_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["subject"]["model"].as_object_mut().unwrap().remove("revision");
    });
    assert_contains(&violations, "missing_subject_field");
}

#[test]
fn negative_present_model_revision_must_be_non_empty_string() {
    for revision in [json!(""), json!(42), Value::Null] {
        let violations = mutant(base_manifest(), |doc| {
            doc["subject"]["model"]["revision"] = revision;
        });
        assert_contains(&violations, "malformed_subject_field");
    }
}

#[test]
fn negative_machine_local_path_in_payload_fails_closed() {
    for leaked in [
        "C:\\Users\\dev\\secret.log",
        "F:\\Temp\\raw-dump.txt",
        "C:/Users/dev/secret.log",
        "D:/Temp/raw-dump.txt",
        "/home/dev/.ssh/id_rsa.pub",
        "/tmp/agent-secret.json",
        "/var/run/agent.sock",
        "/opt/perl-lsp/config.json",
        "/usr/local/lib/perl",
        "\\\\build-server\\share\\secret.log",
        "//build-server/share/secret.log",
        "//build-server.example/share/secret.log",
        "file:///tmp/agent-secret.json",
        "../private/secret.json",
        "..\\private\\secret.json",
        "%USERPROFILE%\\notes.md",
    ] {
        let violations = mutant(base_manifest(), |doc| {
            doc["events"][0]["payload"] = json!({"path_note": leaked});
        });
        assert_contains(&violations, "local_path_in_payload");
    }
}

#[test]
fn positive_uri_text_is_not_misclassified_as_a_drive_path() {
    let violations = mutant(base_manifest(), |doc| {
        doc["metadata"] = json!({
            "documentation": "https://example.test/perl-lsp",
            "endpoint": "http://localhost:3000/status",
            "drive_like_url": "https://example.test/C:/tmp",
            "posix_like_url": "https://example.test/home/dev/docs",
            "traversal_like_url": "https://example.test/../docs",
            "protocol_relative_url": "//example.test/C:/tmp",
        });
    });
    assert!(
        !violations.iter().any(|violation| violation.code == "local_path_in_payload"),
        "URI text must not trigger a local-path violation: {violations:?}"
    );
}

#[test]
fn negative_uri_boundary_does_not_exempt_a_following_local_path() {
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][0]["payload"] = json!({
            "message": "https://example.test/docs /tmp/secret"
        });
    });
    assert_contains(&violations, "local_path_in_payload");
}

#[test]
fn negative_machine_local_path_outside_payload_fails_closed() {
    // Subject identity strings are part of the retained document too.
    let violations = mutant(base_manifest(), |doc| {
        doc["subject"]["tool"]["name"] = json!("/home/dev/perl-lsp-xtask");
    });
    assert_contains(&violations, "local_path_in_payload");
}

#[test]
fn negative_caller_controlled_diagnostic_path_is_not_echoed() {
    let leaked = "C:/Users/dev/api_key=hunter2";
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut().unwrap().insert(leaked.to_string(), json!(true));
    });
    assert_contains(&violations, "unknown_field");
    let details = violations.iter().map(|violation| violation.detail.as_str()).collect::<Vec<_>>();
    assert!(details.iter().all(|detail| !detail.contains(leaked)));
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
fn negative_chain_of_thought_keys_are_normalized_across_metadata_and_payload() {
    for (location, key) in [
        ("metadata", "chain_of_thought"),
        ("metadata", "chainOfThought"),
        ("events", "chain_of_thought"),
        ("events", "chainOfThought"),
    ] {
        let violations = mutant(base_manifest(), |doc| {
            if location == "metadata" {
                doc["metadata"] = json!({key: "hidden reasoning text"});
            } else {
                doc["events"][0]["payload"] = json!({key: "hidden reasoning text"});
            }
        });
        assert_contains(&violations, "cot_key_in_payload");
    }
}

#[test]
fn negative_nested_chain_of_thought_key_fails_closed() {
    // The chain-of-thought key guard recurses: a prohibited key below the
    // payload's outer object (through objects and arrays) is still rejected.
    let violations = mutant(base_manifest(), |doc| {
        doc["events"][0]["payload"] =
            json!({"nested": {"deep": [{"thinking": "secret reasoning"}]}});
    });
    assert_contains(&violations, "cot_key_in_payload");
}

#[test]
fn negative_malformed_packet_digest_fails_closed() {
    // The digest charset is strict lowercase hex: g-z (or uppercase) is
    // malformed, not a plausible digest.
    let violations = mutant(base_manifest(), |doc| {
        doc["identity"]["packet_digest"] = json!("g".repeat(64));
    });
    assert_contains(&violations, "malformed_digest");
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
fn negative_camel_case_mutable_live_state_embedded_fails_closed() {
    let violations = mutant(base_manifest(), |doc| {
        doc["metadata"] = json!({"wakeEvent": "checks complete", "leaseOwner": "agent"});
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

// ---------------------------------------------------------------------------
// Stamp honesty: stamping never reports success without writing a digest.
// ---------------------------------------------------------------------------

#[test]
fn stamp_requires_semantic_envelope_inputs() {
    let mut without_identity = base_manifest();
    without_identity.as_object_mut().unwrap().remove("identity");
    assert!(stamp_manifest(&mut without_identity).is_err(), "missing identity must error");

    let mut without_run_id = base_manifest();
    without_run_id.as_object_mut().unwrap().remove("run_id");
    assert!(stamp_manifest(&mut without_run_id).is_err(), "missing run_id must error");

    let mut without_disposition = base_manifest();
    without_disposition.as_object_mut().unwrap().remove("disposition");
    assert!(stamp_manifest(&mut without_disposition).is_err(), "missing disposition must error");

    let mut without_subject = base_manifest();
    without_subject.as_object_mut().unwrap().remove("subject");
    assert!(stamp_manifest(&mut without_subject).is_err(), "missing subject must error");

    let mut non_array_events = base_manifest();
    non_array_events["events"] = json!("not-an-array");
    assert!(stamp_manifest(&mut non_array_events).is_err(), "non-array events must error");

    let mut non_array_results = base_manifest();
    non_array_results["results"] = json!({"oops": true});
    assert!(stamp_manifest(&mut non_array_results).is_err(), "non-array results must error");

    // A document that failed to stamp carries no fabricated digest.
    let before = without_identity.clone();
    let mut after = without_identity;
    let _ = stamp_manifest(&mut after);
    assert_eq!(
        after["identity"].get("packet_digest"),
        before["identity"].get("packet_digest"),
        "a failed stamp must not write a packet digest"
    );
}

#[test]
fn stamp_rejects_unsafe_manifests_before_mutating_them() {
    let mut credential_manifest = base_manifest();
    credential_manifest["metadata"] = json!({"accessToken": "should-not-persist"});
    let credential_before = credential_manifest.clone();
    assert!(
        stamp_manifest(&mut credential_manifest).is_err(),
        "credential-bearing metadata must fail closed before stamping"
    );
    assert_eq!(credential_manifest, credential_before);

    let mut missing_revision = base_manifest();
    missing_revision["subject"]["model"].as_object_mut().unwrap().remove("revision");
    let missing_revision_before = missing_revision.clone();
    assert!(
        stamp_manifest(&mut missing_revision).is_err(),
        "missing model revision must fail closed before stamping"
    );
    assert_eq!(missing_revision, missing_revision_before);
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
    assert!(is_lowercase_sha256_hex(&first), "digests are strict lowercase 64-hex");

    let mut changed = doc.clone();
    changed["events"][0]["payload"]["note"] = json!("different observation");
    let third =
        record_digest(changed["events"][0].as_object_mut().unwrap()).expect("fields present");
    assert_ne!(first, third, "content change must change the digest");
}

#[test]
fn digest_charset_is_strict_lowercase_hex() {
    assert!(is_lowercase_sha256_hex(&"0123456789abcdef".repeat(4)));
    assert!(is_lowercase_sha256_hex(&"a".repeat(64)));
    assert!(!is_lowercase_sha256_hex(&"g".repeat(64)), "g-z are not hex");
    assert!(!is_lowercase_sha256_hex(&"A".repeat(64)), "uppercase is not canonical");
    assert!(!is_lowercase_sha256_hex(&format!("{}g", "a".repeat(63))), "64 chars, non-hex tail");
    assert!(!is_lowercase_sha256_hex("abcd"), "wrong length");
}

#[test]
fn canonical_form_is_key_order_insensitive_array_order_sensitive() {
    let inserted_one_way = json!({"b": 1, "a": {"z": 2, "y": [3, 4]}});
    let inserted_other_way = json!({"a": {"y": [3, 4], "z": 2}, "b": 1});
    assert_eq!(
        canonical_form(&inserted_one_way),
        canonical_form(&inserted_other_way),
        "equal documents must hash equally regardless of key insertion order"
    );

    let ascending = json!({"arr": [1, 2]});
    let descending = json!({"arr": [2, 1]});
    assert_ne!(
        canonical_form(&ascending),
        canonical_form(&descending),
        "array element order stays semantic input"
    );
}

#[test]
fn record_digest_is_key_order_insensitive() {
    let insertion_one: Map<String, Value> =
        json!({"seq": 0, "kind": "observation", "at_ms": 0, "payload": {"x": 1, "y": 2}})
            .as_object()
            .expect("object")
            .clone();
    let insertion_other: Map<String, Value> =
        json!({"payload": {"y": 2, "x": 1}, "at_ms": 0, "kind": "observation", "seq": 0})
            .as_object()
            .expect("object")
            .clone();
    assert_eq!(
        record_digest(&insertion_one),
        record_digest(&insertion_other),
        "record digests must not depend on object key insertion order"
    );
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

#[test]
fn invalid_run_id_is_redacted_from_rendered_reports() {
    let mut invalid = stamped(base_manifest());
    invalid["run_id"] = json!("api_key=hunter2");
    let rows = collect_rows(vec![("invalid.json".to_string(), invalid)].as_slice());
    let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
    let json = render_report(&rows, DogfoodReportFormat::Json);
    assert!(!markdown.contains("api_key=hunter2"), "invalid run_id leaked in markdown report");
    assert!(!json.contains("api_key=hunter2"), "invalid run_id leaked in JSON report");
    assert!(markdown.contains("<redacted-invalid-run-id>"));
    assert!(json.contains("<redacted-invalid-run-id>"));
}

#[test]
fn invalid_disposition_is_redacted_while_valid_disposition_is_preserved() {
    let valid = stamped(base_manifest());
    let mut invalid = stamped(base_manifest());
    invalid["disposition"] = json!("api_key=hunter2");
    let entries = vec![("valid.json".to_string(), valid), ("invalid.json".to_string(), invalid)];
    let rows = collect_rows(&entries);
    let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
    let json = render_report(&rows, DogfoodReportFormat::Json);

    assert!(!markdown.contains("api_key=hunter2"), "invalid disposition leaked in markdown report");
    assert!(!json.contains("api_key=hunter2"), "invalid disposition leaked in JSON report");
    assert!(
        markdown.contains("| completed | valid |"),
        "valid disposition changed in markdown report"
    );

    let report: Value = serde_json::from_str(&json).expect("valid JSON report");
    let runs = report["runs"].as_array().expect("runs array");
    assert!(runs
        .iter()
        .any(|run| { run["disposition"] == "completed" && run["validity"] == "valid" }));
    assert!(runs.iter().any(|run| {
        run["disposition"] == "<redacted-invalid-disposition>" && run["validity"] == "invalid"
    }));
}

#[test]
fn every_closed_disposition_is_preserved_in_both_report_projections() {
    for disposition in DISPOSITIONS {
        let mut doc = base_manifest();
        doc["disposition"] = json!(*disposition);
        let doc = stamped(doc);
        let entries = vec![(format!("{disposition}.json"), doc)];
        let rows = collect_rows(&entries);

        assert!(rows[0].violations.is_empty(), "{disposition} must remain valid");
        let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
        assert!(
            markdown.contains(&format!("| {disposition} | valid |")),
            "Markdown report must preserve {disposition}: {markdown}"
        );

        let json = render_report(&rows, DogfoodReportFormat::Json);
        let report: Value = serde_json::from_str(&json).expect("JSON report is valid");
        assert_eq!(report["runs"][0]["disposition"], json!(*disposition));
        assert_eq!(report["runs"][0]["validity"], json!("valid"));
    }
}

#[test]
fn unknown_disposition_violation_does_not_retain_the_untrusted_value() {
    let leaked = "api_key=hunter2";
    let violations = mutant(base_manifest(), |doc| {
        doc["disposition"] = json!(leaked);
    });
    let violation = violations
        .iter()
        .find(|violation| violation.code == "unknown_disposition")
        .expect("unknown disposition must be reported");
    assert!(!violation.detail.contains(leaked), "validator detail leaked {leaked}");
    assert_eq!(violation.detail, "manifest: unknown disposition (value redacted)");
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

#[test]
fn expected_errors_file_is_exactly_the_pinned_matrix() {
    let root = project_root().expect("project root resolves");
    let expected_path = root.join(FIXTURE_DIR).join(INVALID_DIR).join("expected_errors.json");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&expected_path).expect("expected_errors.json readable"),
    )
    .expect("expected_errors.json parses");
    let map = expected.as_object().expect("object map");
    assert_eq!(
        map.len(),
        EXPECTED_INVALID.len(),
        "expected_errors.json must pin exactly the committed matrix"
    );
    for name in map.keys() {
        assert!(
            EXPECTED_INVALID.iter().any(|(pinned, _)| pinned == name),
            "expected_errors.json entry {name} is not pinned in EXPECTED_INVALID"
        );
    }
}

#[test]
fn invalid_directory_is_exactly_the_pinned_matrix() {
    let root = project_root().expect("project root resolves");
    let invalid_dir = root.join(FIXTURE_DIR).join(INVALID_DIR);
    let mut on_disk: Vec<String> = std::fs::read_dir(&invalid_dir)
        .expect("invalid fixture dir readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name != "expected_errors.json")
        .collect();
    on_disk.sort();
    let mut pinned: Vec<String> =
        EXPECTED_INVALID.iter().map(|(name, _)| (*name).to_string()).collect();
    pinned.sort();
    assert_eq!(on_disk, pinned, "every committed mutant fixture must be pinned, and vice versa");
}

#[test]
fn committed_valid_fixtures_validate_and_match_their_golden_reports() {
    let root = project_root().expect("project root resolves");
    let fixture_dir = root.join(FIXTURE_DIR);
    let mut entries = Vec::new();
    for name in VALID_FIXTURES {
        let (source, doc) = load_manifest(&fixture_dir.join(name)).expect("fixture parses");
        assert!(
            validate_manifest(&doc).is_empty(),
            "{name} must validate: {:?}",
            violation_codes(&validate_manifest(&doc))
        );
        entries.push((source, doc));
    }
    let rows = collect_rows(&entries);
    for (format, extension) in
        [(DogfoodReportFormat::Markdown, "md"), (DogfoodReportFormat::Json, "json")]
    {
        let path = fixture_dir
            .join(GOLDEN_DIR)
            .join(format!("agent_packet_dogfood_core.advisory.{extension}"));
        let golden = std::fs::read_to_string(&path).expect("golden vector readable");
        assert_eq!(golden, render_report(&rows, format), "{} drifted", path.display());
    }
}
