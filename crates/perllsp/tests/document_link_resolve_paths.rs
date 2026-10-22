//! Literal file document links resolved by Cargo's exact public server binary.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, anyhow, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

fn resolve_file_cases(cases: &[(&str, &str)]) -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let initialized = client.request(
        json!(1),
        "initialize",
        json!({"processId": null, "capabilities": {}}),
        TIMEOUT,
    )?;
    ensure!(initialized.get("error").is_none(), "initialize failed: {initialized}");
    client.notify("initialized", json!({}))?;

    let uri = "file:///workspace/document_link_resolve_paths.pl";
    let source = cases.iter().map(|(path, _)| format!("require '{path}';\n")).collect::<String>();
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": source
        }}),
    )?;
    let response = client.request(
        json!(2),
        "textDocument/documentLink",
        json!({"textDocument": {"uri": uri}}),
        TIMEOUT,
    )?;
    let links = response["result"]
        .as_array()
        .ok_or_else(|| anyhow!("documentLink did not return links: {response}"))?;
    ensure!(links.len() == cases.len(), "expected one deferred link per require: {links:?}");
    for (index, (path, expected)) in cases.iter().enumerate() {
        let link = links
            .iter()
            .find(|link| link.pointer("/data/path").and_then(Value::as_str) == Some(*path))
            .ok_or_else(|| anyhow!("missing file link for {path}: {links:?}"))?;
        ensure!(link["data"]["type"] == "file", "wrong link kind: {link}");
        ensure!(link.get("target").is_none(), "file link must be deferred: {link}");
        let resolved =
            client.request(json!(index + 3), "documentLink/resolve", link.clone(), TIMEOUT)?;
        ensure!(resolved.get("error").is_none(), "resolve failed for {path}: {resolved}");
        ensure!(
            resolved.pointer("/result/target").and_then(Value::as_str) == Some(*expected),
            "wrong target for {path}: expected {expected}, received {resolved}"
        );
        ensure!(resolved["result"]["range"] == link["range"], "resolve changed source range");
    }

    let shutdown = client.request(json!("shutdown"), "shutdown", Value::Null, TIMEOUT)?;
    ensure!(
        shutdown.get("error").is_none() && shutdown.get("result").is_some_and(Value::is_null),
        "shutdown failed: {shutdown}"
    );
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "server did not exit cleanly");
    client.assert_transport_clean()
}

#[test]
fn absolute_file_links_resolve_parent_segments() -> Result<()> {
    resolve_file_cases(&[
        ("C:/pkg/sub/../target.pl", "file:///C:/pkg/target.pl"),
        ("//server/share/sub/../target.pl", "file://server/share/target.pl"),
        ("C:/pkg/a/b/../../target.pl", "file:///C:/pkg/target.pl"),
        ("//server/share/a/b/../../target.pl", "file://server/share/target.pl"),
        ("C:/pkg/./sub/../Hash#Part.pl", "file:///C:/pkg/Hash%23Part.pl"),
        ("//server/share/./sub/../Percent%2e.pl", "file://server/share/Percent%252e.pl"),
    ])
}

#[test]
fn absolute_file_links_preserve_literal_filename_controls() -> Result<()> {
    resolve_file_cases(&[
        ("C:/pkg/target.pl", "file:///C:/pkg/target.pl"),
        ("//server/share/target.pl", "file://server/share/target.pl"),
        ("C:/pkg/Hash#Part.pl", "file:///C:/pkg/Hash%23Part.pl"),
        ("//server/share/Query?Part.pl", "file://server/share/Query%3FPart.pl"),
        ("C:/pkg/%2e%2e/target.pl", "file:///C:/pkg/%252e%252e/target.pl"),
        ("//server/share/%2e%2e/target.pl", "file://server/share/%252e%252e/target.pl"),
        ("C:/../../target.pl", "file:///C:/target.pl"),
    ])
}

#[test]
fn absolute_file_links_resolve_terminal_parent_segments() -> Result<()> {
    resolve_file_cases(&[
        ("C:/pkg/sub/..", "file:///C:/pkg/"),
        ("//server/share/sub/..", "file://server/share/"),
        ("C:/pkg/.", "file:///C:/pkg/"),
        ("//server/share/.", "file://server/share/"),
        ("C:/..", "file:///C:/"),
    ])
}

#[test]
fn absolute_file_links_preserve_unc_share_root() -> Result<()> {
    resolve_file_cases(&[
        ("//server/share/../target.pl", "file://server/share/target.pl"),
        ("//server/share/../../target.pl", "file://server/share/target.pl"),
        ("//server/share/a/../../../Hash#Part.pl", "file://server/share/Hash%23Part.pl"),
        ("//server/share/..", "file://server/share/"),
        ("//server/share/../%2e%2e/target.pl", "file://server/share/%252e%252e/target.pl"),
    ])
}
