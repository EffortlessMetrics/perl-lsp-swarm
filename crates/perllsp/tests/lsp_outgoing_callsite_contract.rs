//! Outgoing edges retain only the selected callable's own callsites.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///outgoing-callsite-contract.pl";
const SOURCE: &str = concat!(
    "sub shared {}\r\n",
    "sub hidden {}\r\n",
    "sub outer {\r\n",
    "    shared();\r\n",
    "    sub inner { shared(); hidden(); }\r\n",
    "    my $callback = sub { shared(); hidden(); };\r\n",
    "    { shared(); }\r\n",
    "    \"😀\"; shared();\r\n",
    "}\r\n",
    "sub plain { shared(); shared(); }\r\n",
);

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, TIMEOUT)?;
    ensure!(response["jsonrpc"] == "2.0" && response["id"] == id, "bad response: {response}");
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().ok_or_else(|| anyhow::anyhow!("missing result: {response}"))
}

fn outgoing(
    client: &mut RealProcessClient,
    name: &str,
    line: u32,
    character: u32,
) -> Result<Value> {
    let prepared = request(
        client,
        &format!("prepare-{name}"),
        "textDocument/prepareCallHierarchy",
        json!({"textDocument": {"uri": URI}, "position": {"line": line, "character": character}}),
    )?;
    let items = prepared.as_array().ok_or_else(|| anyhow::anyhow!("no items: {prepared}"))?;
    ensure!(items.len() == 1 && items[0]["name"] == name, "wrong callable: {prepared}");
    request(
        client,
        &format!("outgoing-{name}"),
        "callHierarchy/outgoingCalls",
        json!({"item": items[0]}),
    )
}

fn ranges(rows: &[(u32, u32, u32)]) -> Value {
    json!(
        rows.iter()
            .map(|&(line, start, end)| json!({
                "start": {"line": line, "character": start},
                "end": {"line": line, "character": end}
            }))
            .collect::<Vec<_>>()
    )
}

fn assert_edges(actual: &Value, expected: &[(&str, Value)]) -> Result<()> {
    let calls = actual.as_array().ok_or_else(|| anyhow::anyhow!("not calls: {actual}"))?;
    ensure!(calls.len() == expected.len(), "wrong edge set: {actual}");
    for (name, from_ranges) in expected {
        let matches: Vec<_> = calls.iter().filter(|call| call["to"]["name"] == *name).collect();
        ensure!(matches.len() == 1, "missing or duplicate {name} edge: {actual}");
        ensure!(matches[0]["to"]["uri"] == URI, "wrong target document: {actual}");
        ensure!(matches[0]["fromRanges"] == *from_ranges, "wrong {name} callsites: {actual}");
    }
    Ok(())
}

#[test]
fn outgoing_calls_exclude_nested_callable_bodies_and_keep_direct_ranges() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let initialized = request(
        &mut client,
        "initialize",
        "initialize",
        json!({
            "processId": null, "rootUri": null, "workspaceFolders": null,
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
    )?;
    ensure!(initialized.get("capabilities").is_some(), "missing capabilities: {initialized}");
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": SOURCE
        }}),
    )?;

    let outer = outgoing(&mut client, "outer", 2, 5)?;
    let inner = outgoing(&mut client, "inner", 4, 9)?;
    let plain = outgoing(&mut client, "plain", 9, 5)?;
    let shutdown = request(&mut client, "shutdown", "shutdown", Value::Null)?;
    ensure!(shutdown.is_null(), "shutdown result: {shutdown}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(TIMEOUT)?;
    ensure!(status.success(), "shutdown failed: {status}; {}", client.stderr_tail());
    client.assert_transport_clean()?;

    // Retention controls also reject an empty result or pruning ordinary blocks.
    assert_edges(&plain, &[("shared", ranges(&[(9, 12, 20), (9, 22, 30)]))])?;
    assert_edges(
        &inner,
        &[("shared", ranges(&[(4, 16, 24)])), ("hidden", ranges(&[(4, 26, 34)]))],
    )?;
    assert_edges(&outer, &[("shared", ranges(&[(3, 4, 12), (6, 6, 14), (7, 10, 18)]))])
}
