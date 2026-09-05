use super::*;
use crate::utils::project_root;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;

const FIXTURE_PATH: &str = "fixtures/release_scope_v2/valid.preparation-pending.json";

fn read_repo_file(path: &str) -> Result<String, String> {
    fs::read_to_string(project_root().join(path))
        .map_err(|error| format!("cannot read {path}: {error}"))
}

fn fixture_text() -> Result<String, String> {
    read_repo_file(FIXTURE_PATH)
}

fn fixture_value() -> Result<Value, String> {
    serde_json::from_str(&fixture_text()?)
        .map_err(|error| format!("fixture is not JSON: {error}"))
}

fn schema_value() -> Result<Value, String> {
    serde_json::from_str(&read_repo_file(RELEASE_SCOPE_V2_SCHEMA_PATH)?)
        .map_err(|error| format!("schema is not JSON: {error}"))
}

fn schema_accepts(value: &Value) -> Result<bool, String> {
    let schema = schema_value()?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| format!("schema does not compile: {error}"))?;
    Ok(validator.is_valid(value))
}

fn model_accepts(value: &Value) -> Result<bool, String> {
    let text = serde_json::to_string(value)
        .map_err(|error| format!("cannot serialize test value: {error}"))?;
    Ok(parse_release_scope_v2(&text).is_ok())
}

fn root_object(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| "fixture root is not an object".to_string())
}

#[test]
fn schema_and_model_accept_the_non_live_fixture() -> Result<(), String> {
    let value = fixture_value()?;
    require(schema_accepts(&value)?, "published schema rejects the valid fixture")?;
    let model = parse_release_scope_v2(&fixture_text()?)?;
    require(model.prepared_swarm_sha.is_none(), "fixture must precede preparation")
}

#[test]
fn canonical_round_trip_is_byte_identical() -> Result<(), String> {
    let input = fixture_text()?;
    let model = parse_release_scope_v2(&input)?;
    let first = canonical_release_scope_v2(&model)?;
    let reparsed = parse_release_scope_v2(&first)?;
    let second = canonical_release_scope_v2(&reparsed)?;
    require(first == second, "second canonical serialization changed bytes")?;
    require(input == first, "checked fixture is not canonical")
}

#[test]
fn prepared_identity_is_explicitly_nullable_then_admissible() -> Result<(), String> {
    let mut value = fixture_value()?;
    root_object(&mut value)?.insert(
        "prepared_swarm_sha".to_string(),
        Value::String("dddddddddddddddddddddddddddddddddddddddd".to_string()),
    );
    require(schema_accepts(&value)?, "schema rejects a prepared identity")?;
    require(model_accepts(&value)?, "model rejects a prepared identity")
}

