use super::{
    JsonRpcRequest, JsonRpcResponse, LspServer, Value, cancelled_response_with_method,
    enhanced_cancelled_response,
};
use crate::cancellation::{PerlLspCancellationToken, ProviderCleanupContext};
use crate::protocol::JsonRpcId;
use std::time::Instant;

pub(super) fn handle_cancel_notification(server: &LspServer, request: &JsonRpcRequest) -> bool {
    if request.method != "$/cancelRequest" {
        return false;
    }

    if let Some(params) = request.params.as_ref()
        && let Some(idv) = params.get("id")
        && let Some(typed_id) = JsonRpcId::try_from_value(idv)
    {
        let start_time = Instant::now();
        if let Ok(_cleanup_context) = server.cancellation_registry.cancel_request(&typed_id) {
            let latency = start_time.elapsed();
            tracing::debug!(latency = ?latency, request = %typed_id, "Enhanced cancellation processed");
            if latency.as_millis() > 50 {
                tracing::warn!(latency = ?latency, "Cancellation latency exceeded 50ms");
            }
        }
        server.mark_cancelled_if_pending(&typed_id);
    }

    true
}

pub(super) fn register_request_cancellation(
    server: &LspServer,
    request_id: Option<&Value>,
    request: &JsonRpcRequest,
) -> Option<JsonRpcResponse> {
    let request_id = request_id?;

    // Convert the raw Value request ID to a typed JsonRpcId at the boundary.
    // If the value isn't a valid request ID shape, skip cancellation tracking.
    let typed_id = JsonRpcId::try_from_value(request_id)?;

    if server.is_cancelled(&typed_id) {
        return Some(cancelled_response_with_method(request_id, &request.method));
    }

    if !method_supports_cancellation(&request.method) {
        return None;
    }

    let token = PerlLspCancellationToken::new(typed_id.clone(), request.method.clone());
    let cleanup_context =
        ProviderCleanupContext::new(request.method.clone(), request.params.clone());

    if let Err(error) = server.cancellation_registry.register_token(token) {
        tracing::trace!(%error, "cancellation: failed to register token");
    }
    if let Err(error) = server.cancellation_registry.register_cleanup(&typed_id, cleanup_context) {
        tracing::trace!(%error, "cancellation: failed to register cleanup");
    }

    if server.cancellation_registry.is_cancelled(&typed_id) {
        if let Some(token) = server.cancellation_registry.get_token(&typed_id) {
            let cleanup_context = server
                .cancellation_registry
                .cancel_request(&typed_id)
                .map_err(|error| {
                    tracing::trace!(%error, "cancellation: failed to cancel request (early)");
                })
                .ok()
                .flatten();
            // Clean up the token/cleanup we just registered so the maps
            // don't retain entries for requests that never reach the handler
            // (#5032).
            server.cancellation_registry.remove_request(&typed_id);
            return Some(enhanced_cancelled_response(&token, cleanup_context.as_ref()));
        }
        // Same cleanup for the fallback path.
        server.cancellation_registry.remove_request(&typed_id);
        return Some(cancelled_response_with_method(request_id, &request.method));
    }

    None
}

fn method_supports_cancellation(method: &str) -> bool {
    matches!(
        method,
        "textDocument/completion"
            | "textDocument/hover"
            | "textDocument/definition"
            | "textDocument/references"
            | "textDocument/documentSymbol"
            | "textDocument/codeAction"
            | "textDocument/formatting"
            | "textDocument/rangeFormatting"
            | "textDocument/rangesFormatting"
            | "textDocument/onTypeFormatting"
            | "textDocument/rename"
            | "workspace/symbol"
            | "callHierarchy/incomingCalls"
            | "callHierarchy/outgoingCalls"
            | "textDocument/prepareTypeHierarchy"
            | "typeHierarchy/prepare"
            | "typeHierarchy/supertypes"
            | "typeHierarchy/subtypes"
            | "textDocument/inlayHint"
    )
}

pub(super) fn finalize_cancellation_state(
    server: &LspServer,
    request_id: Option<&Value>,
) -> Option<JsonRpcResponse> {
    let request_id = request_id?;
    // Convert at the boundary; if conversion fails, skip registry interaction.
    let typed_id = JsonRpcId::try_from_value(request_id)?;

    if let Some(token) = server.cancellation_registry.get_token(&typed_id)
        && token.is_cancelled()
    {
        let cleanup_context = server
            .cancellation_registry
            .cancel_request(&typed_id)
            .map_err(|error| {
                tracing::trace!(%error, "cancellation: failed to cancel request (post-dispatch)");
            })
            .ok()
            .flatten();
        server.cancellation_registry.remove_request(&typed_id);
        return Some(enhanced_cancelled_response(&token, cleanup_context.as_ref()));
    }

    server.cancellation_registry.remove_request(&typed_id);
    None
}

#[cfg(test)]
mod tests {
    use super::{
        JsonRpcRequest, LspServer, Value, handle_cancel_notification, method_supports_cancellation,
        register_request_cancellation,
    };
    use serde_json::json;

