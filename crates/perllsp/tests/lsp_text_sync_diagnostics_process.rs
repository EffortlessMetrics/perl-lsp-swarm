//! Full-sync rejection and recovery through the exact public JSON-RPC process.
//! Ranged edits remain unsupported; fresh processes independently parse the
//! literal final document as the oracle for diagnostics and UTF-16 references.

#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///text-sync-diagnostics.pl";
const OLD: &str = "print \"😀\"; sub before { return 1; }\r\nbefore();\r\n";
const FINAL: &str = "use strict;\r\n# kept warning control 😀\r\nprint \"🚀\"; sub after { return 2; }\r\nafter();\r\n";

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn request(client: &mut RealProcessClient, method: &str, params: Value) -> Result<Value> {
    let id = json!(method);
    let response = client.request(id.clone(), method, params, timeout())?;
    ensure!(response.get("jsonrpc") == Some(&json!("2.0")), "invalid response: {response}");
    ensure!(response.get("id") == Some(&id), "response ID mismatch: {response}");
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn start() -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact_with_env(&[
        ("PERL_LSP_DIAGNOSTIC_MODE", "normal"),
        ("RUST_LOG", "warn"),
    ])?;
    let root_uri = url::Url::from_file_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("fixture workspace cannot be expressed as a file URI"))?;
    let result = request(
        &mut client,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri.as_str(),
            "workspaceFolders": [{ "uri": root_uri.as_str(), "name": "fixture" }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "textDocument": { "diagnostic": {} }
            }
        }),
    )?;
    ensure!(
        result.pointer("/capabilities/textDocumentSync/change") == Some(&json!(1)),
        "fixture requires advertised Full synchronization: {result}"
    );
    ensure!(
        result.pointer("/capabilities/positionEncoding") == Some(&json!("utf-16")),
        "fixture requires UTF-16: {result}"
    );
    client.notify("initialized", json!({}))?;
    Ok(client)
}

fn finish(mut client: RealProcessClient) -> Result<()> {
    ensure!(request(&mut client, "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(timeout())?.success(), "server did not exit cleanly");
    client.assert_transport_clean()
}

fn open(client: &mut RealProcessClient, text: &str) -> Result<()> {
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": { "uri": URI, "languageId": "perl", "version": 1, "text": text }
        }),
    )?;
    wait_for_parse(client)
}

fn change(client: &mut RealProcessClient, version: i32, changes: Value) -> Result<()> {
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": version }, "contentChanges": changes
        }),
    )
}

fn wait_for_parse(client: &mut RealProcessClient) -> Result<()> {
    let deadline = Instant::now() + timeout();
    loop {
        let result = request(
            client,
            "textDocument/semanticTokens/full",
            json!({
                "textDocument": { "uri": URI }
            }),
        )?;
        if result.get("resultId").and_then(Value::as_str).is_some()
            && result.get("data").and_then(Value::as_array).is_some_and(|data| !data.is_empty())
        {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "current parse never became ready: {result}");
    }
}

fn diagnostics(client: &mut RealProcessClient, previous: Option<&str>) -> Result<Value> {
    request(
        client,
        "textDocument/diagnostic",
        json!({
            "textDocument": { "uri": URI }, "previousResultId": previous
        }),
    )
}

fn references(client: &mut RealProcessClient, line: u32, character: u32) -> Result<Value> {
    request(
        client,
        "textDocument/references",
        json!({
            "textDocument": { "uri": URI }, "position": { "line": line, "character": character },
            "context": { "includeDeclaration": true }
        }),
    )
}

fn sorted_array(value: &Value) -> Result<Vec<Value>> {
    let mut items = value.as_array().cloned().context("expected array")?;
    items.sort_by_key(Value::to_string);
    Ok(items)
}

