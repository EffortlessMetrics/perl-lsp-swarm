use super::{
    SchemaError, expect_array, expect_boolean, expect_integer, expect_null, expect_object,
    expect_string, expect_uinteger, observed,
};
use serde_json::{Map, Value};

pub(super) fn null_only(method: &str, value: &Value) -> Result<(), SchemaError> {
    expect_null(Some(method), "$.payload", value)
}

pub(super) fn null_or_empty_object(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() || value.as_object().is_some_and(Map::is_empty) {
        Ok(())
    } else {
        Err(SchemaError::at_value(Some(method), "$.params", "null or empty object", value))
    }
}

pub(super) fn array_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let _ = expect_array(Some(method), "$.result", value)?;
    Ok(())
}

pub(super) fn nullable_object_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() || value.is_object() {
        Ok(())
    } else {
        Err(SchemaError::at_value(Some(method), "$.result", "null or object", value))
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
        if key.starts_with("perl") || key.starts_with("$/") {
            return Err(SchemaError::new(
                Some(method),
                format!("$.result.{key}"),
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

pub(super) fn text_document_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")
}

pub(super) fn text_document_position_params(
    method: &str,
    value: &Value,
) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    validate_position(
        method,
        object.get("position").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.position", "object", "missing")
        })?,
        "$.params.position",
    )
}

pub(super) fn reference_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    let context = expect_object(
        Some(method),
        "$.params.context",
        object.get("context").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.context", "object", "missing")
        })?,
    )?;
    let _ = expect_boolean(
        Some(method),
        "$.params.context.includeDeclaration",
        context.get("includeDeclaration"),
    )?;
    Ok(())
}

pub(super) fn rename_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.newName", object.get("newName"))?;
    Ok(())
}

pub(super) fn formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    let _ = expect_object(
        Some(method),
        "$.params.options",
        object.get("options").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.options", "object", "missing")
        })?,
    )?;
    Ok(())
}

pub(super) fn range_formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    formatting_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    validate_range(
        method,
        object
            .get("range")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.range", "range", "missing"))?,
        "$.params.range",
    )
}

pub(super) fn ranges_formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    formatting_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    let ranges = expect_array(
        Some(method),
        "$.params.ranges",
        object
            .get("ranges")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.ranges", "array", "missing"))?,
    )?;
    for (index, range) in ranges.iter().enumerate() {
        validate_range(method, range, &format!("$.params.ranges[{index}]"))?;
    }
    Ok(())
}

pub(super) fn inline_completion_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    if let Some(context) = object.get("context") {
        let _ = expect_object(Some(method), "$.params.context", context)?;
    }
    Ok(())
}

pub(super) fn did_open_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let document = expect_object(
        Some(method),
        "$.params.textDocument",
        object.get("textDocument").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.textDocument", "object", "missing")
        })?,
    )?;
    let _ = expect_string(Some(method), "$.params.textDocument.uri", document.get("uri"))?;
    let _ = expect_string(
        Some(method),
        "$.params.textDocument.languageId",
        document.get("languageId"),
    )?;
    let _ = expect_integer(Some(method), "$.params.textDocument.version", document.get("version"))?;
    let _ = expect_string(Some(method), "$.params.textDocument.text", document.get("text"))?;
    Ok(())
}

pub(super) fn did_change_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let document = expect_object(
        Some(method),
        "$.params.textDocument",
        object.get("textDocument").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.textDocument", "object", "missing")
        })?,
    )?;
    let _ = expect_string(Some(method), "$.params.textDocument.uri", document.get("uri"))?;
    if let Some(version) = document.get("version")
        && !version.is_null()
        && version.as_i64().is_none()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.textDocument.version",
            "integer or null",
            version,
        ));
    }
    let changes = expect_array(
        Some(method),
        "$.params.contentChanges",
        object.get("contentChanges").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.contentChanges", "array", "missing")
        })?,
    )?;
    for (index, change) in changes.iter().enumerate() {
        let path = format!("$.params.contentChanges[{index}]");
        let change = expect_object(Some(method), &path, change)?;
        let _ = expect_string(Some(method), &format!("{path}.text"), change.get("text"))?;
        if let Some(range) = change.get("range") {
            validate_range(method, range, &format!("{path}.range"))?;
        }
    }
    Ok(())
}

