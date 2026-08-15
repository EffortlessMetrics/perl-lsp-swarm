//! JSON-RPC response construction for dispatched requests.

use super::super::{JsonRpcError, JsonRpcId, JsonRpcResponse, Value};
use super::request_cancellation::finalize_cancellation_state;
use perl_parser_core::ErrorCategory;
use std::error::Error;

/// Product-owned classification wrapper for a language-neutral JSON-RPC error.
///
/// The wire error stays free of parser/repository policy. Stable protocol codes
/// may receive a category here, while generic `RequestFailed` and private server
/// codes remain explicitly unclassified until their originating operation
/// supplies one. The source error is retained rather than stringified.
#[derive(Debug)]
struct ClassifiedProtocolFailure {
    error: JsonRpcError,
    category: Option<ErrorCategory>,
    provenance: &'static str,
}

impl ClassifiedProtocolFailure {
    fn from_wire(error: JsonRpcError) -> Self {
        match error.code {
            // Stable protocol-shape errors.
            -32700 | -32600 | -32601 | -32602 | -32000 | -32002 => {
                Self::with_originating_category(
                    error,
                    ErrorCategory::Protocol,
                    "stable_jsonrpc_protocol_code",
                )
            }
            // Internal error has one stable product meaning.
            -32603 => Self::with_originating_category(
                error,
                ErrorCategory::Bug,
                "stable_jsonrpc_internal_error",
            ),
            // Standard cancellation/currentness outcomes are retry/timing state.
            -32800 | -32801 | -32802 => Self::with_originating_category(
                error,
                ErrorCategory::Transient,
                "stable_lsp_terminal_code",
            ),
            // RequestFailed is deliberately generic and cannot recover cause.
            -32803 => Self::unclassified(
                error,
                "request_failed_requires_originating_category",
            ),
            // Private/server-specific codes likewise require construction context.
            _ => Self::unclassified(
                error,
                "server_specific_code_requires_originating_category",
            ),
        }
    }

    fn with_originating_category(
        error: JsonRpcError,
        category: ErrorCategory,
        provenance: &'static str,
    ) -> Self {
        Self { error, category: Some(category), provenance }
    }

    fn unclassified(error: JsonRpcError, provenance: &'static str) -> Self {
        Self { error, category: None, provenance }
    }

    fn category(&self) -> Option<&ErrorCategory> {
        self.category.as_ref()
    }

    fn provenance(&self) -> &'static str {
        self.provenance
    }

    fn into_error(self) -> JsonRpcError {
        self.error
    }
}

impl std::fmt::Display for ClassifiedProtocolFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl Error for ClassifiedProtocolFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
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
            let classified = ClassifiedProtocolFailure::from_wire(error);
            let category = classified
                .category()
                .map_or("unclassified", ErrorCategory::as_str);
            let provenance = classified.provenance();
            let error = classified.into_error();
            tracing::debug!(
                method = %method,
                error = ?error,
                error_category = category,
                error_category_provenance = provenance,
                "Sending error response"
            );
            Some(JsonRpcResponse { jsonrpc: "2.0", id, result: None, error: Some(error) })
        }
        Err(error) => {
            let classified = ClassifiedProtocolFailure::from_wire(error);
            let category = classified
                .category()
                .map_or("unclassified", ErrorCategory::as_str);
            let provenance = classified.provenance();
            let error = classified.into_error();
            tracing::debug!(
                method = %method,
                error = ?error,
                error_category = category,
                error_category_provenance = provenance,
                "Suppressed error response for notification request"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_wire_codes_have_owned_product_classification() {
        for (code, expected) in [
            (-32700, ErrorCategory::Protocol),
            (-32600, ErrorCategory::Protocol),
            (-32601, ErrorCategory::Protocol),
            (-32602, ErrorCategory::Protocol),
            (-32002, ErrorCategory::Protocol),
            (-32603, ErrorCategory::Bug),
            (-32800, ErrorCategory::Transient),
            (-32801, ErrorCategory::Transient),
            (-32802, ErrorCategory::Transient),
        ] {
            let classified =
                ClassifiedProtocolFailure::from_wire(JsonRpcError::new(code, "fixture"));
            assert_eq!(classified.category(), Some(&expected), "code {code}");
        }
    }

    #[test]
    fn request_failed_requires_explicit_originating_category() {
        let classified =
            ClassifiedProtocolFailure::from_wire(JsonRpcError::new(-32803, "request failed"));
        assert!(classified.category().is_none());
        assert_eq!(
            classified.provenance(),
            "request_failed_requires_originating_category"
        );
    }

    #[test]
    fn explicit_origin_preserves_category_and_source_error() {
        let classified = ClassifiedProtocolFailure::with_originating_category(
            JsonRpcError::new(-32803, "dependency unavailable"),
            ErrorCategory::Infra,
            "workspace_dependency_lookup",
        );

        assert_eq!(classified.category(), Some(&ErrorCategory::Infra));
        assert_eq!(classified.provenance(), "workspace_dependency_lookup");
        let source = classified.source().expect("source error must be retained");
        let source = source
            .downcast_ref::<JsonRpcError>()
            .expect("source remains the typed JSON-RPC error");
        assert_eq!(source.code, -32803);
    }

    #[test]
    fn message_and_data_cannot_classify_private_server_codes() {
        let first = ClassifiedProtocolFailure::from_wire(JsonRpcError::with_data(
            -32099,
            "transient retry please",
            serde_json::json!({"category": "infra"}),
        ));
        let second = ClassifiedProtocolFailure::from_wire(JsonRpcError::new(
            -32099,
            "definitely a user error",
        ));

        assert!(first.category().is_none());
        assert!(second.category().is_none());
        assert_eq!(first.provenance(), second.provenance());
    }
}
