//! In-process proof that test-only protocol routes are absent from ordinary
//! product builds (#9207).
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

#[test]
fn slow_operation_is_method_not_found_without_test_api_profile() -> TestResult {
    let server = init_server();
    let response = server
        .handle_request(JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer(42_i64)),
            method: "$/test/slowOperation".to_string(),
            params: Some(json!({})),
        })
        .ok_or("slowOperation request must produce a response envelope")?;

    let code = response.error.as_ref().map(|error| error.code);
    assert_eq!(
        code,
        Some(-32601),
        "ordinary product dispatch must not accept test-only slowOperation, got {response:?}"
    );
    Ok(())
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
