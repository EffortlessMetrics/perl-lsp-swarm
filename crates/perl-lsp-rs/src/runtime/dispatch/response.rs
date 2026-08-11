//! JSON-RPC response construction for dispatched requests.

use super::super::{JsonRpcError, JsonRpcId, JsonRpcResponse, Value};
use super::request_cancellation::finalize_cancellation_state;
use perl_parser_core::ErrorCategory;

/// Provisionally classify a JsonRpcError by its error code (#4980 PR-1).
///
/// Once JsonRpcError implements ErrorClass directly, this function should
/// delegate to `error.error_class()`. Until then, we classify by the
/// well-known JSON-RPC / LSP error codes so the tracing layer captures
/// structured error category data without string sniffing.
fn classify_jsonrpc_error(error: &JsonRpcError) -> ErrorCategory {
    match error.code {
        // -32700 Parse error: malformed JSON — protocol violation.
        // -32600 Invalid Request: not a valid request — protocol violation.
        -32700 | -32600 => ErrorCategory::Protocol,
        // -32601 Method not found: unsupported method — protocol.
        -32601 => ErrorCategory::Protocol,
        // -32602 Invalid params: bad arguments — user error.
        -32602 => ErrorCategory::UserError,
        // -32603 Internal error: our bug.
        -32603 => ErrorCategory::Bug,
        // -32000 ServerNotInitialized: lifecycle protocol.
        -32000 | -32002 => ErrorCategory::Protocol,
        // -32800/-32801 RequestCancelled/ContentModified: transient.
        -32800 | -32801 => ErrorCategory::Transient,
        // Any other code: unknown, classify as Bug for visibility.
        _ => ErrorCategory::Bug,
    }
}

pub(super) fn finalize_response(
    request_id: Option<&Value>,
    routed: RoutedResponse,
) -> Option<JsonRpcResponse> {
    match routed {
        RoutedResponse::Immediate(response) => Some(response),
        RoutedResponse::Handler { id, method, should_respond, result } => {
            // Check for enhanced cancellation with provider context before cleanup.
            // This preserves cancelled responses for requests that are interrupted while
            // handlers are already running.
            if let Some(cancelled) = finalize_cancellation_state(request_id) {
                return Some(cancelled);
            }

            build_response(id, &method, should_respond, result)
        }
    }
}

pub(super) enum RoutedResponse {
    Immediate(JsonRpcResponse),
    Handler {
        id: Option<Value>,
        method: String,
        should_respond: bool,
        result: Result<Option<Value>, JsonRpcError>,
    },
}

fn build_response(
    id: Option<Value>,
    method: &str,
    should_respond: bool,
    result: Result<Option<Value>, JsonRpcError>,
) -> Option<JsonRpcResponse> {
    let id = id.as_ref().and_then(JsonRpcId::from_value);
    match result {
        Ok(Some(result)) if should_respond => {
            tracing::trace!(method = %method, "Sending successful response");
            Some(JsonRpcResponse {
                jsonrpc: "2.0",
                id: id.clone(),
                result: Some(result),
                error: None,
            })
        }
        Ok(Some(_)) => {
            tracing::trace!(method = %method, "Request is a notification (id missing), no response");
            None
        }
        Ok(None) => {
            tracing::trace!(method = %method, "Request is a notification, no response");
            None
        }
        Err(error) if should_respond => {
            let category = classify_jsonrpc_error(&error);
            tracing::debug!(
                method = %method,
                error = ?error,
                error_category = category.as_str(),
                "Sending error response"
            );
            Some(JsonRpcResponse { jsonrpc: "2.0", id, result: None, error: Some(error) })
        }
        Err(error) => {
            let category = classify_jsonrpc_error(&error);
            tracing::debug!(
                method = %method,
                error = ?error,
                error_category = category.as_str(),
                "Suppressed error response for notification request"
            );
            None
        }
    }
}
