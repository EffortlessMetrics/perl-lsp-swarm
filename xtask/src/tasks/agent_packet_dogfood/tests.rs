//! Red-first proof seam for the shared dogfood core (#11024 family brief).
//!
//! Every mutated-packet negative control below is written BEFORE the
//! validating rules they exercise land, against synthetic fixtures only.
//! Each control pins one stable reason code; a green run proves the
//! validator rejects each mutant class instead of rendering plausible prose.

use super::*;
use color_eyre::eyre::{ContextCompat, ensure};
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
    let record = json!({
        "seq": seq,
        "kind": kind,
        "payload": payload,
        "digest": "0",
        "at_ms": seq * 10,
    });
    // Stamp computes over {domain, seq, kind, at_ms?, payload}; give the
    // placeholder a fixed at_ms so stamps stay reproducible across edits.
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

fn stamped(base: Value) -> Result<Value> {
    let mut doc = base;
    stamp_manifest(&mut doc).context("stamp succeeds")?;
    Ok(doc)
}

/// Mutate `doc` at pointer with `f`, returning violations of the mutant.
fn mutant<F: FnOnce(&mut Value) -> Result<()>>(base: Value, f: F) -> Result<Vec<Violation>> {
    let mut doc = stamped(base)?;
    f(&mut doc)?;
    Ok(validate_manifest(&doc))
}

fn assert_contains(violations: &[Violation], expected: &str) -> Result<()> {
    let codes = violation_codes(violations);
    ensure!(codes.contains(&expected), "expected reason code {expected}, got {codes:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Fail-closed negative controls (the red-first entry proofs).
// ---------------------------------------------------------------------------

#[test]
fn negative_missing_packet_digest_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/identity")
            .context("missing fixture path /identity")?
            .as_object_mut()
            .context("required fixture value missing")?
            .remove("packet_digest");
        Ok(())
    })?;
    assert_contains(&violations, "missing_identity_field")?;
    Ok(())
}

#[test]
fn negative_missing_subject_metadata_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut().context("required fixture value missing")?.remove("subject");
        Ok(())
    })?;
    assert_contains(&violations, "missing_subject")?;
    Ok(())
}

#[test]
fn negative_missing_model_identity_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/subject")
            .context("missing fixture path /subject")?
            .as_object_mut()
            .context("required fixture value missing")?
            .remove("model");
        Ok(())
    })?;
    assert_contains(&violations, "missing_subject_field")?;
    Ok(())
}

#[test]
fn negative_dropped_scope_ceiling_fails_closed() -> Result<()> {
    let dropped = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/subject/permissions")
            .context("missing fixture path /subject/permissions")?
            .as_object_mut()
            .context("required fixture value missing")?
            .remove("ceiling");
        Ok(())
    })?;
    assert_contains(&dropped, "missing_scope_ceiling")?;

    let emptied = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/subject/permissions/ceiling")
            .context("missing fixture path /subject/permissions/ceiling")? = json!([]);
        Ok(())
    })?;
    assert_contains(&emptied, "missing_scope_ceiling")?;

    let no_permissions_at_all = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/subject")
            .context("missing fixture path /subject")?
            .as_object_mut()
            .context("required fixture value missing")?
            .remove("permissions");
        Ok(())
    })?;
    assert_contains(&no_permissions_at_all, "missing_scope_ceiling")?;
    Ok(())
}

#[test]
fn negative_tampered_event_payload_mid_run_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/1/payload/op")
            .context("missing fixture path /events/1/payload/op")? =
            json!("didOpen-tampered-after-stamping");
        Ok(())
    })?;
    assert_contains(&violations, "record_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_tampered_packet_envelope_mid_run_fails_closed() -> Result<()> {
    // Records restamp cleanly but the envelope recorded in the manifest was
    // captured before an attacker reordered the observable history: the
    // recomputed envelope must diverge from identity.packet_digest.
    let violations = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/events/0")
            .context("missing fixture path /events/0")?
            .as_object_mut()
            .context("required fixture value missing")?
            .insert("kind".to_string(), json!("error"));
        let events = (*doc.pointer("/events").unwrap_or(&Value::Null))
            .as_array()
            .context("required fixture value missing")?;
        let result_digests = vec![Some(
            (*doc.pointer("/results/0/digest").unwrap_or(&Value::Null))
                .as_str()
                .context("required fixture value missing")?
                .to_string(),
        )];
        let run_id = (*doc.pointer("/run_id").unwrap_or(&Value::Null))
            .as_str()
            .context("required fixture value missing")?
            .to_string();
        let mut recomputed_events = Vec::new();
        for event in events {
            recomputed_events.push(Some(
                record_digest(event.as_object().context("required fixture value missing")?)
                    .context("record fields present")?,
            ));
        }
        let identity = (*doc.pointer("/identity").unwrap_or(&Value::Null))
            .as_object()
            .context("identity object")?;
        let honest_envelope = envelope_digest(
            &envelope_identity(identity),
            doc.pointer("/subject").unwrap_or(&Value::Null),
            &run_id,
            "completed",
            &recomputed_events,
            &result_digests,
            doc.pointer("/human_intervention").unwrap_or(&Value::Null),
        );
        *doc.pointer_mut("/identity/packet_digest")
            .context("missing fixture path /identity/packet_digest")? = json!(honest_envelope);
        // Now mutate what the envelope covers WITHOUT restamping again:
        *doc.pointer_mut("/disposition").context("missing fixture path /disposition")? =
            json!("refused");
        Ok(())
    })?;
    assert_contains(&violations, "packet_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_tampered_subject_ceiling_mid_run_fails_closed() -> Result<()> {
    // The envelope binds the complete subject metadata: widening the
    // permission scope ceiling after stamping must invalidate the packet.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/subject/permissions/ceiling")
            .context("missing fixture path /subject/permissions/ceiling")? =
            json!(["workspace:read", "workspace:write", "network:any"]);
        Ok(())
    })?;
    assert_contains(&violations, "packet_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_tampered_tree_sha_mid_run_fails_closed() -> Result<()> {
    // The envelope binds packet/tree/spec identity: swapping the source tree
    // after stamping must invalidate the packet.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/identity/tree_sha")
            .context("missing fixture path /identity/tree_sha")? =
            json!("3333333333333333333333333333333333333333333333333333333333333333");
        Ok(())
    })?;
    assert_contains(&violations, "packet_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_tampered_model_identity_mid_run_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/subject/model/id")
            .context("missing fixture path /subject/model/id")? = json!("shadow-model-y");
        Ok(())
    })?;
    assert_contains(&violations, "packet_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_tampered_intervention_is_bound_to_packet_digest() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/human_intervention/0/reason")
            .context("missing fixture path /human_intervention/0/reason")? =
            json!("operator approved a different scope");
        Ok(())
    })?;
    assert_contains(&violations, "packet_digest_mismatch")?;
    Ok(())
}

