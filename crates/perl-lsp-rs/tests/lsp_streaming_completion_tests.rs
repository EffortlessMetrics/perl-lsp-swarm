//! Integration tests for the streaming inline completion protocol.
//!
//! Validates the `textDocument/perlInlineCompletionStream` custom request,
//! including `$/progress` emission, session management, and fallback behavior.
//!
//! Run with:
//! ```bash
//! RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --features expose_lsp_test_api \
//!     -- streaming --test-threads=2
//! ```

// Tests are permitted to use `.expect()` on Result/Option per the repo's
// coding standards (unlike production code, where it is banned).
#![allow(clippy::expect_used)]

mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Helper: initialize server with default capabilities.
fn init_harness() -> Result<LspHarness, String> {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    Ok(harness)
}

/// Helper: send a generic-client AI enable attempt via didChangeConfiguration.
///
/// Since #4997 no generic LSP settings channel can arm remote AI egress or
/// toggle its streaming authorization: this payload is rejected by the
/// server and previously accepted state is preserved. The helper remains so
/// transport-level regressions prove that exact rejection end-to-end.
fn enable_ai_streaming(harness: &mut LspHarness) {
    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "aiCompletion": {
                        "enabled": true,
                        "streaming": {
                            "enabled": true
                        }
                    }
                }
            }
        }),
    );
    // Give the server time to process the configuration change.
    std::thread::sleep(Duration::from_millis(50));
}

/// Helper: generic enable attempt with fallback=false, the payload shape that
/// used to select the no-backend progress contract before #4997.
fn enable_ai_streaming_progress_contract(harness: &mut LspHarness) {
    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "aiCompletion": {
                        "enabled": true,
                        "fallback": false,
                        "streaming": {
                            "enabled": true
                        }
                    }
                }
            }
        }),
    );
    std::thread::sleep(Duration::from_millis(50));
}

/// Helper: generic enable-plus-disable-streaming attempt. Both directions are
/// unauthorized under #4997; neither may change AI state.
fn enable_ai_disable_streaming(harness: &mut LspHarness) {
    harness.notify(
        "workspace/didChangeConfiguration",
        json!({
            "settings": {
                "perl": {
                    "aiCompletion": {
                        "enabled": true,
                        "streaming": {
                            "enabled": false
                        }
                    }
                }
            }
        }),
    );
    std::thread::sleep(Duration::from_millis(50));
}

// ==================== Generic-channel rejection (#4997) ====================

/// Security regression (#4997): a generic client forwarding workspace-derived
/// `aiCompletion.enabled=true` over didChangeConfiguration must not enter the
/// streaming route at all. The request falls back to the deterministic
/// one-shot handler (items returned directly) and zero `$/progress`
/// notifications are emitted for the token — proof the hostile payload armed
/// nothing.
#[test]
fn hostile_generic_enable_cannot_enter_streaming_route() -> TestResult {
    let mut harness = init_harness()?;
    enable_ai_streaming_progress_contract(&mut harness);

    let uri = "file:///streaming_test.pl";
    harness.open(uri, "use strict;\nmy $obj = Package->")?;

    // Drain any startup notifications (diagnostics, etc.)
    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 19 },
            "partialResultToken": "stream-token-1"
        }),
    )?;

    // The one-shot fallback returns items directly instead of the streaming
    // route's null response.
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("expected deterministic fallback items, got: {result}"))?;
    assert!(
        !items.is_empty(),
        "deterministic one-shot fallback should return completions for 'Package->'"
    );

    // Zero stream sessions were started: no $/progress for this token.
    let progress_notifications = harness.drain_notifications(Some("$/progress"), 500);
    let matching: Vec<_> = progress_notifications
        .iter()
        .filter(|n| n.pointer("/params/token").and_then(|v| v.as_str()) == Some("stream-token-1"))
        .collect();

    assert!(
        matching.is_empty(),
        "generic enablement must not start a stream session; got {} progress notifications",
        progress_notifications.len()
    );

    Ok(())
}

/// Security regression (#4997): the same rejection holds when the hostile
/// payload also tries to disable streaming — neither direction may change AI
/// state, and the request still resolves through the deterministic path.
#[test]
fn hostile_generic_disable_streaming_is_equally_unauthorized() -> TestResult {
    let mut harness = init_harness()?;
    enable_ai_disable_streaming(&mut harness);

    let uri = "file:///fallback_streaming_disabled.pl";
    harness.open(uri, "my $obj = Package->")?;

    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 0, "character": 19 },
            "partialResultToken": "stream-disabled-token"
        }),
    )?;

    // Falls back to one-shot -- returns items, not null; no stream session.
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("expected items array in fallback response")?;
    assert!(!items.is_empty(), "one-shot fallback should return completions");

    let progress = harness.drain_notifications(Some("$/progress"), 200);
    let matching: Vec<_> = progress
        .iter()
        .filter(|n| {
            n.pointer("/params/token").and_then(|v| v.as_str()) == Some("stream-disabled-token")
        })
        .collect();
    assert!(
        matching.is_empty(),
        "unauthorized disable attempt must not open or close any stream session"
    );

    Ok(())
}

