//! Exact-process name-span regression slice for #8183. Broader declaration
//! hierarchy migration remains #8157; these fixtures use one block package.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///document-symbol-name-spans.pm";
const TIMEOUT: Duration = Duration::from_secs(10);
// Earlier keyword substrings and later body repetitions discriminate both
// first-match and last-match source-search approximations.
const CLEAN: &str = "# fixture\npackage age {\n    sub { 0 };\n    sub ub { 'ub age' }\n}\n";
const BROKEN: &str = "# fixture\npackage age {\n    sub { 0 };\n    sub ub { 'ub age' + }\n}\n";
const RENAMED: &str = "# fixture\npackage ack {\n    sub { 0 };\n    sub b { 'b ack' }\n}\n";
const CONDITIONAL: &str =
    "# fixture\npackage age {\n    if (1) {\n        sub ub { 'ub age' }\n    }\n}\n";

fn range(line: u32, start: u32, end_line: u32, end: u32) -> Value {
    json!({"start": {"line": line, "character": start},
           "end": {"line": end_line, "character": end}})
}

/// Literal coordinates, counted from the fixture text independently of the
/// parser, semantic extractors, position conversion and source substring search.
fn expected(package: &str, sub: &str, sub_end: u32, name_end: u32) -> Value {
    json!([{
        "name": package, "kind": 2, "detail": "",
        "range": range(1, 0, 4, 1),
        "selectionRange": range(1, 8, 1, 11),
        "children": [{
            "name": sub, "kind": 12, "detail": "",
            "range": range(3, 4, 3, sub_end),
            "selectionRange": range(3, 8, 3, name_end),
            "children": []
        }]
    }])
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, TIMEOUT)?;
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn start(source: &str) -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    let result = request(
        &mut client,
        "initialize",
        "initialize",
        json!({
            "processId": null, "rootUri": null, "workspaceFolders": null,
            "capabilities": {"textDocument": {"documentSymbol": {
                "hierarchicalDocumentSymbolSupport": true
            }}}
        }),
    )?;
    ensure!(
        result["capabilities"]["documentSymbolProvider"] == true,
        "document symbols not advertised: {result}"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": source
        }}),
    )?;
    Ok(client)
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

/// Await the exact current names/hierarchy/full ranges, then separately assert
/// the selection oracle. A wrong selection cannot masquerade as parse readiness.
fn current_outline(client: &mut RealProcessClient, gold: &Value) -> Result<Value> {
    let deadline = Instant::now() + TIMEOUT;
    let shape = without_selection(gold.clone());
    loop {
        let actual = request(
            client,
            "symbols",
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": URI}}),
        )?;
        if without_selection(actual.clone()) == shape {
            return Ok(actual);
        }
        ensure!(Instant::now() < deadline, "current outline never reached literal shape: {actual}");
    }
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    ensure!(request(client, "shutdown", "shutdown", Value::Null)?.is_null(), "shutdown result");
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "candidate exit failed");
    client.assert_transport_clean()
}

#[test]
fn package_name_does_not_select_the_package_keyword() -> Result<()> {
    let mut client = start(CLEAN)?;
    let gold = expected("age", "ub", 23, 10);
    let actual = current_outline(&mut client, &gold)?;
    assert_eq!(actual[0]["selectionRange"], gold[0]["selectionRange"]);
    finish(&mut client)
}

#[test]
fn nested_sub_name_does_not_select_the_sub_keyword() -> Result<()> {
    let mut client = start(CLEAN)?;
    let gold = expected("age", "ub", 23, 10);
    let actual = current_outline(&mut client, &gold)?;
    assert_eq!(
        actual[0]["children"][0]["selectionRange"],
        gold[0]["children"][0]["selectionRange"]
    );
    finish(&mut client)
}

/// The canonical declaration walker must reach declarations below control-flow
/// containers too. Looking up only the existing flat records and falling back
/// to substring inference on a missing record would leave this case wrong.
#[test]
fn package_scoped_sub_inside_condition_has_an_exact_name_span() -> Result<()> {
    let mut client = start(CONDITIONAL)?;
    let gold = json!([{
        "name": "age", "kind": 2, "detail": "",
        "range": range(1, 0, 5, 1),
        "selectionRange": range(1, 8, 1, 11),
        "children": [{
            "name": "ub", "kind": 12, "detail": "",
            "range": range(3, 8, 3, 27),
            "selectionRange": range(3, 12, 3, 14),
            "children": []
        }]
    }]);
    assert_eq!(current_outline(&mut client, &gold)?, gold);
    finish(&mut client)
}

