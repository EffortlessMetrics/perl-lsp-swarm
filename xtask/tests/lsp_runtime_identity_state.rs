use color_eyre::eyre::{Result, eyre};
use serde_json::{Map, Value};
use xtask::lsp_runtime_identity_state::{
    concept_ids, embedded_bundle_json, embedded_document, render_index_str, semantic_digest_str,
    validate_embedded, validate_str,
};

fn value() -> Result<Value> {
    serde_json::from_str(&embedded_bundle_json()?).map_err(Into::into)
}

fn encode(value: &Value) -> Result<String> {
    serde_json::to_string(value).map_err(Into::into)
}

fn object_mut<'a>(value: &'a mut Value, pointer: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| eyre!("expected object at {pointer}"))
}

fn row_mut<'a>(
    value: &'a mut Value,
    collection: &str,
    id_field: &str,
    id: &str,
) -> Result<&'a mut Map<String, Value>> {
    let rows = value
        .get_mut(collection)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("expected array {collection}"))?;
    for row in rows {
        if row.get(id_field).and_then(Value::as_str) == Some(id) {
            return row
                .as_object_mut()
                .ok_or_else(|| eyre!("row {id} in {collection} is not an object"));
        }
    }
    Err(eyre!("missing {collection} row {id}"))
}

fn reject(value: &Value, needle: &str) -> Result<()> {
    let raw = encode(value)?;
    let Err(error) = validate_str(&raw) else {
        return Err(eyre!("mutated identity/state vocabulary must fail closed"));
    };
    let rendered = format!("{error:#}");
    assert!(rendered.contains(needle), "expected error containing {needle:?}; got {rendered}");
    Ok(())
}

#[test]
fn embedded_contract_and_human_index_are_current() -> Result<()> {
    validate_embedded()?;
    let bundle = embedded_bundle_json()?;
    let rendered = render_index_str(&bundle)?;
    assert!(embedded_document().contains(rendered.trim_end()));
    let ids = concept_ids()?;
    assert!(ids.contains(&"request_key".to_string()));
    assert!(ids.contains(&"terminal_selected".to_string()));
    assert!(ids.contains(&"delivery_fate".to_string()));
    // The promised downstream set is exact, not sampled: any added or
    // removed identity, boundary term, or state must update this list.
    let expected: Vec<String> = [
        "application_completed",
        "application_completion",
        "application_ready",
        "application_unavailable",
        "cancellation_requested",
        "client_consumed",
        "connection_closed",
        "connection_closing",
        "connection_id",
        "connection_open",
        "currentness_token",
        "deadline_expired",
        "deadline_id",
        "delivery_fate",
        "delivery_fate_known",
        "enqueued",
        "execution_admitted",
        "execution_rejected",
        "failure_id",
        "ingress_sequence",
        "lifecycle_transition_id",
        "message_id",
        "message_observed",
        "mutation_barrier_id",
        "operation_id",
        "output_admitted",
        "output_failed",
        "output_flush_committed",
        "output_id",
        "output_prepared",
        "output_sequence",
        "output_write_committed",
        "owner_cleaned",
        "progress_key",
        "protocol_exited",
        "protocol_initialized",
        "protocol_shutdown_requested",
        "protocol_uninitialized",
        "protocol_valid",
        "publication_committed",
        "publication_id",
        "request_key",
        "request_registered",
        "request_tracking_reserved",
        "reservation_id",
        "reverse_request_key",
        "route_id",
        "running",
        "session_id",
        "stale_publication_rejected",
        "task_group_settled",
        "task_id",
        "terminal_decision",
        "terminal_selected",
        "terminal_signal",
        "work_id",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(ids, expected);
    Ok(())
}

#[test]
fn source_order_does_not_change_semantic_digest_or_index() -> Result<()> {
    let original = embedded_bundle_json()?;
    let original_digest = semantic_digest_str(&original)?;
    let original_index = render_index_str(&original)?;

    let mut shuffled = value()?;
    for key in [
        "axes",
        "identities",
        "boundary_terms",
        "states",
        "relations",
        "ambiguous_terms",
        "journeys",
    ] {
        shuffled
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| eyre!("missing array {key}"))?
            .reverse();
    }
    object_mut(&mut shuffled, "/authority")?
        .get_mut("consumers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing authority consumers"))?
        .reverse();

    let raw = encode(&shuffled)?;
    assert_eq!(semantic_digest_str(&raw)?, original_digest);
    assert_eq!(render_index_str(&raw)?, original_index);
    Ok(())
}