// ==================== Fallback: AI disabled ====================

/// When AI completion is disabled, the streaming request should fall back
/// to the one-shot inline completion handler and return items directly.
#[test]
fn streaming_completion_without_ai_falls_back_to_one_shot() -> TestResult {
    let mut harness = init_harness()?;
    // AI is disabled by default; do NOT call enable_ai_streaming.

    let uri = "file:///fallback_ai_disabled.pl";
    harness.open(uri, "my $obj = Package->")?;

    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 0, "character": 19 },
            "partialResultToken": "fallback-token-1"
        }),
    )?;

    // With AI disabled, the handler falls back to one-shot inline completion,
    // which returns an items array (not null).
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("expected items array in fallback response")?;
    assert!(!items.is_empty(), "one-shot fallback should return completions for 'Package->'");
    assert_eq!(
        items[0]["insertText"].as_str(),
        Some("new()"),
        "expected 'new()' from deterministic one-shot handler"
    );

    // No $/progress should have been emitted for this request.
    let progress = harness.drain_notifications(Some("$/progress"), 200);
    let matching: Vec<_> = progress
        .iter()
        .filter(|n| n.pointer("/params/token").and_then(|v| v.as_str()) == Some("fallback-token-1"))
        .collect();
    assert!(matching.is_empty(), "no progress notifications expected when AI is disabled");

    Ok(())
}

/// When AI is enabled but streaming specifically is disabled, the streaming
/// request should also fall back to one-shot.
///
/// Superseded by `hostile_generic_disable_streaming_is_equally_unauthorized`
/// above: under #4997 the enable-plus-disable payload this test used to send
/// is rejected wholesale, and that test additionally proves no stream session
/// was opened or torn down by either direction of the hostile payload.
#[test]
fn streaming_completion_with_streaming_disabled_falls_back() -> TestResult {
    let mut harness = init_harness()?;
    enable_ai_disable_streaming(&mut harness);

    let uri = "file:///fallback_streaming_disabled.pl";
    harness.open(uri, "my $obj = Package->")?;

    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 0, "character": 19 },
            "partialResultToken": "stream-disabled-token"
        }),
    )?;

    // Falls back to one-shot -- returns items, not null.
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("expected items array in fallback response")?;
    assert!(!items.is_empty(), "one-shot fallback should return completions");

    Ok(())
}

// ==================== Fallback: no partialResultToken ====================

/// When the client omits partialResultToken, the handler must fall back to
/// one-shot inline completion regardless of AI config.
#[test]
fn streaming_completion_without_partial_result_token_falls_back() -> TestResult {
    let mut harness = init_harness()?;
    enable_ai_streaming(&mut harness);

    let uri = "file:///no_token.pl";
    harness.open(uri, "my $obj = Package->")?;

    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    // Omit partialResultToken entirely.
    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 0, "character": 19 }
        }),
    )?;

    // Without a token, falls back to one-shot -- returns items.
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("expected items array when partialResultToken is missing")?;
    assert!(!items.is_empty(), "one-shot fallback should return completions");

    Ok(())
}

// ==================== Session cancellation ====================
// Session-cancellation semantics (second request at the same position
// cancels the first, distinct session IDs per token) are proven by
// `streaming_completion_cancel_rotates_session_identity` in the gated
// `mock_streaming_completion_tests` module below: arming AI now requires the
// trusted test API (#4997), which only that module can reach.

// ==================== URI cancellation ====================

/// After closing a document, subsequent streaming requests for that URI
/// should return null without crashing.
#[test]
fn streaming_completion_on_closed_doc_returns_null() -> TestResult {
    let mut harness = init_harness()?;
    enable_ai_streaming(&mut harness);

    let uri = "file:///closed_doc.pl";
    harness.open(uri, "use strict;\nmy $x = 1;\n")?;
    harness.wait_for_idle(Duration::from_millis(200));
    let _ = harness.drain_notifications(None, 100);

    // Close the document
    harness.close(uri)?;
    std::thread::sleep(Duration::from_millis(50));

    // Request streaming on the now-closed document.
    let result = harness.request(
        "textDocument/perlInlineCompletionStream",
        json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 5 },
            "partialResultToken": "closed-doc-token"
        }),
    )?;

    // Should gracefully return null (document not found).
    assert!(result.is_null(), "streaming on closed doc should return null");

    Ok(())
}

// ==================== Missing params ====================

/// Sending the streaming request without params should return an error.
#[test]
fn streaming_completion_missing_params_returns_error() -> TestResult {
    let mut harness = init_harness()?;

    // Send request without valid textDocument params.
    // The harness request method wraps params, but we can send malformed params.
    let result = harness.request("textDocument/perlInlineCompletionStream", json!({}));

    // Should return an error (missing textDocument.uri).
    assert!(result.is_err(), "streaming request with empty params should error");

    Ok(())
}

