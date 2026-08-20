//! Integration tests for the AI inline completion path.
//!
//! These tests exercise `handle_inline_completion` end-to-end through the
//! JSON-RPC dispatch layer, injecting mock AI backends to verify each
//! fallback path:
//!
//! 1. AI disabled (default) -- deterministic completions returned
//! 2. AI enabled, backend succeeds -- AI result returned
//! 3. AI enabled, backend times out, fallback=true -- deterministic fallback
//! 4. AI enabled, backend rate-limited, fallback=true -- deterministic fallback
//! 5. AI enabled, backend errors, fallback=false -- empty result
//! 6. AI enabled, no backend registered, fallback=true -- deterministic fallback
//! 7. AI enabled, no backend registered, fallback=false -- empty result
//! 8. AI enabled, backend errors, fallback=true -- deterministic fallback
//!
//! Requires the `expose_lsp_test_api` feature to access `LspServer::test_*`
//! methods.
//!
//! Run with:
//!   RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs \
//!     --features expose_lsp_test_api \
//!     --test lsp_ai_inline_completion_tests -- --test-threads=2

#![cfg(feature = "expose_lsp_test_api")]

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;
use std::sync::Arc;

// ── Test helpers ────────────────────────────────────────────────────────────

fn setup_server() -> Result<LspServer, Box<dyn std::error::Error>> {
    let server = LspServer::new();

    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "initialize".to_string(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
    };
    server.handle_request(init_request);

    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    };
    server.handle_request(initialized_notification);

    Ok(server)
}

fn open_doc(server: &LspServer, uri: &str, text: &str) {
    let request = JsonRpcRequest {
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
    };
    server.handle_request(request);
}

fn inline_completion(
    server: &LspServer,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    response.result.ok_or("result field present".into())
}

// ── Mock backends ───────────────────────────────────────────────────────────

/// Mock backend that returns a fixed completion string.
struct MockSuccessBackend {
    response: String,
}

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
    for MockSuccessBackend
{
    fn stream(
        &self,
        _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        )
            -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        sink(perl_lsp_rs_core::providers::inline_completion::StreamChunk {
            text: self.response.clone(),
            is_final: true,
        });
        Ok(())
    }
}

/// Mock backend that always returns a timeout error.
struct MockTimeoutBackend;

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
    for MockTimeoutBackend
{
    fn stream(
        &self,
        _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        _sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        )
            -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        Err(perl_lsp_rs_core::providers::inline_completion::BackendError::Timeout)
    }
}

/// Mock backend that always returns a rate-limited error.
struct MockRateLimitedBackend;

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
    for MockRateLimitedBackend
{
    fn stream(
        &self,
        _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        _sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        )
            -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        Err(perl_lsp_rs_core::providers::inline_completion::BackendError::RateLimited)
    }
}

/// Mock backend that always returns a provider error.
struct MockErrorBackend;

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend for MockErrorBackend {
    fn stream(
        &self,
        _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        _sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        )
            -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        Err(perl_lsp_rs_core::providers::inline_completion::BackendError::Provider(
            "test error".into(),
        ))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

/// When AI completion is disabled (the default), the handler returns
/// deterministic results regardless of any registered backend.
#[test]
fn test_ai_disabled_returns_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_test.pl";
    open_doc(&server, uri, "use ");

    // Install a success backend but leave AI disabled (default)
    server.test_install_ai_backend(Some(Arc::new(MockSuccessBackend {
        response: "AI_RESPONSE".into(),
    })));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    // Should get deterministic results (strict;, warnings;, etc.), not AI
    assert!(!items.is_empty());
    let texts: Vec<&str> = items.iter().filter_map(|item| item["insertText"].as_str()).collect();
    assert!(
        texts.contains(&"strict;"),
        "expected deterministic 'strict;' when AI is disabled, got: {texts:?}",
    );
    assert!(
        !texts.iter().any(|t| t.contains("AI_RESPONSE")),
        "AI response should not appear when AI is disabled"
    );
    Ok(())
}

/// When AI is enabled and the backend succeeds, the AI result is returned.
#[test]
fn test_ai_enabled_success_backend_returns_ai_result() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_success.pl";
    // Use a prefix that triggers deterministic completions so we can
    // verify that AI results take priority.
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, true);
    server.test_install_ai_backend(Some(Arc::new(MockSuccessBackend {
        response: "AI_COMPLETION_TEXT".into(),
    })));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(!items.is_empty(), "expected AI completion items");
    let first_text = items[0]["insertText"].as_str().ok_or("insertText not a string")?;
    assert_eq!(first_text, "AI_COMPLETION_TEXT", "expected AI backend result to be returned");
    Ok(())
}

