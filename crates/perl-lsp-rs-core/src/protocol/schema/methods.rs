use super::payloads::{
    cancel_params, initialize_params, initialize_result, null_or_empty_object, null_params,
    null_result, show_message_request_params, show_message_request_result, window_message_params,
};
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

/// Lifecycle, cancellation, and the window-message family. Remaining method
/// payloads stay on #10477 so each family stays within RIPR's review budget.
static METHOD_SCHEMAS: &[MethodSchema] = &[
    notification!("$/cancelRequest", ClientToServer, Lsp317, cancel_params),
    notification!("exit", ClientToServer, Lsp317, null_or_empty_object),
    request!("initialize", ClientToServer, Lsp317, initialize_params, initialize_result),
    notification!("initialized", ClientToServer, Lsp317, null_or_empty_object),
    request!("shutdown", ClientToServer, Lsp317, null_params, null_result),
    // The base protocol lets either party cancel a request it previously sent.
    notification!("$/cancelRequest", ServerToClient, Lsp317, cancel_params),
    notification!("window/logMessage", ServerToClient, Lsp317, window_message_params),
    notification!("window/showMessage", ServerToClient, Lsp317, window_message_params),
    request!(
        "window/showMessageRequest",
        ServerToClient,
        Lsp317,
        show_message_request_params,
        show_message_request_result
    ),
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
