//! JSON-RPC response construction for dispatched requests.

use super::super::{JsonRpcError, JsonRpcId, JsonRpcResponse, Value};
use super::request_cancellation::finalize_cancellation_state;
use perl_parser_core::ErrorCategory;

/// Legacy Perl-adapter classification for a finalized JSON-RPC error.
///
/// [`JsonRpcError`] intentionally owns only wire facts. This slice preserves
/// the adapter's existing code-only mapping; it does not claim parity with the
/// deleted trait mapping, which disagreed on several codes. #7612 replaces both
/// with originating classification and provenance. Do not move this policy
/// back into the generic protocol type.
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
        // -32000 ServerErrorEnd boundary; -32002 ServerNotInitialized.
        // The legacy adapter maps both to Protocol pending #7612.
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
