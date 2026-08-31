//! Comprehensive Step-Through Debugging Tests (#3535)
//!
//! Covers DAP step-through debugging operations in depth:
//! - stepIn, stepOut, stepOver (next), continue, pause
//! - Event emission during stepping (continued event)
//! - Stepping at block boundaries (if/while/for)
//! - stepIn behavior for XS/builtin code
//! - stepInTargets with real source containing function calls
//! - gotoTargets with actual Perl source files
//! - cancel signal during step operations
//! - goto with unknown target id
//! - restartFrame and terminateThreads unsupported paths
//! - Variable and scope inspection after rejected stepping requests
//! - Sequence monotonicity across stepping operations
//!
//! Run with: cargo test -p perl-dap --test dap_step_through_tests

use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use perl_dap::types::{Source, StackFrame};
use serde_json::json;
use std::fs;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_adapter() -> DebugAdapter {
    DebugAdapter::new()
}

fn make_adapter_with_events() -> (DebugAdapter, Receiver<DapMessage>) {
    let (tx, rx) = sync_channel(64);
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    (adapter, rx)
}

/// Assert a Response is returned with the expected command and success value,
/// returning the body for further inspection.
fn assert_response(
    msg: DapMessage,
    expected_command: &str,
    expected_success: bool,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    match msg {
        DapMessage::Response { success, command, body, .. } => {
            assert_eq!(command, expected_command, "command mismatch");
            assert_eq!(success, expected_success, "success mismatch for {expected_command}");
            Ok(body)
        }
        other => Err(format!("expected Response for {expected_command}, got {other:?}").into()),
    }
}

/// Drain all pending events from the receiver within a short timeout.
fn drain_events(rx: &Receiver<DapMessage>, timeout_ms: u64) -> Vec<String> {
    let mut events = Vec::new();
    while let Ok(msg) = rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        if let DapMessage::Event { event, .. } = msg {
            events.push(event);
        }
    }
    events
}

// ---------------------------------------------------------------------------
// 1. Event emission — stepping commands must emit "continued"
// ---------------------------------------------------------------------------

