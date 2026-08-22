//! Request dispatch and routing for the LSP server
//!
//! This module implements the JSON-RPC request/response routing layer for the Perl LSP server.
//! It handles incoming requests, dispatches them to appropriate handlers, and manages
//! cancellation tokens for responsive user experience.
//!
//! # Architecture
//!
//! The dispatch layer is organized into focused submodules:
//!
//! - **text_document**: Handlers for document-level operations (completion, hover, definition, etc.)
//! - **workspace**: Handlers for workspace-level operations (symbols, configuration, file events)
//! - **lifecycle**: Handlers for server lifecycle (initialize, shutdown, exit)
//! - **cancellation**: Request cancellation support with provider cleanup context
//! - **experimental**: Experimental features and test endpoints
//!
//! # Request Flow
//!
//! 1. Request arrives via JSON-RPC transport
//! 2. Cancellation token registered for long-running operations
//! 3. Method string matched to handler in `routing::route_request`
//! 4. Handler invoked with params and optional request ID
//! 5. Response returned (or None for notifications)
//! 6. Cancellation token cleaned up
//!
//! # Cancellation Support
//!
//! Long-running operations (completion, references, workspace symbols) support LSP cancellation:
//!
//! - `$/cancelRequest` notifications mark requests as cancelled
//! - Handlers periodically check cancellation state
//! - Enhanced cancellation includes provider cleanup context for resource management
//! - Performance target: <50ms cancellation response time
//!
//! # Performance Characteristics
//!
//! - **Dispatch overhead**: <1ms for method routing
//! - **Cancellation setup**: <5ms for token registration
//! - **Response serialization**: <10ms for typical responses
//!
//! # Error Handling
//!
//! - ServerNotInitialized (-32002): Returned for requests before initialization
//! - MethodNotFound (-32601): Returned for unknown/unsupported methods
//! - Enhanced error responses include method context for debugging

mod ast_explorer;
mod cancellation;
mod experimental;
mod formatting_policy;
mod lifecycle;
mod preflight;
mod request_cancellation;
mod response;
mod routing;
mod text_document;
mod workspace;

pub(crate) use cancellation::enhanced_cancelled_response;

#[cfg(test)]
use super::*;
use super::{JsonRpcRequest, JsonRpcResponse, LspServer, Value, cancelled_response_with_method};

impl LspServer {
    /// Handle a JSON-RPC request
    pub fn handle_request(&self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let context = preflight::RequestContext::from_request(&request);

        match preflight::prepare_request(self, &request, &context) {
            preflight::PreflightOutcome::Continue => {}
            preflight::PreflightOutcome::NotificationHandled => return None,
            preflight::PreflightOutcome::Respond(response) => return Some(response),
        }

        let routed =
            formatting_policy::route(self, &request, context.id.clone(), context.should_respond)
                .unwrap_or_else(|| {
                    self.route_request(request, context.id.clone(), context.should_respond)
                });
        response::finalize_response(context.id.as_ref(), routed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Integer(id)),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn requests_are_accepted_after_initialize_even_without_initialized_notification() {
        let server = LspServer::new();

        let before_initialize = server
            .handle_request(request(1, "custom/unknown", None))
            .and_then(|response| response.error)
            .map(|error| error.code);
        assert_eq!(before_initialize, Some(-32002));

        let initialize = server.handle_request(request(2, "initialize", Some(json!({}))));
        assert!(
            initialize.as_ref().is_some_and(|response| response.error.is_none()),
            "initialize request should succeed"
        );

        let after_initialize = server
            .handle_request(request(3, "custom/unknown", None))
            .and_then(|response| response.error)
            .map(|error| error.code);
        assert_eq!(after_initialize, Some(-32601));
    }

    #[test]
    fn first_use_hot_paths_are_wrapped_by_shared_latency_recorder() {
        let routing = include_str!("routing.rs");
        for method in [
            "initialize",
            "textDocument/didOpen",
            "textDocument/didChange",
            "textDocument/completion",
            "textDocument/hover",
            "textDocument/definition",
            "textDocument/references",
            "textDocument/signatureHelp",
            "textDocument/semanticTokens/full",
        ] {
            assert!(routing.contains(method), "routing table must include hot path `{method}`");
        }

        let recorder_calls =
            routing.matches("record_lsp_request_latency(&method, request_start)").count();
        assert!(
            recorder_calls >= 2,
            "normal and cancellable dispatch paths must record shared request latency"
        );
    }

    /// Capability gating (#4629): when a feature flag in `AdvertisedFeatures`
    /// is `false`, the handler must return `method_not_advertised` (code
    /// −32601) rather than silently executing.
    #[test]
    fn disabled_features_return_method_not_advertised() {
        let server = LspServer::new();

        // Initialize so preflight allows non-lifecycle requests.
        let init = server.handle_request(request(1, "initialize", Some(json!({}))));
        assert!(init.is_some_and(|r| r.error.is_none()), "initialize should succeed");

        // Disable several feature flags.
        {
            let mut features = server.advertised_features.lock();
            features.formatting = false;
            features.semantic_tokens = false;
            features.code_action = false;
            features.folding_range = false;
            features.document_symbol = false;
        }
        server
            .advertised_feature_ids
            .lock()
            .retain(|id| *id != perl_lsp_rs_core::features::ids::LSP_FORMATTING);

        // Each disabled handler must return method_not_advertised (−32601).
        let cases: &[(&str, Option<Value>)] = &[
            ("textDocument/formatting", Some(json!({"textDocument": {"uri": "file:///test.pm"}}))),
            (
                "textDocument/semanticTokens/full",
                Some(json!({"textDocument": {"uri": "file:///test.pm"}})),
            ),
            ("textDocument/codeAction", Some(json!({"textDocument": {"uri": "file:///test.pm"}}))),
            (
                "textDocument/foldingRange",
                Some(json!({"textDocument": {"uri": "file:///test.pm"}})),
            ),
            (
                "textDocument/documentSymbol",
                Some(json!({"textDocument": {"uri": "file:///test.pm"}})),
            ),
        ];

        for (method, params) in cases {
            let resp = server.handle_request(request(2, method, params.clone()));
            let code = resp.and_then(|r| r.error).map(|e| e.code).unwrap_or(0);
            assert_eq!(
                code, -32601,
                "disabled feature `{method}` should return method_not_advertised (-32601), got {code}"
            );
        }

        // Re-enabling a feature should restore normal behaviour (not the gate error).
        {
            let mut features = server.advertised_features.lock();
            features.folding_range = true;
        }
        let resp = server.handle_request(request(
            3,
            "textDocument/foldingRange",
            Some(json!({"textDocument": {"uri": "file:///test.pm"}})),
        ));
        let code = resp.and_then(|r| r.error).map(|e| e.code).unwrap_or(0);
        assert_ne!(
            code, -32601,
            "re-enabled folding_range should not return method_not_advertised"
        );
    }

    /// An invalid *notification* (no `id`) must be dropped silently rather than
    /// answered, because JSON-RPC forbids replying to a notification. This
    /// covers the `!context.should_respond` branch of the preflight structural
    /// rejection, which the request-shaped tests below cannot reach. The
    /// overlong method name trips the generic length bound without involving
    /// any content or charset policy (#8895).
    #[test]
    fn invalid_notification_is_dropped_without_a_response() {
        let server = LspServer::new();
        let _ = server.handle_request(request(1, "initialize", Some(json!({}))));

        let notification = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: format!("textDocument/{}", "x".repeat(200)),
            params: None,
        };
        assert!(
            server.handle_request(notification).is_none(),
            "an invalid notification must be dropped, not answered"
        );
    }