pub(super) fn did_save_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    if let Some(text) = object.get("text")
        && !text.is_null()
        && !text.is_string()
    {
        return Err(SchemaError::at_value(Some(method), "$.params.text", "string or null", text));
    }
    Ok(())
}

pub(super) fn completion_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    if let Some(items) = value.as_array() {
        return validate_object_array(method, items, "$.result");
    }
    let object = expect_object(Some(method), "$.result", value)?;
    let items = expect_array(
        Some(method),
        "$.result.items",
        object
            .get("items")
            .ok_or_else(|| SchemaError::new(Some(method), "$.result.items", "array", "missing"))?,
    )?;
    validate_object_array(method, items, "$.result.items")?;
    let _ = expect_boolean(Some(method), "$.result.isIncomplete", object.get("isIncomplete"))?;
    if let Some(defaults) = object.get("itemDefaults") {
        let _ = expect_object(Some(method), "$.result.itemDefaults", defaults)?;
    }
    Ok(())
}

pub(super) fn hover_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let object = expect_object(Some(method), "$.result", value)?;
    let contents = object.get("contents").ok_or_else(|| {
        SchemaError::new(Some(method), "$.result.contents", "hover contents", "missing")
    })?;
    if !contents.is_string() && !contents.is_object() && !contents.is_array() {
        return Err(SchemaError::at_value(
            Some(method),
            "$.result.contents",
            "string, markup object, or array",
            contents,
        ));
    }
    if let Some(range) = object.get("range") {
        validate_range(method, range, "$.result.range")?;
    }
    Ok(())
}

pub(super) fn location_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    if let Some(items) = value.as_array() {
        for (index, item) in items.iter().enumerate() {
            validate_location_or_link(method, item, &format!("$.result[{index}]"))?;
        }
        return Ok(());
    }
    validate_location_or_link(method, value, "$.result")
}

pub(super) fn references_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let items = expect_array(Some(method), "$.result", value)?;
    for (index, item) in items.iter().enumerate() {
        validate_location(method, item, &format!("$.result[{index}]"))?;
    }
    Ok(())
}

pub(super) fn workspace_edit_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    validate_workspace_edit(method, value, "$.result")
}

pub(super) fn text_edits_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let edits = expect_array(Some(method), "$.result", value)?;
    for (index, edit) in edits.iter().enumerate() {
        validate_text_edit(method, edit, &format!("$.result[{index}]"))?;
    }
    Ok(())
}

pub(super) fn semantic_tokens_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let object = expect_object(Some(method), "$.result", value)?;
    let data = expect_array(
        Some(method),
        "$.result.data",
        object.get("data").ok_or_else(|| {
            SchemaError::new(Some(method), "$.result.data", "uinteger array", "missing")
        })?,
    )?;
    for (index, token) in data.iter().enumerate() {
        let _ = expect_uinteger(Some(method), &format!("$.result.data[{index}]"), Some(token))?;
    }
    if data.len() % 5 != 0 {
        return Err(SchemaError::new(
            Some(method),
            "$.result.data",
            "semantic token data length divisible by 5",
            data.len().to_string(),
        ));
    }
    if let Some(result_id) = object.get("resultId") {
        let _ = expect_string(Some(method), "$.result.resultId", Some(result_id))?;
    }
    Ok(())
}

pub(super) fn diagnostic_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let kind = expect_string(Some(method), "$.result.kind", object.get("kind"))?;
    match kind {
        "full" => {
            let diagnostics = expect_array(
                Some(method),
                "$.result.items",
                object.get("items").ok_or_else(|| {
                    SchemaError::new(Some(method), "$.result.items", "array", "missing")
                })?,
            )?;
            validate_object_array(method, diagnostics, "$.result.items")?;
        }
        "unchanged" => {
            let _ = expect_string(Some(method), "$.result.resultId", object.get("resultId"))?;
        }
        other => {
            return Err(SchemaError::new(
                Some(method),
                "$.result.kind",
                "full or unchanged",
                other,
            ));
        }
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

pub(super) fn progress_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_progress_token(method, object.get("token"), "$.params.token")?;
    if object.get("value").is_none() {
        return Err(SchemaError::new(Some(method), "$.params.value", "progress value", "missing"));
    }
    Ok(())
}

pub(super) fn progress_create_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_progress_token(method, object.get("token"), "$.params.token")
}