#[test]
fn exact_name_spans_survive_repeated_queries_and_edit_recovery() -> Result<()> {
    let mut client = start(CLEAN)?;
    for (version, source, gold) in [
        (1, CLEAN, expected("age", "ub", 23, 10)),
        (2, BROKEN, expected("age", "ub", 25, 10)),
        (3, RENAMED, expected("ack", "b", 21, 9)),
        (4, CLEAN, expected("age", "ub", 23, 10)),
    ] {
        if version > 1 {
            client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": URI, "version": version},
                    "contentChanges": [{"text": source}]
                }),
            )?;
        }
        for _ in 0..3 {
            assert_eq!(current_outline(&mut client, &gold)?, gold, "version {version}");
        }
    }
    finish(&mut client)
}

/// Stable names cannot prove that geometry belongs to the current document.
/// Move both declarations, then reopen the URI at a reset version and move
/// them again. These are settled transitions, not held-worker ABA coverage.
#[test]
fn unchanged_names_get_current_ranges_after_edits_and_reopen() -> Result<()> {
    let shifted =
        "# moved\r\n\r\npackage age {\r\n    sub { 0 };\r\n        sub ub { 'ub age' }\r\n}\r\n";
    let reopened =
        "# reopened\n# same names\n\npackage age {\n  sub { 0 };\n  sub ub { 'ub age' }\n}\n";
    let after_reopen = "# after reopen\r\n\r\n\r\n\r\npackage age {\r\n    sub { 0 };\r\n      sub ub { 'ub age' }\r\n}\r\n";
    // Literal UTF-16 coordinates, independent of the production parser/mappers.
    let shifted_gold = json!([{
        "name": "age", "kind": 2, "detail": "",
        "range": range(2, 0, 5, 1),
        "selectionRange": range(2, 8, 2, 11),
        "children": [{
            "name": "ub", "kind": 12, "detail": "",
            "range": range(4, 8, 4, 27),
            "selectionRange": range(4, 12, 4, 14),
            "children": []
        }]
    }]);
    let reopened_gold = json!([{
        "name": "age", "kind": 2, "detail": "",
        "range": range(3, 0, 6, 1),
        "selectionRange": range(3, 8, 3, 11),
        "children": [{
            "name": "ub", "kind": 12, "detail": "",
            "range": range(5, 2, 5, 21),
            "selectionRange": range(5, 6, 5, 8),
            "children": []
        }]
    }]);
    let after_reopen_gold = json!([{
        "name": "age", "kind": 2, "detail": "",
        "range": range(4, 0, 7, 1),
        "selectionRange": range(4, 8, 4, 11),
        "children": [{
            "name": "ub", "kind": 12, "detail": "",
            "range": range(6, 6, 6, 25),
            "selectionRange": range(6, 10, 6, 12),
            "children": []
        }]
    }]);

    let mut client = start(CLEAN)?;
    let initial_gold = expected("age", "ub", 23, 10);
    assert_eq!(current_outline(&mut client, &initial_gold)?, initial_gold, "initial open");
    for (version, source, gold) in [(2, shifted, &shifted_gold), (3, CLEAN, &initial_gold)] {
        client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": URI, "version": version},
                "contentChanges": [{"text": source}]
            }),
        )?;
        assert_eq!(current_outline(&mut client, gold)?, *gold, "edit version {version}");
    }

    client.notify("textDocument/didClose", json!({"textDocument": {"uri": URI}}))?;
    assert_eq!(
        request(
            &mut client,
            "symbols-closed",
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": URI}}),
        )?,
        json!([]),
        "closed document must not retain its outline"
    );
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": reopened
        }}),
    )?;
    assert_eq!(current_outline(&mut client, &reopened_gold)?, reopened_gold, "reopen version 1");
    // Version 2 repeats across lifetimes, but its geometry must not: a cache
    // keyed only by URI/version/name must not reuse the pre-close selections.
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": URI, "version": 2},
            "contentChanges": [{"text": after_reopen}]
        }),
    )?;
    assert_eq!(
        current_outline(&mut client, &after_reopen_gold)?,
        after_reopen_gold,
        "edit after reopen"
    );
    finish(&mut client)
}
