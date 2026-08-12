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
    request!(
        "initialize",
        ClientToServer,
        Lsp317,
        initialize_params,
        initialize_result
    ),
    notification!(
        "initialized",
        ClientToServer,
        Lsp317,
        null_or_empty_object
    ),
    request!(
        "shutdown",
        ClientToServer,
        Lsp317,
        null_only,
        null_only
    ),
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
    notification!(
        "textDocument/didChange",
        ClientToServer,
        Lsp317,
        did_change_params
    ),
    notification!(
        "textDocument/didClose",
        ClientToServer,
        Lsp317,
        text_document_params
    ),
    notification!(
        "textDocument/didOpen",
        ClientToServer,
        Lsp317,
        did_open_params
    ),
    notification!(
        "textDocument/didSave",
        ClientToServer,
        Lsp317,
        did_save_params
    ),
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
    notification!(
        "window/logMessage",
        ServerToClient,
        Lsp317,
        log_message_params
    ),
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