pub(super) fn registration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let registrations = expect_array(
        Some(method),
        "$.params.registrations",
        object.get("registrations").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.registrations", "array", "missing")
        })?,
    )?;
    for (index, registration) in registrations.iter().enumerate() {
        let path = format!("$.params.registrations[{index}]");
        let registration = expect_object(Some(method), &path, registration)?;
        let _ = expect_string(Some(method), &format!("{path}.id"), registration.get("id"))?;
        let _ = expect_string(Some(method), &format!("{path}.method"), registration.get("method"))?;
        if let Some(options) = registration.get("registerOptions")
            && !options.is_null()
            && !options.is_object()
        {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.registerOptions"),
                "object or null",
                options,
            ));
        }
    }
    Ok(())
}

pub(super) fn unregistration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    // The pinned LSP source preserves this historical field-name typo. The
    // corrected spelling is intentionally not accepted without an explicit
    // compatibility decision and separate contract proof.
    let object = expect_object(Some(method), "$.params", value)?;
    let values = object.get("unregisterations").ok_or_else(|| {
        SchemaError::new(
            Some(method),
            "$.params.unregisterations",
            "unregistration array",
            "missing",
        )
    })?;
    let values = expect_array(Some(method), "$.params.unregisterations", values)?;
    for (index, item) in values.iter().enumerate() {
        let path = format!("$.params.unregisterations[{index}]");
        let item = expect_object(Some(method), &path, item)?;
        let _ = expect_string(Some(method), &format!("{path}.id"), item.get("id"))?;
        let _ = expect_string(Some(method), &format!("{path}.method"), item.get("method"))?;
    }
    Ok(())
}

pub(super) fn configuration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let items = expect_array(
        Some(method),
        "$.params.items",
        object
            .get("items")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.items", "array", "missing"))?,
    )?;
    for (index, item) in items.iter().enumerate() {
        let path = format!("$.params.items[{index}]");
        let item = expect_object(Some(method), &path, item)?;
        if let Some(scope_uri) = item.get("scopeUri")
            && !scope_uri.is_null()
            && !scope_uri.is_string()
        {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.scopeUri"),
                "string or null",
                scope_uri,
            ));
        }
        if let Some(section) = item.get("section")
            && !section.is_null()
            && !section.is_string()
        {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.section"),
                "string or null",
                section,
            ));
        }
    }
    Ok(())
}

pub(super) fn apply_edit_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    if let Some(label) = object.get("label") {
        let _ = expect_string(Some(method), "$.params.label", Some(label))?;
    }
    validate_workspace_edit(
        method,
        object.get("edit").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.edit", "workspace edit", "missing")
        })?,
        "$.params.edit",
    )
}

pub(super) fn apply_edit_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let _ = expect_boolean(Some(method), "$.result.applied", object.get("applied"))?;
    if let Some(reason) = object.get("failureReason") {
        let _ = expect_string(Some(method), "$.result.failureReason", Some(reason))?;
    }
    if let Some(change) = object.get("failedChange") {
        let _ = expect_uinteger(Some(method), "$.result.failedChange", Some(change))?;
    }
    Ok(())
}

pub(super) fn show_message_request_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_integer(Some(method), "$.params.type", object.get("type"))?;
    let _ = expect_string(Some(method), "$.params.message", object.get("message"))?;
    if let Some(actions) = object.get("actions") {
        validate_object_array(
            method,
            expect_array(Some(method), "$.params.actions", actions)?,
            "$.params.actions",
        )?;
    }
    Ok(())
}

pub(super) fn show_document_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.uri", object.get("uri"))?;
    Ok(())
}

pub(super) fn show_document_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let _ = expect_boolean(Some(method), "$.result.success", object.get("success"))?;
    Ok(())
}

pub(super) fn publish_diagnostics_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.uri", object.get("uri"))?;
    let diagnostics = expect_array(
        Some(method),
        "$.params.diagnostics",
        object.get("diagnostics").ok_or_else(|| {
            SchemaError::new(Some(method), "$.params.diagnostics", "array", "missing")
        })?,
    )?;
    validate_object_array(method, diagnostics, "$.params.diagnostics")?;
    if let Some(version) = object.get("version")
        && !version.is_null()
        && version.as_i64().is_none()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.version",
            "integer or null",
            version,
        ));
    }
    Ok(())
}

