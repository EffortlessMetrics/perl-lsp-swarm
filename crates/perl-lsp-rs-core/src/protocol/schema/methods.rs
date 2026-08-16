use super::payloads::*;
use super::{Direction, MessageKind, ProtocolVersion, SchemaError};
use serde_json::Value;

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
        inline_completion_result
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
    request!("textDocument/rename", ClientToServer, Lsp317, rename_params, workspace_edit_result),
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
    request!("client/registerCapability", ServerToClient, Lsp317, registration_params, null_only),
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
    request!("workspace/applyEdit", ServerToClient, Lsp317, apply_edit_params, apply_edit_result),
    request!("workspace/configuration", ServerToClient, Lsp317, configuration_params, array_result),
    request!("workspace/diagnostic/refresh", ServerToClient, Lsp317, null_only, null_only),
    request!("workspace/semanticTokens/refresh", ServerToClient, Lsp317, null_only, null_only),
];

pub(super) fn schema_for(
    method: &str,
    wire_direction: Direction,
    kind: MessageKind,
) -> Option<&'static MethodSchema> {
    let (declared_direction, declared_kind) = match kind {
        MessageKind::SuccessResponse | MessageKind::ErrorResponse => {
            // A response travels opposite its originating request, and schemas
            // are registered in the request direction. This is the only place
            // that inversion happens; callers pass the wire direction as-is.
            (wire_direction.opposite(), MessageKind::Request)
        }
        other => (wire_direction, other),
    };

    METHOD_SCHEMAS.iter().find(|schema| {
        schema.method == method
            && schema.direction == declared_direction
            && schema.kind == declared_kind
    })
}

fn inline_completion_result(method: &str, value: &Value) -> Result<(), SchemaError> {
    if value.is_null() {
        return Ok(());
    }

    let (items, items_path) = if let Some(items) = value.as_array() {
        (items, "$.result")
    } else {
        let object = value.as_object().ok_or_else(|| {
            SchemaError::at_value(
                Some(method),
                "$.result",
                "null, inline-completion item array, or InlineCompletionList object",
                value,
            )
        })?;
        let items = object.get("items").and_then(Value::as_array).ok_or_else(|| {
            SchemaError::new(Some(method), "$.result.items", "array", "missing or non-array")
        })?;
        (items, "$.result.items")
    };

    for (index, item) in items.iter().enumerate() {
        let path = format!("{items_path}[{index}]");
        let object = item.as_object().ok_or_else(|| {
            SchemaError::at_value(Some(method), &path, "InlineCompletionItem object", item)
        })?;
        let insert_text = object.get("insertText").ok_or_else(|| {
            SchemaError::new(
                Some(method),
                format!("{path}.insertText"),
                "string or StringValue object",
                "missing",
            )
        })?;
        if !insert_text.is_string() && !insert_text.is_object() {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.insertText"),
                "string or StringValue object",
                insert_text,
            ));
        }
        if let Some(filter_text) = object.get("filterText")
            && !filter_text.is_string()
        {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.filterText"),
                "string",
                filter_text,
            ));
        }
        if let Some(command) = object.get("command")
            && !command.is_object()
        {
            return Err(SchemaError::at_value(
                Some(method),
                format!("{path}.command"),
                "Command object",
                command,
            ));
        }
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_lookup_uses_the_originating_request_direction() {
        let initialize =
            schema_for("initialize", Direction::ServerToClient, MessageKind::SuccessResponse)
                .expect("initialize response must resolve its client-originated request schema");
        assert_eq!(initialize.direction, Direction::ClientToServer);

        let configuration = schema_for(
            "workspace/configuration",
            Direction::ClientToServer,
            MessageKind::ErrorResponse,
        )
        .expect("configuration response must resolve its server-originated request schema");
        assert_eq!(configuration.direction, Direction::ServerToClient);

        let request =
            schema_for("textDocument/hover", Direction::ClientToServer, MessageKind::Request)
                .expect("request direction must remain unchanged");
        assert_eq!(request.direction, Direction::ClientToServer);
    }

    #[test]
    fn inline_completion_result_uses_the_inline_item_union() {
        inline_completion_result(
            "textDocument/inlineCompletion",
            &json!([{ "insertText": "candidate" }]),
        )
        .expect("direct InlineCompletionItem arrays are valid");
        inline_completion_result(
            "textDocument/inlineCompletion",
            &json!({ "items": [{ "insertText": { "value": "candidate" } }] }),
        )
        .expect("InlineCompletionList objects are valid");
        inline_completion_result("textDocument/inlineCompletion", &Value::Null)
            .expect("null is a valid inline-completion result");

        let missing = inline_completion_result(
            "textDocument/inlineCompletion",
            &json!([{ "label": "ordinary completion item" }]),
        )
        .expect_err("ordinary CompletionItem shape must not pass as InlineCompletionItem");
        assert_eq!(missing.path, "$.result[0].insertText");

        let wrong = inline_completion_result(
            "textDocument/inlineCompletion",
            &json!({ "items": [{ "insertText": 7 }] }),
        )
        .expect_err("inline insertText must use the declared union");
        assert_eq!(wrong.path, "$.result.items[0].insertText");
    }
}
