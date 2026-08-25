//! Exact-process withdrawal proof for the legacy organize-imports edit
//! (issue #8305).
//!
//! The tests run Cargo's exact public `perllsp` integration-test binary over
//! stdio and prove that a real Perl document with executable statements between
//! import-looking lines can never receive the withdrawn line-oriented organizer
//! edit — not through an unfiltered request, not through a
//! `context.only: ["source.organizeImports"]` request, and not through
//! `codeAction/resolve` of a forged action. They also pin that the initialize
//! response no longer advertises `source.organizeImports`.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const URI: &str = "file:///organize-imports-withdrawal.pl";

/// Import-looking lines with executable statements in between. The withdrawn
/// organizer replaced the whole first-to-last import interval, which would
/// destroy the two middle statements.
const SOURCE: &str = "use warnings;\nmy $middle = 41;\nprint \"$middle\\n\";\nuse strict;\nuse Data::Dumper;\n\nprint Dumper({ answer => $middle });\n";

fn timeout() -> Duration {
    Duration::from_secs(15)
}

fn assert_success_response(response: &Value, id: &Value, method: &str) -> Result<Value> {
    ensure!(
        response.get("jsonrpc") == Some(&json!("2.0")),
        "{method} response omitted JSON-RPC 2.0: {response}"
    );
    ensure!(response.get("id") == Some(id), "{method} response ID mismatch: {response}");
    ensure!(response.get("error").is_none(), "{method} returned an error: {response}");
    response
        .get("result")
        .cloned()
        .with_context(|| format!("{method} response omitted result: {response}"))
}

fn request_success(
    client: &mut RealProcessClient,
    id: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = json!(id);
    let response = client.request(id.clone(), method, params, timeout())?;
    assert_success_response(&response, &id, method)
}

fn initialize(client: &mut RealProcessClient) -> Result<()> {
    let result = request_success(
        client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perl-lsp-organize-imports-withdrawal",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                },
                "textDocument": {
                    "codeAction": {
                        "disabledSupport": true,
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": ["quickfix", "refactor", "source", "source.organizeImports"]
                            }
                        }
                    }
                }
            },
            "workspaceFolders": null
        }),
    )?;
    let capabilities = result
        .get("capabilities")
        .and_then(Value::as_object)
        .with_context(|| format!("initialize result omitted capabilities: {result}"))?;

    let kinds = capabilities
        .get("codeActionProvider")
        .and_then(|provider| provider.get("codeActionKinds"))
        .and_then(Value::as_array)
        .with_context(|| format!("initialize result omitted codeActionProvider kinds: {result}"))?;
    ensure!(
        kinds.iter().all(|kind| kind.as_str() != Some("source.organizeImports")),
        "source.organizeImports must not be advertised while the legacy organizer is withdrawn (#8305): {kinds:?}"
    );

    client.notify("initialized", json!({}))
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let result = request_success(client, "shutdown", "shutdown", Value::Null)?;
    ensure!(result.is_null(), "shutdown must return null: {result}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "candidate exited unsuccessfully: {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

fn did_open(client: &mut RealProcessClient) -> Result<()> {
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": URI,
                "languageId": "perl",
                "version": 1,
                "text": SOURCE
            }
        }),
    )
}

fn code_actions(client: &mut RealProcessClient, id: &str, only: Option<&[&str]>) -> Result<Value> {
    let mut context = json!({ "diagnostics": [] });
    if let Some(kinds) = only {
        context["only"] = json!(kinds);
    }
    request_success(
        client,
        id,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": URI },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 5, "character": 0 }
            },
            "context": context
        }),
    )
}

fn reject_legacy_organizer(actions: &Value) -> Result<()> {
    let actions = actions
        .as_array()
        .with_context(|| format!("code actions response was not an array: {actions}"))?;
    for action in actions {
        let kind = action.get("kind").and_then(Value::as_str).unwrap_or("");
        ensure!(
            kind != "source.organizeImports",
            "withdrawn source.organizeImports action reached a live client: {action}"
        );
        let title = action.get("title").and_then(Value::as_str).unwrap_or("");
        ensure!(
            !title.eq_ignore_ascii_case("organize imports"),
            "an action reused the withdrawn organizer title: {action}"
        );
        if let Some(changes) =
            action.get("edit").and_then(|edit| edit.get("changes")).and_then(Value::as_object)
        {
            for edits in changes.values() {
                for edit in
                    edits.as_array().into_iter().flatten().filter_map(|edit| edit.get("range"))
                {
                    ensure!(
                        !edit_spans_executable_middle(edit),
                        "an edit spans the executable statements between import-looking lines: {action}"
                    );
                }
            }
        }
    }
    Ok(())
}

/// Zero-based lines of SOURCE carrying executable (non-import, non-comment,
/// non-POD) text: 1–2 are the statements between import-looking lines, 6 is the
/// trailing statement. A multi-line replacement covering any of them would
/// destroy unrelated source bytes — the defect class of the withdrawn sorter.
const EXECUTABLE_LINES: &[u64] = &[1, 2, 6];

/// No edit may span multiple lines while covering an executable line; the
/// legacy organizer replaced exactly such a first-to-last import interval.
/// Single-line edits stay allowed so unrelated quick fixes remain available.
fn edit_spans_executable_middle(range: &Value) -> bool {
    let start_line = range.get("start").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    let end_line = range.get("end").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    match (start_line, end_line) {
        (Some(start), Some(end)) if end > start => {
            EXECUTABLE_LINES.iter().any(|line| *line >= start && *line <= end)
        }
        _ => false,
    }
}

#[test]
fn filtered_organize_imports_request_returns_no_legacy_edit_over_stdio() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-filtered", Some(&["source.organizeImports"]))?;
    reject_legacy_organizer(&actions)?;

    finish(&mut client)
}

#[test]
fn unfiltered_request_and_resolve_cannot_reach_the_withdrawn_organizer() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-unfiltered", None)?;
    reject_legacy_organizer(&actions)?;

    // A forged resolve-shaped action carrying the withdrawn kind must come back
    // without any injected workspace edit.
    let resolved = request_success(
        &mut client,
        "resolve-forged",
        "codeAction/resolve",
        json!({
            "title": "Organize imports",
            "kind": "source.organizeImports",
            "data": { "uri": URI }
        }),
    )?;
    ensure!(
        resolved.get("edit").is_none(),
        "codeAction/resolve fabricated an edit for the withdrawn organizer: {resolved}"
    );

    finish(&mut client)
}
