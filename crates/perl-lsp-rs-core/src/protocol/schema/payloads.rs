use super::{SchemaError, expect_null, expect_object};
use serde_json::{Map, Value};

pub(super) fn null_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    expect_null(Some(method), "$.params", value)
}

pub(super) fn null_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    expect_null(Some(method), "$.result", value)
}

pub(super) fn null_or_empty_object(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() || value.as_object().is_some_and(Map::is_empty) {
        Ok(())
    } else {
        Err(SchemaError::at_value(Some(method), "$.params", "null or empty object", value))
    }
}

pub(super) fn initialize_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_object(
        Some(method),
        "$.params.capabilities",
        object.get("capabilities").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.capabilities", "object", "missing")
        })?,
    )?;
    if let Some(process_id) = object.get("processId")
        && !process_id.is_null()
        && process_id.as_i64().is_none()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.processId",
            "integer or null",
            process_id,
        ));
    }
    if let Some(root_uri) = object.get("rootUri")
        && !root_uri.is_null()
        && !root_uri.is_string()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.rootUri",
            "string or null",
            root_uri,
        ));
    }
    if let Some(folders) = object.get("workspaceFolders")
        && !folders.is_null()
        && !folders.is_array()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.workspaceFolders",
            "array or null",
            folders,
        ));
    }
    Ok(())
}

pub(super) fn initialize_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let capabilities = expect_object(
        Some(method),
        "$.result.capabilities",
        object.get("capabilities").ok_or_else(|| {
            SchemaError::new(Some(method), "$.result.capabilities", "object", "missing")
        })?,
    )?;
    for key in object.keys() {
        // Compared case-insensitively: `PerlLsp` and `PERL_LSP` are the same
        // project namespace as `perlLsp` for the purpose of this boundary.
        if key.to_ascii_lowercase().starts_with("perl") || key.starts_with("$/") {
            return Err(SchemaError::new(
                Some(method),
                format!("$.result{}", super::object_key_segment(key)),
                "project metadata under capabilities.experimental",
                "forbidden top-level project extension",
            ));
        }
    }
    if let Some(experimental) = capabilities.get("experimental")
        && !experimental.is_null()
        && !experimental.is_object()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.result.capabilities.experimental",
            "object or null",
            experimental,
        ));
    }
    Ok(())
}

pub(super) fn cancel_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    match object.get("id") {
        Some(Value::Number(number)) if number.as_i64().is_some() => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(value) => {
            Err(SchemaError::at_value(Some(method), "$.params.id", "integer or string", value))
        }
        None => Err(SchemaError::new(Some(method), "$.params.id", "request ID", "missing")),
    }
}
