//! Regression tests for issue #964: session.stack_frames must be cleared on resume.
//!
//! All six resume-path handlers (continue, next, stepIn, stepOut, pause, goto) must
//! clear `session.stack_frames` so that a `stackTrace` request arriving between resume
//! and the next `stopped` event returns an empty list rather than the stale snapshot
//! from the previous stop.
//!
//! Tests use `seed_session_for_test` / `inject_stack_frames_for_test` /
//! `stack_frames_snapshot_for_test` helpers (cfg(test) only) to exercise the path
//! without requiring a live `perl -d` session.

use perl_dap::debug_adapter::DebugAdapter;
use perl_dap::types::{Source, StackFrame};
use serde_json::json;

/// Build a stale StackFrame as if it were left over from a prior stop.
fn stale_frame() -> StackFrame {
    StackFrame {
        id: 1,
        name: "main::foo".to_string(),
        source: Source {
            name: Some("foo.pl".to_string()),
            path: "/tmp/foo.pl".to_string(),
            source_reference: None,
        },
        line: 42,
        column: 1,
        end_line: None,
        end_column: None,
    }
}

/// Precondition helper: assert a stale frame is present before the resume command.
fn assert_stale_frame_present(adapter: &DebugAdapter, label: &str) {
    assert_eq!(
        adapter.stack_frames_snapshot_for_test().len(),
        1,
        "precondition failed for {label}: expected 1 stale frame before resume"
    );
}

/// Postcondition helper: assert frames are cleared after the resume command.
fn assert_frames_cleared(adapter: &DebugAdapter, label: &str) {
    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be cleared after {label}"
    );
}

/// Skip the test when `perl` is not on PATH (seed_session_for_test spawns `perl -e 1`).
fn perl_available() -> bool {
    std::process::Command::new("perl").arg("-e").arg("1").output().is_ok()
}

#[test]
fn test_stack_frames_cleared_on_continue() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "continue");

    adapter.handle_request(1, "continue", None);

    assert_frames_cleared(&adapter, "continue");
}

#[test]
fn test_stack_frames_cleared_on_next() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "next");

    adapter.handle_request(1, "next", Some(json!({"threadId": 1})));

    assert_frames_cleared(&adapter, "next");
}

#[test]
fn test_stack_frames_cleared_on_step_in() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "stepIn");

    adapter.handle_request(1, "stepIn", Some(json!({"threadId": 1})));

    assert_frames_cleared(&adapter, "stepIn");
}

#[test]
fn test_stack_frames_cleared_on_step_out() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "stepOut");

    adapter.handle_request(1, "stepOut", Some(json!({"threadId": 1})));

    assert_frames_cleared(&adapter, "stepOut");
}

/// Test that goto with a valid registered target clears stack_frames.
///
/// This exercises the session block in handle_goto where `session.stack_frames.clear()`
/// is the changed production line.  Without `register_goto_target_for_test` the handler
/// returns early at "Unknown goto target id", never reaching the clear.
#[test]
fn test_stack_frames_cleared_on_goto_with_valid_target() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "goto");

    // Register a target so the handler passes the "Unknown goto target" guard
    // and reaches the session block where stack_frames.clear() is called.
    adapter.register_goto_target_for_test(1, "/tmp/test.pl", 10);
    adapter.handle_request(1, "goto", Some(json!({"threadId": 1, "targetId": 1})));

    // The goto handler clears stack_frames when it reaches the session block.
    assert_frames_cleared(&adapter, "goto");
}

/// Test that goto with an invalid targetId does not panic and does not corrupt state.
#[test]
fn test_goto_with_invalid_target_no_panic() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);

    // Unregistered targetId — handler returns error before reaching session block.
    let response = adapter.handle_request(1, "goto", Some(json!({"threadId": 1, "targetId": 9999})));
    // Must not panic; must return a Response.
    assert!(
        matches!(response, perl_dap::debug_adapter::DapMessage::Response { .. }),
        "expected Response for invalid targetId"
    );
}

#[test]
fn test_stack_frames_cleared_on_pause() {
    if !perl_available() {
        return;
    }
    let mut adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_stale_frame_present(&adapter, "pause");

    // pause sends SIGINT; it clears stack_frames regardless of whether the signal succeeds
    adapter.handle_request(1, "pause", None);

    assert_frames_cleared(&adapter, "pause");
}