// ==================== Capability advertisement ====================

/// The server should advertise `perlInlineCompletionStream` in experimental
/// capabilities after initialization.
#[test]
fn streaming_completion_capability_advertised() -> TestResult {
    let mut harness = LspHarness::new();
    // The experimental custom request is advertised only to clients that
    // declared the standard inline-completion capability (#7682).
    let init_result = harness.initialize(Some(json!({
        "textDocument": { "inlineCompletion": { "dynamicRegistration": false } }
    })))?;

    let experimental = init_result
        .pointer("/capabilities/experimental")
        .ok_or("expected experimental capabilities")?;
    assert_eq!(
        experimental.get("perlInlineCompletionStream"),
        Some(&json!(true)),
        "server should advertise perlInlineCompletionStream capability"
    );

    Ok(())
}

// ==================== Progress payload schema ====================
// The full progress-payload schema (method, token echo, value fields) is
// proven by `streaming_completion_progress_schema_validation_armed` in the
// gated `mock_streaming_completion_tests` module below: arming AI now
// requires the trusted test API (#4997), which only that module can reach.

// Mock streaming-backend coverage includes:
// 1. Multiple intermediate $/progress notifications with increasing sequence numbers
// 2. Each chunk's cumulative text in the items array
// 3. Cancellation mid-stream via session cancel-previous semantics
// 4. Backend error propagation and graceful termination

#[cfg(feature = "expose_lsp_test_api")]
mod mock_streaming_completion_tests {
    use parking_lot::Mutex;
    use perl_lsp::{JsonRpcRequest, LspServer};
    use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
    use serde_json::{Value, json};
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[derive(Clone)]
    struct TestOutputCapture {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl TestOutputCapture {
        fn new() -> Self {
            Self { buffer: Arc::new(Mutex::new(Vec::new())) }
        }

        fn messages(&self) -> Vec<Value> {
            let bytes = self.buffer.lock().clone();
            parse_jsonrpc_frames(&bytes)
        }
    }

    impl Write for TestOutputCapture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.buffer.lock().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn parse_jsonrpc_frames(bytes: &[u8]) -> Vec<Value> {
        let mut framer = ContentLengthFramer::new();
        let mut messages = Vec::new();
        framer.push(bytes);
        while let Ok(Some(message)) = framer.try_next() {
            if let Ok(value) = serde_json::from_slice::<Value>(&message) {
                messages.push(value);
            }
        }
        messages
    }

    fn wait_for_progress_messages(
        capture: &TestOutputCapture,
        token: &str,
        timeout: Duration,
    ) -> Vec<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let messages = capture.messages();
            let matching: Vec<_> = messages
                .into_iter()
                .filter(|msg| {
                    msg.get("method").and_then(|v| v.as_str()) == Some("$/progress")
                        && msg.pointer("/params/token").and_then(|v| v.as_str()) == Some(token)
                })
                .collect();
            let has_final = matching.iter().any(|msg| {
                msg.pointer("/params/value/isFinal").and_then(Value::as_bool).unwrap_or(false)
            });
            if has_final || Instant::now() >= deadline {
                return matching;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn create_server() -> (LspServer, TestOutputCapture) {
        let capture = TestOutputCapture::new();
        let output = Box::new(capture.clone()) as Box<dyn Write + Send>;
        let server = LspServer::with_output(Arc::new(Mutex::new(output)));

        let init_request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
            method: "initialize".into(),
            params: Some(json!({
                "processId": std::process::id(),
                "rootUri": "file:///workspace",
                "capabilities": {
                    "textDocument": {
                        "inlineCompletion": { "dynamicRegistration": false },
                    }
                }
            })),
        };
        let _ = server.handle_request(init_request);

        let initialized = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "initialized".into(),
            params: Some(json!({})),
        };
        let _ = server.handle_request(initialized);

        server.test_configure_ai_completion(true, true);
        let config_request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "workspace/didChangeConfiguration".into(),
            params: Some(json!({
                "settings": {
                    "perl": {
                        "aiCompletion": {
                            "enabled": true,
                            "streaming": {
                                "enabled": true
                            }
                        }
                    }
                }
            })),
        };
        let _ = server.handle_request(config_request);

