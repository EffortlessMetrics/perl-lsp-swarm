//! Required real-process contract for the exact Cargo-built `perl-lsp` binary.
//!
//! These tests deliberately do not use PATH discovery, target-directory
//! probing, `cargo run`, or an in-process `LspServer`. They exercise the shipped
//! stdio loop and fail if Cargo's exact candidate is absent.

mod support;

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;
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
    ensure!(response.get("jsonrpc") == Some(&json!("2.0")), "missing JSON-RPC version: {response}");
    ensure!(response.get("id") == Some(expected), "response ID mismatch: {response}");
    Ok(())
}

fn initialize_and_notify(client: &mut RealProcessClient, id: Value) -> Result<Value> {
    let response = initialize(client, id.clone())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize did not return server capabilities: {response}"
    );
    client.notify("initialized", json!({}))?;
    Ok(response)
}

fn shutdown_and_exit(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response = client.request(id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("result").is_some_and(Value::is_null), "shutdown must return null: {response}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "clean shutdown exited with {status}; stderr={}", client.stderr_tail());
    client.assert_transport_clean()
}

#[test]
fn exact_candidate_completes_legal_lifecycle() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    ensure!(client.candidate_path().is_file(), "exact candidate path disappeared");

    initialize_and_notify(&mut client, json!("initialize-1"))?;
    shutdown_and_exit(&mut client, json!("shutdown-1"))
}

#[test]
fn request_before_initialize_returns_server_not_initialized_and_preserves_string_id() -> Result<()> {
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
    ensure!(response.pointer("/error/code") == Some(&json!(-32002)), "expected ServerNotInitialized: {response}");

    initialize_and_notify(&mut client, json!(2))?;
    shutdown_and_exit(&mut client, json!(3))
}

#[test]
fn numeric_and_string_ids_round_trip_over_fragmented_and_coalesced_frames() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;

    let initialize_id = json!(7);
    let initialize_message = json!({
        "jsonrpc": "2.0",
        "id": initialize_id,
        "method": "initialize",
        "params": {
            "processId": null,
            "clientInfo": { "name": "fragmented-client", "version": "1" },
            "rootUri": null,
            "capabilities": {},
            "workspaceFolders": null
        }
    });
    let initialize_frame = RealProcessClient::encode_message(&initialize_message);
    let first_split = initialize_frame.len().min(9);
    let second_split = initialize_frame.len().min(first_split.saturating_add(17));
    client.send_raw_chunks(&[
        &initialize_frame[..first_split],
        &initialize_frame[first_split..second_split],
        &initialize_frame[second_split..],
    ])?;

    let initialize_response = client.receive_response(&initialize_id, timeout())?;
    assert_response_id(&initialize_response, &initialize_id)?;
    ensure!(initialize_response.pointer("/result/capabilities").is_some());
    client.notify("initialized", json!({}))?;

    let numeric_id = json!(42);
    let string_id = json!("utf8-✓");
    let numeric_frame = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": numeric_id,
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "first" }
    }));
    let string_frame = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": string_id,
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "π and ✓ prove byte-length framing" }
    }));
    let mut coalesced = numeric_frame;
    coalesced.extend_from_slice(&string_frame);
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
fn notification_is_not_answered_and_a_later_request_forms_an_ordered_barrier() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;

    client.notify("custom/unknownNotification", json!({ "marker": "no-response" }))?;
    let barrier_id = json!("barrier");
    let barrier = client.request(
        barrier_id.clone(),
        "$/perl-lsp/watchdog",
        json!({}),
        timeout(),
    )?;
    assert_response_id(&barrier, &barrier_id)?;
    client.assert_no_null_id_response_pending()?;

    shutdown_and_exit(&mut client, json!(2))
}

#[test]
fn requests_after_shutdown_are_rejected_before_clean_exit() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;

    let shutdown = client.request(json!(2), "shutdown", Value::Null, timeout())?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null), "shutdown failed: {shutdown}");

    let after_shutdown_id = json!("after-shutdown");
    let after_shutdown = client.request(
        after_shutdown_id.clone(),
        "$/perl-lsp/watchdog",
        json!({}),
        timeout(),
    )?;
    assert_response_id(&after_shutdown, &after_shutdown_id)?;
    ensure!(
        after_shutdown.pointer("/error/code") == Some(&json!(-32600)),
        "request after shutdown must return InvalidRequest: {after_shutdown}"
    );

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown followed by exit should succeed: {status}");
    client.assert_transport_clean()
}

#[test]
fn exit_without_shutdown_returns_failure_status() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(!status.success(), "exit without shutdown must fail, got {status}");
    ensure!(status.code() == Some(1), "exit without shutdown must use status 1, got {status}");
    client.assert_transport_clean()
}