fn assert_final_matches_fresh(client: &mut RealProcessClient) -> Result<()> {
    let mut fresh = start()?;
    open(&mut fresh, FINAL)?;
    let report = diagnostics(client, None)?;
    let oracle = diagnostics(&mut fresh, None)?;
    ensure!(report["kind"] == "full" && oracle["kind"] == "full");
    ensure!(
        oracle["items"].as_array().is_some_and(|items| !items.is_empty()),
        "fresh diagnostic retention control must be nonempty: {oracle}"
    );
    ensure!(sorted_array(&report["items"])? == sorted_array(&oracle["items"])?);
    let column = u32::try_from("print \"🚀\"; sub ".encode_utf16().count())?;
    let actual = sorted_array(&references(client, 2, column + 1)?)?;
    let expected = sorted_array(&references(&mut fresh, 2, column + 1)?)?;
    ensure!(!expected.is_empty(), "fresh reference oracle was vacuous");
    ensure!(actual == expected, "post-recovery references differ: {actual:?} != {expected:?}");
    ensure!(
        actual.iter().any(|location| location["range"]
            == json!({
                "start": { "line": 2, "character": column },
                "end": { "line": 2, "character": column + 5 }
            })),
        "reference declaration did not use exact UTF-16 range: {actual:?}"
    );
    let id = report["resultId"].as_str().context("recovered report omitted reusable ID")?;
    ensure!(diagnostics(client, Some(id))?["kind"] == "unchanged");
    finish(fresh)
}

#[test]
fn ranged_edits_cannot_resurrect_pull_diagnostics_or_cache_ids() -> Result<()> {
    let mut client = start()?;
    open(&mut client, OLD)?;
    let baseline = diagnostics(&mut client, None)?;
    ensure!(
        baseline["items"].as_array().is_some_and(|items| !items.is_empty()),
        "predecessor diagnostic control must be nonempty: {baseline}"
    );
    let old_id = baseline["resultId"].as_str().context("baseline omitted result ID")?;
    // Two sequential UTF-16 edits: replace the complete astral scalar, then
    // replace the CRLF boundary. Even aligned ranges are outside Full sync.
    change(
        &mut client,
        2,
        json!([
            { "range": { "start": { "line": 0, "character": 7 },
                         "end": { "line": 0, "character": 9 } }, "text": "🚀" },
            { "range": { "start": { "line": 0, "character": 36 },
                         "end": { "line": 1, "character": 0 } }, "text": "\n" }
        ]),
    )?;
    let unavailable = diagnostics(&mut client, Some(old_id))?;
    ensure!(
        unavailable == json!({ "kind": "full", "items": [] }),
        "rejected ranged edits resurrected predecessor diagnostics or a cache ID: {unavailable}"
    );
    ensure!(
        references(&mut client, 0, 16)? == json!([]),
        "desynchronized references remained current"
    );
    // A valid first replacement followed by a ranged member commits nothing;
    // later rejected edits must keep the document unavailable.
    change(
        &mut client,
        3,
        json!([
            { "text": FINAL },
            { "range": { "start": { "line": 3, "character": 0 },
                         "end": { "line": 3, "character": 5 } }, "text": "before" }
        ]),
    )?;
    ensure!(diagnostics(&mut client, None)? == json!({ "kind": "full", "items": [] }));
    change(&mut client, 4, json!([{ "text": FINAL }]))?;
    wait_for_parse(&mut client)?;
    assert_final_matches_fresh(&mut client)?;
    finish(client)
}

#[test]
fn ordered_full_replacements_and_stale_ranges_match_fresh_unicode_parse() -> Result<()> {
    let mut client = start()?;
    open(&mut client, OLD)?;
    change(&mut client, 5, json!([{ "text": "sub intermediate { }\n" }, { "text": FINAL }]))?;
    wait_for_parse(&mut client)?;
    assert_final_matches_fresh(&mut client)?;
    // A proven stale ranged packet must not invalidate newer accepted text.
    change(
        &mut client,
        4,
        json!([{
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 0, "character": 1 } }, "text": "x"
        }]),
    )?;
    assert_final_matches_fresh(&mut client)?;
    finish(client)
}