#[test]
fn semantic_change_moves_digest() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "states", "id", "application_completed")?
        .insert("proposition".into(), Value::String("different proposition".into()));
    let raw = encode(&changed)?;
    assert_ne!(semantic_digest_str(&raw)?, semantic_digest_str(&embedded_bundle_json()?)?);
    Ok(())
}

#[test]
fn unknown_schema_version_and_field_fail_closed() -> Result<()> {
    let mut version = value()?;
    version["version"] = Value::from(2);
    reject(&version, "unknown vocabulary schema/version")?;

    let mut field = value()?;
    object_mut(&mut field, "/authority")?.insert("current_sha".into(), Value::String("abc".into()));
    reject(&field, "unknown field")?;
    Ok(())
}

#[test]
fn request_state_cannot_collapse_to_one_linear_phase() -> Result<()> {
    let mut changed = value()?;
    object_mut(&mut changed, "/request_state")?
        .insert("kind".into(), Value::String("linear_phase".into()));
    reject(&changed, "orthogonal_axes")
}

#[test]
fn one_authority_does_not_require_one_global_lock() -> Result<()> {
    let mut changed = value()?;
    object_mut(&mut changed, "/generic_boundary/one_authority")?
        .insert("global_lock".into(), Value::Bool(true));
    reject(&changed, "must not require one object, actor, global lock")
}

#[test]
fn request_key_preserves_numeric_string_and_connection_session_scope() -> Result<()> {
    let mut variants = value()?;
    row_mut(&mut variants, "identities", "id", "request_key")?
        .insert("variants".into(), serde_json::json!(["string"]));
    reject(&variants, "request_key.variants")?;

    let mut scope = value()?;
    row_mut(&mut scope, "identities", "id", "request_key")?
        .insert("scoped_by".into(), serde_json::json!(["session_id"]));
    reject(&scope, "identity request_key scopes changed")
}

#[test]
fn request_progress_and_reverse_domains_remain_independent() -> Result<()> {
    let mut changed = value()?;
    let relations = changed
        .get_mut("relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing relations"))?;
    relations
        .retain(|row| row.get("id").and_then(Value::as_str) != Some("request_independent_reverse"));
    reject(&changed, "request_key|independent_of|reverse_request_key")
}

#[test]
fn currentness_remains_opaque_and_owner_validated() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "identities", "id", "currentness_token")?
        .insert("opaque".into(), Value::Bool(false));
    reject(&changed, "opaque and owner-validated")
}

#[test]
fn runtime_cannot_claim_client_consumption() -> Result<()> {
    let mut boundary = value()?;
    object_mut(&mut boundary, "/generic_boundary")?
        .insert("client_consumption_claimable".into(), Value::Bool(true));
    reject(&boundary, "client consumption is outside")?;

    let mut state = value()?;
    row_mut(&mut state, "states", "id", "client_consumed")?
        .insert("runtime_claimable".into(), Value::Bool(true));
    reject(&state, "client_consumed must remain external-only")
}

#[test]
fn ambiguous_stage_terms_require_exact_propositions() -> Result<()> {
    let mut changed = value()?;
    let terms = changed
        .get_mut("ambiguous_terms")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing ambiguous_terms"))?;
    terms.retain(|row| row.get("term").and_then(Value::as_str) != Some("accepted"));
    reject(&changed, "ambiguous_terms denominator mismatch")
}

