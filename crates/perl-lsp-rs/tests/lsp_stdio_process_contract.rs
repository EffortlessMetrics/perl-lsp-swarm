//! Required real-process contract for Cargo's exact `perl-lsp` binary.
//!
//! These tests do not use PATH discovery, target-directory probing, `cargo
//! run`, or an in-process `LspServer`.

mod support;

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::real_process::RealProcessClient;

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn initialize(client: &mut RealProcessClient, id: Value) -> Result<Value> {
    client.request(
        id,
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perl-lsp-exact-process-contract",
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
    ensure!(
        response.get("jsonrpc") == Some(&json!("2.0")),
        "missing JSON-RPC version: {response}"
    );
    ensure!(
        response.get("id") == Some(expected),
        "response ID mismatch: {response}"
    );
    Ok(())
}

fn initialize_and_notify(
    client: &mut RealProcessClient,
    id: Value,
) -> Result<Value> {
    let response = initialize(client, id.clone())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize omitted capabilities: {response}"
    );
    client.notify("initialized", json!({}))?;
    Ok(response)
}

fn shutdown_and_exit(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response =
        client.request(id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.get("result").is_some_and(Value::is_null),
        "shutdown must return null: {response}"
    );
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "clean shutdown exited with {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

#[test]
fn exact_candidate_completes_legal_lifecycle() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    ensure!(
        client.candidate_path().is_file(),
        "exact candidate path disappeared"
    );
    initialize_and_notify(&mut client, json!("initialize-1"))?;
    shutdown_and_exit(&mut client, json!("shutdown-1"))
}

#[test]
fn request_before_initialize_returns_server_not_initialized() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let request_id = json!("before-initialize");
    let response = client.request(
        request_id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///not-open.pl" },
            "position": { "line": 0, "character": 0 }
        }),
        timeout(),
    )?;
    assert_response_id(&response, &request_id)?;
    ensure!(
        response.pointer("/error/code") == Some(&json!(-32002)),
        "expected ServerNotInitialized: {response}"
    );
    initialize_and_notify(&mut client, json!(2))?;
    shutdown_and_exit(&mut client, json!(3))
}

#[test]
fn fragmented_and_coalesced_frames_preserve_ids() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let initialize_id = json!(7);
    let initialize_message = json!({
        "jsonrpc": "2.0",
        "id": initialize_id.clone(),
        "method": "initialize",
        "params": {
            "processId": null,
            "clientInfo": {
                "name": "fragmented-client",
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
    client.send_raw_chunks(&[
        &frame[..first],
        &frame[first..second],
        &frame[second..],
    ])?;

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
    ensure!(
        numeric_response
            .get("result")
            .is_some_and(Value::is_null)
    );
    ensure!(
        string_response
            .get("result")
            .is_some_and(Value::is_null)
    );
    shutdown_and_exit(&mut client, json!(43))
}

#[test]
fn notification_is_not_answered() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;
    client.notify(
        "custom/unknownNotification",
        json!({ "marker": "no-response" }),
    )?;
    let barrier_id = json!("barrier");
    let barrier = client.request(
        barrier_id.clone(),
        "$/perl-lsp/watchdog",
        json!({}),
        timeout(),
    )?;
    assert_response_id(&barrier, &barrier_id)?;
    client.assert_no_response_pending()?;
    shutdown_and_exit(&mut client, json!(2))
}

#[test]
fn requests_after_shutdown_are_rejected() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;
    let shutdown =
        client.request(json!(2), "shutdown", Value::Null, timeout())?;
    ensure!(
        shutdown.get("result").is_some_and(Value::is_null),
        "shutdown failed: {shutdown}"
    );

    let request_id = json!("after-shutdown");
    let response = client.request(
        request_id.clone(),
        "$/perl-lsp/watchdog",
        json!({}),
        timeout(),
    )?;
    assert_response_id(&response, &request_id)?;
    ensure!(
        response.pointer("/error/code") == Some(&json!(-32600)),
        "expected InvalidRequest after shutdown: {response}"
    );
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn exit_without_shutdown_returns_status_one() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(!status.success(), "exit without shutdown succeeded");
    ensure!(
        status.code() == Some(1),
        "expected status 1, got {status}"
    );
    client.assert_transport_clean()
}

#[test]
fn strict_parser_rejects_truncated_and_oversized_headers() -> Result<()> {
    let truncated = b"Content-Length: 12\r\n";
    let error = RealProcessClient::parse_stdout_frame_for_test(truncated)
        .expect_err("truncated header must fail");
    ensure!(error.to_string().contains("header"));

    let oversized = vec![b'x'; 5 * 1024];
    let error = RealProcessClient::parse_stdout_frame_for_test(&oversized)
        .expect_err("oversized header must fail");
    ensure!(error.to_string().contains("exceeded"));
    Ok(())
}

#[test]
fn event_queue_overflow_does_not_deadlock_drop() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let mut requests = Vec::new();
    for id in 0..400 {
        requests.extend_from_slice(&RealProcessClient::encode_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "$/perl-lsp/watchdog",
            "params": {}
        })));
    }
    client.send_raw_bytes(&requests)?;

    let deadline = Instant::now() + timeout();
    while !client.event_queue_overflowed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    ensure!(client.event_queue_overflowed(), "queue did not overflow");

    let started = Instant::now();
    drop(client);
    ensure!(
        started.elapsed() < Duration::from_secs(2),
        "overflow cleanup blocked"
    );
    Ok(())
}

#[test]
fn terminal_check_rejects_unmatched_response() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;
    client.send_raw_bytes(&RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": "orphan-response",
        "method": "$/perl-lsp/watchdog",
        "params": {}
    })))?;

    let shutdown_id = json!(2);
    let shutdown = client.request(
        shutdown_id.clone(),
        "shutdown",
        Value::Null,
        timeout(),
    )?;
    assert_response_id(&shutdown, &shutdown_id)?;
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "candidate exit failed: {status}");

    let error = client
        .assert_transport_clean()
        .expect_err("unmatched response must fail terminal proof");
    ensure!(error.to_string().contains("unconsumed terminal message"));
    Ok(())
}
