//! Binary version regression test
//!
//! This test ensures that the LSP server binary being tested matches the
//! expected crate version. This catches the "stale binary" problem where
//! tests accidentally run against an old installed binary instead of the
//! freshly-built one.
//!
//! If this test fails, you're running against the wrong binary!
//! Common causes:
//! - Stale release binary in target/release/perllsp
//! - Old perllsp installed in PATH
//! - PERL_LSP_BIN pointing to wrong binary
//!
//! Fix: Run `cargo build -p perllsp --bin perllsp` before the implementation tests.

// Integration tests print diagnostic output for CI troubleshooting.
#![allow(clippy::print_stderr)]

mod common;

use serde_json::json;
use std::time::Duration;

/// The expected version from the crate being tested
const EXPECTED_VERSION: &str = env!("CARGO_PKG_VERSION");

fn send_initialize_with_timeout(
    server: &common::LspServer,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = json!(1);
    common::send_request_no_wait(
        server,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": params,
        }),
    );

    common::read_response_matching(
        server,
        &id,
        // 45s floor: these are the suite's coldest server starts — the
        // binary spawns while the rest of the suite contends for CPU, and
        // the previous 15s floor proved too tight on loaded runners (two
        // PR-Smoke reds on unrelated candidates the same day, both
        // "initialize request timed out"). Still bounded: a genuinely hung
        // server fails the test instead of hanging it forever.
        common::adaptive_timeout().max(Duration::from_secs(45)),
    )
    .ok_or_else(|| "initialize request timed out before the server returned serverInfo".to_owned())
}

#[test]
fn lsp_server_version_matches_crate_version() -> Result<(), String> {
    // Start the server using the same resolution logic as other tests
    let server = common::start_lsp_server();

    // Send initialize request
    let response = send_initialize_with_timeout(
        &server,
        json!({
            "capabilities": {},
            "clientInfo": {"name": "version-test", "version": "0"},
            "rootUri": null,
            "workspaceFolders": null
        }),
    )?;

    // Extract serverInfo.version from the response
    let server_version = response
        .get("result")
        .and_then(|r| r.get("serverInfo"))
        .and_then(|s| s.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");

    // Assert version matches
    assert_eq!(
        server_version,
        EXPECTED_VERSION,
        "\n\
        ╔════════════════════════════════════════════════════════════════════╗\n\
        ║ WRONG BINARY VERSION DETECTED!                                     ║\n\
        ╠════════════════════════════════════════════════════════════════════╣\n\
        ║ Expected: {expected:50} ║\n\
        ║ Got:      {got:50} ║\n\
        ╠════════════════════════════════════════════════════════════════════╣\n\
        ║ You are running tests against a stale or incorrect perl-lsp binary ║\n\
        ║                                                                    ║\n\
        ║ FIX: Run one of these commands:                                    ║\n\
        ║   cargo build -p perllsp --bin perllsp # Rebuild the product          ║\n\
        ║   cargo test -p perl-lsp-rs        # Rebuild and test                 ║\n\
        ║                                                                    ║\n\
        ║ If using PERL_LSP_BIN, verify it points to the correct binary.     ║\n\
        ╚════════════════════════════════════════════════════════════════════╝\n",
        expected = EXPECTED_VERSION,
        got = server_version,
    );

    // Clean shutdown
    common::shutdown_and_exit(&server);

    eprintln!("✓ Server version {} matches expected {}", server_version, EXPECTED_VERSION);

    Ok(())
}

#[test]
fn lsp_server_identifier_is_perl_lsp() -> Result<(), String> {
    // Start the server
    let server = common::start_lsp_server();

    // Send initialize request
    let response = send_initialize_with_timeout(
        &server,
        json!({
            "capabilities": {},
            "rootUri": null
        }),
    )?;

    // Extract serverInfo.name from the response
    let server_name = response
        .get("result")
        .and_then(|r| r.get("serverInfo"))
        .and_then(|s| s.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");

    assert_eq!(
        server_name, "perl-lsp",
        "Server identifier should be 'perl-lsp', got '{}'",
        server_name
    );

    // Clean shutdown
    common::shutdown_and_exit(&server);

    Ok(())
}
