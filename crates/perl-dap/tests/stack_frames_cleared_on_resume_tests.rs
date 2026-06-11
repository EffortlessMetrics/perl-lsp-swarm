//! Regression tests for issue #964: session.stack_frames must be cleared on every
//! resume path so that a stackTrace arriving before the next `stopped` event
//! does not serve stale frames from the previous stop.
//!
//! Run with: cargo test -p perl-dap --test stack_frames_cleared_on_resume_tests

use perl_dap::api::{TypesSource, TypesStackFrame};
use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;

// ─── helpers ────────────────────────────────────────────────────────────────

fn stale_frame() -> TypesStackFrame {
    TypesStackFrame::new(1, "main::before_resume", TypesSource::new("/tmp/stale.pl"), 42)
}

/// Seed a session and inject one stale frame; return an adapter ready for a
/// resume-command test.
fn adapter_with_stale_frame() -> DebugAdapter {
    let adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![stale_frame()]);
    assert_eq!(
        adapter.stack_frames_snapshot_for_test().len(),
        1,
        "precondition: stale frame must be present before resume"
    );
    adapter
}

// ─── continue ───────────────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_continue() {
    let mut adapter = adapter_with_stale_frame();

    let _response = adapter.handle_request(1, "continue", None);

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after continue — stale frames must not survive a resume"
    );
}

// ─── next (step over) ───────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_next() {
    let mut adapter = adapter_with_stale_frame();

    let _response = adapter.handle_request(1, "next", None);

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after next"
    );
}

// ─── stepIn ─────────────────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_step_in() {
    let mut adapter = adapter_with_stale_frame();

    let _response = adapter.handle_request(1, "stepIn", None);

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after stepIn"
    );
}

// ─── stepOut ────────────────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_step_out() {
    let mut adapter = adapter_with_stale_frame();

    let _response = adapter.handle_request(1, "stepOut", None);

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after stepOut"
    );
}

// ─── pause ──────────────────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_pause() {
    let mut adapter = adapter_with_stale_frame();

    let _response = adapter.handle_request(1, "pause", Some(json!({"threadId": 1})));

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after pause — SIGINT path must also clear the snapshot"
    );
}

// ─── goto ───────────────────────────────────────────────────────────────────

#[test]
fn stack_frames_cleared_on_goto() {
    let mut adapter = adapter_with_stale_frame();

    // Provide a stored goto target so the handler reaches the session block.
    adapter.inject_goto_target_for_test(1, "/tmp/goto_target.pl".to_string(), 10);

    let response = adapter.handle_request(1, "goto", Some(json!({"threadId": 1, "targetId": 1})));

    // The request succeeds (session has a live stdin from seed_session_for_test).
    match response {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "goto");
        }
        other => panic!("expected Response, got {other:?}"),
    }

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "stack_frames must be empty after goto"
    );
}

// ─── regression: multiple frames cleared ────────────────────────────────────

#[test]
fn multiple_stale_frames_all_cleared_on_continue() {
    let adapter = DebugAdapter::new();
    adapter.seed_session_for_test();
    adapter.inject_stack_frames_for_test(vec![
        TypesStackFrame::new(1, "main::outer", TypesSource::new("/tmp/a.pl"), 10),
        TypesStackFrame::new(2, "main::inner", TypesSource::new("/tmp/b.pl"), 5),
        TypesStackFrame::new(3, "main::leaf", TypesSource::new("/tmp/c.pl"), 1),
    ]);
    assert_eq!(
        adapter.stack_frames_snapshot_for_test().len(),
        3,
        "precondition: three stale frames"
    );

    let mut adapter = adapter;
    adapter.handle_request(1, "continue", None);

    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "all three stale frames must be cleared by continue"
    );
}

// ─── sanity: no session — accessor returns empty ─────────────────────────────

#[test]
fn no_session_snapshot_is_empty() {
    let adapter = DebugAdapter::new();
    // No seed — no session at all
    assert!(
        adapter.stack_frames_snapshot_for_test().is_empty(),
        "snapshot with no session must return empty vec"
    );
}
