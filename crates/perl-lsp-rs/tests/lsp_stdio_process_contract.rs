//! Required real-process contract for Cargo's exact `perl-lsp` binary.
//!
//! These tests do not use PATH discovery, target-directory probing, `cargo
//! run`, or an in-process `LspServer`. The required `lsp_smoke` gate includes
//! this file from `semantic_definition.rs`, so these cases are merge-blocking.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

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
        "initialize omitted capabilities: {response}"
    );
    client.notify("initialized", json!({}))?;
    Ok(response)
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
        "clean shutdown exited with {status}; stderr={}",
        client.stderr_tail()
    );
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
fn outbound_method_messages_omit_null_params_and_reject_scalars() -> Result<()> {
    let shutdown =
        RealProcessClient::method_message_for_test(Some(json!(1)), "shutdown", Value::Null)?;
    ensure!(
        shutdown
            == json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "shutdown"
            }),
        "shutdown wire shape included forbidden fields: {shutdown}"
    );

    let exit = RealProcessClient::method_message_for_test(None, "exit", Value::Null)?;
    ensure!(
        exit == json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
        "exit wire shape included forbidden fields: {exit}"
    );

    for scalar in [json!(1), json!(true), json!("invalid")] {
        ensure!(
            RealProcessClient::method_message_for_test(None, "notification", scalar).is_err(),
            "scalar params were accepted"
        );
    }
    Ok(())
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
fn serialized_notification_is_not_answered() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;

    // `$/setTrace` and `shutdown` share the exclusive lifecycle queue. Receiving
    // the shutdown response therefore proves the preceding notification was
    // processed; a response emitted for it would have been observed first and
    // retained as unmatched by `receive_response`.
    client.notify("$/setTrace", json!({ "value": "off" }))?;
    let shutdown_id = json!("serialized-barrier");
    let shutdown = client.request(shutdown_id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&shutdown, &shutdown_id)?;
    ensure!(
        shutdown.get("result").is_some_and(Value::is_null),
        "shutdown must return null: {shutdown}"
    );
    client.assert_no_response_pending()?;

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn requests_after_shutdown_are_rejected() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;
    let shutdown = client.request(json!(2), "shutdown", Value::Null, timeout())?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null), "shutdown failed: {shutdown}");

    let request_id = json!("after-shutdown");
    let response =
        client.request(request_id.clone(), "$/perl-lsp/watchdog", json!({}), timeout())?;
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
    ensure!(status.code() == Some(1), "expected status 1, got {status}");
    client.assert_transport_clean()
}

#[test]
fn strict_parser_rejects_truncated_oversized_and_lf_only_headers() -> Result<()> {
    let truncated = b"Content-Length: 12\r\n";
    let error = RealProcessClient::parse_stdout_frame_for_test(truncated)
        .expect_err("truncated header must fail");
    ensure!(error.to_string().contains("header"));

    let oversized = vec![b'x'; 5 * 1024];
    let error = RealProcessClient::parse_stdout_frame_for_test(&oversized)
        .expect_err("oversized header must fail");
    ensure!(error.to_string().contains("exceeded"));

    let body = br#"{"jsonrpc":"2.0","method":"window/logMessage"}"#;
    let mut frame = format!("Content-Length: {}\n\n", body.len()).into_bytes();
    frame.extend_from_slice(body);
    let error = RealProcessClient::parse_stdout_frame_for_test(&frame)
        .expect_err("LF-only framing must fail");
    ensure!(
        error.to_string().contains("CRLF"),
        "LF-only framing failed for the wrong reason: {error:#}"
    );
    Ok(())
}

#[test]
fn response_envelopes_reject_request_fields_and_malformed_errors() -> Result<()> {
    let valid_result = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": null
    });
    ensure!(RealProcessClient::is_valid_response_for_test(&valid_result));

    let valid_error = json!({
        "jsonrpc": "2.0",
        "id": "request",
        "error": { "code": -32603, "message": "failure" }
    });
    ensure!(RealProcessClient::is_valid_response_for_test(&valid_error));

    for invalid in [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": null,
            "params": {}
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": "-32603", "message": "wrong code type" }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32603, "message": 7 }
        }),
    ] {
        ensure!(
            !RealProcessClient::is_valid_response_for_test(&invalid),
            "invalid response was accepted: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn terminal_notification_requires_jsonrpc_2() -> Result<()> {
    let valid = json!({
        "jsonrpc": "2.0",
        "method": "window/logMessage",
        "params": { "type": 3, "message": "ok" }
    });
    ensure!(
        RealProcessClient::is_valid_server_notification_for_test(&valid),
        "valid JSON-RPC notification was rejected"
    );

    let missing_version = json!({
        "method": "window/logMessage",
        "params": { "type": 3, "message": "missing" }
    });
    ensure!(
        !RealProcessClient::is_valid_server_notification_for_test(&missing_version),
        "method-shaped object without jsonrpc was accepted"
    );

    let wrong_version = json!({
        "jsonrpc": "1.0",
        "method": "window/logMessage",
        "params": { "type": 3, "message": "wrong" }
    });
    ensure!(
        !RealProcessClient::is_valid_server_notification_for_test(&wrong_version),
        "wrong JSON-RPC version was accepted"
    );

    let scalar_params = json!({
        "jsonrpc": "2.0",
        "method": "window/logMessage",
        "params": "not-an-object-or-array"
    });
    ensure!(
        !RealProcessClient::is_valid_server_notification_for_test(&scalar_params),
        "notification with scalar params was accepted"
    );

    let result_hybrid = json!({
        "jsonrpc": "2.0",
        "method": "window/logMessage",
        "result": null
    });
    ensure!(
        !RealProcessClient::is_valid_server_notification_for_test(&result_hybrid),
        "method/response hybrid was accepted as a notification"
    );

    let error_hybrid = json!({
        "jsonrpc": "2.0",
        "method": "window/logMessage",
        "error": { "code": -32603, "message": "hybrid" }
    });
    ensure!(
        !RealProcessClient::is_valid_server_notification_for_test(&error_hybrid),
        "method/error hybrid was accepted as a notification"
    );

    Ok(())
}

#[test]
fn server_request_requires_request_only_members_and_structured_params() -> Result<()> {
    let method = "client/registerCapability";
    let valid = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": { "registrations": [] }
    });
    ensure!(
        RealProcessClient::is_valid_server_request_for_test(&valid, method),
        "valid server request was rejected"
    );

    for invalid in [
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": method,
            "params": true
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": method,
            "result": null
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": method,
            "error": { "code": -32603, "message": "hybrid" }
        }),
    ] {
        ensure!(
            !RealProcessClient::is_valid_server_request_for_test(&invalid, method),
            "invalid server request was accepted: {invalid}"
        );
    }
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
    ensure!(started.elapsed() < Duration::from_secs(2), "overflow cleanup blocked");
    Ok(())
}

#[test]
fn terminal_check_rejects_unmatched_response() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize_and_notify(&mut client, json!(1))?;

    let orphan_id = json!("orphan-response");
    client.send_raw_bytes(&RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": orphan_id.clone(),
        "method": "$/perl-lsp/watchdog",
        "params": {}
    })))?;
    let orphan = client.receive_response_and_retain(&orphan_id, timeout())?;
    assert_response_id(&orphan, &orphan_id)?;

    let shutdown_id = json!(2);
    let shutdown = client.request(shutdown_id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&shutdown, &shutdown_id)?;
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "candidate exit failed: {status}");

    let error =
        client.assert_transport_clean().expect_err("unmatched response must fail terminal proof");
    ensure!(error.to_string().contains("unconsumed terminal message"));
    Ok(())
}
