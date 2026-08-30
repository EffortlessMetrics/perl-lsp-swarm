use color_eyre::eyre::{Result, eyre};
use serde_json::{Map, Value};
use xtask::lsp_runtime_identity_state::{
    concept_ids, embedded_bundle_json, embedded_document, render_index_str,
    semantic_digest_str, validate_embedded, validate_str,
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
    let error = validate_str(&raw).expect_err("mutated identity/state vocabulary must fail closed");
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains(needle),
        "expected error containing {needle:?}; got {rendered}"
    );
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
    assert_ne!(
        semantic_digest_str(&raw)?,
        semantic_digest_str(&embedded_bundle_json()?)?
    );
    Ok(())
}

#[test]
fn unknown_schema_version_and_field_fail_closed() -> Result<()> {
    let mut version = value()?;
    version["version"] = Value::from(2);
    reject(&version, "unknown vocabulary schema/version")?;

    let mut field = value()?;
    object_mut(&mut field, "/authority")?
        .insert("current_sha".into(), Value::String("abc".into()));
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
    reject(&scope, "request_key.scoped_by")
}

#[test]
fn request_progress_and_reverse_domains_remain_independent() -> Result<()> {
    let mut changed = value()?;
    let relations = changed
        .get_mut("relations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("missing relations"))?;
    relations.retain(|row| {
        row.get("id").and_then(Value::as_str) != Some("request_independent_reverse")
    });
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
        let mut changed = value()?;
        let relations = changed
            .get_mut("relations")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| eyre!("missing relations"))?;
        relations.retain(|row| row.get("id").and_then(Value::as_str) != Some(relation_id));
        reject(&changed, "missing required relationship")?;
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
        let mut changed = value()?;
        let relations = changed
            .get_mut("relations")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| eyre!("missing relations"))?;
        relations.retain(|row| row.get("id").and_then(Value::as_str) != Some(relation_id));
        reject(&changed, "missing required relationship")?;
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
        let mut changed = value()?;
        let relations = changed
            .get_mut("relations")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| eyre!("missing relations"))?;
        relations.retain(|row| row.get("id").and_then(Value::as_str) != Some(relation_id));
        reject(&changed, "missing required relationship")?;
    }
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
