//! Exact-process withdrawal proof for the PL700 prose-driven whole-line
//! removal edit (issue #11079).
//!
//! The tests run Cargo's exact public `perllsp` integration-test binary over
//! stdio and prove that a real Perl document can never receive the withdrawn
//! import-removal edit — not through an unfiltered request, not through a
//! `context.only: ["quickfix"]` request, not through a request whose context
//! carries a producer-shaped PL700 diagnostic, and not through
//! `codeAction/resolve` of a forged action — while unrelated proven quick
//! fixes (the missing-pragma helper) stay reachable.
//!
//! Clients with and without `codeAction.disabledSupport` are both driven: the
//! withdrawn action is omitted either way; no disabled stub carrying an edit
//! is emitted.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const URI: &str = "file:///pl700-withdrawal.pl";

/// Line 1 loads a module that stays unused; line 2 is executable and must be
/// untouched. The document deliberately lacks `use strict` so the missing-
/// pragma quick fix remains available as an unrelated-family control.
const SOURCE: &str = "use warnings;\nuse Local::Thing;\nprint \"hello\\n\";\n";

/// Zero-based index of the import line no edit may touch.
const IMPORT_LINE: u64 = 1;
/// Zero-based index of the executable line no edit may destroy.
const EXECUTABLE_LINE: u64 = 2;

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

fn initialize(client: &mut RealProcessClient, disabled_support: bool) -> Result<()> {
    let mut code_action = json!({
        "codeActionLiteralSupport": {
            "codeActionKind": {
                "valueSet": ["quickfix", "refactor", "source"]
            }
        }
    });
    if disabled_support {
        code_action["disabledSupport"] = json!(true);
    }

    let result = request_success(
        client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perl-lsp-pl700-withdrawal",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                },
                "textDocument": {
                    "codeAction": code_action
                }
            },
            "workspaceFolders": null
        }),
    )?;

    let capabilities = result
        .get("capabilities")
        .and_then(Value::as_object)
        .with_context(|| format!("initialize result omitted capabilities: {result}"))?;
    ensure!(
        capabilities.get("codeActionProvider").is_some(),
        "server must keep advertising code actions for surviving families: {capabilities:?}"
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

/// A producer-shaped PL700 diagnostic exactly as the native unused-import lint
/// presents it (range spans the directive, prose names the module).
fn producer_shaped_pl700_context() -> Value {
    json!({
        "diagnostics": [{
            "range": {
                "start": { "line": IMPORT_LINE, "character": 0 },
                "end": { "line": IMPORT_LINE, "character": 16 }
            },
            "severity": 4,
            "code": "PL700",
            "source": "perl-lsp",
            "message": "Module 'Local::Thing' appears to be unused"
        }]
    })
}

fn code_actions(
    client: &mut RealProcessClient,
    id: &str,
    only: Option<&[&str]>,
    with_pl700_context: bool,
) -> Result<Value> {
    let mut context = if with_pl700_context {
        producer_shaped_pl700_context()
    } else {
        json!({ "diagnostics": [] })
    };
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
                "end": { "line": 3, "character": 0 }
            },
            "context": context
        }),
    )
}

fn edit_covers_line(range: &Value, line: u64) -> bool {
    let start_line = range.get("start").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    let end_line = range.get("end").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    match (start_line, end_line) {
        (Some(start), Some(end)) => start <= line && line <= end,
        _ => false,
    }
}

fn edit_spans_multiple_lines(range: &Value) -> bool {
    let start_line = range.get("start").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    let end_line = range.get("end").and_then(|pos| pos.get("line")).and_then(Value::as_u64);
    match (start_line, end_line) {
        (Some(start), Some(end)) => end > start,
        _ => false,
    }
}

fn reject_withdrawn_import_edit(actions: &Value) -> Result<()> {
    let actions = actions
        .as_array()
        .with_context(|| format!("code actions response was not an array: {actions}"))?;
    for action in actions {
        let title = action.get("title").and_then(Value::as_str).unwrap_or("");
        ensure!(
            !title.contains("Remove unused"),
            "an action reused the withdrawn PL700 removal presentation: {action}"
        );
        let kind = action.get("kind").and_then(Value::as_str).unwrap_or("");
        // The withdrawn PL700 family is diagnostic-keyed quick fixes. The
        // separate organize-imports family (#8305) has its own containment
        // lane and is neither asserted present nor absent here.
        if kind != "quickfix" {
            continue;
        }
        if let Some(changes) =
            action.get("edit").and_then(|edit| edit.get("changes")).and_then(Value::as_object)
        {
            for edits in changes.values() {
                for edit in
                    edits.as_array().into_iter().flatten().filter_map(|edit| edit.get("range"))
                {
                    // Nothing legitimate targets the diagnosed import line in
                    // this document while PL700 removal is withdrawn (#11079).
                    ensure!(
                        !edit_covers_line(edit, IMPORT_LINE),
                        "a quick-fix edit touches the diagnosed import line while PL700 \
                         removal is withdrawn (#11079): {action}"
                    );
                    // A multi-line span swallowing the executable statement
                    // would be interval-replacement damage.
                    ensure!(
                        !(edit_spans_multiple_lines(edit)
                            && edit_covers_line(edit, EXECUTABLE_LINE)),
                        "a multi-line quick fix spans the executable statement after the \
                         import: {action}"
                    );
                }
            }
        }
    }
    Ok(())
}

fn assert_pragma_control_survives(actions: &Value) -> Result<()> {
    let actions = actions.as_array().with_context(|| "not an array")?;
    ensure!(
        actions.iter().any(|action| {
            action.get("kind").and_then(Value::as_str) == Some("quickfix")
                && action
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.contains("use strict"))
        }),
        "unrelated missing-pragma quick fix must remain available: {actions:?}"
    );
    Ok(())
}

#[test]
fn unfiltered_request_with_producer_diagnostic_returns_no_import_edit() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-unfiltered", None, true)?;
    reject_withdrawn_import_edit(&actions)?;
    assert_pragma_control_survives(&actions)?;

    finish(&mut client)
}

#[test]
fn filtered_quickfix_request_fails_closed_over_stdio() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-filtered", Some(&["quickfix"]), true)?;
    reject_withdrawn_import_edit(&actions)?;

    finish(&mut client)
}

#[test]
fn minimal_client_without_disabled_support_receives_truthful_omission() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, false)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-minimal", None, true)?;
    reject_withdrawn_import_edit(&actions)?;

    finish(&mut client)
}

#[test]
fn forged_resolve_and_command_shapes_cannot_inject_the_withdrawn_edit() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    // A resolve-shaped forgery presenting itself as a PL700 removal must come
    // back without any injected workspace edit.
    let resolved = request_success(
        &mut client,
        "resolve-forged",
        "codeAction/resolve",
        json!({
            "title": "Remove unused 'use Local::Thing;'",
            "kind": "quickfix",
            "data": { "uri": URI }
        }),
    )?;
    ensure!(
        resolved.get("edit").is_none(),
        "codeAction/resolve fabricated an edit for the withdrawn PL700 removal: {resolved}"
    );

    // No executeCommand route exists for this family: probing a fabricated
    // import-removal command must be rejected as unknown, never succeed.
    let id = json!("commands-probe");
    let probe = client.request(
        id.clone(),
        "workspace/executeCommand",
        json!({ "command": "perl.removeUnusedImport", "arguments": [{ "uri": URI }] }),
        timeout(),
    )?;
    ensure!(
        probe.get("id") == Some(&id) && probe.get("error").is_some(),
        "a fabricated import-removal command must fail as unknown, got: {probe}"
    );

    finish(&mut client)
}
