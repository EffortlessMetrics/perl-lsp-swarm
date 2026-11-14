//! Harness-profile proof for classified test-only protocol routes (#9207).
//!
//! Ordinary **product** exclusion is proven by:
//! - `cargo xtask activation check-test-api-product` (routing cfg + capability snapshots)
//! - `perllsp` exact-process test (`lsp_test_api_product_exclusion_process.rs`)
//!
//! Integration tests compile with `cfg(test)`, so `$/test/slowOperation` remains
//! reachable here by design; this file only pins harness retention when
//! `expose_lsp_test_api` is enabled.
//!
//! Run:
//!     RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs \
//!         --test test_api_protocol_product_exclusion_tests -- --test-threads=2

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

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

#[cfg(feature = "expose_lsp_test_api")]
mod harness_profile {
    use super::*;

    #[test]
    fn slow_operation_remains_available_in_test_api_harness_profile() -> TestResult {
        let server = init_server();
        let response = server
            .handle_request(JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: Some(perl_lsp::protocol::JsonRpcId::Integer(43_i64)),
                method: "$/test/slowOperation".to_string(),
                params: Some(json!({})),
            })
            .ok_or("slowOperation request must produce a response envelope")?;

        assert!(
            response.error.is_none(),
            "harness profile must keep slowOperation reachable, got {response:?}"
        );
        Ok(())
    }
}
