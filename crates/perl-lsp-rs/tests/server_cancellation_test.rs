//! Server-side cancellation test for the `$/test/slowOperation` endpoint.
//!
//! Requires the `expose_lsp_test_api` feature because the test endpoint is
//! disabled when neither test mode nor that feature is enabled (issue #4632).
//!
//! ```text
//! cargo test -p perl-lsp-rs --features expose_lsp_test_api --test server_cancellation_test
//! ```
#![cfg(feature = "expose_lsp_test_api")]

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

#[test]
fn server_side_cancellation_emits_err_server_cancelled() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    // Initialize server
    let _ = server.handle_request(serde_json::from_value::<JsonRpcRequest>(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "rootUri": null,
            "capabilities": {}
        }
    }))?);
    let _ = server.handle_request(serde_json::from_value::<JsonRpcRequest>(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "initialized",
        "params": {}
    }))?);

    // Request slow operation with server-side timeout
    let response = server.handle_request(serde_json::from_value::<JsonRpcRequest>(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "$/test/slowOperation",
        "params": {"serverTimeoutMs": 200}
    }))?);

    let resp = response.ok_or("expected JSON-RPC response")?;
    let err = resp.error.ok_or("expected error response")?;
    assert_eq!(err.code, -32802, "expected ERR_SERVER_CANCELLED");

    Ok(())
}
