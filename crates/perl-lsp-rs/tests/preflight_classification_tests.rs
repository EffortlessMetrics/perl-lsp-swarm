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
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn error_code(response: &Value) -> Option<i64> {
    response.get("error").and_then(|e| e.get("code")).and_then(Value::as_i64)
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
