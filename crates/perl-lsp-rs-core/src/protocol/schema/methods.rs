use super::{
    Direction, MessageKind, ProtocolVersion, SchemaError, expect_array, expect_boolean,
    expect_integer, expect_null, expect_object, expect_string, expect_uinteger, observed,
};
use serde_json::{Map, Value};

pub(super) type PayloadValidator = fn(&str, &Value) -> Result<(), SchemaError>;

pub(super) struct MethodSchema {
    pub(super) method: &'static str,
    pub(super) direction: Direction,
    pub(super) kind: MessageKind,
    pub(super) version: ProtocolVersion,
    pub(super) params: PayloadValidator,
    pub(super) result: Option<PayloadValidator>,
}

macro_rules! request {
    ($method:literal, $direction:ident, $version:ident, $params:ident, $result:ident) => {
        MethodSchema {
            method: $method,
            direction: Direction::$direction,
            kind: MessageKind::Request,
            version: ProtocolVersion::$version,
            params: $params,
            result: Some($result),
        }
    };
}

macro_rules! notification {
    ($method:literal, $direction:ident, $version:ident, $params:ident) => {
        MethodSchema {
            method: $method,
            direction: Direction::$direction,
            kind: MessageKind::Notification,
            version: ProtocolVersion::$version,
            params: $params,
            result: None,
        }
    };
}

static METHOD_SCHEMAS: &[MethodSchema] = &[
    notification!("$/cancelRequest", ClientToServer, Lsp317, cancel_params),
    notification!("$/progress", ClientToServer, Lsp317, progress_params),
    notification!("exit", ClientToServer, Lsp317, null_or_empty_object),
    request!("initialize", ClientToServer, Lsp317, initialize_params, initialize_result),
    notification!("initialized", ClientToServer, Lsp317, null_or_empty_object),
    request!("shutdown", ClientToServer, Lsp317, null_only, null_only),
    request!(
        "textDocument/completion",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        completion_result
    ),
    request!(
        "textDocument/declaration",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        location_result
    ),
    request!(
        "textDocument/definition",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        location_result
    ),
    request!(
        "textDocument/diagnostic",
        ClientToServer,
        Lsp317,
        text_document_params,
        diagnostic_result
    ),
    notification!("textDocument/didChange", ClientToServer, Lsp317, did_change_params),
    notification!("textDocument/didClose", ClientToServer, Lsp317, text_document_params),
    notification!("textDocument/didOpen", ClientToServer, Lsp317, did_open_params),
    notification!("textDocument/didSave", ClientToServer, Lsp317, did_save_params),
    request!(
        "textDocument/formatting",
        ClientToServer,
        Lsp317,
        formatting_params,
        text_edits_result
    ),
    request!(
        "textDocument/hover",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        hover_result
    ),
    request!(
        "textDocument/implementation",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        location_result
    ),
    request!(
        "textDocument/inlineCompletion",
        ClientToServer,
        Lsp318Development,
        inline_completion_params,
        completion_result
    ),
    request!(
        "textDocument/rangeFormatting",
        ClientToServer,
        Lsp317,
        range_formatting_params,
        text_edits_result
    ),
    request!(
        "textDocument/rangesFormatting",
        ClientToServer,
        Lsp318Development,
        ranges_formatting_params,
        text_edits_result
    ),
    request!(
        "textDocument/references",
        ClientToServer,
        Lsp317,
        reference_params,
        references_result
    ),
    request!(
        "textDocument/rename",
        ClientToServer,
        Lsp317,
        rename_params,
        workspace_edit_result
    ),
    request!(
        "textDocument/semanticTokens/full",
        ClientToServer,
        Lsp317,
        text_document_params,
        semantic_tokens_result
    ),
    request!(
        "textDocument/typeDefinition",
        ClientToServer,
        Lsp317,
        text_document_position_params,
        location_result
    ),
    notification!("$/progress", ServerToClient, Lsp317, progress_params),
    request!(
        "client/registerCapability",
        ServerToClient,
        Lsp317,
        registration_params,
        null_only
    ),
    request!(
        "client/unregisterCapability",
        ServerToClient,
        Lsp317,
        unregistration_params,
        null_only
    ),
    notification!(
        "textDocument/publishDiagnostics",
        ServerToClient,
        Lsp317,
        publish_diagnostics_params
    ),
    notification!("window/logMessage", ServerToClient, Lsp317, log_message_params),
    request!(
        "window/showDocument",
        ServerToClient,
        Lsp317,
        show_document_params,
        show_document_result
    ),
    request!(
        "window/showMessageRequest",
        ServerToClient,
        Lsp317,
        show_message_request_params,
        nullable_object_result
    ),
    request!(
        "window/workDoneProgress/create",
        ServerToClient,
        Lsp317,
        progress_create_params,
        null_only
    ),
    request!(
        "workspace/applyEdit",
        ServerToClient,
        Lsp317,
        apply_edit_params,
        apply_edit_result
    ),
    request!(
        "workspace/configuration",
        ServerToClient,
        Lsp317,
        configuration_params,
        array_result
    ),
    request!(
        "workspace/diagnostic/refresh",
        ServerToClient,
        Lsp317,
        null_only,
        null_only
    ),
    request!(
        "workspace/semanticTokens/refresh",
        ServerToClient,
        Lsp317,
        null_only,
        null_only
    ),
];

