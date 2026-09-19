//! Integration tests for handle_pause session-presence vs signal-delivery
//! vs thread-identity separation.
//!
//! Regression for #898 follow-up: `handle_pause` must distinguish "no session"
//! (actionable guidance message) from "session present but signal delivery failed"
//! (accurate "Failed to pause debugger" message).
//!
//! Since #8294 every thread-scoped request must also name the live synthetic
//! execution context (`threadId`); a request that omits or mismatches it is
//! rejected before any signal is delivered. The session-present tests here name
//! the attached-PID identity so the request reaches the pause failure path
//! (on Windows, the explicit PID-attached unsupported response) instead of
//! being rejected by identity. Session-present fixtures must not call
//! `handle_request` with `None` (#14516).
//!
//! Run with: cargo test -p perl-dap --features test-helpers --test pause_signal_delivery_tests

use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::{must_some_with, must_with};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

/// PID 0 is the test-only fail-closed sentinel: `send_interrupt_signal`
/// rejects it before reaching the OS, so a matching `threadId` cannot signal
/// an unrelated same-user process while a session remains present.
const FAIL_CLOSED_ATTACHED_PID: u32 = 0;

fn adapter_with_fail_closed_pid() -> DebugAdapter {
    let adapter = DebugAdapter::new();
    adapter.seed_attached_pid_for_test(FAIL_CLOSED_ATTACHED_PID);
    adapter
}

fn pause(adapter: &mut DebugAdapter, args: Option<Value>) -> (bool, Option<String>) {
    match adapter.handle_request(1, "pause", args) {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "pause");
            (success, message)
        }
        other => must_with(
            Err::<(bool, Option<String>), _>(format!("{other:?}")),
            "expected DapMessage::Response for pause",
        ),
    }
}

fn pause_failure_message(adapter: &mut DebugAdapter, args: Option<Value>) -> String {
    let (success, message) = pause(adapter, args);
    assert!(!success, "pause must fail, got success with {message:?}");
    must_some_with(message, "failed pause must include a message")
}

fn matching_thread_id_args() -> Option<Value> {
    Some(json!({ "threadId": FAIL_CLOSED_ATTACHED_PID }))
}

fn assert_no_session_guidance(msg: &str) {
    assert!(
        msg.contains("no Perl debug session is active"),
        "no-session pause must contain the guidance message, got: {msg}"
    );
    assert!(
        !msg.contains("Failed to pause debugger"),
        "no-session pause must NOT say 'Failed to pause debugger', got: {msg}"
    );
    assert!(
        !msg.contains("does not name the live execution context"),
        "no-session pause must NOT be an identity rejection, got: {msg}"
    );
}

fn assert_identity_rejection(msg: &str, detail: &str) {
    assert!(
        msg.contains("does not name the live execution context"),
        "identity rejection must name the live-context contract, got: {msg}"
    );
    assert!(msg.contains(detail), "identity rejection must include '{detail}', got: {msg}");
    assert!(
        !msg.contains("no Perl debug session is active"),
        "identity rejection must NOT produce the no-session guidance, got: {msg}"
    );
    assert!(
        !msg.contains("Failed to pause debugger"),
        "identity rejection must NOT be reported as signal failure, got: {msg}"
    );
    assert!(
        !msg.contains("unsupported"),
        "identity rejection must NOT reach the Windows PID-attached unsupported path, got: {msg}"
    );
}

fn assert_signal_delivery_failure(msg: &str) {
    assert!(
        !msg.contains("no Perl debug session is active"),
        "signal-delivery failure must NOT produce the no-session guidance, got: {msg}"
    );
    assert!(
        !msg.contains("does not name the live execution context"),
        "signal-delivery failure must NOT be an identity rejection, got: {msg}"
    );
    #[cfg(windows)]
    let expected_message = "Pause is unsupported for PID-attached sessions on Windows";
    #[cfg(not(windows))]
    let expected_message = "Failed to pause debugger";
    assert!(
        msg.contains(expected_message),
        "signal-delivery failure must say '{expected_message}', got: {msg}"
    );
}

/// Keep Windows process control from regressing to arbitrary console-group
/// signaling. The Win32 console-event API takes a process-group ID, not an
/// arbitrary process ID.
#[test]
fn test_windows_process_control_source_policy() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("debug_adapter")
        .join("process.rs");
    let source = fs::read_to_string(source_path)?;

    if source.contains("GenerateConsoleCtrlEvent") {
        return Err("process control must not call GenerateConsoleCtrlEvent".into());
    }
    if source.contains("CTRL_C_EVENT") {
        return Err("process control must not use CTRL_C_EVENT".into());
    }
    Ok(())
}

/// No active session → "no Perl debug session is active" guidance message.
///
/// `None` is correct here: nothing is live, so `#8294` does not require a
/// `threadId`. This is not a session-present fixture.
#[test]
fn test_pause_no_session_returns_guidance_message() {
    let mut adapter = DebugAdapter::new();
    let msg = pause_failure_message(&mut adapter, None);
    assert_no_session_guidance(&msg);
}

