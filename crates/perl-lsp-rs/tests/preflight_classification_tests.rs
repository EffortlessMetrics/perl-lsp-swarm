//! End-to-end preflight layering regressions for issue #8895.
//!
//! These run the real `perllsp` binary through the strict process harness and
//! pin the JSON-RPC error classification at the changed boundary:
//!
//! ```text
//! malformed/over-bound generic request      -> -32600 InvalidRequest
//! valid unknown/unimplemented method        -> -32601 MethodNotFound
//! known method with wrong parameter shape   -> -32602 InvalidParams
//! application/command/path policy refusal   -> method-owned typed failure
//! ```
//!
//! Content that merely *looks* browser-dangerous (`<script>`, `javascript:`)
//! is inert data in params: it must never trigger a generic rejection, while
//! sink-owned policies (command identity, URI resolution) keep refusing what
//! they own.

mod support;

use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn error_code(response: &Value) -> Option<i64> {
    response.get("error").and_then(|e| e.get("code")).and_then(Value::as_i64)
}

fn wait_for_message_containing(
    harness: &mut LspHarness,
    method: &str,
    fragment: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("{method} containing {fragment:?} was not received").into());
        }
        let params = harness.wait_for_notification(method, remaining)?;
        if params["message"].as_str().is_some_and(|message| message.contains(fragment)) {
            return Ok(params);
        }
    }
}

/// A refused first didOpen has no stored document or response envelope. The
/// client must still receive a bounded refusal on the wire (#16653).
#[test]
fn refused_first_did_open_notifies_the_client() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;
    let uri = "file:///refused-first-open.pl";
    let ceiling = perl_lsp_rs_core::runtime::input_validation::text_sync_params_ceiling();
    harness.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "x".repeat(ceiling + 1),
            }
        }),
    );
    let notice = harness.wait_for_notification("window/showMessage", Duration::from_secs(5))?;
    let message = notice["message"].as_str().ok_or("missing refusal message")?;
    assert_eq!(notice["type"], 2, "refusal must be a visible warning: {notice}");
    assert!(message.contains(uri), "refusal must identify the unopened document: {notice}");
    assert!(message.contains("was not opened"), "refusal must name the lost didOpen: {notice}");
    assert!(message.len() < 1_024, "refusal UI message must be bounded: {} bytes", message.len());

    // The client may keep editing after the first open failed. Each refused
    // change remains visible in its log without raising another popup.
    harness.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": "x".repeat(ceiling + 1)}],
        }),
    );
    let logged = wait_for_message_containing(
        &mut harness,
        "window/logMessage",
        "textDocument/didChange for file:///refused-first-open.pl was rejected at",
    )?;
    assert!(
        logged["message"].as_str().is_some_and(|message| message.contains(uri)),
        "unassociated change refusal must reach the client log: {logged}"
    );
    // A following request is a dispatcher barrier: the refused notification
    // and all of its outbound messages completed before this response.
    let barrier = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 901,
        "method": "custom/barrierAfterRefusedChange",
        "params": {},
    }));
    assert_eq!(error_code(&barrier), Some(-32601));
    // The harness also reparses prior raw output while waiting for a request
    // response, so an old didOpen warning can appear in this queue again.
    // Match the refused change itself, not an unrelated or replayed warning.
    let change_popups: Vec<_> = harness
        .drain_notifications(Some("window/showMessage"), 0)
        .into_iter()
        .filter(|notification| {
            notification["params"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("textDocument/didChange"))
        })
        .collect();
    assert!(
        change_popups.is_empty(),
        "a refused change after an unopened document must not raise another popup: {change_popups:?}"
    );

    // Missing URI still gets a generic refusal; there is no response to a
    // notification, and no document identity can safely be inferred.
    harness
        .notify("textDocument/didOpen", json!({"textDocument": {"text": "x".repeat(ceiling + 1)}}));
    let unknown = harness.wait_for_notification("window/showMessage", Duration::from_secs(5))?;
    assert!(
        unknown["message"].as_str().is_some_and(|message| message.contains("could be identified")),
        "missing-URI refusal must still be visible: {unknown}"
    );

    harness.notify(
        "textDocument/didChange",
        json!({"contentChanges": [{"text": "x".repeat(ceiling + 1)}]}),
    );
    let unknown_change =
        wait_for_message_containing(&mut harness, "window/logMessage", "could be identified")?;
    assert!(
        unknown_change["message"]
            .as_str()
            .is_some_and(|message| message.contains("could be identified")),
        "missing-URI change refusal must reach the client log: {unknown_change}"
    );

    // The URI itself can be the oversize input. It must identify the file
    // without echoing megabytes or control characters into the client UI.
    let long_uri = format!("file:///\n{}", "x".repeat(ceiling + 1));
    harness.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": long_uri, "languageId": "perl", "version": 1, "text": "x"}}),
    );
    let bounded = harness.wait_for_notification("window/showMessage", Duration::from_secs(5))?;
    let bounded_message = bounded["message"].as_str().ok_or("missing bounded refusal message")?;
    assert!(bounded_message.len() < 1_024, "URI refusal echoed too much data");
    assert!(!bounded_message.contains('\n'), "URI refusal contained a control character");
    assert!(bounded_message.contains("file:///?"), "URI prefix was not preserved: {bounded}");
    Ok(())
}