/// AI output that worsens the current parse state must not reach the client.
#[test]
fn test_ai_invalid_parse_output_is_filtered() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_invalid_parse.pl";
    let source = "";
    open_doc(&server, uri, source);

    server.test_configure_ai_completion(true, false);
    server.test_install_ai_backend(Some(Arc::new(MockSuccessBackend {
        response: "my $value = ;".into(),
    })));

    let result = inline_completion(&server, uri, 0, source.encode_utf16().count() as u32)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(items.is_empty(), "parse-unsafe AI output must be filtered: {items:?}");
    Ok(())
}

/// When AI is enabled with fallback=true and the backend times out,
/// the handler falls back to deterministic completions.
#[test]
fn test_ai_timeout_with_fallback_returns_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_timeout.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, true);
    server.test_install_ai_backend(Some(Arc::new(MockTimeoutBackend)));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(!items.is_empty(), "expected deterministic fallback on timeout");
    let texts: Vec<&str> = items.iter().filter_map(|item| item["insertText"].as_str()).collect();
    assert!(
        texts.contains(&"strict;"),
        "expected deterministic 'strict;' on timeout fallback, got: {texts:?}",
    );
    Ok(())
}

/// When AI is enabled with fallback=true and the backend is rate-limited,
/// the handler falls back to deterministic completions.
#[test]
fn test_ai_rate_limited_with_fallback_returns_deterministic()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_ratelimit.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, true);
    server.test_install_ai_backend(Some(Arc::new(MockRateLimitedBackend)));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(!items.is_empty(), "expected deterministic fallback on rate limit");
    let texts: Vec<&str> = items.iter().filter_map(|item| item["insertText"].as_str()).collect();
    assert!(
        texts.contains(&"strict;"),
        "expected deterministic 'strict;' on rate limit fallback, got: {texts:?}",
    );
    Ok(())
}

/// When AI is enabled with fallback=false and the backend errors,
/// the handler returns an empty result (no fallback).
#[test]
fn test_ai_error_without_fallback_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_error_nofb.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, false);
    server.test_install_ai_backend(Some(Arc::new(MockErrorBackend)));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(
        items.is_empty(),
        "expected empty result when AI errors and fallback is disabled, got {} items",
        items.len()
    );
    Ok(())
}

/// When AI is enabled but no backend is registered, the handler falls back
/// to deterministic completions (try_ai_inline_completion returns Ok(vec![])
/// which triggers fallback).
#[test]
fn test_ai_no_backend_registered_returns_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_nobackend.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, true);
    // Do NOT install any backend -- ai_inline_backend remains None

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(!items.is_empty(), "expected deterministic fallback when no backend is registered");
    let texts: Vec<&str> = items.iter().filter_map(|item| item["insertText"].as_str()).collect();
    assert!(
        texts.contains(&"strict;"),
        "expected deterministic 'strict;' when no backend is registered, got: {texts:?}",
    );
    Ok(())
}

/// When AI is enabled with fallback=false and no backend is registered,
/// the handler returns empty (no backend = Ok(empty), fallback disabled
/// returns empty).
#[test]
fn test_ai_no_backend_no_fallback_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_nobackend_nofb.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, false);
    // No backend installed

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(
        items.is_empty(),
        "expected empty result when no backend and fallback disabled, got {} items",
        items.len()
    );
    Ok(())
}

/// When AI is enabled with fallback=true and the backend returns a provider
/// error, the handler falls back to deterministic completions.
#[test]
fn test_ai_provider_error_with_fallback_returns_deterministic()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_error_fb.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, true);
    server.test_install_ai_backend(Some(Arc::new(MockErrorBackend)));

    let result = inline_completion(&server, uri, 0, 4)?;
    let items = result["items"].as_array().ok_or("items array")?;

    assert!(!items.is_empty(), "expected deterministic fallback on provider error");
    let texts: Vec<&str> = items.iter().filter_map(|item| item["insertText"].as_str()).collect();
    assert!(
        texts.contains(&"strict;"),
        "expected deterministic 'strict;' on provider error fallback, got: {texts:?}",
    );
    Ok(())
}

// ── Automatic requests are local-first ──────────────────────────────────────

/// Mock backend that counts how many times it was consulted and blocks for the
/// caller's whole timeout budget, standing in for a slow or unreachable remote.
struct CountingSlowBackend {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
    for CountingSlowBackend
{
    fn stream(
        &self,
        req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        _sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        )
            -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(req.timeout_ms.min(200)));
        Err(perl_lsp_rs_core::providers::inline_completion::BackendError::Timeout)
    }
}