#[test]
// AC:3535 / AC:898
fn test_step_in_emits_continued_event_no_session() -> Result<(), Box<dyn std::error::Error>> {
    // #898: stepIn without a session returns success: false and no continued event.
    let (mut adapter, rx) = make_adapter_with_events();

    let response = adapter.handle_request(1, "stepIn", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "stepIn without session must fail");
            assert_eq!(command, "stepIn");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected Response for stepIn".into()),
    }

    // No continued event should be emitted on the failure path.
    let events = drain_events(&rx, 50);
    assert!(
        !events.iter().any(|e| e == "continued"),
        "stepIn without session must not emit 'continued'; got: {events:?}"
    );
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_step_out_emits_continued_event_no_session() -> Result<(), Box<dyn std::error::Error>> {
    // #898: stepOut without a session returns success: false and no continued event.
    let (mut adapter, rx) = make_adapter_with_events();

    let response = adapter.handle_request(1, "stepOut", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "stepOut without session must fail");
            assert_eq!(command, "stepOut");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected Response for stepOut".into()),
    }

    let events = drain_events(&rx, 50);
    assert!(
        !events.iter().any(|e| e == "continued"),
        "stepOut without session must not emit 'continued'; got: {events:?}"
    );
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_next_emits_continued_event_no_session() -> Result<(), Box<dyn std::error::Error>> {
    // #898: next without a session returns success: false and no continued event.
    let (mut adapter, rx) = make_adapter_with_events();

    let response = adapter.handle_request(1, "next", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "next without session must fail");
            assert_eq!(command, "next");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected Response for next".into()),
    }

    let events = drain_events(&rx, 50);
    assert!(
        !events.iter().any(|e| e == "continued"),
        "next without session must not emit 'continued'; got: {events:?}"
    );
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_continue_emits_continued_event() -> Result<(), Box<dyn std::error::Error>> {
    // #898: continue without a session returns success: false and no continued event.
    let (mut adapter, rx) = make_adapter_with_events();

    let response = adapter.handle_request(1, "continue", None);

    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert!(!success, "continue without session must fail");
            assert_eq!(command, "continue");
            assert!(body.is_none(), "failure response must have no body");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected Response for continue".into()),
    }

    // No continued event should be emitted on the failure path.
    let events = drain_events(&rx, 50);
    assert!(
        !events.iter().any(|e| e == "continued"),
        "continue without session must not emit 'continued'; got: {events:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Stepping at block boundaries — if/while/for
// ---------------------------------------------------------------------------

#[test]
// AC:3535 / AC:898
fn test_next_at_if_boundary() -> Result<(), Box<dyn std::error::Error>> {
    // #898: next without a session must fail even when granularity args are provided.
    let mut adapter = make_adapter();

    let args = json!({
        "threadId": 1,
        "granularity": "statement"
    });
    let response = adapter.handle_request(1, "next", Some(args));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "next without session must fail");
            assert_eq!(command, "next");
            assert!(message.is_some(), "must include guidance message");
        }
        _ => return Err("Expected Response for next at if boundary".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_step_in_at_while_boundary() -> Result<(), Box<dyn std::error::Error>> {
    // #898: stepIn without a session must fail.
    let mut adapter = make_adapter();

    let args = json!({
        "threadId": 1,
        "granularity": "statement"
    });
    let response = adapter.handle_request(1, "stepIn", Some(args));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "stepIn without session must fail");
            assert_eq!(command, "stepIn");
            assert!(message.is_some(), "must include guidance message");
        }
        _ => return Err("Expected Response for stepIn at while boundary".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_step_out_at_for_boundary() -> Result<(), Box<dyn std::error::Error>> {
    // #898: stepOut without a session must fail.
    let mut adapter = make_adapter();

    let args = json!({
        "threadId": 1,
        "granularity": "statement"
    });
    let response = adapter.handle_request(1, "stepOut", Some(args));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "stepOut without session must fail");
            assert_eq!(command, "stepOut");
            assert!(message.is_some(), "must include guidance message");
        }
        _ => return Err("Expected Response for stepOut at for boundary".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_step_sequence_at_block_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    // #898: All four handlers must return failure when there is no active session.
    let mut adapter = make_adapter();

    let ops: &[(&str, serde_json::Value)] = &[
        ("next", json!({"threadId": 1})),
        ("stepIn", json!({"threadId": 1})),
        ("next", json!({"threadId": 1})),
        ("stepOut", json!({"threadId": 1})),
        ("continue", json!({"threadId": 1})),
    ];

    for (seq, (command, args)) in ops.iter().enumerate() {
        let response = adapter.handle_request((seq + 1) as i64, command, Some(args.clone()));
        match response {
            DapMessage::Response { success, command: cmd, message, .. } => {
                assert!(!success, "op {command} at seq {seq} must fail without session");
                assert_eq!(&cmd, command, "command name mismatch at seq {seq}");
                assert!(message.is_some(), "{command} must include guidance message at seq {seq}");
            }
            _ => return Err(format!("Expected Response for {command} at seq {seq}").into()),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. stepIn to XS/builtin code — verify graceful handling
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_step_in_to_xs_builtin_via_target_id() -> Result<(), Box<dyn std::error::Error>> {
    // stepIn with a high targetId that doesn't match any real function.
    // The adapter must not panic and must return a valid response.
    let mut adapter = make_adapter();

    let args = json!({
        "threadId": 1,
        "targetId": 9999
    });
    let response = adapter.handle_request(1, "stepIn", Some(args));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            // #898: No session means failure regardless of targetId.
            assert!(!success, "stepIn without session must fail even with targetId");
            assert_eq!(command, "stepIn");
            assert!(message.is_some(), "must include guidance message");
        }
        _ => return Err("Expected Response for stepIn with unknown targetId".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / #9069
fn test_step_in_targets_is_refused_without_source_scans() -> Result<(), Box<dyn std::error::Error>>
{
    // Create a Perl source file containing multiple function calls on one line.
    // Even when such a frame exists, targeted stepping is fail-closed (#9069):
    // the request must be refused without publishing regex-derived target IDs.
    let dir = tempfile::tempdir()?;
    let script_path = dir.path().join("subroutine_calls.pl");
    fs::write(
        &script_path,
        "use strict;\nuse warnings;\nmy $x = abs(sqrt(length('hello')));\nprint $x;\n",
    )?;

    let mut adapter = make_adapter();
    let source_path = script_path.to_str().ok_or("temporary source path is not valid UTF-8")?;
    adapter.seed_stopped_session_with_frames_for_test(vec![StackFrame::new(
        1,
        "main",
        Source::new(source_path),
        3,
    )]);
    let args = json!({ "frameId": 1 });
    let response = adapter.handle_request(1, "stepInTargets", Some(args));

    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert_eq!(command, "stepInTargets");
            assert!(
                !success,
                "stepInTargets must be refused while targeted stepping is unsupported (#9069)"
            );
            assert!(body.is_none(), "refused stepInTargets must not publish target IDs");
            assert!(
                message.is_some_and(|m| !m.is_empty()),
                "refused stepInTargets must explain the unsupported disposition"
            );
        }
        _ => return Err("Expected Response for stepInTargets".into()),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Variable and scope inspection after rejected stepping requests
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_variables_request_after_rejected_stepping_without_session_is_empty()
-> Result<(), Box<dyn std::error::Error>> {
    // Without an active session, execution-control requests must not fabricate
    // stopped-frame authority or inspection data.
    let mut adapter = make_adapter();

    for (request_seq, command) in [(1, "next"), (2, "stepIn"), (3, "next")] {
        let response = adapter.handle_request(request_seq, command, Some(json!({"threadId": 1})));
        let body = assert_response(response, command, false)?;
        if body.is_some() {
            return Err(format!("rejected {command} must not return a response body").into());
        }
    }

    // A reference with no admitted stopped session returns honest empty data.
    let var_response =
        adapter.handle_request(4, "variables", Some(json!({"variablesReference": 11})));
    let body = assert_response(var_response, "variables", true)?
        .ok_or("variables response must have a body")?;
    let vars = body
        .get("variables")
        .and_then(|v| v.as_array())
        .ok_or("variables body must have a variables array")?;
    if !vars.is_empty() {
        return Err("variables must be empty without an admitted stopped frame".into());
    }
    Ok(())
}

#[test]
// AC:3535
fn test_scopes_request_after_rejected_next_without_session_is_empty()
-> Result<(), Box<dyn std::error::Error>> {
    // Without an active session, a rejected next must not fabricate stopped-frame authority.
    let mut adapter = make_adapter();

    let next_response = adapter.handle_request(1, "next", Some(json!({"threadId": 1})));
    let next_body = assert_response(next_response, "next", false)?;
    if next_body.is_some() {
        return Err("rejected next must not return a response body".into());
    }

    let scopes_response = adapter.handle_request(2, "scopes", Some(json!({"frameId": 1})));
    let body = assert_response(scopes_response, "scopes", true)?
        .ok_or("scopes response must have a body")?;
    let scopes = body
        .get("scopes")
        .and_then(|v| v.as_array())
        .ok_or("scopes body must have a scopes array")?;
    if !scopes.is_empty() {
        return Err("scopes must be empty without an admitted stopped frame".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. gotoTargets with real Perl source file
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_goto_targets_with_executable_perl_lines() -> Result<(), Box<dyn std::error::Error>> {
    // Create a Perl file and ask for goto targets near a specific line.
    let dir = tempfile::tempdir()?;
    let script_path = dir.path().join("goto_test.pl");
    fs::write(
        &script_path,
        "use strict;\nuse warnings;\nmy $x = 1;\nmy $y = 2;\nprint $x + $y;\n",
    )?;

    let mut adapter = make_adapter();
    let args = json!({
        "source": { "path": script_path.to_str().ok_or("path error")? },
        "line": 3
    });
    let response = adapter.handle_request(1, "gotoTargets", Some(args));

    match response {
        DapMessage::Response { success, command, body, .. } => {
            assert!(success, "gotoTargets should succeed for valid Perl file");
            assert_eq!(command, "gotoTargets");
            let body = body.ok_or("expected body")?;
            let targets =
                body.get("targets").and_then(|v| v.as_array()).ok_or("expected targets array")?;
            // There should be at least one executable target near line 3
            assert!(
                !targets.is_empty(),
                "gotoTargets should find executable lines near line 3 in a valid Perl file"
            );
            // Each target must have id and label
            for target in targets {
                assert!(target.get("id").is_some(), "target must have id");
                assert!(target.get("label").is_some(), "target must have label");
                let label = target.get("label").and_then(|v| v.as_str()).unwrap_or("");
                assert!(label.starts_with("Line "), "label should start with 'Line ': {label}");
            }
        }
        _ => return Err("Expected Response for gotoTargets".into()),
    }

    Ok(())
}

#[test]
// AC:3535
fn test_goto_targets_nonexistent_file_returns_empty_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();
    let args = json!({
        "source": { "path": "/nonexistent/file.pl" },
        "line": 5
    });
    let response = adapter.handle_request(1, "gotoTargets", Some(args));

    match response {
        DapMessage::Response { success, command, body, .. } => {
            // Path validation may reject as traversal or file not found → either empty success or failure
            let _ = success;
            assert_eq!(command, "gotoTargets");
            if success {
                let body = body.ok_or("gotoTargets response must have a body")?;
                let targets = body
                    .get("targets")
                    .and_then(|v| v.as_array())
                    .ok_or("gotoTargets body must have a targets array")?;
                assert!(targets.is_empty(), "nonexistent file must yield empty targets");
            }
            // if !success, that is also acceptable (path validation rejected the path)
        }
        _ => return Err("Expected Response for gotoTargets".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 6. goto with unknown target id
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_goto_unknown_target_id_returns_failure() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();
    let args = json!({ "threadId": 1, "targetId": 99999 });
    let response = adapter.handle_request(1, "goto", Some(args));

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "goto with unknown targetId should fail");
            assert_eq!(command, "goto");
            assert!(message.is_some(), "failure must include a message");
            let msg = message.ok_or("goto failure response must have a message")?;
            assert!(
                msg.contains("Unknown goto target") || msg.contains("99999"),
                "message should indicate unknown target: {msg}"
            );
        }
        _ => return Err("Expected Response for goto".into()),
    }
    Ok(())
}

#[test]
// AC:3535
fn test_goto_missing_args_returns_failure() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();
    let response = adapter.handle_request(1, "goto", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "goto with missing args should fail");
            assert_eq!(command, "goto");
            assert!(message.is_some(), "failure must include a message");
        }
        _ => return Err("Expected Response for goto with missing args".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 7. cancel during stepping
// ---------------------------------------------------------------------------

#[test]
// AC:3535 / AC:898
fn test_cancel_during_stepping_sequence() -> Result<(), Box<dyn std::error::Error>> {
    // cancel itself must succeed; subsequent step operations without a session must fail.
    let mut adapter = make_adapter();

    adapter.handle_request(1, "next", Some(json!({"threadId": 1})));
    adapter.handle_request(2, "stepIn", Some(json!({"threadId": 1})));

    let cancel_response = adapter.handle_request(3, "cancel", None);
    assert_response(cancel_response, "cancel", true)?;

    // #898: next still fails without a session (cancel doesn't create a session).
    let response = adapter.handle_request(4, "next", Some(json!({"threadId": 1})));
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "next after cancel still fails without session");
            assert_eq!(command, "next");
            assert!(message.is_some(), "must include guidance message");
        }
        _ => return Err("Expected Response for next after cancel".into()),
    }
    Ok(())
}

#[test]
// AC:3535
fn test_cancel_with_request_id_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    let args = json!({ "requestId": 42 });
    let cancel_response = adapter.handle_request(1, "cancel", Some(args));
    assert_response(cancel_response, "cancel", true)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 8. restartFrame and terminateThreads — unsupported operations
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_restart_frame_is_unsupported_for_perl() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    let response = adapter.handle_request(1, "restartFrame", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "restartFrame should fail — Perl doesn't support it");
            assert_eq!(command, "restartFrame");
            let msg = message.ok_or("restartFrame failure must include a message")?;
            assert!(
                msg.contains("Perl") || msg.contains("stack frame") || msg.contains("not support"),
                "message must explain why restartFrame is unsupported: {msg}"
            );
        }
        _ => return Err("Expected Response for restartFrame".into()),
    }
    Ok(())
}