pub(super) fn log_message_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_integer(Some(method), "$.params.type", object.get("type"))?;
    let _ = expect_string(Some(method), "$.params.message", object.get("message"))?;
    Ok(())
}

fn validate_text_document(
    method: &str,
    object: &Map<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    let document = expect_object(
        Some(method),
        path,
        object
            .get("textDocument")
            .ok_or_else(|| SchemaError::new(Some(method), path, "object", "missing"))?,
    )?;
    let _ = expect_string(Some(method), &format!("{path}.uri"), document.get("uri"))?;
    Ok(())
}

fn validate_position(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    let _ = expect_uinteger(Some(method), &format!("{path}.line"), object.get("line"))?;
    let _ = expect_uinteger(Some(method), &format!("{path}.character"), object.get("character"))?;
    Ok(())
}

fn validate_range(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    validate_position(
        method,
        object.get("start").ok_or_else(|| {
            SchemaError::new(Some(method), format!("{path}.start"), "position", "missing")
        })?,
        &format!("{path}.start"),
    )?;
    validate_position(
        method,
        object.get("end").ok_or_else(|| {
            SchemaError::new(Some(method), format!("{path}.end"), "position", "missing")
        })?,
        &format!("{path}.end"),
    )
}

fn validate_location(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    let _ = expect_string(Some(method), &format!("{path}.uri"), object.get("uri"))?;
    validate_range(
        method,
        object.get("range").ok_or_else(|| {
            SchemaError::new(Some(method), format!("{path}.range"), "range", "missing")
        })?,
        &format!("{path}.range"),
    )
}

fn validate_location_or_link(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    if object.get("uri").is_some() {
        return validate_location(method, value, path);
    }
    let _ = expect_string(Some(method), &format!("{path}.targetUri"), object.get("targetUri"))?;
    validate_range(
        method,
        object.get("targetRange").ok_or_else(|| {
            SchemaError::new(Some(method), format!("{path}.targetRange"), "range", "missing")
        })?,
        &format!("{path}.targetRange"),
    )?;
    validate_range(
        method,
        object.get("targetSelectionRange").ok_or_else(|| {
            SchemaError::new(
                Some(method),
                format!("{path}.targetSelectionRange"),
                "range",
                "missing",
            )
        })?,
        &format!("{path}.targetSelectionRange"),
    )
}

fn validate_text_edit(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    validate_range(
        method,
        object.get("range").ok_or_else(|| {
            SchemaError::new(Some(method), format!("{path}.range"), "range", "missing")
        })?,
        &format!("{path}.range"),
    )?;
    let _ = expect_string(Some(method), &format!("{path}.newText"), object.get("newText"))?;
    Ok(())
}

fn validate_workspace_edit(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    if let Some(changes) = object.get("changes") {
        let changes = expect_object(Some(method), &format!("{path}.changes"), changes)?;
        for (uri, edits) in changes {
            if uri.is_empty() {
                return Err(SchemaError::new(
                    Some(method),
                    format!("{path}.changes.<uri>"),
                    "non-empty URI",
                    "empty",
                ));
            }
            let edits = expect_array(Some(method), &format!("{path}.changes.{uri}"), edits)?;
            for (index, edit) in edits.iter().enumerate() {
                validate_text_edit(method, edit, &format!("{path}.changes.{uri}[{index}]"))?;
            }
        }
    }
    if let Some(changes) = object.get("documentChanges") {
        let _ = expect_array(Some(method), &format!("{path}.documentChanges"), changes)?;
    }
    if let Some(annotations) = object.get("changeAnnotations") {
        let _ = expect_object(Some(method), &format!("{path}.changeAnnotations"), annotations)?;
    }
    Ok(())
}

fn validate_progress_token(
    method: &str,
    value: Option<&Value>,
    path: &str,
) -> Result<(), SchemaError> {
    match value {
        Some(Value::Number(number)) if number.as_i64().is_some() => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(value) => Err(SchemaError::at_value(
            Some(method),
            path,
            "integer or string progress token",
            value,
        )),
        None => Err(SchemaError::new(Some(method), path, "progress token", "missing")),
    }
}

fn validate_object_array(method: &str, values: &[Value], path: &str) -> Result<(), SchemaError> {
    for (index, item) in values.iter().enumerate() {
        let _ = expect_object(Some(method), &format!("{path}[{index}]"), item)?;
    }
    Ok(())
}
