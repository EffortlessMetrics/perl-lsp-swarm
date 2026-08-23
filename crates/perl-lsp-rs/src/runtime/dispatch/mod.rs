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
mod method_direction;
mod preflight;
mod request_cancellation;
mod response;
mod routing;
mod text_document;
mod workspace;

pub(crate) use method_direction::{EnvelopeKind, outbound_admission};

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

    /// Initialize the server through the real lifecycle so direction tests
    /// observe post-handshake admission behavior.
    fn initialize(server: &LspServer) {
        let init = server.handle_request(request(
            1,
            "initialize",
            Some(json!({
                "processId": 1,
                "rootUri": "file:///direction-tests",
                "capabilities": {}
            })),
        ));
        assert!(init.is_some_and(|response| response.error.is_none()), "initialize must succeed");
        let initialized = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "initialized".to_string(),
            params: Some(json!({})),
        };
        assert!(server.handle_request(initialized).is_none());
    }

    fn error_code(response: Option<JsonRpcResponse>) -> Option<i32> {
        response.and_then(|response| response.error).map(|error| error.code)
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

    // ------------------------------------------------------------------
    // #8896 method-direction admission: exact-process negative controls.
    // ------------------------------------------------------------------

    const APPLY_EDIT_URI: &str = "file:///direction-tests/edit-target.pl";

    fn open_edit_target(server: &LspServer) {
        let did_open = request(
            10,
            "textDocument/didOpen",
            Some(json!({
                "textDocument": {
                    "uri": APPLY_EDIT_URI,
                    "languageId": "perl",
                    "version": 1,
                    "text": "print 'Hello';\nprint 'World';\n"
                }
            })),
        );
        assert!(server.handle_request(did_open).is_none(), "didOpen is a notification");
    }

    fn edit_target_state(server: &LspServer) -> Option<(String, i64)> {
        let documents = server.documents_guard();
        documents
            .get(APPLY_EDIT_URI)
            .map(|document| (document.text.clone(), i64::from(document.version)))
    }

    /// A client-originated `workspace/applyEdit` request must be answered
    /// `-32601` and must not reach the removed reversed application handler:
    /// document text and version show an exact zero delta afterwards.
    #[test]
    fn wrong_direction_apply_edit_request_is_rejected_without_document_mutation() {
        let server = LspServer::new();
        initialize(&server);
        open_edit_target(&server);
        let before = edit_target_state(&server);

        let response = server.handle_request(request(
            11,
            "workspace/applyEdit",
            Some(json!({
                "edit": { "changes": { APPLY_EDIT_URI: [ {
                    "range": {
                        "start": {"line": 0, "character": 6},
                        "end": {"line": 0, "character": 13}
                    },
                    "newText": "\"Modified\""
                } ] } }}
            )),
        ));

        assert_eq!(
            error_code(response),
            Some(-32601),
            "wrong-direction applyEdit must surface as MethodNotFound"
        );
        let after = edit_target_state(&server);
        assert_eq!(
            before, after,
            "rejected wrong-direction applyEdit must leave documents untouched"
        );
    }

    /// The same reversed method sent without an ID must produce no response
    /// and no application mutation at all.
    #[test]
    fn wrong_direction_apply_edit_notification_is_silently_ignored() {
        let server = LspServer::new();
        initialize(&server);
        open_edit_target(&server);
        let before = edit_target_state(&server);

        let notification = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "workspace/applyEdit".to_string(),
            params: Some(json!({ "edit": { "changes": {} } })),
        };
        assert!(
            server.handle_request(notification).is_none(),
            "wrong-direction notifications must never produce a response"
        );
        assert_eq!(before, edit_target_state(&server), "ignored notifications must not mutate");
    }

    /// A client-sent `workspace/configuration` request must return `-32601`
    /// instead of being treated as a configuration response, and must leave no
    /// pending reverse-request state behind.
    #[test]
    fn wrong_direction_configuration_request_is_rejected_without_config_effects() {
        let server = LspServer::new();
        initialize(&server);
        let config_before = {
            let config = server.workspace_config.lock();
            (
                config.include_paths.clone(),
                config.use_system_inc,
                config.use_perl5lib,
                config.resolution_timeout_ms,
            )
        };

        let response = server.handle_request(request(
            12,
            "workspace/configuration",
            Some(json!({
                "items": [{ "section": "perl.workspace.includePaths" }]
            })),
        ));

        assert_eq!(
            error_code(response),
            Some(-32601),
            "client-sent workspace/configuration must surface as MethodNotFound"
        );
        {
            let config = server.workspace_config.lock();
            let config_after = (
                config.include_paths.clone(),
                config.use_system_inc,
                config.use_perl5lib,
                config.resolution_timeout_ms,
            );
            assert_eq!(config_after, config_before, "config must be untouched");
        }
        assert!(
            server.pending_workspace_configuration_requests.lock().is_empty(),
            "a rejected inbound configuration request must not create pending reverse-request state"
        );
    }

    /// Client-sent registration requests must never activate or deactivate
    /// features; both stay MethodNotFound rather than gaining reversed arms.
    #[test]
    fn client_sent_registration_requests_cannot_change_features() {
        let server = LspServer::new();
        initialize(&server);
        let caps = server.client_capabilities.lock();
        let (apply_edit_before, config_support_before, inline_before) = (
            caps.workspace_apply_edit_support,
            caps.workspace_configuration_support,
            caps.inline_completion_dynamic_registration_support,
        );
        drop(caps);

        for (index, method) in
            ["client/registerCapability", "client/unregisterCapability"].into_iter().enumerate()
        {
            let response = server.handle_request(request(
                20 + index as i64,
                method,
                Some(json!({ "registrations": [] })),
            ));
            assert_eq!(error_code(response), Some(-32601), "{method} must stay MethodNotFound");
        }
        let caps = server.client_capabilities.lock();
        assert_eq!(
            (
                caps.workspace_apply_edit_support,
                caps.workspace_configuration_support,
                caps.inline_completion_dynamic_registration_support,
            ),
            (apply_edit_before, config_support_before, inline_before),
            "wrong-direction registration traffic must not change negotiated capabilities"
        );
    }

    /// Server→client notifications received from the client are ignored with
    /// no response and no dispatch.
    #[test]
    fn server_to_client_notifications_from_the_client_are_ignored() {
        let server = LspServer::new();
        initialize(&server);
        for method in ["window/showMessage", "$/progress", "textDocument/publishDiagnostics"] {
            let notification = JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: None,
                method: method.to_string(),
                params: Some(json!({})),
            };
            assert!(
                server.handle_request(notification).is_none(),
                "{method} from the client must be silently ignored"
            );
        }
    }

    /// Ordinary client→server routes keep working after the direction gate:
    /// a provider request answers normally and a settings notification still
    /// applies its legitimate effect.
    #[test]
    fn normal_client_to_server_routes_still_dispatch() -> anyhow::Result<()> {
        let server = LspServer::new();
        initialize(&server);
        open_edit_target(&server);

        let hover = server.handle_request(request(
            30,
            "textDocument/hover",
            Some(json!({
                "textDocument": { "uri": APPLY_EDIT_URI },
                "position": { "line": 0, "character": 8 }
            })),
        ));
        anyhow::ensure!(hover.is_some_and(|r| r.error.is_none()), "hover must still answer");

        let formatting_before = server.config.lock().perltidy_enabled;
        assert!(
            server
                .handle_request(JsonRpcRequest {
                    _jsonrpc: "2.0".to_string(),
                    id: None,
                    method: "workspace/didChangeConfiguration".to_string(),
                    params: Some(json!({
                        "settings": { "perl": { "formatting": { "enabled": !formatting_before } } }
                    })),
                })
                .is_none()
        );
        let formatting_after = server.config.lock().perltidy_enabled;
        anyhow::ensure!(
            formatting_after != formatting_before,
            "didChangeConfiguration must retain its legitimate effect ({formatting_before} → {formatting_after})"
        );
        Ok(())
    }

    /// JSON-RPC responses carry no method and are classified by the transport
    /// before routing (#7010 owns the registry-owned replacement). Numeric and
    /// string IDs alike surface as the internal carrier notification — never
    /// as any standard method entering `route_request`.
    #[test]
    fn response_envelopes_are_classified_before_method_routing()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::transport::framing::ContentLengthMessageReader;

        let mut wire: Vec<u8> = Vec::new();
        for body in [
            r#"{"jsonrpc":"2.0","id":41,"result":{"applied":true}}"#,
            r#"{"jsonrpc":"2.0","id":"41","result":{"sections":[]}}"#,
        ] {
            let framed = perl_lsp_rs_core::transport::frame(body.as_bytes());
            wire.extend_from_slice(&framed);
        }

        let mut reader = ContentLengthMessageReader::new();
        let mut cursor: &[u8] = &wire;
        let mut seen = Vec::new();
        while let Some(request) = reader.read_next(&mut cursor)? {
            seen.push(request.method.clone());
            // Routing the classified carrier consumes it without emitting a
            // JSON-RPC response back to the client. A fresh server per frame
            // keeps this test free of cross-test background state.
            let server = LspServer::new();
            initialize(&server);
            assert!(
                server.handle_request(request).is_none(),
                "carrier frame must not produce a response envelope"
            );
        }
        assert_eq!(
            seen,
            vec!["$/perl-lsp/clientResponse".to_string(), "$/perl-lsp/clientResponse".to_string()],
            "numeric and string response IDs must both classify to the carrier, not to standard methods"
        );
        Ok(())
    }

    /// The common outbound request seam fails closed for client→server
    /// methods: no id is reserved and no frame escapes.
    #[test]
    fn outbound_send_request_refuses_client_to_server_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        use parking_lot::Mutex;
        use std::io;
        use std::io::Write;
        use std::sync::Arc;

        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let server = LspServer::with_output(Arc::new(Mutex::new(
            Box::new(Capture(buffer.clone())) as Box<dyn Write + Send>,
        )));
        server.initialized.store(true, Ordering::Release);

        let refused = server.send_request("textDocument/hover", json!({"textDocument": {}}));
        assert!(
            matches!(refused, Err(ref error) if error.kind() == io::ErrorKind::InvalidData),
            "client→server method must be refused at the outbound seam, got {refused:?}"
        );
        let admitted = server.send_request("workspace/configuration", json!({"items": []}))?;
        assert_ne!(admitted.as_i32(), 0, "admitted server→client request keeps its reserved id");
        let mut frames = String::new();
        for _ in 0..100 {
            frames = String::from_utf8(buffer.lock().clone())?;
            if frames.contains("workspace/configuration") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !frames.contains("textDocument/hover"),
            "refused method must never reach the wire: {frames}"
        );
        assert!(
            frames.contains("workspace/configuration"),
            "legitimate server→client request must still be emitted: {frames}"
        );
        Ok(())
    }
}
