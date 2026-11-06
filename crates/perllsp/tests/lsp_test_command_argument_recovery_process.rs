//! Literal JSON-RPC admission and same-session retry proof for legacy test commands.
//! The Unix retry executes a real disk TAP fixture; it does not claim current-buffer
//! execution or the wider typed testing/runner migration.
#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const RUN_TEST_ERROR: &str = "perl.runTest: arguments[0] must be a test ID string; retry with arguments: [\"file:///path/to/test.t::test name\"].";
const RUN_FILE_ERROR: &str = "perl.runTestFile: arguments[0] must be a file URI string; retry with arguments: [\"file:///path/to/test.t\"].";

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn request(client: &mut RealProcessClient, id: &str, params: Value) -> Result<Value> {
    client.request(json!(id), "workspace/executeCommand", params, timeout())
}

fn error_response(id: &str, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id,
        "error": {"code": code, "message": message, "data": null}})
}

fn initialize(client: &mut RealProcessClient) -> Result<()> {
    let response = client.request(
        json!("initialize"),
        "initialize",
        json!({"processId": null, "rootUri": null, "capabilities": {}}),
        timeout(),
    )?;
    ensure!(response["id"] == "initialize" && response.get("error").is_none(), "{response}");
    let commands = response.pointer("/result/capabilities/executeCommandProvider/commands");
    ensure!(
        commands.is_some_and(|value| value.as_array().is_some_and(|commands| {
            commands.contains(&json!("perl.runTest"))
                && commands.contains(&json!("perl.runTestFile"))
        })),
        "the tested commands must actually be advertised: {response}"
    );
    client.notify("initialized", json!({}))
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let response = client.request(json!("shutdown"), "shutdown", Value::Null, timeout())?;
    assert_eq!(response, json!({"jsonrpc": "2.0", "id": "shutdown", "result": null}));
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(timeout())?.success(), "{}", client.stderr_tail());
    client.assert_transport_clean()
}

#[test]
fn malformed_test_arguments_name_the_actual_field() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;
    for (command, message) in
        [("perl.runTest", RUN_TEST_ERROR), ("perl.runTestFile", RUN_FILE_ERROR)]
    {
        let invalid_arguments = [
            None,
            Some(json!([])),
            Some(json!([null])),
            Some(json!([17])),
            Some(json!([false])),
            Some(json!([{}])),
            Some(json!([[]])),
            Some(json!([null, "file:///path/to/test.t"])),
        ];
        for (index, arguments) in invalid_arguments.into_iter().enumerate() {
            let id = format!("{command}-{index}");
            let mut params = json!({"command": command});
            if let Some(arguments) = arguments {
                params["arguments"] = arguments;
            }
            assert_eq!(request(&mut client, &id, params)?, error_response(&id, -32602, message));
        }
    }
    finish(&mut client)
}

#[test]
fn unchanged_request_shapes_and_unsupported_commands_retain_their_contracts() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;
    client.send_raw_bytes(&RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0", "id": "missing-params", "method": "workspace/executeCommand"
    })))?;
    assert_eq!(
        client.receive_response(&json!("missing-params"), timeout())?,
        json!({"jsonrpc": "2.0", "id": "missing-params", "error": {
            "code": -32602, "message": "workspace/executeCommand: missing required parameter 'params'",
            "data": {"errorType": "executeCommand", "originalError": "workspace/executeCommand: missing required parameter 'params'"}
        }})
    );
    for arguments in [Value::Null, json!(17), json!({}), json!("file:///test.t")] {
        assert_eq!(
            request(
                &mut client,
                "non-array",
                json!({"command": "perl.runTestFile", "arguments": arguments})
            )?,
            error_response("non-array", -32602, "'arguments' must be an array when present")
        );
    }
    assert_eq!(
        request(&mut client, "unknown", json!({"command": "perl.noSuchCommand", "arguments": []}))?,
        error_response("unknown", -32601, "Unknown command: perl.noSuchCommand")
    );
    assert_eq!(
        request(
            &mut client,
            "unsupported",
            json!({"command": "perl.runSubtest", "arguments": ["demo"]})
        )?,
        error_response("unsupported", -32601, "runSubtest 'demo' is not implemented server-side")
    );
    // A present string still reaches the original domain behavior, even empty.
    assert_eq!(
        request(
            &mut client,
            "empty-test-id",
            json!({"command": "perl.runTest", "arguments": [""]})
        )?,
        json!({"jsonrpc": "2.0", "id": "empty-test-id", "result": {"status": "error", "message": "Invalid test ID"}})
    );
    assert_eq!(
        request(
            &mut client,
            "empty-file-uri",
            json!({"command": "perl.runTestFile", "arguments": [""]})
        )?,
        json!({"jsonrpc": "2.0", "id": "empty-file-uri", "result": {"status": "error", "message": "Document not found"}})
    );
    finish(&mut client)
}

