//! Integration tests for handle_pause session-presence vs signal-delivery separation.
//!
//! Regression for #898 follow-up: `handle_pause` must distinguish "no session"
//! (actionable guidance message) from "session present but signal delivery failed"
//! (accurate "Failed to pause debugger" message).
//!
//! Run with: cargo test -p perl-dap --test pause_signal_delivery_tests

use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::{must, must_some};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

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
/// This is the CORRECT behavior: when there is truly no session, the user needs
/// actionable guidance to start one.
#[test]
fn test_pause_no_session_returns_guidance_message() {
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "pause", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "pause without session must fail");
            assert_eq!(command, "pause");
            let msg = must_some(message);
            assert!(
                msg.contains("no Perl debug session is active"),
                "no-session pause must contain the guidance message, got: {msg}"
            );
            assert!(
                !msg.contains("Failed to pause debugger"),
                "no-session pause must NOT say 'Failed to pause debugger', got: {msg}"
            );
        }
        _ => {
            must(Err::<(), _>("Expected DapMessage::Response for pause with no session"));
            unreachable!()
        }
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
    let mut adapter = DebugAdapter::new();
    // PID 0 is a test-only fail-closed sentinel: send_interrupt_signal rejects
    // it before reaching the OS, so this test cannot signal an unrelated
    // same-user process while a session remains present.
    let attached_pid = 0;
    adapter.seed_attached_pid_for_test(attached_pid);

    let response = adapter.handle_request(1, "pause", Some(json!({"threadId": attached_pid})));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "pause with signal delivery failure must fail");
            assert_eq!(command, "pause");
            let msg = must_some(message);
            // Must NOT fire the no-session guidance (the session IS present).
            assert!(
                !msg.contains("no Perl debug session is active"),
                "signal-delivery failure must NOT produce the no-session guidance, got: {msg}"
            );
            // Must produce the accurate platform-specific signal-failure message.
            #[cfg(windows)]
            let expected_message = "Pause is unsupported for PID-attached sessions on Windows";
            #[cfg(not(windows))]
            let expected_message = "Failed to pause debugger";
            assert!(
                msg.contains(expected_message),
                "signal-delivery failure must say '{expected_message}', got: {msg}"
            );
        }
        _ => {
            must(Err::<(), _>(
                "Expected DapMessage::Response for pause with signal delivery failure",
            ));
            unreachable!()
        }
    }
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

    let response = adapter.handle_request(1, "pause", Some(json!({"threadId": parent_pid})));
    let DapMessage::Response { success, message, .. } = response else {
        return Err("expected a response for PID-attached pause".into());
    };
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
