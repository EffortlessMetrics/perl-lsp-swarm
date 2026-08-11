use super::authority::sha256_hex;
use super::classify::classify;
use super::model::{AuthoritySource, LoadedManifest, Observation, PublicationManifest};
use color_eyre::eyre::{Result, bail, eyre};
use regex::Regex;
use serde_json::Value;

const CLEAN: &str = include_str!("../../../fixtures/publication_drift/clean.json");
const DRIFT: &str =
    include_str!("../../../fixtures/publication_drift/windows_arm64_target_drift.json");
const NOT_PROVEN: &str = include_str!("../../../fixtures/publication_drift/missing_manifest.json");
const AUTHORITY: &[u8] =
    include_bytes!("../../../fixtures/publication_drift/publication_manifest.v1.json");
const MANIFEST_SCHEMA: &str = include_str!("../../../schemas/publication_manifest.v1.schema.json");
const RECEIPT_SCHEMA: &str =
    include_str!("../../../schemas/publication_drift_receipt.v1.schema.json");

#[test]
fn publication_manifest_fixture_conforms_to_schema() -> Result<()> {
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA)?;
    let manifest: Value = serde_json::from_slice(AUTHORITY)?;
    validate_schema(&manifest, &schema, &schema, "publication_manifest")
}

#[test]
fn emitted_clean_drift_and_not_proven_receipts_conform_to_schema() -> Result<()> {
    let schema: Value = serde_json::from_str(RECEIPT_SCHEMA)?;
    let cases = [
        ("clean", receipt_value(CLEAN, fixture_authority()?)?, "clean"),
        ("drift", receipt_value(DRIFT, fixture_authority()?)?, "drift"),
        ("not_proven", receipt_value(NOT_PROVEN, AuthoritySource::Missing)?, "not_proven"),
    ];

    for (name, receipt, expected_verdict) in cases {
        validate_schema(&receipt, &schema, &schema, name)?;
        let verdict = receipt
            .get("verdict")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("{name}: receipt verdict is missing or not a string"))?;
        if verdict != expected_verdict {
            bail!("{name}: expected verdict {expected_verdict:?}, found {verdict:?}");
        }
    }
    Ok(())
}

#[test]
fn receipt_schema_rejects_missing_and_extra_top_level_fields() -> Result<()> {
    let schema: Value = serde_json::from_str(RECEIPT_SCHEMA)?;

    let mut missing = receipt_value(CLEAN, fixture_authority()?)?;
    missing
        .as_object_mut()
        .ok_or_else(|| eyre!("clean receipt is not an object"))?
        .remove("comparison_version");
    if validate_schema(&missing, &schema, &schema, "missing-field receipt").is_ok() {
        bail!("receipt without comparison_version passed schema validation");
    }

    let mut extra = receipt_value(CLEAN, fixture_authority()?)?;
    extra
        .as_object_mut()
        .ok_or_else(|| eyre!("clean receipt is not an object"))?
        .insert("unexpected".to_string(), Value::Bool(true));
    if validate_schema(&extra, &schema, &schema, "extra-field receipt").is_ok() {
        bail!("receipt with an unexpected field passed schema validation");
    }
    Ok(())
}

fn receipt_value(raw: &str, authority: AuthoritySource) -> Result<Value> {
    let observation: Observation = serde_json::from_str(raw)?;
    Ok(serde_json::to_value(classify(observation, authority))?)
}

fn fixture_authority() -> Result<AuthoritySource> {
    let document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    Ok(AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: sha256_hex(AUTHORITY) }))
}

fn validate_schema(value: &Value, schema: &Value, root: &Value, context: &str) -> Result<()> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference.strip_prefix('#').ok_or_else(|| {
            eyre!("{context}: unsupported external schema reference {reference:?}")
        })?;
        let target = root
            .pointer(pointer)
            .ok_or_else(|| eyre!("{context}: unresolved schema reference {reference:?}"))?;
        return validate_schema(value, target, root, context);
    }

    if let Some(candidates) = schema.get("oneOf").and_then(Value::as_array) {
        let matches = candidates
            .iter()
            .filter(|candidate| validate_schema(value, candidate, root, context).is_ok())
            .count();
        if matches != 1 {
            bail!("{context}: expected exactly one oneOf branch, matched {matches}");
        }
        return Ok(());
    }

    if let Some(expected) = schema.get("const") {
        if value != expected {
            bail!("{context}: value {value} does not match const {expected}");
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            bail!("{context}: value {value} is not in enum {allowed:?}");
        }
    }
    if let Some(schema_type) = schema.get("type") {
        if !matches_type(value, schema_type)? {
            bail!("{context}: value {value} does not match type {schema_type}");
        }
    }

    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required {
                let key = key
                    .as_str()
                    .ok_or_else(|| eyre!("{context}: schema required entry is not a string"))?;
                if !object.contains_key(key) {
                    bail!("{context}: missing required key {key:?}");
                }
            }
        }

        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
                for key in object.keys() {
                    if !properties.contains_key(key) {
                        bail!("{context}: unexpected key {key:?}");
                    }
                }
            }
            for (key, property_schema) in properties {
                if let Some(property) = object.get(key) {
                    validate_schema(property, property_schema, root, &format!("{context}.{key}"))?;
                }
            }
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64) {
            if array.len() < minimum as usize {
                bail!("{context}: expected at least {minimum} items");
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate_schema(item, item_schema, root, &format!("{context}[{index}]"))?;
            }
        }
    }

    if let Some(string) = value.as_str() {
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
            if string.chars().count() < minimum as usize {
                bail!("{context}: string is shorter than minLength {minimum}");
            }
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            let regex = Regex::new(pattern)
                .map_err(|error| eyre!("{context}: invalid schema pattern {pattern:?}: {error}"))?;
            if !regex.is_match(string) {
                bail!("{context}: string {string:?} does not match {pattern:?}");
            }
        }
    }

    if value.is_number() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64) {
            let number = value
                .as_i64()
                .ok_or_else(|| eyre!("{context}: minimum applies to a non-integer value"))?;
            if number < minimum {
                bail!("{context}: integer {number} is below minimum {minimum}");
            }
        }
    }

    if let Some(forbidden) = schema.get("not") {
        if validate_schema(value, forbidden, root, context).is_ok() {
            bail!("{context}: value matches forbidden schema");
        }
    }

    Ok(())
}

fn matches_type(value: &Value, schema_type: &Value) -> Result<bool> {
    match schema_type {
        Value::String(kind) => Ok(matches_single_type(value, kind)),
        Value::Array(kinds) => {
            for kind in kinds {
                let kind = kind
                    .as_str()
                    .ok_or_else(|| eyre!("schema type array contains a non-string value"))?;
                if matches_single_type(value, kind) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => bail!("schema type must be a string or array of strings"),
    }
}

fn matches_single_type(value: &Value, kind: &str) -> bool {
    match kind {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        _ => false,
    }
}