#[test]
fn complete_disposition_vocabulary_includes_already_included() -> Result<(), String> {
    let model = parse_release_scope_v2(&fixture_text()?)?;
    let observed = model
        .observed_pull_requests
        .iter()
        .map(|item| item.disposition)
        .collect::<BTreeSet<_>>();
    let expected = [
        ReleaseDisposition::Blocker018,
        ReleaseDisposition::Candidate018,
        ReleaseDisposition::Post018,
        ReleaseDisposition::Superseded,
        ReleaseDisposition::AlreadyIncluded,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    require(observed == expected, "fixture does not exercise every disposition")
}

#[test]
fn missing_frozen_identity_and_unknown_fields_fail_closed() -> Result<(), String> {
    let mut missing = fixture_value()?;
    root_object(&mut missing)?.remove("frozen_product_sha");
    require(!schema_accepts(&missing)?, "schema accepted missing frozen identity")?;
    require(!model_accepts(&missing)?, "model accepted missing frozen identity")?;

    let mut unknown = fixture_value()?;
    root_object(&mut unknown)?.insert("tag".to_string(), Value::String("v0.18.0".to_string()));
    require(!schema_accepts(&unknown)?, "schema accepted unknown publication authority")?;
    require(!model_accepts(&unknown)?, "model accepted unknown publication authority")
}

#[test]
fn feature_intake_and_invalidation_classes_are_load_bearing() -> Result<(), String> {
    let mut open_intake = fixture_value()?;
    open_intake["freeze_rules"]["feature_intake_closed"] = Value::Bool(false);
    require(!schema_accepts(&open_intake)?, "schema accepted open feature intake")?;
    require(!model_accepts(&open_intake)?, "model accepted open feature intake")?;

    let mut collapsed = fixture_value()?;
    collapsed["invalidation"]["release_metadata_change"] =
        collapsed["invalidation"]["product_change"].clone();
    require(!schema_accepts(&collapsed)?, "schema accepted collapsed invalidation classes")?;
    require(!model_accepts(&collapsed)?, "model accepted collapsed invalidation classes")?;

    let mut shortened = fixture_value()?;
    let invalidates = shortened["invalidation"]["product_change"]["invalidates"]
        .as_array_mut()
        .ok_or_else(|| "product invalidation set is not an array".to_string())?;
    invalidates.remove(0);
    require(!schema_accepts(&shortened)?, "schema accepted incomplete product invalidation")?;
    require(!model_accepts(&shortened)?, "model accepted incomplete product invalidation")
}

#[test]
fn malformed_blocker_proof_and_topology_digest_fail_closed() -> Result<(), String> {
    let mut bad_proof = fixture_value()?;
    bad_proof["blockers"][0]["proof"]["ref"] = Value::String("not-evidence".to_string());
    require(!schema_accepts(&bad_proof)?, "schema accepted malformed blocker proof")?;
    require(!model_accepts(&bad_proof)?, "model accepted malformed blocker proof")?;

    let mut bad_topology = fixture_value()?;
    bad_topology["topology"]["digest"] = Value::String("sha256:short".to_string());
    require(!schema_accepts(&bad_topology)?, "schema accepted malformed topology digest")?;
    require(!model_accepts(&bad_topology)?, "model accepted malformed topology digest")
}

#[test]
fn unknown_or_duplicate_pr_dispositions_cannot_hide_in_the_model() -> Result<(), String> {
    let mut unknown = fixture_value()?;
    unknown["observed_pull_requests"][0]["disposition"] =
        Value::String("release-maybe".to_string());
    require(!schema_accepts(&unknown)?, "schema accepted unknown PR disposition")?;
    require(!model_accepts(&unknown)?, "model accepted unknown PR disposition")?;

    let mut duplicate = fixture_value()?;
    duplicate["observed_pull_requests"][1]["number"] = Value::from(90001_u64);
    require(
        schema_accepts(&duplicate)?,
        "duplicate-number premise should remain a semantic model check",
    )?;
    require(!model_accepts(&duplicate)?, "model accepted duplicate PR identity")
}

#[test]
fn v1_admission_receipt_cannot_masquerade_as_v2() -> Result<(), String> {
    let v1 = r#"{
      "schema": 1,
      "release": "0.18.0",
      "track": "public-beta",
      "phase": "admission-frozen",
      "observation_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "frozen_swarm_sha": null
    }"#;
    require(parse_release_scope_v2(v1).is_err(), "v1 admission receipt parsed as v2")
}

#[test]
fn fixture_contains_placeholders_not_live_release_state() -> Result<(), String> {
    let value = fixture_value()?;
    let root = value
        .as_object()
        .ok_or_else(|| "fixture root is not an object".to_string())?;
    for forbidden in ["tag", "channels", "candidate", "authorization", "published"] {
        require(!root.contains_key(forbidden), format!("fixture contains live field {forbidden}"))?;
    }
    require(value["prepared_swarm_sha"].is_null(), "fixture invents a prepared subject")?;

    let mut strings = Vec::new();
    collect_strings(&value, &mut strings);
    for candidate in strings {
        if is_sha(candidate) {
            let Some(first) = candidate.as_bytes().first() else {
                return Err("SHA candidate unexpectedly empty".to_string());
            };
            require(
                candidate.as_bytes().iter().all(|byte| byte == first),
                format!("fixture contains a non-placeholder SHA: {candidate}"),
            )?;
        }
    }
    Ok(())
}

#[test]
fn receipt_registry_points_to_the_published_schema() -> Result<(), String> {
    let registry: toml::Value = toml::from_str(&read_repo_file(".ci/receipts/registry.toml")?)
        .map_err(|error| format!("receipt registry is invalid TOML: {error}"))?;
    let receipts = registry
        .get("receipt")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "receipt registry has no receipt array".to_string())?;
    let entry = receipts.iter().find(|item| {
        item.get("check").and_then(toml::Value::as_str) == Some(RELEASE_SCOPE_V2_CHECK)
    })
    .ok_or_else(|| "release-scope-v2 is not registered".to_string())?;
    require(
        entry.get("schema").and_then(toml::Value::as_str)
            == Some(RELEASE_SCOPE_V2_SCHEMA_PATH),
        "release-scope-v2 registry entry points to another schema",
    )
}

fn collect_strings<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => output.push(text),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, output);
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                collect_strings(item, output);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