#[test]
fn negative_unsorted_event_sequences_fail_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/2/seq").context("missing fixture path /events/2/seq")? = json!(5);
        Ok(())
    })?;
    assert_contains(&violations, "event_seq_not_contiguous")?;
    Ok(())
}

#[test]
fn negative_result_sequence_must_be_contiguous() -> Result<()> {
    // Result records carry their own sequence space and their own reason
    // code: a consumer reading the code alone can tell which space failed.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/results/0/seq").context("missing fixture path /results/0/seq")? =
            json!(7);
        Ok(())
    })?;
    assert_contains(&violations, "result_seq_not_contiguous")?;
    Ok(())
}

#[test]
fn negative_empty_required_events_fail_closed() -> Result<()> {
    // `events: []` is a purported observable run with no observations: it
    // contradicts the schema's minItems: 1 and must fail closed.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events").context("missing fixture path /events")? = json!([]);
        Ok(())
    })?;
    assert_contains(&violations, "missing_events")?;
    Ok(())
}

#[test]
fn negative_present_non_array_results_fail_closed() -> Result<()> {
    // A present `results` field with a non-array value must not pass as an
    // absent optional field.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/results").context("missing fixture path /results")? = json!({"seq": 0});
        Ok(())
    })?;
    assert_contains(&violations, "not_an_object")?;

    let non_array_events = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events").context("missing fixture path /events")? = json!({"seq": 0});
        Ok(())
    })?;
    assert_contains(&non_array_events, "not_an_object")?;
    Ok(())
}

#[test]
fn negative_oversized_excerpt_fails_closed() -> Result<()> {
    let huge = "x".repeat(MAX_RECORD_BYTES * 3);
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/0/payload")
            .context("missing fixture path /events/0/payload")? = json!({"log": huge});
        Ok(())
    })?;
    assert_contains(&violations, "retention_bound_exceeded")?;
    Ok(())
}

#[test]
fn negative_credential_in_payload_fails_closed() -> Result<()> {
    for leaked in ["api_key=hunter2", "-----BEGIN OPENSSH PRIVATE KEY-----"] {
        let violations = mutant(base_manifest(), |doc| {
            *doc.pointer_mut("/events/0/payload")
                .context("missing fixture path /events/0/payload")? = json!({"note": leaked});
            Ok(())
        })?;
        assert_contains(&violations, "credential_in_payload")?;
    }
    Ok(())
}

#[test]
fn negative_credential_outside_payload_fails_closed() -> Result<()> {
    // Hygiene scans the complete retained document: a credential in allowed
    // metadata no longer survives stamping or validation.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? =
            json!({"debug": "api_key=hunter2"});
        Ok(())
    })?;
    assert_contains(&violations, "credential_in_payload")?;
    Ok(())
}

#[test]
fn negative_metadata_must_match_the_schema_object_boundary() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? =
            json!("caller metadata");
        Ok(())
    })?;
    assert_contains(&violations, "not_an_object")?;
    Ok(())
}

#[test]
fn negative_structured_credential_key_fails_closed() -> Result<()> {
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
            *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? =
                json!({"nested": [{ key: "hunter2" }]});
            Ok(())
        })?;
        assert_contains(&violations, "credential_in_payload")?;
    }
    Ok(())
}