pub(super) fn schema_for(
    method: &str,
    direction: Direction,
    kind: MessageKind,
) -> Option<&'static MethodSchema> {
    let declared_kind = match kind {
        MessageKind::SuccessResponse | MessageKind::ErrorResponse => MessageKind::Request,
        other => other,
    };
    METHOD_SCHEMAS.iter().find(|schema| {
        schema.method == method && schema.direction == direction && schema.kind == declared_kind
    })
}

/// Return deterministic method/direction/kind/version identities for drift checks.
#[must_use]
pub fn registered_schema_identities() -> Vec<String> {
    METHOD_SCHEMAS
        .iter()
        .map(|schema| {
            format!(
                "{}:{}:{}:{}",
                schema.direction.schema_token(),
                schema.kind.schema_token(),
                schema.method,
                schema.version.schema_token()
            )
        })
        .collect()
}

fn null_only(method: &str, value: &Value) -> Result<(), SchemaError> {
    expect_null(Some(method), "$.payload", value)
}

fn null_or_empty_object(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() || value.as_object().is_some_and(Map::is_empty) {
        Ok(())
    } else {
        Err(SchemaError::at_value(
            Some(method),
            "$.params",
            "null or empty object",
            value,
        ))
    }
}

fn array_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let _ = expect_array(Some(method), "$.result", value)?;
    Ok(())
}

fn nullable_object_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() || value.is_object() {
        Ok(())
    } else {
        Err(SchemaError::at_value(
            Some(method),
            "$.result",
            "null or object",
            value,
        ))
    }
}

fn initialize_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_object(
        Some(method),
        "$.params.capabilities",
        object
            .get("capabilities")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.capabilities", "object", "missing"))?,
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

fn initialize_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let capabilities = expect_object(
        Some(method),
        "$.result.capabilities",
        object
            .get("capabilities")
            .ok_or_else(|| SchemaError::new(Some(method), "$.result.capabilities", "object", "missing"))?,
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

fn text_document_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")
}

fn text_document_position_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    validate_position(
        method,
        object
            .get("position")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.position", "object", "missing"))?,
        "$.params.position",
    )
}

fn reference_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    let context = expect_object(
        Some(method),
        "$.params.context",
        object
            .get("context")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.context", "object", "missing"))?,
    )?;
    let _ = expect_boolean(
        Some(method),
        "$.params.context.includeDeclaration",
        context.get("includeDeclaration"),
    )?;
    Ok(())
}

fn rename_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.newName", object.get("newName"))?;
    Ok(())
}

fn formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    let _ = expect_object(
        Some(method),
        "$.params.options",
        object
            .get("options")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.options", "object", "missing"))?,
    )?;
    Ok(())
}

fn range_formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn ranges_formatting_params(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn inline_completion_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    text_document_position_params(method, value)?;
    let object = expect_object(Some(method), "$.params", value)?;
    if let Some(context) = object.get("context") {
        let _ = expect_object(Some(method), "$.params.context", context)?;
    }
    Ok(())
}

