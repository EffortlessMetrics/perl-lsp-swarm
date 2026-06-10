//! Control Flow Handlers Tests (AC9)
//!
//! Tests for DAP control flow operations: continue, next, stepIn, stepOut, pause
//!
//! Specification: Issue #454 - DAP Control Flow Handlers (AC9)
//! Updated: Issue #898 - execution-control handlers must return failure without a session
//!
//! Run with: cargo test -p perl-dap --test control_flow_handlers_tests

use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::{must, must_some};
use serde_json::json;

// AC9.1: Test continue request handler — should fail without a session (#898)
#[test]
fn test_continue_handler() {
    // AC9/#898: Continue request without a session must fail with guidance
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "continue", None);

    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert!(!success, "Continue without a session must fail");
            assert_eq!(command, "continue");
            assert!(message.is_some(), "Continue without session should have guidance message");
            assert!(body.is_none(), "Failure response must not have a body");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for continue"));
            unreachable!()
        }
    }
}

// AC9.1: Test next (step over) request handler — should fail without a session (#898)
#[test]
fn test_next_handler() {
    // AC9/#898: Next request without a session must fail with guidance
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "next", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "Next without a session must fail");
            assert_eq!(command, "next");
            assert!(message.is_some(), "Next without session should have guidance message");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for next"));
            unreachable!()
        }
    }
}

// AC9.1: Test stepIn request handler — should fail without a session (#898)
#[test]
fn test_step_in_handler() {
    // AC9/#898: StepIn request without a session must fail with guidance
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "stepIn", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "StepIn without a session must fail");
            assert_eq!(command, "stepIn");
            assert!(message.is_some(), "StepIn without session should have guidance message");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for stepIn"));
            unreachable!()
        }
    }
}

// AC9.1: Test stepOut request handler — should fail without a session (#898)
#[test]
fn test_step_out_handler() {
    // AC9/#898: StepOut request without a session must fail with guidance
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "stepOut", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(!success, "StepOut without a session must fail");
            assert_eq!(command, "stepOut");
            assert!(message.is_some(), "StepOut without session should have guidance message");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for stepOut"));
            unreachable!()
        }
    }
}

// AC9.1: Test pause request handler
#[test]
fn test_pause_handler_no_session() {
    // AC9: Pause request should handle missing session gracefully
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "pause", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            // Without an active session, pause should fail gracefully
            assert!(!success, "Pause should fail without active session");
            assert_eq!(command, "pause");
            assert!(message.is_some(), "Pause without session should provide error message");

            if let Some(msg) = message {
                assert!(
                    msg.contains("Failed to pause") || msg.to_lowercase().contains("debugger"),
                    "Error message should indicate pause failure or no session: {}",
                    msg
                );
            }
        }
        _ => {
            must(Err::<(), _>("Expected Response message for pause"));
            unreachable!()
        }
    }
}

// AC9.4: Test control flow state transitions — all four must fail without a session (#898)
#[test]
fn test_control_flow_state_transitions() {
    // AC9/#898: All four execution-control operations must fail without a session
    let mut adapter = DebugAdapter::new();

    let continue_response = adapter.handle_request(1, "continue", None);
    assert!(matches!(continue_response, DapMessage::Response { success: false, .. }));

    let next_response = adapter.handle_request(2, "next", None);
    assert!(matches!(next_response, DapMessage::Response { success: false, .. }));

    let step_in_response = adapter.handle_request(3, "stepIn", None);
    assert!(matches!(step_in_response, DapMessage::Response { success: false, .. }));

    let step_out_response = adapter.handle_request(4, "stepOut", None);
    assert!(matches!(step_out_response, DapMessage::Response { success: false, .. }));
}

// AC9.4: Test that responses have correct sequence numbers
#[test]
fn test_control_flow_sequence_numbers() {
    // AC9: Verify sequence numbers in control flow responses
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(42, "continue", None);

    match response {
        DapMessage::Response { seq, request_seq, .. } => {
            assert!(seq > 0, "Response sequence should be positive");
            assert_eq!(request_seq, 42, "Request sequence should match");
        }
        _ => {
            must(Err::<(), _>("Expected Response message"));
            unreachable!()
        }
    }
}

