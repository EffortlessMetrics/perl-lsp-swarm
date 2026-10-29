//! Typed reads for the modeled GitHub fields. Error messages name fields,
//! never echo values from external payloads.
use super::model::Observed;
use serde_json::{Map, Value};

pub(super) fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{label} must be an object"))
}

pub(super) fn string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| format!("{field} must be a nonempty bounded string"))
}

pub(super) fn boolean(value: &Value, field: &str) -> Result<bool, String> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{field} must be present and boolean"))
}

pub(super) fn unsigned(value: &Value, field: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{field} must be present and an unsigned integer"))
}

pub(super) fn id(value: &Value, field: &str) -> Result<u64, String> {
    unsigned(value, field).and_then(|id| {
        if id > 0 { Ok(id) } else { Err(format!("{field} must be a positive identifier")) }
    })
}

pub(super) fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{field} must be present and an array"))
}

pub(super) fn observation<T>(value: Result<T, String>) -> Observed<T> {
    match value {
        Ok(value) => Observed::observed(value),
        Err(detail) => Observed::not_proven(detail),
    }
}

pub(super) fn nullable_id(
    value: &Value,
    field: &str,
    required: bool,
) -> Result<Option<u64>, String> {
    match value.get(field) {
        Some(Value::Null) => Ok(None),
        None if !required => Ok(None),
        Some(_) => id(value, field).map(Some),
        None => Err(format!("{field} was omitted")),
    }
}

pub(super) fn review_count(value: &Value, field: &str, maximum: u64) -> Result<u32, String> {
    let count = unsigned(value, field)?;
    if count > maximum {
        return Err(format!("{field} exceeds GitHub's maximum of {maximum} reviews"));
    }
    u32::try_from(count).map_err(|_| format!("{field} exceeded the receipt integer range"))
}
