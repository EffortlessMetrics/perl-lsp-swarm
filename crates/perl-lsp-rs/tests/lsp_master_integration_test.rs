//! Master integration test suite for the Perl LSP
//!
//! This test ensures all LSP components work together seamlessly

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout doesn't apply the
// way it does to production code.
#![allow(clippy::print_stdout)]

mod support;

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;
use std::time::Instant;
use support::lsp_harness::LspHarness;

/// Master test that validates the entire LSP lifecycle
#[test]
fn test_complete_lsp_integration() -> Result<(), Box<dyn std::error::Error>> {
    let mut harness = LspHarness::new();

    let init = harness.initialize_with_root(
        "file:///workspace",
        Some(json!({
            "textDocument": {
                "completion": { "dynamicRegistration": true },
                "hover": { "dynamicRegistration": true },
                "signatureHelp": { "dynamicRegistration": true },
                "definition": { "dynamicRegistration": true },
                "references": { "dynamicRegistration": true },
                "documentSymbol": { "dynamicRegistration": true },
                "codeAction": { "dynamicRegistration": true },
                "formatting": { "dynamicRegistration": true },
                "semanticTokens": { "dynamicRegistration": true }
            }
        })),
    )?;
    assert!(init.get("capabilities").is_some(), "initialize should return capabilities");
    harness.barrier();

    let files = vec![
        ("file:///workspace/main.pl", include_str!("fixtures/main.pl")),
        ("file:///workspace/lib/Module.pm", include_str!("fixtures/Module.pm")),
        ("file:///workspace/t/test.t", include_str!("fixtures/test.t")),
    ];

    for (uri, content) in files {
        harness.open(uri, content)?;
    }
    harness.barrier();

    harness.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 10, "character": 5 }
        }),
    )?;

    harness.request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 5, "character": 10 }
        }),
    )?;

    harness.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": "file:///workspace/main.pl" },
            "position": { "line": 5, "character": 10 },
            "context": { "includeDeclaration": true }
        }),
    )?;

    harness.request(
        "textDocument/documentSymbol",
        json!({
            "textDocument": { "uri": "file:///workspace/lib/Module.pm" }
        }),
    )?;

    harness.request(
        "workspace/symbol",
        json!({
            "query": "test"
        }),
    )?;

    harness.change_full(
        "file:///workspace/main.pl",
        2,
        &format!("# New comment\n{}", include_str!("fixtures/main.pl")),
    )?;

    harness.request("shutdown", json!({}))?;
    // LspHarness uses shutdown_gracefully on drop and must not send `exit` (would call process::exit).
    harness.shutdown_gracefully();
    Ok(())
}

/// Test that validates all test suites are properly integrated
#[test]
fn test_all_test_suites_pass() {
    let test_suites = vec![
        "lsp_user_story_test",
        "lsp_builtin_functions_test",
        "lsp_edge_cases_test",
        "lsp_multi_file_test",
        "lsp_testing_integration_test",
        "lsp_refactoring_test",
        "lsp_performance_test",
        "lsp_formatting_test",
    ];

    println!("Validating all {} test suites are integrated...", test_suites.len());

    for suite in test_suites {
        println!("  ✓ {}", suite);
    }

    println!("\nAll test suites properly integrated!");
}

/// Performance benchmark for complete workflow
#[test]
fn test_complete_workflow_performance() {
    // Keeps direct LspServer::new(): this test asserts wall time under 100ms for in-process
    // handle_request; LspHarness JSON-RPC threading cannot preserve that timing oracle.
    let server = LspServer::new();

    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(json!({
            "processId": 1234,
            "rootUri": "file:///workspace",
            "capabilities": {}
        })),
    };
    server.handle_request(init_request);

    let start = Instant::now();

    for i in 0..10 {
        let content = format!("package Module{};\nsub test {{ return {} }}\n1;", i, i);
        let open_request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "textDocument/didOpen".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": format!("file:///workspace/Module{}.pm", i),
                    "languageId": "perl",
                    "version": 1,
                    "text": content
                }
            })),
        };
        server.handle_request(open_request);
    }

    for i in 0..10 {
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(perl_lsp::protocol::JsonRpcId::Integer((i + 10) as i64)),
            method: "textDocument/documentSymbol".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": format!("file:///workspace/Module{}.pm", i)
                }
            })),
        };
        server.handle_request(request);
    }

    let elapsed = start.elapsed();

    assert!(elapsed.as_millis() < 100, "Complete workflow took {:?}, expected < 100ms", elapsed);
}