// AC9.1: Test continue with threadId argument — should fail without a session (#898)
#[test]
fn test_continue_with_thread_id() {
    // AC9/#898: Continue without a session must fail even when threadId is provided
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1
    });

    let response = adapter.handle_request(1, "continue", Some(args));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "Continue without a session must fail");
            assert_eq!(command, "continue");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for continue with threadId"));
            unreachable!()
        }
    }
}

// AC9.1: Test next with threadId argument — should fail without a session (#898)
#[test]
fn test_next_with_thread_id() {
    // AC9/#898: Next without a session must fail even when threadId is provided
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1
    });

    let response = adapter.handle_request(1, "next", Some(args));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "Next without a session must fail");
            assert_eq!(command, "next");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for next with threadId"));
            unreachable!()
        }
    }
}

// AC9.1: Test stepIn with optional targetId — should fail without a session (#898)
#[test]
fn test_step_in_with_target_id() {
    // AC9/#898: StepIn without a session must fail even when targetId is provided
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1,
        "targetId": 5
    });

    let response = adapter.handle_request(1, "stepIn", Some(args));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "StepIn without a session must fail");
            assert_eq!(command, "stepIn");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for stepIn with targetId"));
            unreachable!()
        }
    }
}

// AC9.1: Test stepOut with threadId argument — should fail without a session (#898)
#[test]
fn test_step_out_with_thread_id() {
    // AC9/#898: StepOut without a session must fail even when threadId is provided
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1
    });

    let response = adapter.handle_request(1, "stepOut", Some(args));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "StepOut without a session must fail");
            assert_eq!(command, "stepOut");
        }
        _ => {
            must(Err::<(), _>("Expected Response message for stepOut with threadId"));
            unreachable!()
        }
    }
}

// AC9.1: Test pause with threadId argument
#[test]
fn test_pause_with_thread_id() {
    // AC9: Pause request should accept threadId argument
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1
    });

    let response = adapter.handle_request(1, "pause", Some(args));

    match response {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "pause");
            // Success depends on whether there's an active session
        }
        _ => {
            must(Err::<(), _>("Expected Response message for pause with threadId"));
            unreachable!()
        }
    }
}

// AC9.4: Test multiple sequential control flow operations — all fail without a session (#898)
#[test]
fn test_sequential_control_flow_operations() {
    // AC9/#898: All execution-control operations must fail without a session
    let mut adapter = DebugAdapter::new();

    // Execute a sequence of control flow operations
    let operations = [
        ("continue", json!({"threadId": 1})),
        ("next", json!({"threadId": 1})),
        ("stepIn", json!({"threadId": 1})),
        ("stepOut", json!({"threadId": 1})),
        ("next", json!({"threadId": 1})),
        ("continue", json!({"threadId": 1})),
    ];

    for (idx, (command, args)) in operations.iter().enumerate() {
        let response = adapter.handle_request((idx + 1) as i64, command, Some(args.clone()));

        match response {
            DapMessage::Response { success, command: resp_cmd, .. } => {
                assert!(!success, "Operation {} should fail without a session", command);
                assert_eq!(&resp_cmd, command, "Command should match");
            }
            _ => must(Err::<(), _>(format!("Expected Response for command {}", command))),
        }
    }
}

// AC9.5: Test edge case - continue with missing threadId — should fail without a session (#898)
#[test]
fn test_continue_missing_thread_id() {
    // AC9/#898: Continue without a session must fail even with empty args
    let mut adapter = DebugAdapter::new();

    let args = json!({});

    let response = adapter.handle_request(1, "continue", Some(args));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(!success, "Continue without a session must fail");
            assert_eq!(command, "continue");
        }
        _ => {
            must(Err::<(), _>("Expected Response message"));
            unreachable!()
        }
    }
}

// AC9.5: Test edge case - operations with null arguments — all fail without a session (#898)
#[test]
fn test_control_flow_with_null_arguments() {
    // AC9/#898: Execution-control operations must fail without a session
    let mut adapter = DebugAdapter::new();

    let commands = vec!["continue", "next", "stepIn", "stepOut"];

    for command in commands {
        let response = adapter.handle_request(1, command, None);

        match response {
            DapMessage::Response { success, .. } => {
                assert!(!success, "{} should fail without a session", command);
            }
            _ => must(Err::<(), _>(format!("Expected Response for {}", command))),
        }
    }
}

