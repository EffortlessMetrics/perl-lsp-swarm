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
use crate::runtime::window::MessageType;
use crate::security::{is_text_sync_method, refusal_desynchronizes_document};

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
            // A notification has no response envelope, so returning here alone
            // makes the refusal invisible: the client keeps a buffer it believes
            // this server holds, and every later answer about that URI is
            // computed from different bytes. For text synchronization that is
            // not a silent no-op, so the refusal is reported to the client and
            // the document is desynchronized instead (#16653).
            report_refused_text_sync_notification(server, &request.method, params_ref, &err);
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

/// What a refused text-sync notification changed, and therefore whether the
/// client still needs to be told.
enum RefusalDisposition {
    /// Nothing to desynchronize: either the method does not carry document
    /// content, or no open document backs this URI. A refused `didOpen` names a
    /// document this server never stored, so no provider can read it and the
    /// refusal is reported every time — it is a discrete client action, not a
    /// repeating stream.
    NothingToDesynchronize,
    /// The document entered full-sync desynchronization on this refusal.
    EnteredDesync,
    /// An earlier refusal already desynchronized the document and already told
    /// the client. Repeating it would raise one popup per keystroke for a
    /// client still editing a buffer this server cannot accept.
    AlreadyDesynchronized,
}

/// Report a refused text-synchronization notification to the client, and put an
/// already-open document into the desynchronization its user-facing providers
/// already honour.
///
/// The refusal happens before routing, so no handler runs and no error envelope
/// exists: without this the client would believe its buffer is synchronized
/// while the server's copy of it is not (#16653).
fn report_refused_text_sync_notification(
    server: &LspServer,
    method: &str,
    params: &Value,
    err: &anyhow::Error,
) {
    if !is_text_sync_method(method) {
        return;
    }

    let Some(uri) = params
        .get("textDocument")
        .and_then(|text_document| text_document.get("uri"))
        .and_then(Value::as_str)
    else {
        // Params that never decoded to a URI name no document and
        // desynchronize nothing. The `tracing::debug!` at the call site is the
        // receipt; there is nothing to tell the client about.
        return;
    };

    let disposition = if refusal_desynchronizes_document(method) {
        desynchronize_open_document(server, uri)
    } else {
        RefusalDisposition::NothingToDesynchronize
    };

    let recovery = match disposition {
        RefusalDisposition::NothingToDesynchronize => "",
        RefusalDisposition::EnteredDesync => " Analysis is paused until the document is reopened.",
        RefusalDisposition::AlreadyDesynchronized => return,
    };

    server.show_message_or_log(
        MessageType::Error,
        &format!("{method} for {uri} was refused: {err}.{recovery}"),
    );
}

/// Enter full-sync desynchronization for `uri` when this server holds it open.
fn desynchronize_open_document(server: &LspServer, uri: &str) -> RefusalDisposition {
    let mut documents = server.documents.lock();
    let Some(document) = server.get_document_mut(&mut documents, uri) else {
        return RefusalDisposition::NothingToDesynchronize;
    };
    if document.full_sync_required() {
        return RefusalDisposition::AlreadyDesynchronized;
    }
    // The same transition the rejected-`didChange` contract uses: user-facing
    // providers stop answering from the stored text until an accepted full
    // replacement recovers it. Formatting already refuses in this state
    // (`CONTENT_MODIFIED`), so a stale snapshot can no longer be returned as
    // edits against a buffer holding different bytes.
    document.mark_full_sync_required();
    RefusalDisposition::EnteredDesync
}

fn auto_initialize_for_compat(server: &LspServer, request: &JsonRpcRequest) {
    if !server.initialized.load(Ordering::Acquire)
        && server.initialization_accepted()
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