#[test]
fn normalized_credential_keys_require_segment_boundaries() -> Result<()> {
    for key in [
        "tokenValue",
        "clientSecretValue",
        "apiKeyId",
        "outer_token_value",
        "api.key",
        "api/key",
        "API.Key",
    ] {
        ensure!(is_credential_key(key), "{key} must be rejected as a credential key");
    }
    for key in ["tokenized", "secretary", "credentialish"] {
        ensure!(!is_credential_key(key), "{key} is not a credential-key segment");
    }
    Ok(())
}

#[test]
fn negative_missing_model_revision_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/subject/model")
            .context("missing fixture path /subject/model")?
            .as_object_mut()
            .context("required fixture value missing")?
            .remove("revision");
        Ok(())
    })?;
    assert_contains(&violations, "missing_subject_field")?;
    Ok(())
}

#[test]
fn negative_present_model_revision_must_be_non_empty_string() -> Result<()> {
    for revision in [json!(""), json!(42), Value::Null] {
        let violations = mutant(base_manifest(), |doc| {
            *doc.pointer_mut("/subject/model/revision")
                .context("missing fixture path /subject/model/revision")? = revision;
            Ok(())
        })?;
        assert_contains(&violations, "malformed_subject_field")?;
    }
    Ok(())
}

#[test]
fn negative_machine_local_path_in_payload_fails_closed() -> Result<()> {
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
        // NOTE: `//dotted-host/C:/...` is deliberately a protocol-relative URL
        // exemption (see `protocol_relative_url_start` and the
        // `positive_uri_text_is_not_misclassified_as_a_drive_path` pin), so
        // those forms must not appear in this reject list. `//cdn/C:/tmp`
        // stays: an undotted host is not a URL and remains UNC evidence.
        "//cdn/C:/tmp",
        "file:///tmp/agent-secret.json",
        "../private/secret.json",
        "..\\private\\secret.json",
        "%USERPROFILE%\\notes.md",
    ] {
        let violations = mutant(base_manifest(), |doc| {
            *doc.pointer_mut("/events/0/payload")
                .context("missing fixture path /events/0/payload")? = json!({"path_note": leaked});
            Ok(())
        })?;
        assert_contains(&violations, "local_path_in_payload")?;
    }
    Ok(())
}

#[test]
fn positive_uri_text_is_not_misclassified_as_a_drive_path() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? = json!({
            "documentation": "https://example.test/perl-lsp",
            "endpoint": "http://localhost:3000/status",
            "drive_like_url": "https://example.test/C:/tmp",
            "posix_like_url": "https://example.test/home/dev/docs",
            "traversal_like_url": "https://example.test/../docs",
            "protocol_relative_drive_like_url": "//example.test/C:/tmp",
        });
        Ok(())
    })?;
    ensure!(
        !violations.iter().any(|violation| violation.code == "local_path_in_payload"),
        "URI text must not trigger a local-path violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn negative_uri_boundary_does_not_exempt_a_following_local_path() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/0/payload")
            .context("missing fixture path /events/0/payload")? = json!({
            "message": "https://example.test/docs /tmp/secret"
        });
        Ok(())
    })?;
    assert_contains(&violations, "local_path_in_payload")?;
    Ok(())
}

#[test]
fn negative_protocol_relative_url_boundary_is_not_a_path_exemption() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/0/payload")
            .context("missing fixture path /events/0/payload")? = json!({
            "message": "//example.test/docs C:/tmp"
        });
        Ok(())
    })?;
    assert_contains(&violations, "local_path_in_payload")?;
    Ok(())
}

#[test]
fn negative_posix_detector_ignores_division_text() -> Result<()> {
    for message in ["ratio / 2", "ratio /2", "text: /v1"] {
        let violations = mutant(base_manifest(), |doc| {
            *doc.pointer_mut("/events/0/payload")
                .context("missing fixture path /events/0/payload")? = json!({"message": message});
            Ok(())
        })?;
        ensure!(
            !violation_codes(&violations).contains(&"local_path_in_payload"),
            "ordinary prose token was classified as a local path: {message:?}"
        );
    }
    Ok(())
}

#[test]
fn negative_posix_detector_rejects_labeled_absolute_paths() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/0/payload")
            .context("missing fixture path /events/0/payload")? =
            json!({"message": "path: /opt/perl-lsp/config.json"});
        Ok(())
    })?;
    assert_contains(&violations, "local_path_in_payload")?;
    Ok(())
}

#[test]
fn duplicate_member_names_are_rejected_before_json_object_collapse() -> Result<()> {
    for text in [
        r#"{"label":1,"label":2}"#,
        r#"{"outer":{"label":1,"label":2}}"#,
        r#"[{"label":1,"label":2}]"#,
        r#"{"la\u0062el":1,"label":2}"#,
    ] {
        let error = scan_raw_manifest(text)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("duplicate member names must be rejected"))?;
        let detail = error.to_string();
        if !detail.contains("duplicate JSON member name") || detail.contains("label") {
            bail!("duplicate-member diagnostic must be generic");
        }
    }
    Ok(())
}