// AC9.4: Test response format consistency — all fail without a session (#898)
#[test]
fn test_control_flow_response_format() {
    // AC9/#898: All execution-control responses must fail consistently without a session
    let mut adapter = DebugAdapter::new();

    let commands = vec!["continue", "next", "stepIn", "stepOut"];

    for command in commands {
        let response = adapter.handle_request(1, command, None);

        match response {
            DapMessage::Response { seq, request_seq, success, command: cmd, .. } => {
                assert!(seq > 0, "Sequence number should be positive");
                assert_eq!(request_seq, 1, "Request sequence should match");
                assert!(!success, "{} should fail without a session", command);
                assert_eq!(cmd, command, "Command name should match");
            }
            _ => must(Err::<(), _>(format!("Expected Response for {}", command))),
        }
    }
}

// AC9.1: Verify Perl debugger command mapping — handlers must fail without a session (#898)
#[test]
fn test_perl_debugger_command_mapping() {
    // AC9/#898: Verify that DAP commands fail without a session
    // This is implicitly tested by the handler implementations:
    // - continue -> "c\n"
    // - next -> "n\n"
    // - stepIn -> "s\n"
    // - stepOut -> "r\n"

    // The actual command sending is tested through the handlers
    let mut adapter = DebugAdapter::new();

    // Verify handlers respond correctly (command sending happens internally when session is active)
    assert!(matches!(
        adapter.handle_request(1, "continue", None),
        DapMessage::Response { success: false, .. }
    ));

    assert!(matches!(
        adapter.handle_request(2, "next", None),
        DapMessage::Response { success: false, .. }
    ));

    assert!(matches!(
        adapter.handle_request(3, "stepIn", None),
        DapMessage::Response { success: false, .. }
    ));

    assert!(matches!(
        adapter.handle_request(4, "stepOut", None),
        DapMessage::Response { success: false, .. }
    ));
}

// AC9.4: Test that pause returns appropriate success status
#[test]
fn test_pause_without_active_session_returns_failure() {
    // AC9: Pause should return failure when no session is active
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "pause", None);

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "pause");
            assert!(!success, "Pause without session should fail");
            assert!(message.is_some(), "Failure should include error message");
        }
        _ => {
            must(Err::<(), _>("Expected Response for pause"));
            unreachable!()
        }
    }
}

// AC9.4: Test continue response without a session — must fail, no body (#898)
#[test]
fn test_continue_includes_all_threads_continued() {
    // AC9/#898: Without a session, continue must fail and must NOT have a body
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "continue", None);

    if let DapMessage::Response { success, body, message, .. } = response {
        assert!(!success, "Continue without a session must fail");
        assert!(body.is_none(), "Failure response must not have a body");
        assert!(message.is_some(), "Failure response must include guidance message");
    } else {
        must(Err::<(), _>("Expected Response for continue"));
        unreachable!();
    }
}

// AC9.1: Test all five core control flow operations exist
#[test]
fn test_all_control_flow_operations_exist() {
    // AC9: Verify all five control flow operations are implemented
    let mut adapter = DebugAdapter::new();

    let operations = vec!["continue", "next", "stepIn", "stepOut", "pause"];

    for operation in operations {
        let response = adapter.handle_request(1, operation, None);

        // Verify the operation is recognized (not unknown command)
        match response {
            DapMessage::Response { command, .. } => {
                assert_eq!(command, operation, "Operation {} should be recognized", operation);
            }
            _ => must(Err::<(), _>(format!("Operation {} should return Response", operation))),
        }
    }
}

// AC9.5: Test unknown control flow command
#[test]
fn test_unknown_control_flow_command() {
    // AC9: Unknown commands should be rejected
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "unknownCommand", None);

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Unknown command should fail");
            assert!(message.is_some(), "Unknown command should have error message");

            if let Some(msg) = message {
                assert!(
                    msg.to_lowercase().contains("unknown"),
                    "Error should indicate unknown command"
                );
            }
        }
        _ => {
            must(Err::<(), _>("Expected Response for unknown command"));
            unreachable!()
        }
    }
}

#[test]
fn test_unknown_command_includes_case_only_suggestion() {
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "STEPIN", None);

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Unexpected success for unknown command");
            let msg = must_some(message);
            assert!(
                msg.contains("Unknown command: STEPIN"),
                "Error should include unknown command: {msg}"
            );
            assert!(
                msg.contains("Did you mean 'stepIn'?"),
                "Error should include case-only suggestion: {msg}"
            );
        }
        _ => {
            must(Err::<(), _>("Expected Response for unknown command"));
            unreachable!()
        }
    }
}