/// POD/documentation text containing `javascript:` is inert data. A
/// `completionItem/resolve` carrying such documentation must not be rejected
/// by any generic content scan (#8895).
#[test]
fn pod_documentation_with_javascript_uri_passes_through() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "completionItem/resolve",
        "params": {
            "label": "some_sub",
            "kind": 3,
            "documentation": {
                "kind": "markdown",
                "value": "See also javascript: links are inert here; quotes <script> from POD."
            }
        }
    }));

    assert!(
        response.get("error").is_none(),
        "resolve with POD `javascript:` documentation must not be an error, got: {response:?}"
    );
    Ok(())
}

/// A diagnostics echo path: `textDocument/codeAction` whose context quotes
/// source text containing `<script>` must pass preflight untouched (#8895).
#[test]
fn code_action_context_quoting_script_tag_passes_through() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": "file:///scripty.pl"},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
            "context": {
                "diagnostics": [{
                    "message": "unexpected token near print '<script>alert(1)</script>';",
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
                }]
            }
        }
    }));

    let code = error_code(&response);
    assert!(
        code.is_none(),
        "codeAction quoting `<script>` source text must not be rejected, got code {code:?}: {response:?}"
    );
    Ok(())
}

/// One initialized server proves the whole classification matrix:
///
/// - oversized params -> -32600 (generic resource bound, explicit);
/// - punctuated custom method / unknown command -> -32601 (routing- and
///   command-identity owned MethodNotFound, not the old charset/policy scan);
/// - malformed known-method params / unresolvable document scheme -> -32602
///   (typed decode and sync-sink policy, not generic InvalidRequest).
#[test]
fn error_classification_matrix_matches_the_layer_boundaries() -> TestResult {
    let mut harness = LspHarness::new();
    harness.initialize(None)?;

    // Generic resource bound stays explicit and deterministic: a payload past
    // the flat serialized-params ceiling is refused before routing with -32600.
    let oversized = "a".repeat(1_000_001);
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "custom/blobSink",
        "params": {"blob": oversized}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32600),
        "oversized params must hit the explicit resource bound with -32600: {}",
        response.get("error").map(|e| e.to_string()).unwrap_or_default()
    );

    // `$/cancelRequest` is still subject to generic resource admission before
    // its special notification handling. The request-shaped envelope makes the
    // -32600 response observable while preserving normal notification behavior.
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 16,
        "method": "$/cancelRequest",
        "params": {"id": 999, "padding": "a".repeat(1_000_001)}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32600),
        "oversized cancelRequest params must be rejected before special dispatch: {response:?}"
    );

    // A syntactically valid custom extension method with punctuation outside
    // the old allowlist reaches routing and is answered by -32601 — proving
    // admission no longer polices method charset (negative control 2).
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "custom/fmt.v2:preview",
        "params": {}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32601),
        "valid unknown punctuated method must return -32601, got: {response:?}"
    );

    // An unknown valid method returns MethodNotFound, not InvalidRequest.
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "custom/definitelyNotAMethod",
        "params": {}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32601),
        "unknown valid method must return -32601, got: {response:?}"
    );

    // Disallowed execute-command identity is refused BY THE COMMAND POLICY at
    // its sink as a method-owned typed failure (-32601 here), not recycled
    // into a generic -32600 protocol rejection (negative control 3 keeps this
    // check alive after the generic scan was removed).
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "workspace/executeCommand",
        "params": {"command": "perl.notARealCommand", "arguments": []}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32601),
        "unknown command must be refused by command policy with -32601, got: {response:?}"
    );

    // Known method, wrong parameter shape -> InvalidParams (-32602) from the
    // owning handler's typed decode, not generic InvalidRequest.
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 14,
        "method": "workspace/executeCommand",
        "params": {"command": "perl.agentContext", "arguments": {"not": "an array"}}
    }));
    assert_eq!(
        error_code(&response),
        Some(-32602),
        "non-array executeCommand arguments must return -32602, got: {response:?}"
    );

    // Path/scheme policy moved to the sync sink: a didOpen *request* carrying
    // a URI the server cannot resolve into workspace paths is answered with
    // the method-owned InvalidParams (-32602), not -32600.
    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "id": 15,
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "ftp://example.com/intruder.pl",
                "languageId": "perl",
                "version": 1,
                "text": "print 1;\n"
            }
        }
    }));
    assert_eq!(
        error_code(&response),
        Some(-32602),
        "didOpen with unresolvable URI scheme must return sink-owned -32602, got: {response:?}"
    );

    Ok(())
}