fn did_open_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let document = expect_object(
        Some(method),
        "$.params.textDocument",
        object
            .get("textDocument")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.textDocument", "object", "missing"))?,
    )?;
    let _ = expect_string(Some(method), "$.params.textDocument.uri", document.get("uri"))?;
    let _ = expect_string(
        Some(method),
        "$.params.textDocument.languageId",
        document.get("languageId"),
    )?;
    let _ = expect_integer(
        Some(method),
        "$.params.textDocument.version",
        document.get("version"),
    )?;
    let _ = expect_string(Some(method), "$.params.textDocument.text", document.get("text"))?;
    Ok(())
}

fn did_change_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let document = expect_object(
        Some(method),
        "$.params.textDocument",
        object
            .get("textDocument")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.textDocument", "object", "missing"))?,
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
        object
            .get("contentChanges")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.contentChanges", "array", "missing"))?,
    )?;
    for (index, change) in changes.iter().enumerate() {
        let change = expect_object(Some(method), &format!("$.params.contentChanges[{index}]"), change)?;
        let _ = expect_string(
            Some(method),
            &format!("$.params.contentChanges[{index}].text"),
            change.get("text"),
        )?;
        if let Some(range) = change.get("range") {
            validate_range(
                method,
                range,
                &format!("$.params.contentChanges[{index}].range"),
            )?;
        }
    }
    Ok(())
}

fn did_save_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_text_document(method, object, "$.params.textDocument")?;
    if let Some(text) = object.get("text")
        && !text.is_null()
        && !text.is_string()
    {
        return Err(SchemaError::at_value(
            Some(method),
            "$.params.text",
            "string or null",
            text,
        ));
    }
    Ok(())
}

fn completion_result(method: &str, value: &Value) -> Result<(), SchemaError> {
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
    if let Some(incomplete) = object.get("isIncomplete") {
        let _ = expect_boolean(Some(method), "$.result.isIncomplete", Some(incomplete))?;
    }
    if let Some(defaults) = object.get("itemDefaults") {
        let _ = expect_object(Some(method), "$.result.itemDefaults", defaults)?;
    }
    Ok(())
}

fn hover_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let object = expect_object(Some(method), "$.result", value)?;
    let contents = object
        .get("contents")
        .ok_or_else(|| SchemaError::new(Some(method), "$.result.contents", "hover contents", "missing"))?;
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

fn location_result(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn references_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let items = expect_array(Some(method), "$.result", value)?;
    for (index, item) in items.iter().enumerate() {
        validate_location(method, item, &format!("$.result[{index}]"))?;
    }
    Ok(())
}

fn workspace_edit_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    validate_workspace_edit(method, value, "$.result")
}

fn text_edits_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let edits = expect_array(Some(method), "$.result", value)?;
    for (index, edit) in edits.iter().enumerate() {
        validate_text_edit(method, edit, &format!("$.result[{index}]"))?;
    }
    Ok(())
}