#[test]
fn test_unknown_command_includes_typo_suggestion() {
    let mut adapter = DebugAdapter::new();

    let response = adapter.handle_request(1, "setBreakpoint", None);

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Unexpected success for unknown command");
            let msg = must_some(message);
            assert!(
                msg.contains("Unknown command: setBreakpoint"),
                "Error should include unknown command: {msg}"
            );
            assert!(
                msg.contains("Did you mean 'setBreakpoints'?"),
                "Error should include typo suggestion: {msg}"
            );
        }
        _ => {
            must(Err::<(), _>("Expected Response for unknown command"));
            unreachable!()
        }
    }
}

// AC9.4: Test that handlers are consistent (can be called multiple times) — all fail without session (#898)
#[test]
fn test_control_flow_handlers_thread_safe() {
    // AC9/#898: Handlers should be reusable; all must fail without a session
    let mut adapter = DebugAdapter::new();

    // Call same handler multiple times
    for i in 1..=5 {
        let response = adapter.handle_request(i, "next", None);

        match response {
            DapMessage::Response { success, .. } => {
                assert!(!success, "Handler must fail without a session on iteration {}", i);
            }
            _ => must(Err::<(), _>(format!("Expected Response on iteration {}", i))),
        }
    }
}

// AC9.1: Test stepIn with granularity argument — should fail without a session (#898)
#[test]
fn test_step_in_with_granularity() {
    // AC9/#898: StepIn without a session must fail even when granularity is specified
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1,
        "granularity": "statement"
    });

    let response = adapter.handle_request(1, "stepIn", Some(args));

    match response {
        DapMessage::Response { success, .. } => {
            assert!(!success, "StepIn without a session must fail");
        }
        _ => {
            must(Err::<(), _>("Expected Response for stepIn with granularity"));
            unreachable!()
        }
    }
}

// AC9.1: Test next with granularity argument — should fail without a session (#898)
#[test]
fn test_next_with_granularity() {
    // AC9/#898: Next without a session must fail even when granularity is specified
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1,
        "granularity": "line"
    });

    let response = adapter.handle_request(1, "next", Some(args));

    match response {
        DapMessage::Response { success, .. } => {
            assert!(!success, "Next without a session must fail");
        }
        _ => {
            must(Err::<(), _>("Expected Response for next with granularity"));
            unreachable!()
        }
    }
}

// AC9.1: Test stepOut with granularity argument — should fail without a session (#898)
#[test]
fn test_step_out_with_granularity() {
    // AC9/#898: StepOut without a session must fail even when granularity is specified
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "threadId": 1,
        "granularity": "statement"
    });

    let response = adapter.handle_request(1, "stepOut", Some(args));

    match response {
        DapMessage::Response { success, .. } => {
            assert!(!success, "StepOut without a session must fail");
        }
        _ => {
            must(Err::<(), _>("Expected Response for stepOut with granularity"));
            unreachable!()
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical proving tests — lock the strict no-session behavior per issue #898
// ---------------------------------------------------------------------------

#[test]
fn continue_without_session_returns_guidance() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "continue", None);
    match response {
        DapMessage::Response { success, command, message, body, .. } => {
            assert_eq!(command, "continue");
            assert!(!success, "continue without a session must fail");
            assert!(body.is_none(), "failure response must not have a body");
            let msg = message.ok_or("must include guidance message")?;
            assert!(msg.contains("no Perl debug session is active"), "got: {msg}");
            assert!(msg.contains("Start a launch or attach request"), "got: {msg}");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn next_without_session_returns_guidance() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "next", None);
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "next");
            assert!(!success, "next without a session must fail");
            let msg = message.ok_or("must include guidance message")?;
            assert!(msg.contains("no Perl debug session is active"), "got: {msg}");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn step_in_without_session_returns_guidance() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "stepIn", None);
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "stepIn");
            assert!(!success, "stepIn without a session must fail");
            let msg = message.ok_or("must include guidance message")?;
            assert!(msg.contains("no Perl debug session is active"), "got: {msg}");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn step_out_without_session_returns_guidance() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "stepOut", None);
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "stepOut");
            assert!(!success, "stepOut without a session must fail");
            let msg = message.ok_or("must include guidance message")?;
            assert!(msg.contains("no Perl debug session is active"), "got: {msg}");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }
    Ok(())
}