/// An automatic request is triggered by a keystroke, so it must not pay for a
/// remote round trip: the backend is never consulted and the deterministic
/// answer is returned without waiting on it.
#[test]
fn test_automatic_request_makes_no_backend_call() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_automatic_local_first.pl";
    open_doc(&server, uri, "use str");

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server.test_configure_ai_completion(true, true);
    server
        .test_install_ai_backend(Some(Arc::new(CountingSlowBackend { calls: Arc::clone(&calls) })));

    let started = std::time::Instant::now();
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 7 },
            "context": { "triggerKind": 2 }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    let elapsed = started.elapsed();
    let result = response.result.ok_or("result field present")?;

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an automatic request must not consult the AI backend"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "the deterministic answer must not wait behind the backend budget: {elapsed:?}"
    );

    let texts: Vec<&str> = result["items"]
        .as_array()
        .ok_or("items array")?
        .iter()
        .filter_map(|item| item["insertText"].as_str())
        .collect();
    assert_eq!(texts, vec!["strict;"], "expected the deterministic candidate");
    Ok(())
}

/// An explicitly invoked request keeps the remote budget: the backend is
/// consulted, and its failure still falls back to the deterministic answer.
#[test]
fn test_invoked_request_still_consults_backend() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_invoked_consults_backend.pl";
    open_doc(&server, uri, "use str");

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server.test_configure_ai_completion(true, true);
    server
        .test_install_ai_backend(Some(Arc::new(CountingSlowBackend { calls: Arc::clone(&calls) })));

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 7 },
            "context": { "triggerKind": 1 }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    let result = response.result.ok_or("result field present")?;

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "an invoked request must still consult the AI backend"
    );
    let texts: Vec<&str> = result["items"]
        .as_array()
        .ok_or("items array")?
        .iter()
        .filter_map(|item| item["insertText"].as_str())
        .collect();
    assert!(texts.contains(&"strict;"), "expected deterministic fallback, got: {texts:?}");
    Ok(())
}

/// External backend text carries no local supporting fact. Even when the
/// backend answers instantly with clean single-line Perl, an automatic request
/// shows the deterministic candidate instead.
#[test]
fn test_automatic_request_never_shows_backend_text() -> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_automatic_no_backend_text.pl";
    open_doc(&server, uri, "use str");

    server.test_configure_ai_completion(true, true);
    server.test_install_ai_backend(Some(Arc::new(MockSuccessBackend { response: "ict;".into() })));

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 7 },
            "context": { "triggerKind": 2 }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    let result = response.result.ok_or("result field present")?;

    let texts: Vec<&str> = result["items"]
        .as_array()
        .ok_or("items array")?
        .iter()
        .filter_map(|item| item["insertText"].as_str())
        .collect();
    assert_eq!(texts, vec!["strict;"], "automatic ghost text must come from local evidence");
    Ok(())
}

/// Issue #10246 parity: the buffered route and the custom stream route share
/// one evaluated finalization seam, so the same external candidate must
/// receive the same selected-completion verdict in both modes. A compatible
/// selected completion keeps the exact accepted replacement range.
#[test]
fn test_invoked_ai_candidate_respects_selected_completion_info_range()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_selected_completion_match.pl";
    open_doc(&server, uri, "use str");

    server.test_configure_ai_completion(true, true);
    server
        .test_install_ai_backend(Some(Arc::new(MockSuccessBackend { response: "strict;".into() })));

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 7 },
            "context": {
                "triggerKind": 1,
                "selectedCompletionInfo": {
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": 7 }
                    },
                    "text": "strict"
                }
            }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    let result = response.result.ok_or("result field present")?;

    let items = result["items"].as_array().ok_or("items array")?;
    let item = items
        .iter()
        .find(|item| item["insertText"].as_str() == Some("strict;"))
        .ok_or("external candidate must survive a compatible selected completion")?;
    assert_eq!(
        item["range"],
        json!({
            "start": { "line": 0, "character": 4 },
            "end": { "line": 0, "character": 7 }
        }),
        "a compatible selected completion must keep the exact accepted replacement range"
    );
    Ok(())
}

/// Issue #10246 parity: an incompatible `selectedCompletionInfo` suppresses
/// the external candidate in the buffered route exactly as in the stream
/// route; with fallback disabled the result is final and empty.
#[test]
fn test_invoked_ai_candidate_suppressed_by_mismatched_selected_completion_info()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_selected_completion_mismatch.pl";
    open_doc(&server, uri, "use ");

    server.test_configure_ai_completion(true, false);
    server
        .test_install_ai_backend(Some(Arc::new(MockSuccessBackend { response: "strict;".into() })));

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 4 },
            "context": {
                "triggerKind": 1,
                "selectedCompletionInfo": {
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": 4 }
                    },
                    "text": "strictlyDifferent"
                }
            }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    let result = response.result.ok_or("result field present")?;

    let items = result["items"].as_array().ok_or("items array")?;
    assert!(
        items.is_empty(),
        "a candidate that does not extend the selected completion must be filtered, got: {items:?}"
    );
    Ok(())
}