        (server, capture)
    }

    /// Variant of `create_server` that arms AI through the trusted test API
    /// (#4997) with deterministic fallback disabled, so the no-backend
    /// progress contract (null response plus a single final `$/progress`
    /// frame carrying an items array) is observable. The trailing
    /// didChangeConfiguration payload is the generic-channel enable shape,
    /// which the server must reject: streaming stays at its trusted default.
    fn create_server_progress_contract() -> (LspServer, TestOutputCapture) {
        let capture = TestOutputCapture::new();
        let output = Box::new(capture.clone()) as Box<dyn Write + Send>;
        let server = LspServer::with_output(Arc::new(Mutex::new(output)));

        let init_request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
            method: "initialize".into(),
            params: Some(json!({
                "processId": std::process::id(),
                "rootUri": "file:///workspace",
                "capabilities": {
                    "textDocument": {
                        "inlineCompletion": { "dynamicRegistration": false },
                    }
                }
            })),
        };
        let _ = server.handle_request(init_request);

        let initialized = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "initialized".into(),
            params: Some(json!({})),
        };
        let _ = server.handle_request(initialized);

        // Trusted-operator stand-in (#4997): no client channel may arm this.
        server.test_configure_ai_completion(true, false);

        let hostile_enable = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "workspace/didChangeConfiguration".into(),
            params: Some(json!({
                "settings": {
                    "perl": {
                        "aiCompletion": {
                            "enabled": true,
                            "streaming": { "enabled": true }
                        }
                    }
                }
            })),
        };
        let _ = server.handle_request(hostile_enable);

        (server, capture)
    }

    /// Progress-contract happy path under a legitimately armed state:
    /// streaming route entered, backend absent, fallback disabled — the
    /// handler returns null and emits exactly one final progress frame with
    /// the full value schema.
    #[test]
    fn streaming_completion_progress_schema_validation_armed() {
        let (server, capture) = create_server_progress_contract();
        open_doc(&server, "file:///schema_test.pl", "my $obj = Package->");

        let result =
            request_streaming_completion(&server, "file:///schema_test.pl", 19, "schema-token");
        assert!(result.is_null(), "streaming route returns null; got: {result}");

        let progress =
            wait_for_progress_messages(&capture, "schema-token", Duration::from_millis(500));
        assert!(!progress.is_empty(), "expected progress notification");

        let notif = &progress[0];
        assert_eq!(notif["method"].as_str(), Some("$/progress"));
        assert_eq!(
            notif.pointer("/params/token").and_then(|v| v.as_str()),
            Some("schema-token"),
            "token must match partialResultToken"
        );

        let value = &notif["params"]["value"];
        assert!(!value.is_null(), "value must be present");
        for field in ["kind", "sessionId", "sequence", "isFinal", "items"] {
            assert!(value.get(field).is_some(), "progress value must contain '{field}'");
        }
        assert_eq!(
            value["kind"].as_str(),
            Some("perlInlineCompletionStream"),
            "progress kind must be 'perlInlineCompletionStream'"
        );
        assert_eq!(value["sequence"].as_u64(), Some(0), "first frame starts at zero");
        assert_eq!(
            value["isFinal"].as_bool(),
            Some(true),
            "no-backend contract emits a single final progress"
        );
        assert!(value["items"].is_array(), "items must be an array");
    }

    /// Two streaming requests at the same position rotate the stream session:
    /// each token gets progress frames carrying distinct session IDs.
    #[test]
    fn streaming_completion_cancel_rotates_session_identity() {
        let (server, capture) = create_server_progress_contract();
        open_doc(&server, "file:///cancel_test.pl", "my $obj = Package->");

        let first =
            request_streaming_completion(&server, "file:///cancel_test.pl", 19, "cancel-token-1");
        assert!(first.is_null(), "first streaming response should be null");

        let second =
            request_streaming_completion(&server, "file:///cancel_test.pl", 19, "cancel-token-2");
        assert!(second.is_null(), "second streaming response should be null");

        // Synchronize on the second token's final frame, then classify the
        // full captured stream by token.
        let _ = wait_for_progress_messages(&capture, "cancel-token-2", Duration::from_millis(500));
        let progress = capture.messages();
        let token1: Vec<_> = progress
            .iter()
            .filter(|n| {
                n.pointer("/params/token").and_then(|v| v.as_str()) == Some("cancel-token-1")
            })
            .collect();
        let token2: Vec<_> = progress
            .iter()
            .filter(|n| {
                n.pointer("/params/token").and_then(|v| v.as_str()) == Some("cancel-token-2")
            })
            .collect();
        assert!(!token1.is_empty(), "first request should emit progress");
        assert!(!token2.is_empty(), "second request should emit progress");

        let sid1 = token1[0].pointer("/params/value/sessionId").and_then(|v| v.as_str());
        let sid2 = token2[0].pointer("/params/value/sessionId").and_then(|v| v.as_str());
        assert_ne!(
            sid1, sid2,
            "two requests at the same position should produce different session IDs"
        );
    }

    fn set_streaming_debounce(server: &LspServer, milliseconds: u64) {
        let config_request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "workspace/didChangeConfiguration".into(),
            params: Some(json!({
                "settings": {
                    "perl": {
                        "aiCompletion": {
                            "streaming": {
                                "updateDebounceMs": milliseconds
                            }
                        }
                    }
                }
            })),
        };
        let _ = server.handle_request(config_request);
    }

    fn open_doc(server: &LspServer, uri: &str, text: &str) {
        let _ = server.handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "textDocument/didOpen".into(),
            params: Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": text,
                }
            })),
        });
    }

    fn request_streaming_completion(
        server: &LspServer,
        uri: &str,
        character: u32,
        token: &str,
    ) -> Value {
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
            method: "textDocument/perlInlineCompletionStream".into(),
            params: Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 0, "character": character },
                "partialResultToken": token,
            })),
        };

        server.handle_request(request).and_then(|response| response.result).unwrap_or(json!(null))
    }

    struct MockChunkBackend {
        chunks: Vec<&'static str>,
        delays_ms: Vec<u64>,
    }

    impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend for MockChunkBackend {
        fn stream(
            &self,
            _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
            sink: &mut dyn FnMut(
                perl_lsp_rs_core::providers::inline_completion::StreamChunk,
            )
                -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
        ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
            for (idx, chunk) in self.chunks.iter().enumerate() {
                if let Some(delay_ms) = self.delays_ms.get(idx)
                    && *delay_ms > 0
                {
                    thread::sleep(Duration::from_millis(*delay_ms));
                }
                let is_final = idx + 1 == self.chunks.len();
                let _ = sink(perl_lsp_rs_core::providers::inline_completion::StreamChunk {
                    text: (*chunk).to_string(),
                    is_final,
                });
            }
            Ok(())
        }
    }

    struct MockErrorChunkBackend;

    impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
        for MockErrorChunkBackend
    {
        fn stream(
            &self,
            _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
            sink: &mut dyn FnMut(
                perl_lsp_rs_core::providers::inline_completion::StreamChunk,
            )
                -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
        ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
            let _ = sink(perl_lsp_rs_core::providers::inline_completion::StreamChunk {
                text: "1".to_string(),
                is_final: false,
            });
            Err(perl_lsp_rs_core::providers::inline_completion::BackendError::Provider(
                "mock stream error".into(),
            ))
        }
    }

    struct MockAuthBackend;

    impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend for MockAuthBackend {
        fn stream(
            &self,
            _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
            _sink: &mut dyn FnMut(
                perl_lsp_rs_core::providers::inline_completion::StreamChunk,
            )
                -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
        ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
            Err(perl_lsp_rs_core::providers::inline_completion::BackendError::Auth(
                "provider rejected credentials".into(),
            ))
        }
    }

    /// Backend that counts invocations so tests can prove a route never
    /// reached the backend.
    struct CountingChunkBackend {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        chunks: Vec<&'static str>,
    }

    impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
        for CountingChunkBackend
    {
        fn stream(
            &self,
            _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
            sink: &mut dyn FnMut(
                perl_lsp_rs_core::providers::inline_completion::StreamChunk,
            )
                -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
        ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            for (idx, chunk) in self.chunks.iter().enumerate() {
                let _ = sink(perl_lsp_rs_core::providers::inline_completion::StreamChunk {
                    text: (*chunk).to_string(),
                    is_final: idx + 1 == self.chunks.len(),
                });
            }
            Ok(())
        }
    }

    /// Variant of `request_streaming_completion` that carries an explicit
    /// `context` object, so tests can drive trigger-kind and
    /// selected-completion policy through the stream route.
    fn request_streaming_completion_with_context(
        server: &LspServer,
        uri: &str,
        character: u32,
        token: &str,
        context: Value,
    ) -> Value {
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
            method: "textDocument/perlInlineCompletionStream".into(),
            params: Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 0, "character": character },
                "partialResultToken": token,
                "context": context,
            })),
        };

        server.handle_request(request).and_then(|response| response.result).unwrap_or(json!(null))
    }

    #[test]
    fn streaming_completion_mock_backend_cumulative_chunks() {
        let (server, capture) = create_server();
        set_streaming_debounce(&server, 0);

        let backend = MockChunkBackend { chunks: vec!["1", "1;"], delays_ms: vec![0, 0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-mock-chunks.pl";
        open_doc(&server, uri, "my $value = ");
        let result = request_streaming_completion(&server, uri, 12, "stream-mock-1");
        assert!(result.is_null());

        let progress =
            wait_for_progress_messages(&capture, "stream-mock-1", Duration::from_millis(500));
        assert_eq!(progress.len(), 2);

        let expected = ["1", "1;"];
        let mut last_sequence = None;
        for idx in 0..progress.len() {
            let value = &progress[idx]["params"]["value"];
            let sequence = value["sequence"].as_u64().expect("progress should include sequence");
            if let Some(previous) = last_sequence {
                assert!(sequence > previous);
            } else {
                assert_eq!(sequence, 0);
            }
            last_sequence = Some(sequence);
            assert_eq!(value["items"][0]["insertText"], expected[idx]);
        }
    }

    #[test]
    fn streaming_completion_filters_parse_unsafe_final_chunk() {
        let (server, capture) = create_server();
        // fallback=false: a filtered final is a typed final-empty decision
        // (#10246); with fallback enabled the deterministic route would own
        // the final content instead.
        server.test_configure_ai_completion(true, false);
        let backend = MockChunkBackend { chunks: vec!["my $value = ;"], delays_ms: vec![0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-mock-invalid.pl";
        open_doc(&server, uri, "");
        let result = request_streaming_completion(&server, uri, 0, "stream-mock-invalid");
        assert!(result.is_null());

        let progress =
            wait_for_progress_messages(&capture, "stream-mock-invalid", Duration::from_millis(500));
        assert_eq!(progress.len(), 1);
        assert!(progress[0]["params"]["value"]["isFinal"].as_bool().unwrap_or(false));
        assert!(progress[0]["params"]["value"]["items"].as_array().is_some_and(Vec::is_empty));
    }

    #[test]
    fn streaming_completion_sequence_starts_at_zero_after_filtered_prefix() {
        let (server, capture) = create_server();
        let backend = MockChunkBackend {
            chunks: vec!["my $value = ;", "my $value = 1;"],
            delays_ms: vec![0, 0],
        };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-mock-filtered-prefix.pl";
        open_doc(&server, uri, "");
        let result = request_streaming_completion(&server, uri, 0, "stream-mock-filtered-prefix");
        assert!(result.is_null());

        let progress = wait_for_progress_messages(
            &capture,
            "stream-mock-filtered-prefix",
            Duration::from_millis(500),
        );
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0]["params"]["value"]["sequence"], 0);
        assert_eq!(progress[0]["params"]["value"]["items"][0]["insertText"], "my $value = 1;");
        assert!(progress[0]["params"]["value"]["isFinal"].as_bool().unwrap_or(false));
    }

    #[test]
    fn streaming_completion_debounces_intermediate_chunks_but_emits_final() {
        let (server, capture) = create_server();
        set_streaming_debounce(&server, 1_000);

        let backend = MockChunkBackend { chunks: vec!["1", "1;", "1;"], delays_ms: vec![0, 0, 0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-debounce.pl";
        open_doc(&server, uri, "my $value = ");
        let result = request_streaming_completion(&server, uri, 12, "stream-debounce-1");
        assert!(result.is_null());

        let progress =
            wait_for_progress_messages(&capture, "stream-debounce-1", Duration::from_millis(500));
        assert_eq!(progress.len(), 2, "first and final updates should be emitted");
        assert_eq!(progress[0]["params"]["value"]["items"][0]["insertText"], "1");
        assert_eq!(progress[1]["params"]["value"]["items"][0]["insertText"], "1;");
        assert_eq!(progress[1]["params"]["value"]["isFinal"], true);
    }

    #[test]
    fn streaming_completion_mock_backend_cancel_previous_isolation() {
        let (server, capture) = create_server();
        let server = Arc::new(server);

        let backend = MockChunkBackend {
            chunks: vec!["fi", "find_", "find_user($id)"],
            delays_ms: vec![0, 120, 120],
        };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-mock-cancel.pl";
        open_doc(&server, uri, "my $obj = Package->");

        let first = {
            let server = Arc::clone(&server);
            let uri = uri.to_string();
            thread::spawn(move || {
                request_streaming_completion(&server, &uri, 19, "stream-cancel-old");
            })
        };

        thread::sleep(Duration::from_millis(30));
        let new_result = request_streaming_completion(&server, uri, 19, "stream-cancel-new");
        assert!(new_result.is_null());

        first.join().expect("first streaming request thread panicked");

        thread::sleep(Duration::from_millis(250));
        let old_progress =
            wait_for_progress_messages(&capture, "stream-cancel-old", Duration::from_millis(300));
        let new_progress =
            wait_for_progress_messages(&capture, "stream-cancel-new", Duration::from_millis(300));

        assert_eq!(old_progress.len(), 1);
        assert!(!new_progress.is_empty());
    }

    #[test]
    fn completion_stream_cancel_storm_keeps_one_live_session() {
        let (server, _capture) = create_server();

        let backend = MockChunkBackend {
            chunks: vec!["fi", "find_", "find_user($id)"],
            delays_ms: vec![20, 20, 20],
        };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-mock-cancel-storm.pl";
        open_doc(&server, uri, "my $obj = Package->");

        for request in 0..25 {
            let result = request_streaming_completion(
                &server,
                uri,
                19,
                &format!("stream-cancel-storm-{request}"),
            );
            assert!(result.is_null(), "streaming completion should respond with null");
            assert_eq!(
                server.memory_state_snapshot().stream_sessions,
                1,
                "same-key stream requests must replace the retained session instead of growing"
            );
        }

        assert_eq!(
            server.memory_state_snapshot().stream_sessions,
            1,
            "a same-key cancel storm should converge to the latest live session only"
        );
    }

    #[test]
    fn streaming_completion_mock_backend_error_sends_final_progress() {
        let (server, capture) = create_server();
        server.test_install_ai_backend(Some(Arc::new(MockErrorChunkBackend)));

        let uri = "file:///streaming-mock-error.pl";
        open_doc(&server, uri, "my $value = ");

        let result = request_streaming_completion(&server, uri, 12, "stream-error-1");
        assert!(result.is_null());

        let deadline = Instant::now() + Duration::from_millis(500);
        let progress = loop {
            let progress =
                wait_for_progress_messages(&capture, "stream-error-1", Duration::from_millis(50));
            let has_final = progress.iter().any(|frame| {
                frame
                    .pointer("/params/value/isFinal")
                    .and_then(Value::as_bool)
                    .is_some_and(|is_final| is_final)
            });
            if has_final || Instant::now() >= deadline {
                break progress;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(!progress.is_empty());
        assert_eq!(progress[0]["params"]["value"]["items"][0]["insertText"], "1");
        let final_progress =
            progress.last().expect("error path should emit at least one progress frame");
        assert!(
            final_progress
                .pointer("/params/value/isFinal")
                .and_then(Value::as_bool)
                .is_some_and(|is_final| is_final),
            "error path should emit a final progress frame"
        );
        assert_eq!(
            final_progress["params"]["value"]["items"][0]["insertText"], "1",
            "error path should preserve final cumulative text"
        );
        assert!(
            final_progress["params"]["value"]["sequence"].as_u64().is_some(),
            "final progress frame should carry sequence"
        );
    }

    #[test]
    fn ai_auth_failure_notifies_once_across_one_shot_and_streaming_paths() {
        let (server, capture) = create_server();
        server.test_install_ai_backend(Some(Arc::new(MockAuthBackend)));

        let uri = "file:///streaming-auth-error.pl";
        open_doc(&server, uri, "my $obj = Package->");

        let _ = request_streaming_completion(&server, uri, 19, "stream-auth-error");

        let deadline = Instant::now() + Duration::from_millis(500);
        let messages = loop {
            let messages: Vec<_> = capture
                .messages()
                .into_iter()
                .filter(|message| {
                    message.get("method").and_then(Value::as_str) == Some("window/showMessage")
                })
                .collect();
            if !messages.is_empty() || Instant::now() >= deadline {
                break messages;
            }
            thread::sleep(Duration::from_millis(10));
        };

        assert_eq!(messages.len(), 1, "authentication failure should be shown once");
        assert_eq!(messages[0]["params"]["type"], 2);
        assert_eq!(
            messages[0]["params"]["message"],
            "AI inline completion authentication failed. Check the configured API key and provider settings."
        );

        server.handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: None,
            method: "workspace/didChangeConfiguration".into(),
            params: Some(json!({
                "settings": {
                    "perl": {
                    // Any client-settings notification starts a new
                    // configuration session; envelope fields like timeoutMs
                    // remain generic-settable (#4997 rejected arm/select
                    // fields are not needed to exercise the reset).
                    "aiCompletion": { "timeoutMs": 2500 }
                }
                }
            })),
        });
        server.test_install_ai_backend(Some(Arc::new(MockAuthBackend)));
        let _ = request_streaming_completion(&server, uri, 19, "stream-auth-error-after-config");

        let deadline = Instant::now() + Duration::from_millis(500);
        let messages_after_config = loop {
            let messages: Vec<_> = capture
                .messages()
                .into_iter()
                .filter(|message| {
                    message.get("method").and_then(Value::as_str) == Some("window/showMessage")
                })
                .collect();
            if messages.len() >= 2 || Instant::now() >= deadline {
                break messages;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(messages_after_config.len(), 2);

        let _ = server.handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".into(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
            method: "textDocument/inlineCompletion".into(),
            params: Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 19 }
            })),
        });
        assert_eq!(
            capture
                .messages()
                .into_iter()
                .filter(|message| {
                    message.get("method").and_then(Value::as_str) == Some("window/showMessage")
                })
                .count(),
            2,
            "one-shot and streaming paths should share the reset deduplication key"
        );
    }

    /// An automatic custom-stream request is invoked-only policy: it must be
    /// delegated to the deterministic-only standard route before any session or
    /// backend work, making zero backend calls and emitting no stream progress.
    #[test]
    fn streaming_completion_automatic_trigger_makes_zero_backend_calls() {
        let (server, capture) = create_server();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let backend = CountingChunkBackend { calls: Arc::clone(&calls), chunks: vec!["1"] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-automatic.pl";
        open_doc(&server, uri, "use str");
        let result = request_streaming_completion_with_context(
            &server,
            uri,
            7,
            "stream-automatic",
            json!({ "triggerKind": 2 }),
        );

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "an automatic custom-stream request must never reach the AI backend"
        );
        assert!(
            !result.is_null(),
            "an automatic request delegates to the standard inline-completion route"
        );
        let texts: Vec<&str> = result["items"]
            .as_array()
            .map(|items| items.iter().filter_map(|item| item["insertText"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            texts,
            vec!["strict;"],
            "the delegated answer must be the deterministic standard-route candidate"
        );
        let progress: Vec<_> = capture
            .messages()
            .into_iter()
            .filter(|message| {
                message.pointer("/params/token").and_then(Value::as_str) == Some("stream-automatic")
            })
            .collect();
        assert!(progress.is_empty(), "a delegated request must not emit stream progress");
    }

    /// The actual `selectedCompletionInfo` reaches the stream route: a selected
    /// completion the external candidate does not extend suppresses it, exactly
    /// as the buffered route would.
    #[test]
    fn streaming_completion_selected_completion_info_mismatch_filters_candidate() {
        let (server, capture) = create_server();
        // fallback=false: the filtered final is a typed final-empty decision.
        server.test_configure_ai_completion(true, false);
        let backend = MockChunkBackend { chunks: vec!["strict;"], delays_ms: vec![0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-selected-mismatch.pl";
        open_doc(&server, uri, "use ");
        let result = request_streaming_completion_with_context(
            &server,
            uri,
            4,
            "stream-selected-mismatch",
            json!({
                "triggerKind": 1,
                "selectedCompletionInfo": {
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": 4 }
                    },
                    "text": "strictlyDifferent"
                }
            }),
        );
        assert!(result.is_null());

        let progress = wait_for_progress_messages(
            &capture,
            "stream-selected-mismatch",
            Duration::from_millis(500),
        );
        assert_eq!(progress.len(), 1);
        let value = &progress[0]["params"]["value"];
        assert!(value["isFinal"].as_bool().unwrap_or(false));
        assert!(
            value["items"].as_array().is_some_and(Vec::is_empty),
            "a candidate that does not extend the selected completion must be filtered"
        );
    }

    /// A compatible selected completion preserves the exact accepted
    /// replacement range through the stream route, matching the buffered
    /// route's contract.
    #[test]
    fn streaming_completion_selected_completion_info_match_preserves_range() {
        let (server, capture) = create_server();
        server.test_configure_ai_completion(true, false);
        let backend = MockChunkBackend { chunks: vec!["strict;"], delays_ms: vec![0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-selected-match.pl";
        open_doc(&server, uri, "use str");
        let result = request_streaming_completion_with_context(
            &server,
            uri,
            7,
            "stream-selected-match",
            json!({
                "triggerKind": 1,
                "selectedCompletionInfo": {
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": 7 }
                    },
                    "text": "strict"
                }
            }),
        );
        assert!(result.is_null());

        let progress = wait_for_progress_messages(
            &capture,
            "stream-selected-match",
            Duration::from_millis(500),
        );
        assert_eq!(progress.len(), 1);
        let value = &progress[0]["params"]["value"];
        assert!(value["isFinal"].as_bool().unwrap_or(false));
        let item = &value["items"][0];
        assert_eq!(item["insertText"], "strict;");
        assert_eq!(
            item["range"],
            json!({
                "start": { "line": 0, "character": 4 },
                "end": { "line": 0, "character": 7 }
            }),
            "a compatible selected completion must keep the exact accepted replacement range"
        );
    }

    /// A filtered final with `fallback=true` is a typed fallback decision: the
    /// deterministic route owns the final content instead of the unsafe
    /// external text.
    #[test]
    fn streaming_completion_filtered_final_with_fallback_returns_deterministic() {
        let (server, capture) = create_server();
        server.test_configure_ai_completion(true, true);
        let backend = MockChunkBackend { chunks: vec!["strict; my $x = ;"], delays_ms: vec![0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-filtered-fallback.pl";
        open_doc(&server, uri, "use str");
        let result = request_streaming_completion_with_context(
            &server,
            uri,
            7,
            "stream-filtered-fallback",
            json!({ "triggerKind": 1 }),
        );
        assert!(result.is_null());

        let progress = wait_for_progress_messages(
            &capture,
            "stream-filtered-fallback",
            Duration::from_millis(500),
        );
        assert_eq!(progress.len(), 1);
        let value = &progress[0]["params"]["value"];
        assert!(value["isFinal"].as_bool().unwrap_or(false));
        let texts: Vec<&str> = value["items"]
            .as_array()
            .map(|items| items.iter().filter_map(|item| item["insertText"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            texts,
            vec!["strict;"],
            "a filtered final with fallback must hand content to the deterministic route"
        );
    }

    /// A filtered final with `fallback=false` is a typed final-empty decision.
    #[test]
    fn streaming_completion_filtered_final_without_fallback_returns_empty() {
        let (server, capture) = create_server();
        server.test_configure_ai_completion(true, false);
        let backend = MockChunkBackend { chunks: vec!["strict; my $x = ;"], delays_ms: vec![0] };
        server.test_install_ai_backend(Some(Arc::new(backend)));

        let uri = "file:///streaming-filtered-no-fallback.pl";
        open_doc(&server, uri, "use str");
        let result = request_streaming_completion_with_context(
            &server,
            uri,
            7,
            "stream-filtered-no-fallback",
            json!({ "triggerKind": 1 }),
        );
        assert!(result.is_null());

        let progress = wait_for_progress_messages(
            &capture,
            "stream-filtered-no-fallback",
            Duration::from_millis(500),
        );
        assert_eq!(progress.len(), 1);
        let value = &progress[0]["params"]["value"];
        assert!(value["isFinal"].as_bool().unwrap_or(false));
        assert!(
            value["items"].as_array().is_some_and(Vec::is_empty),
            "a filtered final without fallback must be final and empty"
        );
    }
}
