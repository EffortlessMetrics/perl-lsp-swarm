//! Named-method selection ranges for the bounded #8183 migration.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///method-name-ranges.pm";
const SOURCE: &str = "class C {\n    method ethod { 'ethod' }\n    method greet { 1 }\n    method ADJUST { 2 }\n    ADJUST { 3 }\n}\n";

fn range(line: u32, start: u32, end_line: u32, end: u32) -> Value {
    json!({"start": {"line": line, "character": start},
           "end": {"line": end_line, "character": end}})
}

/// Literal coordinates counted independently of parser and source-search code.
fn expected() -> Value {
    json!([{
        "name": "C", "kind": 2, "detail": "",
        "range": range(0, 0, 5, 1), "selectionRange": range(0, 6, 0, 7),
        "children": [
            {"name": "ethod", "kind": 6, "detail": "", "children": [],
             "range": range(1, 4, 1, 28), "selectionRange": range(1, 11, 1, 16)},
            {"name": "greet", "kind": 6, "detail": "", "children": [],
             "range": range(2, 4, 2, 22), "selectionRange": range(2, 11, 2, 16)},
            {"name": "ADJUST", "kind": 6, "detail": "", "children": [],
             "range": range(3, 4, 3, 23), "selectionRange": range(3, 11, 3, 17)},
            {"name": "ADJUST", "kind": 6, "detail": "", "children": [],
             "range": range(4, 4, 4, 16), "selectionRange": range(4, 4, 4, 10)}
        ]
    }])
}

fn without_selection(mut value: Value) -> Value {
    if let Some(symbols) = value.as_array_mut() {
        for symbol in symbols {
            if let Some(object) = symbol.as_object_mut() {
                object.remove("selectionRange");
                if let Some(children) = object.get_mut("children") {
                    *children = without_selection(children.take());
                }
            }
        }
    }
    value
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, TIMEOUT)?;
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn outline() -> Result<Value> {
    let mut client = RealProcessClient::spawn_exact()?;
    let initialized = request(
        &mut client,
        "init",
        "initialize",
        json!({
            "processId": null, "rootUri": null, "workspaceFolders": null,
            "capabilities": {"textDocument": {"documentSymbol": {
                "hierarchicalDocumentSymbolSupport": true
            }}}
        }),
    )?;
    ensure!(
        initialized["capabilities"]["documentSymbolProvider"] == true,
        "symbols not advertised"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": SOURCE
        }}),
    )?;
    let shape = without_selection(expected());
    let deadline = Instant::now() + TIMEOUT;
    let actual = loop {
        let actual = request(
            &mut client,
            "symbols",
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": URI}}),
        )?;
        // Readiness depends on current names, hierarchy and full ranges only.
        // A wrong selection must reach the assertion rather than time out.
        if without_selection(actual.clone()) == shape {
            break actual;
        }
        ensure!(Instant::now() < deadline, "outline did not reach literal shape: {actual}");
    };
    ensure!(
        request(&mut client, "shutdown", "shutdown", Value::Null)?.is_null(),
        "shutdown result"
    );
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "candidate exit failed");
    client.assert_transport_clean()?;
    Ok(actual)
}

#[test]
fn method_name_does_not_select_keyword_or_body_repetition() -> Result<()> {
    assert_eq!(outline()?, expected());
    Ok(())
}

#[test]
fn ordinary_method_named_adjust_and_adjust_block_stay_visible() -> Result<()> {
    let actual = outline()?;
    let gold = expected();
    assert_eq!(actual[0]["selectionRange"], gold[0]["selectionRange"]);
    for index in 1..4 {
        assert_eq!(actual[0]["children"][index], gold[0]["children"][index]);
    }
    Ok(())
}
