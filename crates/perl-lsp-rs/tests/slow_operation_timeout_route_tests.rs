//! Production-route proof for the `$/test/slowOperation` server-side timeout.
//!
//! The unit tests in `runtime::dispatch::experimental` call the handler
//! directly. These drive the same seam the way a client reaches it — through
//! `handle_request` and the `"$/test/slowOperation"` routing arm — so the
//! evidence covers the real dispatch path, not just the function body.
//!
//! The seam under proof:
//!
//! ```ignore
//! if let Some(to) = timeout && start.elapsed() >= to { .. }
//! ```
//!
//! Each test moves exactly one term of that condition.
//!
//! Run:
//!     cargo test -p perl-lsp-rs --features expose_lsp_test_api \
//!         --test slow_operation_timeout_route_tests

#![cfg(feature = "expose_lsp_test_api")]
// Integration test: `expect()` carries the assertion message. The
// workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `SERVER_CANCELLED` from the LSP error table, pinned literally so a swapped
/// error variant fails rather than silently passing.
const SERVER_CANCELLED: i32 = -32802;

fn init_server() -> LspServer {
    let server = LspServer::new();
    server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(json!({"processId": null, "rootUri": null, "capabilities": {}})),
    });
    server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    });
    server
}

fn slow_operation(server: &LspServer, params: Option<serde_json::Value>) -> JsonRpcResponseParts {
    let response = server
        .handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(7_i64)),
            method: "$/test/slowOperation".to_string(),
            params,
        })
        .expect("$/test/slowOperation is a request and must produce a response");

    JsonRpcResponseParts {
        result: response.result,
        error_code: response.error.map(|error| error.code),
    }
}

struct JsonRpcResponseParts {
    result: Option<serde_json::Value>,
    error_code: Option<i32>,
}

#[test]
fn elapsed_past_the_timeout_returns_server_cancelled_over_the_route() -> TestResult {
    let server = init_server();

    let parts = slow_operation(&server, Some(json!({"serverTimeoutMs": 1})));

    assert_eq!(
        parts.error_code,
        Some(SERVER_CANCELLED),
        "an expired server timeout must surface as SERVER_CANCELLED through the dispatch route"
    );
    Ok(())
}

#[test]
fn absent_timeout_completes_over_the_route() -> TestResult {
    // Moves `let Some(to) = timeout` to None; everything else is unchanged.
    let server = init_server();

    let parts = slow_operation(&server, None);

    assert_eq!(parts.error_code, None, "no serverTimeoutMs must not cancel the request");
    assert_eq!(parts.result, Some(json!({"status": "completed", "iterations": 20})));
    Ok(())
}

#[test]
fn timeout_not_yet_reached_completes_over_the_route() -> TestResult {
    // Moves only `start.elapsed() >= to` to false: the operation runs ~1s, so a
    // 60s budget is never reached.
    let server = init_server();

    let parts = slow_operation(&server, Some(json!({"serverTimeoutMs": 60_000})));

    assert_eq!(parts.error_code, None, "a budget beyond the operation's runtime must not fire");
    assert_eq!(parts.result, Some(json!({"status": "completed", "iterations": 20})));
    Ok(())
}
