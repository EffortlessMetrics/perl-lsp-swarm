//! Request preflight checks before routing.
//!
//! Keeps cancellation registration and compatibility initialization separate from
//! method routing and response construction.

// #7098 co-locates the bounded lifecycle substrate at the request-admission
// boundary. It does not become the live admission owner until #7100 wires it
// into prepare_request, cancellation, supersession, and response finalization.
#[expect(dead_code, reason = "incoming request owner remains unwired until #7100")]
#[path = "request_lifecycle.rs"]
pub(super) mod request_lifecycle;

use super::super::{
    JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, LspServer, Ordering, Value,
};
use super::request_cancellation::{handle_cancel_notification, register_request_cancellation};

pub(super) struct RequestContext {
    pub(super) id: Option<Value>,
    pub(super) should_respond: bool,
}

impl RequestContext {
    pub(super) fn from_request(request: &JsonRpcRequest) -> Self {
        let id = request.id.as_ref().map(JsonRpcId::to_value);
        let should_respond = id.is_some();

        Self { id, should_respond }
    }
}

pub(super) enum PreflightOutcome {
    Continue,
    NotificationHandled,
    Respond(JsonRpcResponse),
}

pub(super) fn prepare_request(
    server: &LspServer,
    request: &JsonRpcRequest,
    context: &RequestContext,
) -> PreflightOutcome {
    // Structural admission runs before cancellation dispatch and registration,
    // deliberately. Cancellation handling remains ahead of token registration.
    //
    // `register_request_cancellation` inserts a token and a cloned cleanup
    // context into the global registry, and the entry is normally removed by
    // `finalize_response` or a handler cleanup guard. Rejecting here returns
    // early and reaches neither, so admitting after registration would leak one
    // registry entry — holding its cloned params — per rejected request, letting
    // a client grow server memory without bound by replaying invalid requests
    // under fresh ids. Nothing in admission needs the registry, so the check
    // simply moves ahead of it.
    //
    // Admission is protocol-generic only (issue #8895): method-name length and
    // serialized-payload resource bounds. It must never inspect parameter
    // *content* — source, documentation, labels, and extension payloads are
    // inert data at this boundary. Method-specific policy lives at its sinks:
    // unknown methods reach routing's -32601, malformed params reach handler
    // typed decode (-32602), and executeCommand/path/trust/rendering refusals
    // are typed failures owned by those operations.
    let null = Value::Null;
    let params_ref = request.params.as_ref().unwrap_or(&null);
    if let Err(err) = crate::security::validate_request_admission(&request.method, params_ref) {
        tracing::debug!(method = %request.method, %err, "Rejected request: structural admission failed");
        if !context.should_respond {
            return PreflightOutcome::NotificationHandled;
        }
        return PreflightOutcome::Respond(JsonRpcResponse {
            jsonrpc: "2.0",
            id: context.id.as_ref().and_then(JsonRpcId::from_value),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: format!("Invalid request: {err}"),
                data: None,
            }),
        });
    }

    if handle_cancel_notification(server, request) {
        return PreflightOutcome::NotificationHandled;
    }

    if let Some(cancelled) = register_request_cancellation(server, context.id.as_ref(), request) {
        return PreflightOutcome::Respond(cancelled);
    }

    auto_initialize_for_compat(server, request);

    PreflightOutcome::Continue
}

fn auto_initialize_for_compat(server: &LspServer, request: &JsonRpcRequest) {
    if !server.initialized.load(Ordering::Acquire)
        && server.initialize_requested.load(Ordering::Acquire)
        && !is_lifecycle_method(&request.method)
    {
        server.auto_initialize_for_compat(&request.method);
        // Mirror the post-`initialized` configuration pull for clients that
        // skip the notification (#7708).
        if server.initialized.load(Ordering::Acquire) {
            server.request_workspace_configuration_for_folders();
        }
    }
}

fn is_lifecycle_method(method: &str) -> bool {
    matches!(method, "initialize" | "initialized" | "shutdown" | "exit")
}