#[cfg(unix)]
#[test]
fn rejected_command_preserves_open_document_and_corrected_retry_returns_real_tap() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;
    let path = client.workspace_path().join("recovery.t");
    let marker = client.workspace_path().join("execution-count");
    let text = format!(
        "use strict;\nuse warnings;\nsub recovery {{ 1 }}\nopen TRACE, '>>', '{}';\nprint TRACE \"run\\n\";\nclose TRACE;\nprint \"1..2\\nok 1 - recovery\\nnot ok 2 - negative\\n\";\nexit 1;\n",
        marker.display()
    );
    std::fs::write(&path, &text)?;
    let uri = url::Url::from_file_path(&path)
        .map_err(|()| anyhow::anyhow!("fixture path is not a file URI"))?
        .to_string();
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": text
        }}),
    )?;
    let symbols = |client: &mut RealProcessClient, id: &str| {
        client.request(
            json!(id),
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
            timeout(),
        )
    };
    let before = symbols(&mut client, "symbols-before")?;
    let entries =
        before["result"].as_array().ok_or_else(|| anyhow::anyhow!("missing symbols: {before}"))?;
    assert_eq!(entries.len(), 1, "the state witness has one unambiguous declaration");
    let recovery = entries
        .iter()
        .find(|entry| entry["name"] == "recovery")
        .ok_or_else(|| anyhow::anyhow!("fixture subroutine absent: {before}"))?;
    assert_eq!(recovery["kind"], 12);
    assert_eq!(
        recovery["range"],
        json!({"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 18}})
    );
    assert_eq!(
        recovery["selectionRange"],
        json!({"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 12}})
    );
    for (command, message) in
        [("perl.runTest", RUN_TEST_ERROR), ("perl.runTestFile", RUN_FILE_ERROR)]
    {
        let id = format!("reject-{command}");
        assert_eq!(
            request(&mut client, &id, json!({"command": command, "arguments": [null, uri]}))?,
            error_response(&id, -32602, message)
        );
        assert!(!marker.exists(), "rejected command must not execute the fixture");
        let after = symbols(&mut client, "symbols-after")?;
        assert_eq!(
            after.get("result"),
            before.get("result"),
            "rejection must preserve document state"
        );
        ensure!(after.get("error").is_none(), "document request failed after rejection: {after}");
    }
    // Literal TAP expectations include a failed row. Process exit and the top-level
    // success status cannot fabricate or erase individual test outcomes.
    let expected = json!({"status": "success", "results": [
        {"testId": format!("{}::- recovery", path.display()), "status": "passed", "message": null, "duration": null},
        {"testId": format!("{}::2 - negative", path.display()), "status": "failed", "message": "not ok 2 - negative", "duration": null}
    ]});
    for (id, command, target, count) in [
        ("retry-file", "perl.runTestFile", uri.clone(), "run\n"),
        ("retry-test", "perl.runTest", format!("{uri}::recovery"), "run\nrun\n"),
    ] {
        assert_eq!(
            request(&mut client, id, json!({"command": command, "arguments": [target]}))?,
            json!({"jsonrpc": "2.0", "id": id, "result": expected})
        );
        assert_eq!(std::fs::read_to_string(&marker)?, count, "one execution per accepted request");
    }
    let after = symbols(&mut client, "symbols-after-retry")?;
    assert_eq!(after.get("result"), before.get("result"));
    ensure!(after.get("error").is_none(), "{after}");
    finish(&mut client)
}