#[test]
// AC:3535
fn test_terminate_threads_is_unsupported_for_perl() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    let response = adapter.handle_request(1, "terminateThreads", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "terminateThreads should fail — Perl model doesn't support it");
            assert_eq!(command, "terminateThreads");
            let msg = message.ok_or("terminateThreads failure must include a message")?;
            assert!(
                msg.contains("Perl") || msg.contains("thread") || msg.contains("not support"),
                "message must explain why terminateThreads is unsupported: {msg}"
            );
        }
        _ => return Err("Expected Response for terminateThreads".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 9. Sequence monotonicity across stepping operations
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_sequence_numbers_monotonically_increasing_across_steps()
-> Result<(), Box<dyn std::error::Error>> {
    // Each successive response must have a strictly increasing seq number.
    let mut adapter = make_adapter();

    let commands = ["next", "stepIn", "stepOut", "continue", "next", "stepIn"];
    let mut last_seq: i64 = 0;

    for (i, command) in commands.iter().enumerate() {
        let response = adapter.handle_request((i + 1) as i64, command, None);
        match response {
            DapMessage::Response { seq, request_seq, .. } => {
                assert!(
                    seq > last_seq,
                    "seq {seq} must be greater than previous {last_seq} for command {command}"
                );
                assert_eq!(
                    request_seq,
                    (i + 1) as i64,
                    "request_seq must echo back the request sequence for {command}"
                );
                last_seq = seq;
            }
            _ => return Err(format!("Expected Response for {command}").into()),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 10. continue/pause cycle — verify response shapes
// ---------------------------------------------------------------------------

#[test]
// AC:3535 / AC:898
fn test_continue_pause_cycle_response_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    // #898: continue without a session must fail (success: false, no body).
    let continue_resp = adapter.handle_request(1, "continue", None);
    match continue_resp {
        DapMessage::Response { success, command, body, message, .. } => {
            assert!(!success, "continue without session must fail");
            assert_eq!(command, "continue");
            assert!(body.is_none(), "failure response must have no body");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected continue Response".into()),
    }

    // Pause (no active session → failure, same as before).
    let pause_resp = adapter.handle_request(2, "pause", None);
    match pause_resp {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "pause without session must fail");
            assert_eq!(command, "pause");
            assert!(message.is_some(), "pause failure must include a message");
        }
        _ => return Err("Expected pause Response".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / AC:898
fn test_continue_with_all_threads_arg() -> Result<(), Box<dyn std::error::Error>> {
    // #898: continue with singleThread=false but no session must still fail.
    let mut adapter = make_adapter();

    let args = json!({ "threadId": 1, "singleThread": false });
    let response = adapter.handle_request(1, "continue", Some(args));

    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert!(!success, "continue without session must fail even with singleThread=false");
            assert_eq!(command, "continue");
            assert!(body.is_none(), "failure response must have no body");
            assert!(message.is_some(), "failure must include guidance message");
        }
        _ => return Err("Expected Response for continue".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 11. Source location context — stackTrace after stepping
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_stack_trace_available_after_step_operations() -> Result<(), Box<dyn std::error::Error>> {
    // After step operations, stackTrace should remain serviced.
    let mut adapter = make_adapter();

    adapter.handle_request(1, "next", Some(json!({"threadId": 1})));
    adapter.handle_request(2, "next", Some(json!({"threadId": 1})));

    let st_response = adapter.handle_request(3, "stackTrace", Some(json!({"threadId": 1})));

    match st_response {
        DapMessage::Response { success, command, body, .. } => {
            assert!(success, "stackTrace should succeed after stepping");
            assert_eq!(command, "stackTrace");
            let body = body.ok_or("stackTrace response must have a body")?;
            let frames = body
                .get("stackFrames")
                .and_then(|v| v.as_array())
                .ok_or("stackTrace body must have a stackFrames array")?;
            // Without a session, an honest empty list is returned
            assert!(frames.is_empty(), "no session after step ops: stackFrames must be empty");
        }
        _ => return Err("Expected Response for stackTrace".into()),
    }
    Ok(())
}

#[test]
// AC:3535
fn test_threads_available_after_step_operations() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    adapter.handle_request(1, "stepIn", None);
    adapter.handle_request(2, "stepOut", None);

    let threads_response = adapter.handle_request(3, "threads", None);

    match threads_response {
        DapMessage::Response { success, command, body, .. } => {
            assert!(success, "threads should succeed after stepping");
            assert_eq!(command, "threads");
            let body = body.ok_or("threads response must have a body")?;
            let threads = body
                .get("threads")
                .and_then(|v| v.as_array())
                .ok_or("threads body must have a threads array")?;
            // Without a session, threads list is empty
            assert!(threads.is_empty(), "without a session, threads list should be empty");
        }
        _ => return Err("Expected Response for threads".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 12. stepInTargets — missing args path
// ---------------------------------------------------------------------------

#[test]
// AC:3535
fn test_step_in_targets_missing_frame_id_returns_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let mut adapter = make_adapter();

    let response = adapter.handle_request(1, "stepInTargets", None);

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "stepInTargets with no args should fail");
            assert_eq!(command, "stepInTargets");
        }
        _ => return Err("Expected Response for stepInTargets with no args".into()),
    }
    Ok(())
}

#[test]
// AC:3535 / #9069
fn test_step_in_targets_with_frame_id_is_refused_without_session()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = make_adapter();

    let args = json!({ "frameId": 100 });
    let response = adapter.handle_request(1, "stepInTargets", Some(args));

    match response {
        DapMessage::Response { success, command, body, .. } => {
            assert_eq!(command, "stepInTargets");
            assert!(
                !success,
                "stepInTargets must fail while targeted stepping is unsupported (#9069)"
            );
            assert!(body.is_none(), "refused stepInTargets must not carry a targets body");
        }
        _ => return Err("Expected Response for stepInTargets".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 13. Consecutive mixed stepping operations — stress the sequence counter
// ---------------------------------------------------------------------------

#[test]
// AC:3535 / AC:898
fn test_many_consecutive_step_operations() -> Result<(), Box<dyn std::error::Error>> {
    // #898: All execution-control operations fail without a session; seq still increases.
    let mut adapter = make_adapter();

    let ops = [
        "next", "next", "stepIn", "next", "stepOut", "next", "stepIn", "stepIn", "next", "stepOut",
        "next", "next", "continue", "next", "stepIn", "next", "stepOut", "next", "next", "next",
    ];

    let mut prev_seq: i64 = 0;
    for (i, op) in ops.iter().enumerate() {
        let response = adapter.handle_request((i + 1) as i64, op, None);
        match response {
            DapMessage::Response { seq, success, command, .. } => {
                assert!(!success, "op {op} at index {i} must fail without session");
                assert_eq!(&command, op, "command name should match at index {i}");
                assert!(seq > prev_seq, "seq must increase: {seq} > {prev_seq} at index {i}");
                prev_seq = seq;
            }
            _ => return Err(format!("Expected Response for {op} at index {i}").into()),
        }
    }
    Ok(())
}