#[test]
fn completion_terminal_publication_and_delivery_cannot_collapse() -> Result<()> {
    for relation_id in [
        "application_completed_forbids_terminal",
        "application_completed_forbids_publication",
        "terminal_forbids_output_admission",
        "publication_forbids_write",
        "write_forbids_client_consumption",
        "output_failure_independent_terminal",
    ] {
        reject_missing_relation(relation_id)?;
    }
    Ok(())
}

#[test]
fn lifecycle_connection_task_delivery_and_cleanup_remain_distinct() -> Result<()> {
    for relation_id in [
        "connection_closed_forbids_protocol_exit",
        "connection_closed_forbids_cleanup",
        "connection_closed_forbids_task_settlement",
        "connection_closed_forbids_delivery_fate",
        "protocol_exit_forbids_connection_closed",
        "protocol_exit_forbids_cleanup",
        "protocol_initialized_independent_readiness",
    ] {
        reject_missing_relation(relation_id)?;
    }
    Ok(())
}

#[test]
fn cleanup_requires_terminal_delivery_and_task_settlement() -> Result<()> {
    for relation_id in [
        "cleanup_requires_terminal",
        "cleanup_requires_delivery_fate",
        "cleanup_requires_task_settlement",
    ] {
        reject_missing_relation(relation_id)?;
    }
    Ok(())
}

/// Remove one relation and assert the refusal names its exact semantic
/// tuple: a loose "missing required relationship" needle cannot tell which
/// tuple the denominator lost.
fn reject_missing_relation(relation_id: &str) -> Result<()> {
    let mut changed = value()?;
    let relations = changed
        .get_mut("relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing relations"))?;
    let removed = relations
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(relation_id))
        .cloned()
        .ok_or_else(|| eyre!("unknown relation {relation_id}"))?;
    let key = format!(
        "{}|{}|{}",
        removed.get("from").and_then(Value::as_str).unwrap_or("?"),
        removed.get("kind").and_then(Value::as_str).unwrap_or("?"),
        removed.get("to").and_then(Value::as_str).unwrap_or("?")
    );
    relations.retain(|row| row.get("id").and_then(Value::as_str) != Some(relation_id));
    reject(&changed, &format!("missing required relationship {key}"))?;
    Ok(())
}

/// Falsifier: narrowing an identity's scope set is refused even though every
/// referenced scope still names a known identity.
#[test]
fn narrowed_identity_scopes_are_refused() -> Result<()> {
    let mut changed = value()?;
    let scopes = row_mut(&mut changed, "identities", "id", "message_id")?
        .get_mut("scoped_by")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing scoped_by"))?;
    scopes.retain(|scope| scope.as_str() != Some("connection_id"));
    reject(&changed, "identity message_id scopes changed")?;
    Ok(())
}

/// Falsifier: an ambiguous term without a source issue is refused.
#[test]
fn ambiguous_term_without_source_is_refused() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "ambiguous_terms", "term", "pending")?
        .insert("source".into(), Value::from(0));
    reject(&changed, "ambiguous_terms pending has no source issue")?;
    Ok(())
}

/// Falsifier: a relation that is both legal and rejected for one journey is refused.
#[test]
fn journey_relation_in_both_lists_is_refused() -> Result<()> {
    let mut changed = value()?;
    let journey = row_mut(&mut changed, "journeys", "id", "ordinary_success")?;
    // Move a forbidden-kind relation into the legal list: the rejected-side
    // kind check passes, so the overlap check must fire.
    let moved = journey
        .get("rejected")
        .and_then(Value::as_array)
        .and_then(|rejected| rejected.first().cloned())
        .ok_or_else(|| eyre!("journey has no rejected relations"))?;
    journey
        .get_mut("relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("journey has no legal relations"))?
        .push(moved);
    reject(&changed, "is both legal and rejected")?;
    Ok(())
}

/// Falsifier: a blank axis proposition is refused.
#[test]
fn blank_axis_proposition_is_refused() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "axes", "id", "terminal")?
        .insert("proposition".into(), Value::String(String::new()));
    reject(&changed, "axis proposition")?;
    Ok(())
}

