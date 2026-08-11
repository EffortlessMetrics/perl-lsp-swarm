//! Executable coverage for unhappy LSP lifecycle sequences.
//!
//! These tests keep the protocol contract at the process boundary.  The
//! in-process lifecycle tests prove individual handlers; this suite proves
//! preflight, routing, framing, and response serialization together.

use serde_json::{Value, json};

mod common;
use common::{initialize_lsp, send_notification, send_request, start_lsp_server};

fn error_code(response: &Value, context: &str) -> Result<i64, String> {
    response
        .get("error")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{context}: expected an error response, got {response:?}"))?
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{context}: error response has no numeric code: {response:?}"))
}

fn assert_error_code(response: &Value, expected: i64, context: &str) {
    let actual = error_code(response, context);
    assert_eq!(actual, Ok(expected), "{context}: unexpected error response {response:?}");
}

#[test]
fn hover_before_initialize_returns_server_not_initialized() {
    let server = start_lsp_server();

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/hover",
            "params": {
                "textDocument": {"uri": "file:///never-opened.pl"},
                "position": {"line": 0, "character": 0}
            }
        }),
    );

    assert_error_code(&response, -32002, "textDocument/hover before initialize");
}

#[test]
fn second_initialize_returns_invalid_request() {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": null,
                "capabilities": {}
            }
        }),
    );

    assert_error_code(&response, -32600, "second initialize");
}

#[test]
fn ranged_change_before_open_is_ignored_without_killing_the_server() {
    let server = start_lsp_server();
    initialize_lsp(&server);

    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": "file:///never-opened.pl",
                    "version": 2
                },
                "contentChanges": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 0}
                    },
                    "rangeLength": 0,
                    "text": "my $x = 1;"
                }]
            }
        }),
    );

    // A notification has no response.  A following request proves the
    // ignored change did not terminate or desynchronize the server.
    let response = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "shutdown",
            "params": null
        }),
    );
    assert!(
        response.get("error").is_none(),
        "shutdown after an unopened ranged change should succeed: {response:?}"
    );
    assert_eq!(response.get("result"), Some(&Value::Null));
}

#[test]
fn requests_after_shutdown_return_invalid_request() {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let shutdown = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        }),
    );
    assert!(shutdown.get("error").is_none(), "initial shutdown should succeed: {shutdown:?}");
    assert_eq!(shutdown.get("result"), Some(&Value::Null));

    let after_shutdown = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/hover",
            "params": {
                "textDocument": {"uri": "file:///after-shutdown.pl"},
                "position": {"line": 0, "character": 0}
            }
        }),
    );
    assert_error_code(&after_shutdown, -32600, "request after shutdown");

    let second_shutdown = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "shutdown",
            "params": null
        }),
    );
    assert_error_code(&second_shutdown, -32600, "second shutdown");
}
