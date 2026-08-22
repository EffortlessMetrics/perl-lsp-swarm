//! Exact-process withdrawal proof for the hard-coded missing-import edits
//! (issue #10690).
//!
//! The tests run Cargo's exact public `perllsp` integration-test binary over
//! stdio and prove that a real Perl document can never receive an import edit
//! derived from the withdrawn hard-coded function→module table — not through an
//! unfiltered request, not through a `context.only: ["quickfix"]` request, not
//! on a minimal client without `codeAction.disabledSupport`, not through
//! `codeAction/resolve` of a forged action, and not via any fabricated command.
//!
//! The surviving PL109 quoting family stays reachable as the unrelated-family
//! control: the same producer-shaped PL109 diagnostic that must not yield an
//! import still yields its quote quick fixes.
//!
//! Provider-unit containment lives in
//! `crates/perl-lsp-rs-core/tests/missing_import_withdrawal_containment_tests.rs`
//! and the rewritten BDD suite.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const URI: &str = "file:///missing-import-withdrawal.pl";

/// Line 2 calls `basename`, a spelling of the withdrawn hard-coded table
/// (`File::Basename`). The document deliberately keeps `use strict` so the
/// PL109 bareword presentation is realistic.
const SOURCE: &str = concat!(
    "use strict;\n",
    "my $path = '/tmp/data.txt';\n",
    "my $base = basename($path);\n",
    "print \"$base\\n\";\n",
);

/// Zero-based index of the call line carrying the PL109 presentation.
const CALL_LINE: u64 = 2;
/// Byte offset of `basename` within that line ("my $base = " prefix).
const CALL_START_CHAR: u64 = 11;
const CALL_END_CHAR: u64 = CALL_START_CHAR + "basename".len() as u64;

/// Modules of the withdrawn hard-coded table; none may be inserted anywhere.
const WITHDRAWN_TABLE_MODULES: &[&str] =
    &["Data::Dumper", "Encode", "File::Basename", "File::Path", "File::Slurp", "JSON"];

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
                "name": "perl-lsp-missing-import-withdrawal",
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

/// A producer-shaped PL109 diagnostic exactly as the native bareword lint
/// presents it (range spans the bareword under `use strict`).
fn producer_shaped_pl109_context() -> Value {
    json!({
        "diagnostics": [{
            "range": {
                "start": { "line": CALL_LINE, "character": CALL_START_CHAR },
                "end": { "line": CALL_LINE, "character": CALL_END_CHAR }
            },
            "severity": 1,
            "code": "PL109",
            "source": "perl-lsp",
            "message": "Bareword 'basename' is not allowed under 'use strict' -- quote it as 'basename' or use it as a subroutine call"
        }]
    })
}

fn code_actions(client: &mut RealProcessClient, id: &str, only: Option<&[&str]>) -> Result<Value> {
    let mut context = producer_shaped_pl109_context();
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

fn reject_withdrawn_import_edits(actions: &Value) -> Result<()> {
    let actions = actions
        .as_array()
        .with_context(|| format!("code actions response was not an array: {actions}"))?;
    for action in actions {
        let title = action.get("title").and_then(Value::as_str).unwrap_or("");
        ensure!(
            !title.starts_with("Import '") && !title.contains("Add missing imports"),
            "an action reused the withdrawn import presentation (#10690): {action}"
        );

        // No edit may insert a table-module directive anywhere in the document.
        if let Some(changes) =
            action.get("edit").and_then(|edit| edit.get("changes")).and_then(Value::as_object)
        {
            for edits in changes.values() {
                for edit in
                    edits.as_array().into_iter().flatten().filter_map(|edit| edit.get("newText"))
                {
                    let text = edit.as_str().unwrap_or("");
                    for module in WITHDRAWN_TABLE_MODULES {
                        let directive = format!("use {module};");
                        ensure!(
                            !text.contains(&directive),
                            "an edit inserts the withdrawn directive '{directive}' \
                             (#10690): {action}"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn assert_quoting_control_survives(actions: &Value) -> Result<()> {
    let actions = actions.as_array().with_context(|| "not an array")?;
    ensure!(
        actions.iter().any(|action| {
            action.get("kind").and_then(Value::as_str) == Some("quickfix")
                && action
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.contains("with single quotes"))
        }),
        "surviving PL109 quoting fixes must remain available: {actions:?}"
    );
    Ok(())
}

#[test]
fn unfiltered_request_with_producer_pl109_returns_no_import_edit() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-unfiltered", None)?;
    reject_withdrawn_import_edits(&actions)?;
    assert_quoting_control_survives(&actions)?;

    finish(&mut client)
}

#[test]
fn filtered_quickfix_request_fails_closed_over_stdio() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-filtered", Some(&["quickfix"]))?;
    reject_withdrawn_import_edits(&actions)?;

    finish(&mut client)
}

#[test]
fn minimal_client_without_disabled_support_receives_truthful_omission() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, false)?;
    did_open(&mut client)?;

    let actions = code_actions(&mut client, "ca-minimal", None)?;
    reject_withdrawn_import_edits(&actions)?;

    finish(&mut client)
}

#[test]
fn forged_resolve_and_fabricated_command_cannot_inject_an_import_edit() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client, true)?;
    did_open(&mut client)?;

    // A resolve-shaped forgery presenting itself as the withdrawn import fix
    // must come back without any injected workspace edit.
    let resolved = request_success(
        &mut client,
        "resolve-forged",
        "codeAction/resolve",
        json!({
            "title": "Import 'File::Basename'",
            "kind": "quickfix",
            "data": { "uri": URI }
        }),
    )?;
    ensure!(
        resolved.get("edit").is_none(),
        "codeAction/resolve fabricated an edit for the withdrawn import fix: {resolved}"
    );

    // No executeCommand route exists for this family: probing a fabricated
    // import-insertion command must be rejected as unknown, never succeed.
    let id = json!("commands-probe");
    let probe = client.request(
        id.clone(),
        "workspace/executeCommand",
        json!({ "command": "perl.addMissingImports", "arguments": [{ "uri": URI }] }),
        timeout(),
    )?;
    ensure!(
        probe.get("id") == Some(&id) && probe.get("error").is_some(),
        "a fabricated import-insertion command must fail as unknown, got: {probe}"
    );

    finish(&mut client)
}
