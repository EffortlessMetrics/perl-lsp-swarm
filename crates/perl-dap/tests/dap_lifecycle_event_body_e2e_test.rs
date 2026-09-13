//! End-to-end content tests for DAP lifecycle event bodies (`terminate`,
//! `disconnect`, `restart`).
//!
//! Existing `session_lifecycle_tests.rs` covers exactly one `terminate` shape:
//! `{ "restart": false }`. This file fills the matrix and locks down the
//! `terminated` event body contract for the other inputs an IDE will send:
//!
//! - `terminate` with `restart: true`: event body must echo `restart: true`
//! - `terminate` with no `restart` arg: event body must NOT include `restart`
//! - `terminate` with empty args (`{}`): event body must NOT include `restart`
//! - `terminate` twice in succession: both calls succeed and emit events
//! - `terminate` then `disconnect`: disconnect succeeds without duplicating
//!   the already-emitted terminal event
//! - `disconnect` (no session): succeeds without inventing a terminal event
//! - `restart` request (no session): fails cleanly per `unsupported` handler
//!
//! All tests are protocol-level: no `perl` process is spawned.

use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use perl_tdd_support::must_some;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn create_test_adapter() -> (DebugAdapter, Receiver<DapMessage>) {
    let (tx, rx) = sync_channel(64);
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    (adapter, rx)
}

fn wait_for_event(rx: &Receiver<DapMessage>, name: &str, timeout_ms: u64) -> Option<Value> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(DapMessage::Event { event, body, .. }) if event == name => {
                return Some(body.unwrap_or(Value::Null));
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    None
}

fn assert_response_success(response: &DapMessage, expected_command: &str) -> TestResult {
    let DapMessage::Response { success, command, message, .. } = response else {
        return Err(format!("expected Response for {expected_command}, got {response:?}").into());
    };
    assert!(*success, "expected success for {expected_command}, got error: {message:?}");
    assert_eq!(command, expected_command, "command field mismatch");
    Ok(())
}

#[test]
fn terminate_with_restart_true_echoes_flag_in_event_body() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    let response = adapter.handle_request(1, "terminate", Some(json!({ "restart": true })));
    assert_response_success(&response, "terminate")?;

    let body = must_some(wait_for_event(&rx, "terminated", 200));
    let restart = body.get("restart").and_then(Value::as_bool);
    assert_eq!(
        restart,
        Some(true),
        "terminate({{restart: true}}) must emit terminated event with restart=true, got body={body}"
    );

    Ok(())
}

#[test]
fn terminate_with_no_arguments_omits_restart_field() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    let response = adapter.handle_request(1, "terminate", None);
    assert_response_success(&response, "terminate")?;

    let body = must_some(wait_for_event(&rx, "terminated", 200));
    // Per debug_adapter::process::handle_terminate: when no restart arg was
    // supplied, the event body must NOT include a restart field. The body may
    // be `null` (no body) or an object without the `restart` key.
    let has_restart = body.get("restart").is_some();
    assert!(
        !has_restart,
        "terminate() without restart arg must NOT include restart field, got body={body}"
    );

    Ok(())
}

#[test]
fn terminate_with_empty_arguments_omits_restart_field() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    let response = adapter.handle_request(1, "terminate", Some(json!({})));
    assert_response_success(&response, "terminate")?;

    let body = must_some(wait_for_event(&rx, "terminated", 200));
    let has_restart = body.get("restart").is_some();
    assert!(
        !has_restart,
        "terminate({{}}) without restart arg must NOT include restart field, got body={body}"
    );

    Ok(())
}

#[test]
fn terminate_twice_in_succession_both_succeed_and_emit_events() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    // First terminate
    let first = adapter.handle_request(1, "terminate", Some(json!({ "restart": false })));
    assert_response_success(&first, "terminate")?;
    let first_body = must_some(wait_for_event(&rx, "terminated", 200));
    assert_eq!(
        first_body.get("restart").and_then(Value::as_bool),
        Some(false),
        "first terminate must echo restart=false"
    );

    // Second terminate must also succeed; adapter is idempotent.
    let second = adapter.handle_request(2, "terminate", Some(json!({ "restart": true })));
    assert_response_success(&second, "terminate")?;
    let second_body = must_some(wait_for_event(&rx, "terminated", 200));
    assert_eq!(
        second_body.get("restart").and_then(Value::as_bool),
        Some(true),
        "second terminate must echo restart=true, independent of first"
    );

    Ok(())
}

#[test]
fn terminate_then_disconnect_does_not_duplicate_terminated_event() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    // First terminal request: terminate.
    let first = adapter.handle_request(1, "terminate", Some(json!({ "restart": false })));
    assert_response_success(&first, "terminate")?;
    let first_body = must_some(wait_for_event(&rx, "terminated", 200));
    assert_eq!(
        first_body.get("restart").and_then(Value::as_bool),
        Some(false),
        "terminate must echo restart=false"
    );

    // VS Code may follow terminate with disconnect. The session is already
    // closed, so disconnect must succeed without inventing a second event.
    let second = adapter.handle_request(2, "disconnect", None);
    assert_response_success(&second, "disconnect")?;
    match rx.try_recv() {
        Err(TryRecvError::Empty) => {}
        Err(error) => return Err(format!("unexpected event receiver state: {error:?}").into()),
        Ok(message) => {
            return Err(format!("disconnect duplicated terminal event: {message:?}").into());
        }
    }

    Ok(())
}

#[test]
fn disconnect_without_session_does_not_emit_terminated_event() -> TestResult {
    let (mut adapter, rx) = create_test_adapter();

    let response = adapter.handle_request(1, "disconnect", None);
    assert_response_success(&response, "disconnect")?;

    // No debugging session ended, so disconnect must not invent a terminated event.
    match rx.try_recv() {
        Err(TryRecvError::Empty) => {}
        Err(error) => return Err(format!("unexpected event receiver state: {error:?}").into()),
        Ok(message) => {
            return Err(format!("no-session disconnect emitted an event: {message:?}").into());
        }
    }

    Ok(())
}

#[test]
fn restart_request_without_session_fails_cleanly() -> TestResult {
    let (mut adapter, _rx) = create_test_adapter();

    // #9581: `restart` is a floored secondary capability. The dispatch gate
    // rejects it before any teardown/spawn/state work, with an explicit
    // unsupported message, rather than panicking or masquerading.
    let response = adapter.handle_request(1, "restart", None);
    let DapMessage::Response { success, command, message, .. } = &response else {
        return Err(format!("expected Response for restart, got {response:?}").into());
    };
    assert!(!*success, "floored restart must fail explicitly (#9581)");
    assert_eq!(command, "restart");
    assert!(message.is_some(), "restart failure must carry a message for the IDE");

    Ok(())
}
