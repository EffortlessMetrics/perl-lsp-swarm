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

/// Failure message for a handshake whose time budget elapsed.
const HANDSHAKE_TIMEOUT_MESSAGE: &str =
    "initialize request timed out before the server returned serverInfo";

/// Per-attempt verdict of the handshake retry contract: exactly one
/// fresh-server retry, granted only to an elapsed timeout.
enum RetryDecision {
    /// Start one fresh server and try again.
    RetryOnce,
    /// Stop retrying and fail the test with this message.
    GiveUp(String),
}

/// Exit status of the server process, for crash diagnostics.
fn disconnected_status(server: &common::LspServer) -> String {
    server
        .process
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .try_wait()
        .ok()
        .flatten()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "process alive but stdout closed".to_owned())
}

/// Apply the retry contract to one handshake attempt: only a first-attempt
/// timeout earns the fresh-server retry; disconnects, malformed responses,
/// successes, and exhausted attempts all give up.
fn classify_attempt(
    attempt: u8,
    outcome: &common::ReadResponseOutcome,
    disconnect_status: &str,
) -> RetryDecision {
    match outcome {
        common::ReadResponseOutcome::TimedOut if attempt < 2 => RetryDecision::RetryOnce,
        common::ReadResponseOutcome::TimedOut => {
            RetryDecision::GiveUp(HANDSHAKE_TIMEOUT_MESSAGE.to_owned())
        }
        common::ReadResponseOutcome::Disconnected => RetryDecision::GiveUp(format!(
            "perl-lsp process terminated during initialization ({disconnect_status}); \
             not retrying a crashed server"
        )),
        common::ReadResponseOutcome::Malformed(detail) => RetryDecision::GiveUp(format!(
            "perl-lsp sent an unparsable initialize response; \
             not retrying a protocol failure ({detail})"
        )),
        common::ReadResponseOutcome::Response(_) => {
            RetryDecision::GiveUp("initialize succeeded; success is not retried".to_owned())
        }
    }
}

/// Initialize a freshly started server, retrying ONCE with a new server
/// when the handshake stalls. The assertion stays strict on every attempt:
/// a server that answers with wrong serverInfo fails all attempts — only
/// the loaded-runner stall (#11848: initialize can exceed a 45s deadline
/// with the handler itself trivial, transport-level) is retried, mirroring
/// the system-inc probe retry in module_resolution.
fn initialize_with_retry(
    params: serde_json::Value,
) -> Result<(common::LspServer, serde_json::Value), String> {
    for attempt in 1..=2u8 {
        let server = common::start_lsp_server();
        match send_initialize_with_timeout(&server, params.clone()) {
            common::ReadResponseOutcome::Response(response) => return Ok((server, response)),
            outcome => match classify_attempt(attempt, &outcome, &disconnected_status(&server)) {
                RetryDecision::RetryOnce => eprintln!(
                    "initialize attempt {attempt}/2 timed out; retrying with a fresh server"
                ),
                RetryDecision::GiveUp(mut err) => {
                    append_stderr_tail(&mut err, &server);
                    return Err(err);
                }
            },
        }
    }
    // Only two consecutive timeouts fall out of the loop.
    Err(HANDSHAKE_TIMEOUT_MESSAGE.to_owned())
}

/// Attach the server's captured stderr tail to a failure message (#11848):
/// the stall family resisted floor raises and retries, and the discarded
/// stderr was the one channel showing what the "server" was doing (an
/// inline cargo compile, as it turned out). A silent server is itself a
/// discriminating fact, so both cases are stated.
fn append_stderr_tail(err: &mut String, server: &common::LspServer) {
    let tail = server.stderr_tail();
    if tail.is_empty() {
        err.push_str("\n(server produced no stderr)");
    } else {
        err.push('\n');
        err.push_str(&tail);
    }
}

fn send_initialize_with_timeout(
    server: &common::LspServer,
    params: serde_json::Value,
) -> common::ReadResponseOutcome {
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

    common::read_response_matching_outcome(
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
}

#[test]
fn lsp_server_version_matches_crate_version() -> Result<(), String> {
    // Start the server and initialize (one fresh-server retry on stall)
    let (server, response) = initialize_with_retry(json!({
        "capabilities": {},
        "clientInfo": {"name": "version-test", "version": "0"},
        "rootUri": null,
        "workspaceFolders": null
    }))?;

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
    // Start the server and initialize (one fresh-server retry on stall)
    let (server, response) = initialize_with_retry(json!({
        "capabilities": {},
        "rootUri": null
    }))?;

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

#[test]
fn retry_contract_retries_first_timeout_once() {
    assert!(matches!(
        classify_attempt(1, &common::ReadResponseOutcome::TimedOut, ""),
        RetryDecision::RetryOnce
    ));
}

#[test]
fn retry_contract_gives_up_on_second_timeout() {
    let decision = classify_attempt(2, &common::ReadResponseOutcome::TimedOut, "");
    assert!(
        matches!(decision, RetryDecision::GiveUp(ref m) if m == HANDSHAKE_TIMEOUT_MESSAGE),
        "a second timeout must exhaust the single retry"
    );
}

#[test]
fn retry_contract_never_retries_success() {
    let response = common::ReadResponseOutcome::Response(json!({"result": {"serverInfo": {}}}));
    assert!(
        matches!(classify_attempt(1, &response, ""), RetryDecision::GiveUp(_)),
        "a successful handshake must not be retried"
    );
}

#[test]
fn retry_contract_fails_disconnected_without_retry() {
    let decision =
        classify_attempt(1, &common::ReadResponseOutcome::Disconnected, "exit code: 101");
    assert!(
        matches!(
            decision,
            RetryDecision::GiveUp(ref message)
                if message.contains("terminated during initialization")
                    && message.contains("exit code: 101")
                    && message.contains("not retrying a crashed server")
        ),
        "a crashed server must fail without a fresh-server retry"
    );
}

#[test]
fn retry_contract_fails_malformed_without_retry() {
    let detail = "invalid JSON in 12-byte frame: expected value".to_owned();
    let decision = classify_attempt(1, &common::ReadResponseOutcome::Malformed(detail), "");
    assert!(
        matches!(
            decision,
            RetryDecision::GiveUp(ref message)
                if message.contains("unparsable initialize response")
                    && message.contains("invalid JSON in 12-byte frame")
                    && message.contains("not retrying a protocol failure")
        ),
        "a malformed response must fail without a fresh-server retry"
    );
}