fn semantic_tokens_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }
    let object = expect_object(Some(method), "$.result", value)?;
    let data = expect_array(
        Some(method),
        "$.result.data",
        object
            .get("data")
            .ok_or_else(|| SchemaError::new(Some(method), "$.result.data", "uinteger array", "missing"))?,
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

fn diagnostic_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let kind = expect_string(Some(method), "$.result.kind", object.get("kind"))?;
    match kind {
        "full" => {
            let diagnostics = expect_array(
                Some(method),
                "$.result.items",
                object
                    .get("items")
                    .ok_or_else(|| SchemaError::new(Some(method), "$.result.items", "array", "missing"))?,
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

fn cancel_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    match object.get("id") {
        Some(Value::Number(number)) if number.as_i64().is_some() => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(value) => Err(SchemaError::at_value(
            Some(method),
            "$.params.id",
            "integer or string",
            value,
        )),
        None => Err(SchemaError::new(Some(method), "$.params.id", "request ID", "missing")),
    }
}

fn progress_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_progress_token(method, object.get("token"), "$.params.token")?;
    if object.get("value").is_none() {
        return Err(SchemaError::new(Some(method), "$.params.value", "progress value", "missing"));
    }
    Ok(())
}

fn progress_create_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    validate_progress_token(method, object.get("token"), "$.params.token")
}

fn registration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let registrations = expect_array(
        Some(method),
        "$.params.registrations",
        object
            .get("registrations")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.registrations", "array", "missing"))?,
    )?;
    for (index, registration) in registrations.iter().enumerate() {
        let path = format!("$.params.registrations[{index}]");
        let registration = expect_object(Some(method), &path, registration)?;
        let _ = expect_string(Some(method), &format!("{path}.id"), registration.get("id"))?;
        let _ = expect_string(
            Some(method),
            &format!("{path}.method"),
            registration.get("method"),
        )?;
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

fn unregistration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let values = object
        .get("unregisterations")
        .or_else(|| object.get("unregistrations"))
        .ok_or_else(|| {
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

fn configuration_params(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn apply_edit_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    if let Some(label) = object.get("label") {
        let _ = expect_string(Some(method), "$.params.label", Some(label))?;
    }
    validate_workspace_edit(
        method,
        object
            .get("edit")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.edit", "workspace edit", "missing"))?,
        "$.params.edit",
    )
}

fn apply_edit_result(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn show_message_request_params(method: &str, value: &Value) -> Result<(), SchemaError> {
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

fn show_document_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.uri", object.get("uri"))?;
    Ok(())
}

fn show_document_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.result", value)?;
    let _ = expect_boolean(Some(method), "$.result.success", object.get("success"))?;
    Ok(())
}

fn publish_diagnostics_params(method: &str, value: &Value) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), "$.params", value)?;
    let _ = expect_string(Some(method), "$.params.uri", object.get("uri"))?;
    let diagnostics = expect_array(
        Some(method),
        "$.params.diagnostics",
        object
            .get("diagnostics")
            .ok_or_else(|| SchemaError::new(Some(method), "$.params.diagnostics", "array", "missing"))?,
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

fn log_message_params(method: &str, value: &Value) -> Result<(), SchemaError> {
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
    let _ = expect_uinteger(
        Some(method),
        &format!("{path}.character"),
        object.get("character"),
    )?;
    Ok(())
}

fn validate_range(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    validate_position(
        method,
        object
            .get("start")
            .ok_or_else(|| SchemaError::new(Some(method), format!("{path}.start"), "position", "missing"))?,
        &format!("{path}.start"),
    )?;
    validate_position(
        method,
        object
            .get("end")
            .ok_or_else(|| SchemaError::new(Some(method), format!("{path}.end"), "position", "missing"))?,
        &format!("{path}.end"),
    )
}

fn validate_location(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    let _ = expect_string(Some(method), &format!("{path}.uri"), object.get("uri"))?;
    validate_range(
        method,
        object
            .get("range")
            .ok_or_else(|| SchemaError::new(Some(method), format!("{path}.range"), "range", "missing"))?,
        &format!("{path}.range"),
    )
}

fn validate_location_or_link(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    if object.get("uri").is_some() {
        return validate_location(method, value, path);
    }
    let _ = expect_string(
        Some(method),
        &format!("{path}.targetUri"),
        object.get("targetUri"),
    )?;
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
        object
            .get("range")
            .ok_or_else(|| SchemaError::new(Some(method), format!("{path}.range"), "range", "missing"))?,
        &format!("{path}.range"),
    )?;
    let _ = expect_string(
        Some(method),
        &format!("{path}.newText"),
        object.get("newText"),
    )?;
    Ok(())
}

fn validate_workspace_edit(method: &str, value: &Value, path: &str) -> Result<(), SchemaError> {
    let object = expect_object(Some(method), path, value)?;
    if object.get("changes").is_none() && object.get("documentChanges").is_none() {
        return Err(SchemaError::new(
            Some(method),
            path,
            "changes or documentChanges workspace edit",
            observed(value),
        ));
    }
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
            let edits = expect_array(
                Some(method),
                &format!("{path}.changes.{uri}"),
                edits,
            )?;
            for (index, edit) in edits.iter().enumerate() {
                validate_text_edit(
                    method,
                    edit,
                    &format!("{path}.changes.{uri}[{index}]"),
                )?;
            }
        }
    }
    if let Some(changes) = object.get("documentChanges") {
        let _ = expect_array(Some(method), &format!("{path}.documentChanges"), changes)?;
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
        None => Err(SchemaError::new(
            Some(method),
            path,
            "progress token",
            "missing",
        )),
    }
}

fn validate_object_array(method: &str, values: &[Value], path: &str) -> Result<(), SchemaError> {
    for (index, item) in values.iter().enumerate() {
        let _ = expect_object(Some(method), &format!("{path}[{index}]"), item)?;
    }
    Ok(())
}
