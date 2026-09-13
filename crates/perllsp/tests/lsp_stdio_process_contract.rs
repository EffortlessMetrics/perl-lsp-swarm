//! Required real-process contract for Cargo's exact public `perllsp` binary.
//!
//! The transport, framing, timeout, event classification, stderr capture, and
//! child cleanup implementation is shared with the `perl-lsp-rs` process suite.
//! This target proves that the installable facade—not a compatibility binary or
//! PATH fallback—is the process completing the public contract.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, bail, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

fn explain_trace(client: &mut RealProcessClient, provider: &str) -> Result<Value> {
    let id = json!("explain-trace");
    let response = client.request(
        id.clone(),
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{"provider": provider}]
        }),
        timeout(),
    )?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("error").is_none(), "explanation request failed: {response}");
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing explanation: {response}"))
}

fn open_trace_fixture(client: &mut RealProcessClient) -> Result<&'static str> {
    let uri = "file:///workspace/provider-trace.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1,
            "text": "my $value = 1;\nprint $value;\n"
        }}),
    )?;
    Ok(uri)
}

#[test]
fn generic_trace_success_does_not_invent_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace"))?;
    let uri = open_trace_fixture(&mut client)?;
    let hover = client.request(
        json!(801),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri}, "position": {"line": 1, "character": 2}
        }),
        timeout(),
    )?;
    assert_response_id(&hover, &json!(801))?;
    ensure!(hover.get("error").is_none(), "valid hover failed: {hover}");
    ensure!(
        hover.pointer("/result/contents").is_some_and(|value| value.to_string().contains("print")),
        "builtin hover control must describe print: {hover}"
    );
    let explanation = explain_trace(&mut client, "hover")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("unknown")),
        "dispatch shape alone cannot establish freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/source_backed_state")
            == Some(&json!("not_proven_by_dispatch_trace")),
        "expected the actual generic dispatch trace: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/live_provider_result_count") == Some(&json!(1)),
        "successful hover trace must still record the actual result: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace"))
}

#[test]
fn generic_trace_error_does_not_invent_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace-error"))?;
    let response = client.request(json!("801"), "textDocument/hover", json!({}), timeout())?;
    assert_response_id(&response, &json!("801"))?;
    ensure!(
        response.pointer("/error/code") == Some(&json!(-32602)),
        "expected invalid params: {response}"
    );
    let explanation = explain_trace(&mut client, "hover")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("unknown")),
        "provider error without accepted-state evidence cannot claim freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/provider_error/code") == Some(&json!(-32602)),
        "trace must retain the provider error: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace-error"))
}

#[test]
fn generic_trace_preserves_provider_owned_full_token_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace-tokens"))?;
    let uri = open_trace_fixture(&mut client)?;
    let response = client.request(
        json!(802),
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": {"uri": uri}
        }),
        timeout(),
    )?;
    assert_response_id(&response, &json!(802))?;
    ensure!(response.get("error").is_none(), "valid full-token request failed: {response}");
    ensure!(
        response
            .pointer("/result/data")
            .and_then(Value::as_array)
            .is_some_and(|data| !data.is_empty()),
        "full-token control must execute the provider: {response}"
    );
    let explanation = explain_trace(&mut client, "semantic_tokens")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("fresh")),
        "dispatcher must preserve provider-owned freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/provider_action")
            == Some(&json!("textDocument/semanticTokens/full")),
        "expected the provider-owned full-token trace: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace-tokens"))
}

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn assert_public_candidate(client: &RealProcessClient) -> Result<()> {
    ensure!(
        client.candidate_name() == "perllsp",
        "public process target selected {}; env={}; path={}",
        client.candidate_name(),
        client.candidate_environment(),
        client.candidate_path().display()
    );
    ensure!(
        client.candidate_environment() == "CARGO_BIN_EXE_perllsp",
        "public process target used the wrong Cargo identity: {}",
        client.candidate_environment()
    );
    ensure!(
        client.candidate_path().is_file(),
        "exact public candidate path disappeared: {}",
        client.candidate_path().display()
    );
    ensure!(
        client.candidate_path().file_stem().and_then(|stem| stem.to_str()) == Some("perllsp"),
        "exact public candidate was not named perllsp: {}",
        client.candidate_path().display()
    );
    Ok(())
}

fn initialize(client: &mut RealProcessClient, id: Value) -> Result<Value> {
    client.request(
        id,
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perllsp-public-process-contract",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            },
            "workspaceFolders": null
        }),
        timeout(),
    )
}

fn assert_response_id(response: &Value, expected: &Value) -> Result<()> {
    ensure!(response.get("jsonrpc") == Some(&json!("2.0")), "missing JSON-RPC version: {response}");
    ensure!(
        response.get("id") == Some(expected),
        "response ID mismatch: expected={expected}; response={response}"
    );
    Ok(())
}

fn initialize_and_notify(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response = initialize(client, id.clone())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize omitted capabilities: {response}"
    );
    client.notify("initialized", json!({}))
}