#[test]
fn repeated_member_names_in_distinct_objects_remain_valid() -> Result<()> {
    scan_raw_manifest(r#"{"left":{"label":1},"right":{"label":2}}"#)?;
    scan_raw_manifest(r#"[{"label":1},{"label":2}]"#)?;
    Ok(())
}

#[test]
fn negative_machine_local_path_outside_payload_fails_closed() -> Result<()> {
    // Subject identity strings are part of the retained document too.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/subject/tool/name")
            .context("missing fixture path /subject/tool/name")? =
            json!("/home/dev/perl-lsp-xtask");
        Ok(())
    })?;
    assert_contains(&violations, "local_path_in_payload")?;
    Ok(())
}

#[test]
fn negative_caller_controlled_diagnostic_path_is_not_echoed() -> Result<()> {
    let leaked = "C:/Users/dev/api_key=hunter2";
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut()
            .context("required fixture value missing")?
            .insert(leaked.to_string(), json!(true));
        Ok(())
    })?;
    assert_contains(&violations, "unknown_field")?;
    let details = violations.iter().map(|violation| violation.detail.as_str()).collect::<Vec<_>>();
    ensure!(
        details.iter().all(|detail| !detail.contains(leaked)),
        "test condition failed: {}",
        stringify!(details.iter().all(|detail| !detail.contains(leaked)))
    );
    Ok(())
}

#[test]
fn negative_chain_of_thought_in_payload_fails_closed() -> Result<()> {
    for key in ["thinking", "chain_of_thought", "scratchpad"] {
        let violations = mutant(base_manifest(), |doc| {
            *doc.pointer_mut("/events/0/payload")
                .context("missing fixture path /events/0/payload")? =
                json!({ key: "hidden reasoning text" });
            Ok(())
        })?;
        assert_contains(&violations, "cot_key_in_payload")?;
    }
    Ok(())
}

#[test]
fn negative_chain_of_thought_keys_are_normalized_across_metadata_and_payload() -> Result<()> {
    for (location, key) in [
        ("metadata", "chain_of_thought"),
        ("metadata", "chainOfThought"),
        ("events", "chain_of_thought"),
        ("events", "chainOfThought"),
    ] {
        let violations = mutant(base_manifest(), |doc| {
            if location == "metadata" {
                *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? =
                    json!({key: "hidden reasoning text"});
            } else {
                *doc.pointer_mut("/events/0/payload")
                    .context("missing fixture path /events/0/payload")? =
                    json!({key: "hidden reasoning text"});
            }
            Ok(())
        })?;
        assert_contains(&violations, "cot_key_in_payload")?;
    }
    Ok(())
}

#[test]
fn negative_nested_chain_of_thought_key_fails_closed() -> Result<()> {
    // The chain-of-thought key guard recurses: a prohibited key below the
    // payload's outer object (through objects and arrays) is still rejected.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/events/0/payload")
            .context("missing fixture path /events/0/payload")? =
            json!({"nested": {"deep": [{"thinking": "secret reasoning"}]}});
        Ok(())
    })?;
    assert_contains(&violations, "cot_key_in_payload")?;
    Ok(())
}

#[test]
fn negative_malformed_packet_digest_fails_closed() -> Result<()> {
    // The digest charset is strict lowercase hex: g-z (or uppercase) is
    // malformed, not a plausible digest.
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/identity/packet_digest")
            .context("missing fixture path /identity/packet_digest")? = json!("g".repeat(64));
        Ok(())
    })?;
    assert_contains(&violations, "malformed_digest")?;
    Ok(())
}

#[test]
fn negative_unknown_root_field_domain_widening_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.as_object_mut()
            .context("required fixture value missing")?
            .insert("distribution_overlay".to_string(), json!({"extra": true}));
        Ok(())
    })?;
    assert_contains(&violations, "unknown_field")?;
    Ok(())
}

#[test]
fn negative_mutable_live_state_embedded_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        doc.pointer_mut("/metadata")
            .context("missing fixture path /metadata")?
            .as_object_mut()
            .context("required fixture value missing")?
            .insert("lease".to_string(), json!({"owner": "runtime"}));
        Ok(())
    })?;
    assert_contains(&violations, "mutable_state_embedded")?;
    Ok(())
}

#[test]
fn negative_camel_case_mutable_live_state_embedded_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/metadata").context("missing fixture path /metadata")? =
            json!({"wakeEvent": "checks complete", "leaseOwner": "agent"});
        Ok(())
    })?;
    assert_contains(&violations, "mutable_state_embedded")?;
    Ok(())
}

#[test]
fn negative_unknown_disposition_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/disposition").context("missing fixture path /disposition")? =
            json!("auto_merged");
        Ok(())
    })?;
    assert_contains(&violations, "unknown_disposition")?;
    Ok(())
}

#[test]
fn negative_dangling_intervention_reference_fails_closed() -> Result<()> {
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/human_intervention/0/before_seq")
            .context("missing fixture path /human_intervention/0/before_seq")? = json!(99);
        Ok(())
    })?;
    assert_contains(&violations, "intervention_seq_unknown")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stamp honesty: stamping never reports success without writing a digest.
// ---------------------------------------------------------------------------