    fn cancellation_request(id: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "$/cancelRequest".to_string(),
            params: Some(json!({ "id": id })),
        }
    }

    fn hover_request() -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "textDocument/hover".to_string(),
            params: Some(json!({})),
        }
    }

    #[test]
    fn scheduler_pending_cancel_survives_pre_registration_and_settlement()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let numeric = Value::from(71003_i64);
        let string = Value::from("71003");
        server.cancellation_registry.remove_request(&crate::protocol::JsonRpcId::Integer(71003));
        server
            .cancellation_registry
            .remove_request(&crate::protocol::JsonRpcId::String("71003".to_string()));

        server.mark_request_pending(&crate::protocol::JsonRpcId::Integer(71003));
        if !handle_cancel_notification(&server, &cancellation_request(numeric.clone())) {
            return Err("cancel notification was not handled".into());
        }
        let cancelled = register_request_cancellation(&server, Some(&numeric), &hover_request())
            .ok_or("pending cancellation was not observed before token registration")?;
        if cancelled.error.as_ref().map(|error| error.code) != Some(-32800) {
            return Err("pending cancellation did not produce request-cancelled".into());
        }

        if !handle_cancel_notification(&server, &cancellation_request(string.clone())) {
            return Err("string cancel notification was not handled".into());
        }
        if register_request_cancellation(&server, Some(&string), &hover_request()).is_some() {
            return Err("unrelated string ID was incorrectly cancelled".into());
        }
        server
            .cancellation_registry
            .remove_request(&crate::protocol::JsonRpcId::String("71003".to_string()));

        server.clear_request_pending(&crate::protocol::JsonRpcId::Integer(71003));
        if register_request_cancellation(&server, Some(&numeric), &hover_request()).is_some() {
            return Err("settled numeric ID retained cancellation state".into());
        }
        server.cancellation_registry.remove_request(&crate::protocol::JsonRpcId::Integer(71003));
        Ok(())
    }

    #[test]
    fn cancellation_does_not_cross_server_sessions() -> Result<(), Box<dyn std::error::Error>> {
        let server_a = LspServer::new();
        let server_b = LspServer::new();
        for id in [json!(71004), json!("71004")] {
            let typed_id = crate::protocol::JsonRpcId::from_value(&id).ok_or("invalid id")?;
            server_b.mark_request_pending(&typed_id);
            if register_request_cancellation(&server_b, Some(&id), &hover_request()).is_some() {
                return Err("fresh B request was unexpectedly cancelled".into());
            }
            // A has no request. An unknown cancel must not touch B.
            handle_cancel_notification(&server_a, &cancellation_request(id.clone()));
            if server_b.cancellation_registry.is_cancelled(&typed_id) {
                return Err("unknown cancellation crossed sessions".into());
            }
            server_a.mark_request_pending(&typed_id);
            if register_request_cancellation(&server_a, Some(&id), &hover_request()).is_some() {
                return Err("fresh A request was unexpectedly cancelled".into());
            }
            // A provider cleanup in B must not remove A's token.
            {
                let _guard = crate::cancellation::RequestCleanupGuard::for_registry(
                    server_b.cancellation_registry.clone(),
                    Some(&typed_id),
                );
            }
            if server_a.cancellation_registry.get_token(&typed_id).is_none() {
                return Err("provider cleanup crossed sessions".into());
            }
            if register_request_cancellation(&server_b, Some(&id), &hover_request()).is_some() {
                return Err("B could not register after provider cleanup".into());
            }
            // Both sessions now have the same wire ID. Cancel A only.
            handle_cancel_notification(&server_a, &cancellation_request(id.clone()));
            let a = super::finalize_cancellation_state(&server_a, Some(&id));
            let b = super::finalize_cancellation_state(&server_b, Some(&id));
            server_a.clear_request_pending(&typed_id);
            server_b.clear_request_pending(&typed_id);
            if a.and_then(|response| response.error).map(|error| error.code) != Some(-32800) {
                return Err("owning session did not cancel its request".into());
            }
            if b.is_some() {
                return Err("another session cancelled server B's request".into());
            }
            // Completion and cancellation must not poison reuse in either session.
            if register_request_cancellation(&server_a, Some(&id), &hover_request()).is_some()
                || register_request_cancellation(&server_b, Some(&id), &hover_request()).is_some()
            {
                return Err("completed cancellation poisoned ID reuse".into());
            }
            super::finalize_cancellation_state(&server_a, Some(&id));
            super::finalize_cancellation_state(&server_b, Some(&id));
        }
        Ok(())
    }

    #[test]
    fn hierarchy_and_formatting_methods_are_registered_for_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        for method in [
            "textDocument/prepareTypeHierarchy",
            "typeHierarchy/prepare",
            "typeHierarchy/supertypes",
            "typeHierarchy/subtypes",
            "textDocument/formatting",
            "textDocument/rangeFormatting",
            "textDocument/rangesFormatting",
            "textDocument/onTypeFormatting",
        ] {
            if !method_supports_cancellation(method) {
                return Err(std::io::Error::other(format!(
                    "{method} must be registered for cancellation"
                ))
                .into());
            }
        }

        Ok(())
    }
}