    /// A valid JSON-RPC method name is never rejected by a punctuation
    /// allowlist (issue #8895). An unknown punctuated method must reach
    /// routing and be answered with MethodNotFound (-32601) — proving both
    /// that admission does not police method charset and the -32600/-32601
    /// distinction at one boundary.
    #[test]
    fn punctuated_unknown_method_returns_32601_not_32600() -> anyhow::Result<()> {
        let server = LspServer::new();
        let _ = server.handle_request(request(1, "initialize", Some(json!({}))));

        let code = server
            .handle_request(request(2, "custom/fmt.v2:preview", None))
            .and_then(|r| r.error)
            .map(|e| e.code);
        anyhow::ensure!(
            code == Some(-32601),
            "valid unknown method must return -32601, got {code:?}"
        );
        Ok(())
    }

    /// The server's internal reverse-request response method contains a hyphen.
    /// It must pass preflight and reach its routing arm rather than being
    /// rejected as an invalid method before dispatch.
    #[test]
    fn internal_client_response_method_reaches_dispatch() -> anyhow::Result<()> {
        let server = LspServer::new();
        let _ = server.handle_request(request(1, "initialize", Some(json!({}))));

        let response =
            server.handle_request(request(2, "$/perl-lsp/clientResponse", Some(json!({}))));
        anyhow::ensure!(
            response.is_none(),
            "internal client response should be handled without a JSON-RPC response"
        );
        Ok(())
    }

    /// Parameter content is inert data at the dispatch boundary (issue #8895):
    /// `<script>` inside params of an unknown custom method must NOT trigger
    /// the old generic InvalidRequest (-32600) scan rejection. The request is
    /// structurally valid, so it reaches routing and is answered with
    /// MethodNotFound (-32601).
    #[test]
    fn script_like_param_content_is_inert_data() -> anyhow::Result<()> {
        let server = LspServer::new();
        let _ = server.handle_request(request(1, "initialize", Some(json!({}))));

        let code = server
            .handle_request(request(
                2,
                "custom/eval",
                Some(json!({"expression": "<script>alert(1)</script>"})),
            ))
            .and_then(|r| r.error)
            .map(|e| e.code);
        anyhow::ensure!(
            code == Some(-32601),
            "script-like param text must not cause a generic -32600 rejection; \
             unknown method should yield -32601, got {code:?}"
        );
        Ok(())
    }

    /// Verify that the `$/test/slowOperation` endpoint is cfg-gated when neither
    /// test mode nor `expose_lsp_test_api` is enabled (issue #4632). The routing
    /// arm and handler must both carry the gate so a non-test, non-feature build
    /// falls through to `METHOD_NOT_FOUND`, while feature-enabled builds retain
    /// the endpoint for cancellation tests.
    #[test]
    fn test_slow_operation_endpoint_is_cfg_gated_from_production() {
        let routing = include_str!("routing.rs");
        let experimental = include_str!("experimental.rs");

        let cfg_gate = "#[cfg(any(test, feature = \"expose_lsp_test_api\"))]";

        // The routing arm for "$/test/slowOperation" must be immediately preceded
        // by the cfg gate attribute on its own line.
        let routing_gated_arm = format!(
            "{cfg_gate}\n            \"$/test/slowOperation\" => \
             self.handle_slow_operation_dispatch(&id, request.params),"
        );
        assert!(
            routing.contains(&routing_gated_arm),
            "the $/test/slowOperation routing arm must be gated by {cfg_gate}"
        );

        // The handler method must also be cfg-gated.
        let handler_gated =
            format!("{cfg_gate}\n    pub(super) fn handle_slow_operation_dispatch(");
        assert!(
            experimental.contains(&handler_gated),
            "handle_slow_operation_dispatch must be gated by {cfg_gate}"
        );
    }
}