#[test]
fn stamp_requires_semantic_envelope_inputs() -> Result<()> {
    let mut without_identity = base_manifest();
    without_identity.as_object_mut().context("required fixture value missing")?.remove("identity");
    ensure!(stamp_manifest(&mut without_identity).is_err(), "missing identity must error");

    let mut without_run_id = base_manifest();
    without_run_id.as_object_mut().context("required fixture value missing")?.remove("run_id");
    ensure!(stamp_manifest(&mut without_run_id).is_err(), "missing run_id must error");

    let mut without_disposition = base_manifest();
    without_disposition
        .as_object_mut()
        .context("required fixture value missing")?
        .remove("disposition");
    ensure!(stamp_manifest(&mut without_disposition).is_err(), "missing disposition must error");

    let mut without_subject = base_manifest();
    without_subject.as_object_mut().context("required fixture value missing")?.remove("subject");
    ensure!(stamp_manifest(&mut without_subject).is_err(), "missing subject must error");

    let mut non_array_events = base_manifest();
    *non_array_events.pointer_mut("/events").context("missing fixture path /events")? =
        json!("not-an-array");
    ensure!(stamp_manifest(&mut non_array_events).is_err(), "non-array events must error");

    let mut non_array_results = base_manifest();
    *non_array_results.pointer_mut("/results").context("missing fixture path /results")? =
        json!({"oops": true});
    ensure!(stamp_manifest(&mut non_array_results).is_err(), "non-array results must error");

    // A document that failed to stamp carries no fabricated digest.
    let before = without_identity.clone();
    let mut after = without_identity;
    let _ = stamp_manifest(&mut after);
    ensure!(
        ((*after.pointer("/identity").unwrap_or(&Value::Null)).get("packet_digest"))
            == ((*before.pointer("/identity").unwrap_or(&Value::Null)).get("packet_digest")),
        "a failed stamp must not write a packet digest"
    );
    Ok(())
}