fn shutdown_and_exit(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response = client.request(id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.get("result").is_some_and(Value::is_null),
        "shutdown must return null: {response}"
    );
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "clean public shutdown exited with {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

#[test]
fn exact_public_candidate_completes_legal_lifecycle() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-public"))?;
    shutdown_and_exit(&mut client, json!("shutdown-public"))
}

#[test]
fn preinitialize_duplicate_initialize_and_post_shutdown_are_deterministic() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;

    let before_id = json!("before-initialize");
    let before = client.request(
        before_id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///not-open.pl" },
            "position": { "line": 0, "character": 0 }
        }),
        timeout(),
    )?;
    assert_response_id(&before, &before_id)?;
    ensure!(
        before.pointer("/error/code") == Some(&json!(-32002)),
        "expected ServerNotInitialized: {before}"
    );

    initialize_and_notify(&mut client, json!(1))?;
    let duplicate_id = json!("duplicate-initialize");
    let duplicate = initialize(&mut client, duplicate_id.clone())?;
    assert_response_id(&duplicate, &duplicate_id)?;
    ensure!(
        duplicate.pointer("/error/code") == Some(&json!(-32600)),
        "duplicate initialize must return InvalidRequest: {duplicate}"
    );

    let shutdown = client.request(json!(2), "shutdown", Value::Null, timeout())?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null), "shutdown failed: {shutdown}");
    let after_id = json!("after-shutdown");
    let after = client.request(after_id.clone(), "$/perl-lsp/watchdog", json!({}), timeout())?;
    assert_response_id(&after, &after_id)?;
    ensure!(
        after.pointer("/error/code") == Some(&json!(-32600)),
        "request after shutdown must return InvalidRequest: {after}"
    );

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn public_candidate_preserves_fragmented_coalesced_and_utf8_frames() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;

    let initialize_id = json!(7);
    let initialize_message = json!({
        "jsonrpc": "2.0",
        "id": initialize_id.clone(),
        "method": "initialize",
        "params": {
            "processId": null,
            "clientInfo": {
                "name": "perllsp-fragmented-client",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {},
            "workspaceFolders": null
        }
    });
    let frame = RealProcessClient::encode_message(&initialize_message);
    let first = frame.len().min(9);
    let second = frame.len().min(first.saturating_add(17));
    client.send_raw_chunks(&[&frame[..first], &frame[first..second], &frame[second..]])?;
    let response = client.receive_response(&initialize_id, timeout())?;
    assert_response_id(&response, &initialize_id)?;
    client.notify("initialized", json!({}))?;

    let numeric_id = json!(42);
    let string_id = json!("utf8-✓");
    let numeric = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": numeric_id.clone(),
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "first" }
    }));
    let string = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": string_id.clone(),
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "π and ✓ prove byte lengths" }
    }));
    let mut coalesced = numeric;
    coalesced.extend_from_slice(&string);
    client.send_raw_bytes(&coalesced)?;

    let numeric_response = client.receive_response(&numeric_id, timeout())?;
    let string_response = client.receive_response(&string_id, timeout())?;
    assert_response_id(&numeric_response, &numeric_id)?;
    assert_response_id(&string_response, &string_id)?;
    ensure!(numeric_response.get("result").is_some_and(Value::is_null));
    ensure!(string_response.get("result").is_some_and(Value::is_null));
    shutdown_and_exit(&mut client, json!(43))
}

#[test]
fn serialized_notification_receives_no_response() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!(1))?;

    client.notify("$/setTrace", json!({ "value": "off" }))?;
    let shutdown_id = json!("serialized-public-barrier");
    let shutdown = client.request(shutdown_id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&shutdown, &shutdown_id)?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null));
    client.assert_no_response_pending()?;

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn exit_without_shutdown_returns_status_one() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!(1))?;
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(!status.success(), "exit without shutdown succeeded");
    ensure!(status.code() == Some(1), "expected status 1, got {status}");
    client.assert_transport_clean()
}

#[test]
fn strict_stdout_parser_rejects_stray_logs_and_lf_only_frames() -> Result<()> {
    let stray_log = b"starting perllsp on stdout\n";
    let Err(stray_error) = RealProcessClient::parse_stdout_frame_for_test(stray_log) else {
        bail!("stray stdout log must fail strict framing");
    };
    ensure!(
        stray_error.to_string().contains("CRLF")
            || stray_error.to_string().contains("header")
            || stray_error.to_string().contains("non-header"),
        "stray log failed for an unexpected reason: {stray_error:#}"
    );

    let body = br#"{"jsonrpc":"2.0","method":"window/logMessage"}"#;
    let mut lf_only = format!("Content-Length: {}\n\n", body.len()).into_bytes();
    lf_only.extend_from_slice(body);
    let Err(lf_error) = RealProcessClient::parse_stdout_frame_for_test(&lf_only) else {
        bail!("LF-only framing must fail");
    };
    ensure!(
        lf_error.to_string().contains("CRLF"),
        "LF-only framing failed for the wrong reason: {lf_error:#}"
    );
    Ok(())
}
