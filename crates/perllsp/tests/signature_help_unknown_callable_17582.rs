//! Unknown callable refusal and recovery through Cargo's exact public binary.
//! This does not qualify canonical callable resolution or runtime dispatch.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, anyhow, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn open_project(source: &str) -> Result<(RealProcessClient, String)> {
    let mut client = RealProcessClient::spawn_exact()?;
    let path = client.workspace_path().join("app.pl");
    std::fs::write(&path, source)?;
    std::fs::create_dir(client.workspace_path().join("lib"))?;
    std::fs::write(
        client.workspace_path().join("lib/Other.pm"),
        "package Other;\nsub unrelated { return 1 }\n1;\n",
    )?;
    let uri = url::Url::from_file_path(&path)
        .map_err(|()| anyhow!("invalid project file URI"))?
        .to_string();
    let root = url::Url::from_directory_path(client.workspace_path())
        .map_err(|()| anyhow!("invalid project root URI"))?
        .to_string();
    let response = client.request(
        json!("init"),
        "initialize",
        json!({
            "rootUri": root, "capabilities": {},
        }),
        Duration::from_secs(5),
    )?;
    ensure!(response.get("error").is_none(), "initialize failed: {response}");
    ensure!(
        response.pointer("/result/capabilities/signatureHelpProvider").is_some(),
        "signature help was not advertised: {response}"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": source,
        }}),
    )?;
    Ok((client, uri))
}

fn signature_help(
    client: &mut RealProcessClient,
    uri: &str,
    id: u32,
    line: u32,
    character: u32,
) -> Result<Value> {
    let response = client.request(
        json!(id),
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        Duration::from_secs(5),
    )?;
    ensure!(response.get("id") == Some(&json!(id)), "wrong response identity: {response}");
    ensure!(response.get("error").is_none(), "signature help failed: {response}");
    response.get("result").cloned().ok_or_else(|| anyhow!("missing terminal result: {response}"))
}

fn change(client: &mut RealProcessClient, uri: &str, version: u32, source: &str) -> Result<()> {
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": version},
            "contentChanges": [{"text": source}],
        }),
    )
}

fn finish(mut client: RealProcessClient) -> Result<()> {
    let response =
        client.request(json!("shutdown"), "shutdown", Value::Null, Duration::from_secs(5))?;
    ensure!(response.get("result") == Some(&Value::Null), "shutdown failed: {response}");
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(Duration::from_secs(5))?.success(), "server exit failed");
    client.assert_transport_clean()
}

fn assert_signature(result: &Value, label: &str, parameters: &[&str], active: u32) {
    let expected_parameters: Vec<Value> = parameters.iter().map(|label| json!(label)).collect();
    assert_eq!(result.pointer("/signatures/0/label"), Some(&json!(label)), "{result}");
    assert_eq!(
        result.get("signatures").and_then(Value::as_array).map(Vec::len),
        Some(1),
        "{result}"
    );
    let actual_parameters: Option<Vec<Value>> = result
        .pointer("/signatures/0/parameters")
        .and_then(Value::as_array)
        .map(|params| params.iter().map(|param| param["label"].clone()).collect());
    assert_eq!(actual_parameters, Some(expected_parameters), "{result}");
    assert_eq!(result.get("activeParameter"), Some(&json!(active)), "{result}");
}

// Parsing is asynchronous. Only literal null may be retried; a wrong non-null
// answer fails immediately, so stale/fabricated signatures cannot be hidden.
fn known_signature(
    client: &mut RealProcessClient,
    uri: &str,
    id: u32,
    line: u32,
    character: u32,
    expected: (&str, &[&str], u32),
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    for attempt in 0..100 {
        let result = signature_help(client, uri, id * 1000 + attempt, line, character)?;
        if !result.is_null() {
            assert_signature(&result, expected.0, expected.1, expected.2);
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "known static signature never became ready: {}",
            expected.0
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(anyhow!("known static signature never became ready: {}", expected.0))
}

#[test]
fn unknown_calls_return_literal_null_at_each_parameter() -> Result<()> {
    let source = "use strict;\nuse warnings;\nmy $value = mystery(10, 20);\n";
    let (mut client, uri) = open_project(source)?;
    // Literal ASCII positions: `mystery(` ends at 20; `10, ` ends at 24.
    for (id, character) in [(1, 20), (2, 24)] {
        assert_eq!(signature_help(&mut client, &uri, id, 2, character)?, Value::Null);
    }
    finish(client)
}

#[test]
fn known_undocumented_sub_and_builtin_keep_parameter_context() -> Result<()> {
    let source = "use feature 'signatures';\nsub calculate ($left, $right) { $left + $right }\nmy $value = calculate(10, 20);\nmy @items;\npush(@items, 1);\n";
    let (mut client, uri) = open_project(source)?;
    for (id, character, active) in [(1, 22, 0), (2, 26, 1)] {
        known_signature(
            &mut client,
            &uri,
            id,
            2,
            character,
            ("sub calculate($left, $right)", &["$left", "$right"], active),
        )?;
    }
    assert_signature(
        &signature_help(&mut client, &uri, 3, 4, 13)?,
        "push ARRAY, LIST",
        &["ARRAY", "LIST"],
        1,
    );
    finish(client)
}

#[test]
fn declaring_deleting_and_redeclaring_recovers_without_fabrication() -> Result<()> {
    let unknown = "use feature 'signatures';\n\nmy $value = recover(10, 20);\n";
    let declared = "use feature 'signatures';\nsub recover ($first, $second) { $first + $second }\nmy $value = recover(10, 20);\n";
    let deleted = "use feature 'signatures';\nsub ready_after_delete ($fresh) { $fresh }\nmy $value = recover(10, 20);\nready_after_delete(1);\n";
    let redeclared = "use feature 'signatures';\nsub recover ($new_first, $new_second) { $new_first + $new_second }\nmy $value = recover(10, 20);\n";
    // Disk stays unknown: each exact signature must come from the unsaved buffer.
    let (mut client, uri) = open_project(unknown)?;
    assert_eq!(signature_help(&mut client, &uri, 1, 2, 24)?, Value::Null);
    change(&mut client, &uri, 2, declared)?;
    known_signature(
        &mut client,
        &uri,
        2,
        2,
        24,
        ("sub recover($first, $second)", &["$first", "$second"], 1),
    )?;
    change(&mut client, &uri, 3, deleted)?;
    assert_eq!(signature_help(&mut client, &uri, 3, 2, 24)?, Value::Null);
    known_signature(
        &mut client,
        &uri,
        5,
        3,
        19,
        ("sub ready_after_delete($fresh)", &["$fresh"], 0),
    )?;
    assert_eq!(signature_help(&mut client, &uri, 6, 2, 24)?, Value::Null);
    change(&mut client, &uri, 4, redeclared)?;
    known_signature(
        &mut client,
        &uri,
        4,
        2,
        24,
        ("sub recover($new_first, $new_second)", &["$new_first", "$new_second"], 1),
    )?;
    finish(client)
}