#[test]
fn stamp_rejects_unsafe_manifests_before_mutating_them() -> Result<()> {
    let mut credential_manifest = base_manifest();
    *credential_manifest.pointer_mut("/metadata").context("missing fixture path /metadata")? =
        json!({"accessToken": "should-not-persist"});
    let credential_before = credential_manifest.clone();
    ensure!(
        stamp_manifest(&mut credential_manifest).is_err(),
        "credential-bearing metadata must fail closed before stamping"
    );
    ensure!(
        (credential_manifest) == (credential_before),
        "test condition failed: {}",
        stringify!((credential_manifest) == (credential_before))
    );

    let mut missing_revision = base_manifest();
    missing_revision
        .pointer_mut("/subject/model")
        .context("missing fixture path /subject/model")?
        .as_object_mut()
        .context("required fixture value missing")?
        .remove("revision");
    let missing_revision_before = missing_revision.clone();
    ensure!(
        stamp_manifest(&mut missing_revision).is_err(),
        "missing model revision must fail closed before stamping"
    );
    ensure!(
        (missing_revision) == (missing_revision_before),
        "test condition failed: {}",
        stringify!((missing_revision) == (missing_revision_before))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Positive paths and determinism.
// ---------------------------------------------------------------------------

#[test]
fn positive_well_formed_synthetic_packets_validate() -> Result<()> {
    let completed = stamped(base_manifest())?;
    ensure!(
        validate_manifest(&completed).is_empty(),
        "stamped completed packet must satisfy the closed core: {:?}",
        violation_codes(&validate_manifest(&completed))
    );

    // A refused transfer run with zero results still validates.
    let mut refused = base_manifest();
    *refused.pointer_mut("/disposition").context("missing fixture path /disposition")? =
        json!("transferred");
    *refused.pointer_mut("/run_id").context("missing fixture path /run_id")? =
        json!("synthetic-refused-transfer-002");
    refused.as_object_mut().context("required fixture value missing")?.remove("results");
    let refused = stamped(refused)?;
    ensure!(
        validate_manifest(&refused).is_empty(),
        "stamped transferred packet must satisfy the closed core: {:?}",
        violation_codes(&validate_manifest(&refused))
    );
    Ok(())
}

#[test]
fn digests_recompute_stably_and_diverge_on_content_change() -> Result<()> {
    let doc = stamped(base_manifest())?;
    let first = record_digest(
        (*doc.pointer("/events/0").unwrap_or(&Value::Null))
            .as_object()
            .context("required fixture value missing")?,
    )
    .context("fields present")?;
    let second = record_digest(
        (*doc.pointer("/events/0").unwrap_or(&Value::Null))
            .as_object()
            .context("required fixture value missing")?,
    )
    .context("fields present")?;
    ensure!((first) == (second), "recomputed digest must be stable");
    ensure!(is_lowercase_sha256_hex(&first), "digests are strict lowercase 64-hex");

    let mut changed = doc.clone();
    *changed
        .pointer_mut("/events/0/payload/note")
        .context("missing fixture path /events/0/payload/note")? = json!("different observation");
    let third = record_digest(
        changed
            .pointer_mut("/events/0")
            .context("missing fixture path /events/0")?
            .as_object_mut()
            .context("required fixture value missing")?,
    )
    .context("fields present")?;
    ensure!((first) != (third), "content change must change the digest");
    Ok(())
}

#[test]
fn digest_charset_is_strict_lowercase_hex() -> Result<()> {
    ensure!(
        is_lowercase_sha256_hex(&"0123456789abcdef".repeat(4)),
        "test condition failed: {}",
        stringify!(is_lowercase_sha256_hex(&"0123456789abcdef".repeat(4)))
    );
    ensure!(
        is_lowercase_sha256_hex(&"a".repeat(64)),
        "test condition failed: {}",
        stringify!(is_lowercase_sha256_hex(&"a".repeat(64)))
    );
    ensure!(!is_lowercase_sha256_hex(&"g".repeat(64)), "g-z are not hex");
    ensure!(!is_lowercase_sha256_hex(&"A".repeat(64)), "uppercase is not canonical");
    ensure!(!is_lowercase_sha256_hex(&format!("{}g", "a".repeat(63))), "64 chars, non-hex tail");
    ensure!(!is_lowercase_sha256_hex("abcd"), "wrong length");
    Ok(())
}

#[test]
fn canonical_form_is_key_order_insensitive_array_order_sensitive() -> Result<()> {
    let inserted_one_way = json!({"b": 1, "a": {"z": 2, "y": [3, 4]}});
    let inserted_other_way = json!({"a": {"y": [3, 4], "z": 2}, "b": 1});
    ensure!(
        (canonical_form(&inserted_one_way)) == (canonical_form(&inserted_other_way)),
        "equal documents must hash equally regardless of key insertion order"
    );

    let ascending = json!({"arr": [1, 2]});
    let descending = json!({"arr": [2, 1]});
    ensure!(
        (canonical_form(&ascending)) != (canonical_form(&descending)),
        "array element order stays semantic input"
    );
    Ok(())
}

#[test]
fn record_digest_is_key_order_insensitive() -> Result<()> {
    let insertion_one: Map<String, Value> =
        json!({"seq": 0, "kind": "observation", "at_ms": 0, "payload": {"x": 1, "y": 2}})
            .as_object()
            .context("object")?
            .clone();
    let insertion_other: Map<String, Value> =
        json!({"payload": {"y": 2, "x": 1}, "at_ms": 0, "kind": "observation", "seq": 0})
            .as_object()
            .context("object")?
            .clone();
    ensure!(
        (record_digest(&insertion_one)) == (record_digest(&insertion_other)),
        "record digests must not depend on object key insertion order"
    );
    Ok(())
}

#[test]
fn stamping_is_idempotent() -> Result<()> {
    let once = stamped(base_manifest())?;
    let mut twice = once.clone();
    stamp_manifest(&mut twice).context("re-stamp succeeds")?;
    ensure!(
        (canonical_form(&once)) == (canonical_form(&twice)),
        "test condition failed: {}",
        stringify!((canonical_form(&once)) == (canonical_form(&twice)))
    );
    Ok(())
}

#[test]
fn metadata_is_non_semantic_for_the_integrity_envelope() -> Result<()> {
    let mut enriched = base_manifest();
    *enriched.pointer_mut("/metadata").context("missing fixture path /metadata")? = json!({
        "generated_at_utc": "2026-08-27T00:00:00Z",
        "extra_caller_note": {"nested": [1, 2, 3]},
    });
    let enriched = stamped(enriched)?;
    let plain = stamped(base_manifest())?;
    ensure!(
        (*enriched.pointer("/identity/packet_digest").unwrap_or(&Value::Null))
            == (*plain.pointer("/identity/packet_digest").unwrap_or(&Value::Null)),
        "metadata must not enter the canonical integrity envelope"
    );
    Ok(())
}

#[test]
fn advisory_report_ordering_is_content_sorted_not_enumeration_sorted() -> Result<()> {
    let mut entries = Vec::new();
    for index in 0..4 {
        let mut doc = base_manifest();
        *doc.pointer_mut("/run_id").context("missing fixture path /run_id")? =
            json!(format!("synthetic-run-{index:03}"));
        entries.push((format!("zz-source-{index}.json"), doc));
    }
    let forward = collect_rows(&entries);
    let mut reversed_entries = entries.clone();
    reversed_entries.reverse();

    let forward_rendered = render_report(&forward, DogfoodReportFormat::Markdown);
    let reverse_rendered =
        render_report(&collect_rows(&reversed_entries), DogfoodReportFormat::Markdown);
    ensure!(
        (forward_rendered) == (reverse_rendered),
        "report must be sorted by content, not caller enumeration order"
    );
    let second_rendered = render_report(&forward, DogfoodReportFormat::Markdown);
    ensure!((forward_rendered) == (second_rendered), "rendering must be deterministic");

    let json_forward = render_report(&forward, DogfoodReportFormat::Json);
    let parsed: Value = serde_json::from_str(&json_forward).context("valid JSON report")?;
    ensure!(
        (*parsed.pointer("/report").unwrap_or(&Value::Null))
            == (json!("agent-packet-dogfood.core.report.v1")),
        "test condition failed: {}",
        stringify!(
            (*parsed.pointer("/report").unwrap_or(&Value::Null))
                == (json!("agent-packet-dogfood.core.report.v1"))
        )
    );
    let runs =
        (*parsed.pointer("/runs").unwrap_or(&Value::Null)).as_array().context("runs array")?;
    let ids: Vec<&str> =
        runs.iter().filter_map(|r| r.get("run_id").and_then(Value::as_str)).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    ensure!((ids) == (sorted_ids), "JSON projection lists runs in sorted order");
    Ok(())
}

#[test]
fn invalid_runs_render_as_invalid_without_panicking() -> Result<()> {
    let mut broken = base_manifest();
    broken.as_object_mut().context("required fixture value missing")?.remove("subject");
    let rows = collect_rows(vec![("broken.json".to_string(), broken)].as_slice());
    ensure!((rows.len()) == (1), "test condition failed: {}", stringify!((rows.len()) == (1)));
    ensure!(
        (rows.first().context("missing report row")?.validity) == ("invalid"),
        "test condition failed: {}",
        stringify!((rows.first().context("missing report row")?.validity) == ("invalid"))
    );
    Ok(())
}

#[test]
fn invalid_run_id_is_redacted_from_rendered_reports() -> Result<()> {
    let mut invalid = stamped(base_manifest())?;
    *invalid.pointer_mut("/run_id").context("missing fixture path /run_id")? =
        json!("api_key=hunter2");
    let rows = collect_rows(vec![("invalid.json".to_string(), invalid)].as_slice());
    let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
    let json = render_report(&rows, DogfoodReportFormat::Json);
    ensure!(!markdown.contains("api_key=hunter2"), "invalid run_id leaked in markdown report");
    ensure!(!json.contains("api_key=hunter2"), "invalid run_id leaked in JSON report");
    ensure!(
        markdown.contains("<redacted-invalid-run-id>"),
        "test condition failed: {}",
        stringify!(markdown.contains("<redacted-invalid-run-id>"))
    );
    ensure!(
        json.contains("<redacted-invalid-run-id>"),
        "test condition failed: {}",
        stringify!(json.contains("<redacted-invalid-run-id>"))
    );
    Ok(())
}

#[test]
fn invalid_disposition_is_redacted_while_valid_disposition_is_preserved() -> Result<()> {
    let valid = stamped(base_manifest())?;
    let mut invalid = stamped(base_manifest())?;
    *invalid.pointer_mut("/disposition").context("missing fixture path /disposition")? =
        json!("api_key=hunter2");
    let entries = vec![("valid.json".to_string(), valid), ("invalid.json".to_string(), invalid)];
    let rows = collect_rows(&entries);
    let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
    let json = render_report(&rows, DogfoodReportFormat::Json);

    ensure!(!markdown.contains("api_key=hunter2"), "invalid disposition leaked in markdown report");
    ensure!(!json.contains("api_key=hunter2"), "invalid disposition leaked in JSON report");
    ensure!(
        markdown.contains("| completed | valid |"),
        "valid disposition changed in markdown report"
    );

    let report: Value = serde_json::from_str(&json).context("valid JSON report")?;
    let runs =
        (*report.pointer("/runs").unwrap_or(&Value::Null)).as_array().context("runs array")?;
    ensure!(
        runs.iter().any(|run| {
            (*run.pointer("/disposition").unwrap_or(&Value::Null)) == "completed"
                && (*run.pointer("/validity").unwrap_or(&Value::Null)) == "valid"
        }),
        "test condition failed: {}",
        stringify!(runs.iter().any(|run| {
            (*run.pointer("/disposition").unwrap_or(&Value::Null)) == "completed"
                && (*run.pointer("/validity").unwrap_or(&Value::Null)) == "valid"
        }))
    );
    ensure!(
        runs.iter().any(|run| {
            (*run.pointer("/disposition").unwrap_or(&Value::Null))
                == "<redacted-invalid-disposition>"
                && (*run.pointer("/validity").unwrap_or(&Value::Null)) == "invalid"
        }),
        "test condition failed: {}",
        stringify!(runs.iter().any(|run| {
            (*run.pointer("/disposition").unwrap_or(&Value::Null))
                == "<redacted-invalid-disposition>"
                && (*run.pointer("/validity").unwrap_or(&Value::Null)) == "invalid"
        }))
    );
    Ok(())
}

#[test]
fn every_closed_disposition_is_preserved_in_both_report_projections() -> Result<()> {
    for disposition in DISPOSITIONS {
        let mut doc = base_manifest();
        *doc.pointer_mut("/disposition").context("missing fixture path /disposition")? =
            json!(*disposition);
        let doc = stamped(doc)?;
        let entries = vec![(format!("{disposition}.json"), doc)];
        let rows = collect_rows(&entries);

        ensure!(
            rows.first().context("missing report row")?.violations.is_empty(),
            "{disposition} must remain valid"
        );
        let markdown = render_report(&rows, DogfoodReportFormat::Markdown);
        ensure!(
            markdown.contains(&format!("| {disposition} | valid |")),
            "Markdown report must preserve {disposition}: {markdown}"
        );

        let json = render_report(&rows, DogfoodReportFormat::Json);
        let report: Value = serde_json::from_str(&json).context("JSON report is valid")?;
        ensure!(
            (*report.pointer("/runs/0/disposition").unwrap_or(&Value::Null))
                == (json!(*disposition)),
            "test condition failed: {}",
            stringify!(
                (*report.pointer("/runs/0/disposition").unwrap_or(&Value::Null))
                    == (json!(*disposition))
            )
        );
        ensure!(
            (*report.pointer("/runs/0/validity").unwrap_or(&Value::Null)) == (json!("valid")),
            "test condition failed: {}",
            stringify!(
                (*report.pointer("/runs/0/validity").unwrap_or(&Value::Null)) == (json!("valid"))
            )
        );
    }
    Ok(())
}

#[test]
fn unknown_disposition_violation_does_not_retain_the_untrusted_value() -> Result<()> {
    let leaked = "api_key=hunter2";
    let violations = mutant(base_manifest(), |doc| {
        *doc.pointer_mut("/disposition").context("missing fixture path /disposition")? =
            json!(leaked);
        Ok(())
    })?;
    let violation = violations
        .iter()
        .find(|violation| violation.code == "unknown_disposition")
        .context("unknown disposition must be reported")?;
    ensure!(!violation.detail.contains(leaked), "validator detail leaked {leaked}");
    ensure!(
        (violation.detail) == ("manifest: unknown disposition (value redacted)"),
        "test condition failed: {}",
        stringify!((violation.detail) == ("manifest: unknown disposition (value redacted)"))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Repository-contract level proofs (fixtures + schema self-check).
// ---------------------------------------------------------------------------

#[test]
fn committed_schema_pins_module_vocabulary() -> Result<()> {
    let root = project_root().context("project root resolves")?;
    let violations = validate_schema_file(&root).context("schema readable")?;
    ensure!(
        violations.is_empty(),
        "committed schema drifted from the pinned closed vocabulary: {violations:?}"
    );
    Ok(())
}

#[test]
fn every_pinned_mutant_has_a_committed_fixture_with_matching_expectation() -> Result<()> {
    let root = project_root().context("project root resolves")?;
    let expected_path = root.join(FIXTURE_DIR).join(INVALID_DIR).join("expected_errors.json");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&expected_path)
            .with_context(|| format!("reading {}", expected_path.display()))
            .context("expected_errors.json readable")?,
    )
    .context("expected_errors.json parses")?;
    let map = expected.as_object().context("object map")?;

    for (name, code) in EXPECTED_INVALID {
        let path = root.join(FIXTURE_DIR).join(INVALID_DIR).join(name);
        ensure!(
            path.exists(),
            "mutant fixture {} is committed (red-first: matrix complete before rules land)",
            path.display()
        );
        let actual = map
            .get(*name)
            .and_then(Value::as_str)
            .ok_or_else(|| color_eyre::eyre::eyre!("{name} is listed in expected_errors.json"))?;
        ensure!((actual) == (*code), "{name} expectation stays aligned with the pinned vocabulary");
        let doc = load_manifest(&path).context("fixture parses")?.1;
        ensure!(
            doc.get("schema").and_then(Value::as_str) == Some(SCHEMA_NAME),
            "{name} declares the shared core schema name"
        );
    }
    Ok(())
}

#[test]
fn expected_errors_file_is_exactly_the_pinned_matrix() -> Result<()> {
    let root = project_root().context("project root resolves")?;
    let expected_path = root.join(FIXTURE_DIR).join(INVALID_DIR).join("expected_errors.json");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&expected_path).context("expected_errors.json readable")?,
    )
    .context("expected_errors.json parses")?;
    let map = expected.as_object().context("object map")?;
    ensure!(
        (map.len()) == (EXPECTED_INVALID.len()),
        "expected_errors.json must pin exactly the committed matrix"
    );
    for name in map.keys() {
        ensure!(
            EXPECTED_INVALID.iter().any(|(pinned, _)| pinned == name),
            "expected_errors.json entry {name} is not pinned in EXPECTED_INVALID"
        );
    }
    Ok(())
}