/// Opposite-direction control: supplying a `threadId` when nothing is live
/// must still be the no-session guidance, not an identity rejection.
/// A validator that rejects ids before checking liveness would fail this.
#[test]
fn test_pause_no_session_with_thread_id_still_returns_guidance() {
    let mut adapter = DebugAdapter::new();
    for args in
        [matching_thread_id_args(), Some(json!({ "threadId": 1 })), Some(json!({ "threadId": -1 }))]
    {
        let msg = pause_failure_message(&mut adapter, args);
        assert_no_session_guidance(&msg);
    }
}

/// Session present (attached pid) + signal delivery fails → "Failed to pause debugger"
/// (NOT the no-session guidance message).
///
/// This is the key regression test for #898 follow-up: before the fix, a signal
/// delivery failure on an active session caused the misleading "no Perl debug session
/// is active" guidance to fire, which is factually wrong when a session exists.
#[test]
fn test_pause_session_present_signal_failure_returns_accurate_error() {
    let mut adapter = adapter_with_fail_closed_pid();

    // #8294 / #14516: the attached PID is the live synthetic execution context
    // id, so the request must name it; otherwise the identity rejection fires
    // before signal delivery and this test would no longer exercise the
    // signal-failure path.
    let msg = pause_failure_message(&mut adapter, matching_thread_id_args());
    assert_signal_delivery_failure(&msg);
}

/// Extra well-formed fields must not divert a matching `threadId` into
/// identity rejection. Guards an over-strict pause argument parser.
#[test]
fn test_pause_matching_thread_id_with_extra_fields_still_reaches_signal_failure() {
    let mut adapter = adapter_with_fail_closed_pid();
    let msg = pause_failure_message(
        &mut adapter,
        Some(json!({ "threadId": FAIL_CLOSED_ATTACHED_PID, "singleThread": true })),
    );
    assert_signal_delivery_failure(&msg);
}

/// #14516 / #8294: a session-present pause that omits `threadId` is an
/// identity rejection. It must not look like no-session guidance (the
/// original stale-fixture failure mode) and must not look like signal
/// failure (which would mean the request reached `send_interrupt_signal`).
#[test]
fn test_pause_session_present_missing_thread_id_is_identity_rejection() {
    let mut adapter = adapter_with_fail_closed_pid();
    for (args, label) in [
        (None, "bare None"),
        (Some(json!({})), "empty object"),
        (Some(json!({ "threadId": null })), "null threadId"),
        (Some(json!({ "threadId": "0" })), "string threadId"),
        (Some(json!({ "threadId": 0.5 })), "non-integer threadId"),
    ] {
        let msg = pause_failure_message(&mut adapter, args);
        assert_identity_rejection(&msg, "missing `threadId`");
        assert!(
            msg.contains("the live synthetic execution context is 0"),
            "{label}: missing-threadId rejection must name the live attached-PID identity, got: {msg}"
        );
    }
}

/// A live attached-PID identity rejects a foreign, negative, or out-of-range
/// `threadId` before any signal is delivered.
#[test]
fn test_pause_session_present_mismatched_thread_id_is_identity_rejection() {
    let mut adapter = adapter_with_fail_closed_pid();

    let stale = pause_failure_message(&mut adapter, Some(json!({ "threadId": 99 })));
    assert_identity_rejection(&stale, "unknown or stale `threadId` 99");

    let negative = pause_failure_message(&mut adapter, Some(json!({ "threadId": -5 })));
    assert_identity_rejection(&negative, "negative `threadId` -5");

    let overflow =
        pause_failure_message(&mut adapter, Some(json!({ "threadId": i64::from(i32::MAX) + 1 })));
    assert_identity_rejection(&overflow, "`threadId` 2147483648 is out of range");
}

/// PID-attached pause must not signal the adapter's own Windows console.
///
/// Windows `GenerateConsoleCtrlEvent` takes a process-group ID, not an
/// arbitrary PID. Using this test process as the attached target makes the
/// regression observable: the test must remain alive and receive an explicit
/// unsupported response instead of a console control event.
#[test]
#[cfg(windows)]
fn test_pause_pid_attach_is_unsupported_without_signaling_parent()
-> Result<(), Box<dyn std::error::Error>> {
    let parent_pid = std::process::id();
    let mut adapter = DebugAdapter::new();
    adapter.seed_attached_pid_for_test(parent_pid);

    // #8294: name the attached-PID identity so the request passes validation and
    // reaches the Windows PID-attached pause path instead of the identity
    // rejection.
    let (success, message) = pause(&mut adapter, Some(json!({ "threadId": parent_pid })));
    if success {
        return Err("PID-attached pause must fail on Windows".into());
    }
    let message = message.ok_or("unsupported pause must include a message")?;
    if !message.contains("unsupported") || !message.contains("Windows") {
        return Err(format!("expected explicit Windows unsupported message, got: {message}").into());
    }
    if std::process::id() != parent_pid {
        return Err("test process identity changed unexpectedly".into());
    }
    Ok(())
}
