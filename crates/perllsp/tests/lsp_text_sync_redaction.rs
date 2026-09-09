//! Exact-process proof that malformed didChange diagnostics do not echo payloads.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn initialize(client: &mut RealProcessClient) -> Result<()> {
    let response = client.request(
        json!("initialize-redaction"),
        "initialize",
        json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {},
            "workspaceFolders": null
        }),
        timeout(),
    )?;
    ensure!(response.get("result").is_some(), "initialize failed: {response}");
    client.notify("initialized", json!({}))
}

fn shutdown(client: &mut RealProcessClient) -> Result<()> {
    let response =
        client.request(json!("shutdown-redaction"), "shutdown", Value::Null, timeout())?;
    ensure!(response.get("result").is_some_and(Value::is_null), "shutdown failed: {response}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "clean shutdown failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn malformed_did_change_stderr_is_payload_free_over_stdio() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;

    let uri = "file:///workspace/redaction-canary.pl";
    let text_canary = "UNSAVED_SOURCE_CANARY_15207_API_KEY=secret";
    let range_canary = "INVALID_RANGE_CANARY_15207";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub old_symbol { return 0; }\n"
            }
        }),
    )?;
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "range": range_canary, "text": text_canary }]
        }),
    )?;

    let later_id = json!(2);
    let later = client.request(
        later_id.clone(),
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
        timeout(),
    )?;
    ensure!(later.get("id") == Some(&later_id), "later request did not complete: {later}");
    ensure!(
        later.get("error").is_none() && later.get("result").is_some_and(Value::is_array),
        "later request returned no successful symbol array: {later}"
    );
    shutdown(&mut client)?;

    let stderr = client.stderr_tail();
    ensure!(
        !stderr.contains("[stderr truncated to last"),
        "stderr capture was truncated; canary absence is inconclusive: {stderr}"
    );
    ensure!(!stderr.contains(text_canary), "stderr leaked document text: {stderr}");
    ensure!(!stderr.contains(range_canary), "stderr leaked malformed range: {stderr}");
    ensure!(stderr.contains("change_index=0"), "stderr lost change index: {stderr}");
    ensure!(
        stderr.contains("error_category=\"invalid_content_change\""),
        "stderr lost stable error category: {stderr}"
    );
    ensure!(stderr.contains(uri), "stderr lost URI context: {stderr}");
    Ok(())
}