#[test]
fn invalid_directory_is_exactly_the_pinned_matrix() -> Result<()> {
    let root = project_root().context("project root resolves")?;
    let invalid_dir = root.join(FIXTURE_DIR).join(INVALID_DIR);
    let mut on_disk: Vec<String> = std::fs::read_dir(&invalid_dir)
        .context("invalid fixture dir readable")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name != "expected_errors.json")
        .collect();
    on_disk.sort();
    let mut pinned: Vec<String> =
        EXPECTED_INVALID.iter().map(|(name, _)| (*name).to_string()).collect();
    pinned.sort();
    ensure!((on_disk) == (pinned), "every committed mutant fixture must be pinned, and vice versa");
    Ok(())
}

#[test]
fn committed_valid_fixtures_validate_and_match_their_golden_reports() -> Result<()> {
    let root = project_root().context("project root resolves")?;
    let fixture_dir = root.join(FIXTURE_DIR);
    let mut entries = Vec::new();
    for name in VALID_FIXTURES {
        let (source, doc) = load_manifest(&fixture_dir.join(name)).context("fixture parses")?;
        ensure!(
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
        let golden = std::fs::read_to_string(&path).context("golden vector readable")?;
        ensure!((golden) == (render_report(&rows, format)), "{} drifted", path.display());
    }
    Ok(())
}
