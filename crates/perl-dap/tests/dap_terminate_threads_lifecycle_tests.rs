//! Lifecycle tests for `terminateThreads` real termination and the
//! `restartFrame` capability-honesty fix.
//!
//! Background (#1663 Phase 0 / #1678): the adapter previously advertised
//! `supportsRestartFrame` and `supportsTerminateThreadsRequest` as `true`
//! while both handlers unconditionally returned `success: false` — a shipped
//! capability/handler contract violation (VS Code enables the UI affordance,
//! then the request errors).
//!
//! Resolution:
//! * `restartFrame` — `perl -d` genuinely cannot restart execution from a
//!   stack frame, so the capability is advertised `false` (honest).
//! * `terminateThreads` — the Perl native adapter exposes a single thread (the
//!   whole debuggee); terminating it terminates the program under debug. The
//!   capability stays `true` and the handler now honors it.

use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};

fn initialize_caps(adapter: &mut DebugAdapter) -> Value {
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { body: Some(body), success: true, .. } => body,
        other => panic!("expected successful initialize response with body, got {other:?}"),
    }
}

fn drain_event_names(rx: &Receiver<DapMessage>) -> Vec<String> {
    let mut events = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let DapMessage::Event { event, .. } = msg {
            events.push(event);
        }
    }
    events
}

#[test]
fn test_restart_frame_capability_advertised_false() {
    // perl -d cannot restart a stack frame, so the capability must be honest.
    let mut adapter = DebugAdapter::new();
    let caps = initialize_caps(&mut adapter);
    assert_eq!(
        caps.get("supportsRestartFrame").and_then(Value::as_bool),
        Some(false),
        "supportsRestartFrame must be false — the handler cannot honor restartFrame"
    );
}

#[test]
fn test_terminate_threads_capability_remains_advertised_true() {
    // terminateThreads IS implementable (terminate the debuggee), so the
    // capability stays advertised and the handler must honor it.
    let mut adapter = DebugAdapter::new();
    let caps = initialize_caps(&mut adapter);
    assert_eq!(
        caps.get("supportsTerminateThreadsRequest").and_then(Value::as_bool),
        Some(true),
        "supportsTerminateThreadsRequest must stay true — the handler now honors it"
    );
}

#[test]
fn test_terminate_threads_without_session_fails_with_guidance() {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "terminateThreads", Some(json!({"threadIds": [1]})));
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "terminateThreads must fail when no debug session is active");
            assert_eq!(command, "terminateThreads");
            let message = message.expect("failure must carry an actionable guidance message");
            assert!(
                message.contains("session"),
                "guidance should reference the missing debug session: {message}"
            );
        }
        other => panic!("expected Response, got {other:?}"),
    }
}

#[test]
fn test_terminate_threads_with_active_session_terminates_debuggee() {
    let (tx, rx) = channel();
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    adapter.seed_running_session_for_test();

    let response = adapter.handle_request(2, "terminateThreads", Some(json!({"threadIds": [1]})));
    match response {
        DapMessage::Response { success, command, body, .. } => {
            assert!(success, "terminateThreads with an active session must succeed");
            assert_eq!(command, "terminateThreads");
            assert!(body.is_none(), "terminateThreads response carries no body");
        }
        other => panic!("expected Response, got {other:?}"),
    }

    let events = drain_event_names(&rx);
    assert!(
        events.iter().any(|e| e == "terminated"),
        "terminating the debuggee must emit a `terminated` event, got {events:?}"
    );

    // The session must be cleared: a follow-up `threads` request reports none.
    match adapter.handle_request(3, "threads", None) {
        DapMessage::Response { body: Some(body), .. } => {
            let threads =
                body.get("threads").and_then(Value::as_array).cloned().unwrap_or_default();
            assert!(
                threads.is_empty(),
                "after terminateThreads the session must be cleared, got {threads:?}"
            );
        }
        other => panic!("expected threads response with body, got {other:?}"),
    }
}

#[test]
fn test_terminate_threads_empty_ids_terminates_debuggee() {
    // An empty (or omitted) threadIds list means "terminate all threads"; for
    // the single-threaded Perl model that is the whole debuggee.
    let (tx, rx) = channel();
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    adapter.seed_running_session_for_test();

    let response = adapter.handle_request(2, "terminateThreads", Some(json!({"threadIds": []})));
    assert!(
        matches!(response, DapMessage::Response { success: true, .. }),
        "empty threadIds must terminate the single Perl thread and succeed"
    );
    assert!(
        drain_event_names(&rx).iter().any(|e| e == "terminated"),
        "empty threadIds termination must emit a `terminated` event"
    );
}

#[test]
fn test_terminate_threads_unrelated_thread_id_is_noop_success() {
    // Requesting termination of a thread the adapter does not manage must not
    // tear down the active debuggee; it is a successful no-op.
    let (tx, rx) = channel();
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    adapter.seed_running_session_for_test();

    let response =
        adapter.handle_request(2, "terminateThreads", Some(json!({"threadIds": [4242]})));
    assert!(
        matches!(response, DapMessage::Response { success: true, .. }),
        "terminating an unmanaged thread id is a successful no-op"
    );
    assert!(
        !drain_event_names(&rx).iter().any(|e| e == "terminated"),
        "an unmanaged thread id must not terminate the active debuggee"
    );

    // The session is still active.
    match adapter.handle_request(3, "threads", None) {
        DapMessage::Response { body: Some(body), .. } => {
            let threads =
                body.get("threads").and_then(Value::as_array).cloned().unwrap_or_default();
            assert_eq!(threads.len(), 1, "the active session must remain after a no-op terminate");
        }
        other => panic!("expected threads response with body, got {other:?}"),
    }
}
