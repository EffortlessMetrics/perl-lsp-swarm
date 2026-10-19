//! Literal UTF-16 parse-error ranges through the public JSON-RPC process.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///parse-diagnostic-utf16.pl";

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn request(client: &mut RealProcessClient, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(method), method, params, timeout())?;
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn start(mode: &str) -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact_with_env(&[
        ("PERL_LSP_DIAGNOSTIC_MODE", mode),
        ("RUST_LOG", "warn"),
    ])?;
    let root = url::Url::from_file_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("workspace cannot be expressed as a file URI"))?;
    let result = request(
        &mut client,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root.as_str(),
            "workspaceFolders": [{ "uri": root.as_str(), "name": "fixture" }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "textDocument": { "diagnostic": {} }
            }
        }),
    )?;
    ensure!(result.pointer("/capabilities/positionEncoding") == Some(&json!("utf-16")));
    ensure!(result.pointer("/capabilities/textDocumentSync/change") == Some(&json!(1)));
    client.notify("initialized", json!({}))?;
    Ok(client)
}

fn wait_for_parse(client: &mut RealProcessClient) -> Result<()> {
    let deadline = Instant::now() + timeout();
    loop {
        let result = request(
            client,
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": URI } }),
        )?;
        if result.get("resultId").and_then(Value::as_str).is_some()
            && result.get("data").and_then(Value::as_array).is_some_and(|data| !data.is_empty())
        {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "current parse never became ready: {result}");
    }
}

fn diagnostics(client: &mut RealProcessClient) -> Result<Vec<Value>> {
    let report =
        request(client, "textDocument/diagnostic", json!({ "textDocument": { "uri": URI } }))?;
    ensure!(report["kind"] == "full", "expected full report: {report}");
    report["items"].as_array().cloned().context("report omitted items")
}

fn residue_range(items: &[Value]) -> Result<&Value> {
    items
        .iter()
        .find(|item| {
            item["code"] == "PL001"
                && item["message"] == "Unexpected same-line residue after the statement"
        })
        .and_then(|item| item.get("range"))
        .context("parser residue diagnostic omitted its range")
}

fn wait_for_fatal_range(client: &mut RealProcessClient, line: u64) -> Result<Value> {
    let deadline = Instant::now() + timeout();
    loop {
        let items = diagnostics(client)?;
        if let Some(range) = items
            .iter()
            .find(|item| {
                item["code"] == "PL001"
                    && item.pointer("/range/start/line") == Some(&json!(line))
                    && item.pointer("/range/start/character") == Some(&json!(5))
            })
            .and_then(|item| item.get("range"))
        {
            return Ok(range.clone());
        }
        ensure!(Instant::now() < deadline, "fatal parser diagnostic never appeared: {items:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn fatal_parse_error_ranges_cover_complete_unicode_scalars_in_normal_pull() -> Result<()> {
    let mut client = start("normal")?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": URI, "languageId": "perl", "version": 1,
                "text": "last 𐐀::LABEL;"
            }
        }),
    )?;
    let astral = wait_for_fatal_range(&mut client, 0)?;

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": "print \"😀\";\nlast é::LABEL;" }]
        }),
    )?;
    let bmp = wait_for_fatal_range(&mut client, 1)?;

    ensure!(
        astral
            == json!({
                "start": { "line": 0, "character": 5 },
                "end": { "line": 0, "character": 7 }
            }),
        "fatal normal-pull error must cover the astral scalar: {astral}"
    );
    ensure!(
        bmp == json!({
            "start": { "line": 1, "character": 5 },
            "end": { "line": 1, "character": 6 }
        }),
        "fatal normal-pull error must cover the BMP scalar after a multiline edit: {bmp}"
    );

    ensure!(request(&mut client, "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(timeout())?.success(), "server did not exit cleanly");
    client.assert_transport_clean()
}

#[test]
fn parse_error_ranges_cover_astral_scalars_after_multiline_edits() -> Result<()> {
    for mode in ["normal", "syntax-only"] {
        check_mode(mode)?;
    }
    Ok(())
}

fn check_mode(mode: &str) -> Result<()> {
    let mut client = start(mode)?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": URI, "languageId": "perl", "version": 1,
                "text": "my $x = 1 𐐀;\n"
            }
        }),
    )?;
    wait_for_parse(&mut client)?;
    let astral_items = diagnostics(&mut client)?;

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": "print \"😀\";\nmy $x = 1 𐐀;\n" }]
        }),
    )?;
    wait_for_parse(&mut client)?;
    let moved_items = diagnostics(&mut client)?;

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 3 },
            "contentChanges": [{ "text": "print \"😀\";\nmy $x = 1 2;\n" }]
        }),
    )?;
    wait_for_parse(&mut client)?;
    let ascii_items = diagnostics(&mut client)?;
    // Literal ranges are independent of the server's position conversion.
    ensure!(
        residue_range(&astral_items)?
            == &json!({
                "start": { "line": 0, "character": 10 },
                "end": { "line": 0, "character": 12 }
            }),
        "{mode} parse error must cover the complete astral scalar: {astral_items:?}"
    );
    ensure!(
        residue_range(&moved_items)?
            == &json!({
                "start": { "line": 1, "character": 10 },
                "end": { "line": 1, "character": 12 }
            }),
        "{mode} edited astral diagnostic must use the new line: {moved_items:?}"
    );
    ensure!(
        residue_range(&ascii_items)?
            == &json!({
                "start": { "line": 1, "character": 10 },
                "end": { "line": 1, "character": 11 }
            }),
        "{mode} ASCII diagnostic must remain one UTF-16 unit: {ascii_items:?}"
    );

    ensure!(request(&mut client, "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(timeout())?.success(), "server did not exit cleanly");
    client.assert_transport_clean()
}

#[test]
fn no_source_fatal_pull_spans_complete_line_endings() -> Result<()> {
    for (prefix, expected) in [
        (
            "\n",
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 1, "character": 0 }
            }),
        ),
        (
            "\r\n",
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 1, "character": 0 }
            }),
        ),
        (
            "\r",
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 1 }
            }),
        ),
    ] {
        let mut client = start("normal")?;
        // The 129th postfix layer exceeds the production depth limit (128),
        // yielding a real fatal NoSource parser diagnostic pinned to byte zero.
        let source = format!("{prefix}$x{};", "->a".repeat(129));
        client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": URI, "languageId": "perl", "version": 1,
                    "text": source
                }
            }),
        )?;
        let deadline = Instant::now() + timeout();
        let range = loop {
            let items = diagnostics(&mut client)?;
            if let Some(range) = items
                .iter()
                .find(|item| {
                    item["code"] == "PL001"
                        && item["message"].as_str().is_some_and(|message| {
                            message.starts_with("Nesting depth limit exceeded")
                        })
                })
                .and_then(|item| item.get("range"))
            {
                break range.clone();
            }
            ensure!(Instant::now() < deadline, "fatal NoSource diagnostic missing: {items:?}");
            std::thread::sleep(Duration::from_millis(10));
        };
        ensure!(range == expected, "{prefix:?} fatal NoSource range: {range}");

        ensure!(request(&mut client, "shutdown", Value::Null)?.is_null());
        client.notify("exit", Value::Null)?;
        ensure!(client.wait_for_exit(timeout())?.success(), "server did not exit cleanly");
        client.assert_transport_clean()?;
    }
    Ok(())
}