#[test]
fn generic_machine_vocabulary_rejects_domain_types() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "states", "id", "running")?
        .insert("proposition".into(), Value::String("Perl provider running".into()));
    reject(&changed, "forbidden generic-domain term")
}

#[test]
fn relationship_semantics_reject_unlisted_extra_key() -> Result<()> {
    let mut changed = value()?;
    let relations = changed
        .get_mut("relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing relations"))?;
    let mut extra = relations.first().cloned().ok_or_else(|| eyre!("empty relations"))?;
    let extra = extra.as_object_mut().ok_or_else(|| eyre!("relation is not an object"))?;
    extra.insert("id".into(), Value::String("extra_relationship".into()));
    extra.insert("kind".into(), Value::String("permits".into()));
    extra.insert("to".into(), Value::String("client_consumed".into()));
    relations.push(Value::Object(extra.clone()));
    // The per-relation binding refusal fires before the denominator
    // comparison: an unlisted id names no binding, so it is rejected here
    // rather than at the tuple-set check.
    reject(&changed, "relationship extra_relationship is bound to")
}

#[test]
fn relationship_ids_cannot_be_renamed_with_references_updated() -> Result<()> {
    let mut changed = value()?;
    row_mut(&mut changed, "relations", "id", "flush_requires_write")?
        .insert("id".into(), Value::String("renamed_flush_requires_write".into()));
    let journeys = changed
        .get_mut("journeys")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing journeys"))?;
    for journey in journeys {
        if let Some(relations) = journey.get_mut("relations").and_then(Value::as_array_mut) {
            for relation in relations {
                if relation.as_str() == Some("flush_requires_write") {
                    *relation = Value::String("renamed_flush_requires_write".into());
                }
            }
        }
    }
    // The renamed id names no binding, so the per-relation refusal fires
    // before the id-set denominator comparison.
    reject(&changed, "relationship renamed_flush_requires_write is bound to")
}

#[test]
fn journey_requires_relation_endpoints_in_facts() -> Result<()> {
    let mut changed = value()?;
    let journey = row_mut(&mut changed, "journeys", "id", "ordinary_success")?;
    let facts = journey
        .get_mut("facts")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing journey facts"))?;
    facts.retain(|fact| fact.as_str() != Some("output_write_committed"));
    reject(&changed, "requires relation flush_requires_write endpoints in facts")
}

#[test]
fn generic_boundary_rejects_forbidden_product_terms() -> Result<()> {
    for (field, text) in [
        ("currentness_law", "Perl-specific boundary"),
        ("one_authority", "Perl-specific authority"),
    ] {
        let mut changed = value()?;
        if field == "currentness_law" {
            object_mut(&mut changed, "/generic_boundary")?
                .insert(field.into(), Value::String(text.into()));
            reject(&changed, "generic_boundary.currentness_law")?;
        } else {
            object_mut(&mut changed, "/generic_boundary/one_authority")?
                .insert("law".into(), Value::String(text.into()));
            reject(&changed, "generic_boundary.one_authority")?;
        }
    }
    Ok(())
}

#[test]
fn journeys_must_use_known_facts_and_collapse_rejections() -> Result<()> {
    let mut unknown = value()?;
    row_mut(&mut unknown, "journeys", "id", "ordinary_success")?
        .insert("facts".into(), serde_json::json!(["not_a_fact"]));
    reject(&unknown, "references unknown id")?;

    let mut wrong_kind = value()?;
    row_mut(&mut wrong_kind, "journeys", "id", "writer_failure")?
        .insert("rejected".into(), serde_json::json!(["failure_requires_admitted"]));
    reject(&wrong_kind, "is neither forbids_inference nor independent_of")
}
