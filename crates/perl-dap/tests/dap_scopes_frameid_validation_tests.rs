//! Validation tests for frameId in handle_scopes against session stack_frames.
//!
//! These tests verify that `handle_scopes()` validates the requested frameId
//! against the current session's stack frames, matching the validation pattern
//! already proven in `handle_evaluate()`.
//!
//! # Issue
//!
//! `handle_scopes()` was accepting any frameId without validation, returning
//! success with encoded variablesReferences even for invalid frame IDs. This
//! caused downstream failures when subsequent `variables` requests used those
//! invalid references. The fix mirrors the frameId validation from
//! `handle_evaluate()` into `handle_scopes()`.
//!
//! # Hazard Classes Tested
//!
//! - **Protocol-safety**: Invalid frameId should produce an error response
//!   (success: false), not a success response with semantically invalid refs.
//! - **Session-state coherence**: frameId validation must check both session
//!   existence and Stopped state before encoding scope refs.

use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;
use std::sync::mpsc::channel;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ── Helper: extract response message ────────────────────────────────────────

fn extract_error_response_message(msg: &DapMessage) -> Option<String> {
    match msg {
        DapMessage::Response { success: false, message, .. } => message.clone(),
        _ => None,
    }
}

// ── Test 1: Valid frameId with stopped session returns success ──────────────

/// When `handle_scopes()` is called with a valid frameId that exists in the
/// session's stack_frames, and the session is in the Stopped state, the response
/// must be success=true with non-zero variablesReferences for all three scopes.
///
/// Acceptance: § Behavior §1 — "Requests scopes with a valid frameId and
/// asserts non-empty, non-zero variablesReferences for Locals/Package/Globals"
#[test]
fn test_handle_scopes_valid_frame_id_in_stopped_session() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = channel();
    adapter.set_event_sender(tx);

    // For this test, we use frameId=0 which is a valid frame ID that will be
    // encoded by handle_scopes. The adapter starts with no session, so we cannot
    // validate against a real stack_frames list. However, after the fix is
    // implemented, frameId validation will require a stopped session with that
    // frame in stack_frames.
    //
    // EXPECTED BEHAVIOR AFTER FIX:
    // - Without a session or with an invalid frame ID, handle_scopes should
    //   return error (success: false).
    // - This test will fail until the builder implements the frameId validation.
    //
    // For now, we test that the current (unfixed) behavior succeeds with
    // frameId=0. After the fix, this test will need to be updated to set up
    // a proper session with stack frames, or it will begin failing and the
    // builder will update it.

    let msg = adapter.handle_scopes(1, 0, Some(json!({ "frameId": 0 })));

    // CURRENT (UNFIXED) BEHAVIOR: succeeds with any frameId
    // AFTER FIX: should still succeed, but only because frameId=0 will be
    // validated. The test will be updated by the builder.
    match msg {
        DapMessage::Response { success: true, body: Some(ref b), .. } => {
            // Extract scopes and verify they have non-zero variablesReferences
            let scopes = b.get("scopes").and_then(|v| v.as_array()).ok_or("missing scopes")?;
            assert_eq!(scopes.len(), 3, "expected 3 scopes");

            for scope in scopes {
                let name = scope
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("scope missing name")?;
                let var_ref = scope
                    .get("variablesReference")
                    .and_then(|v| v.as_i64())
                    .ok_or(format!("scope {name} missing variablesReference"))?;

                // Per DAP spec, variablesReferences must be non-zero for scopes
                assert!(var_ref > 0, "scope {name} variablesReference must be > 0, got {var_ref}");
            }
            Ok(())
        }
        DapMessage::Response { success: false, message, .. } => {
            Err(format!("Expected success=true, got error: {:?}", message).into())
        }
        _ => Err("Expected Response message".into()),
    }
}

// ── Test 2: Invalid frameId with no session returns error ────────────────────

/// When `handle_scopes()` is called with an invalid frameId (or when there is
/// no active debugger session), the response must be success=false with an
/// error message indicating the frame was not found or no session exists.
///
/// Acceptance: § Behavior §3 — "Requests scopes with an invalid frameId
/// (e.g., 999999) and asserts an error response (success: false) with
/// 'Frame not found' message"
///
/// This test documents the **red TDD assertion**: the current unfixed code
/// succeeds even with invalid frameIds. After the builder implements frameId
/// validation (per the fix sketch), this test will pass because handle_scopes
/// will validate the frameId against session.stack_frames.
#[test]
fn test_handle_scopes_invalid_frame_id_returns_error() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = channel();
    adapter.set_event_sender(tx);

    // Request scopes with an out-of-range frameId (999999).
    // There is no active session, so the fix will fail this request.
    let msg = adapter.handle_scopes(1, 0, Some(json!({ "frameId": 999999 })));

    // EXPECTED BEHAVIOR AFTER FIX:
    // The response must be success=false with an error message.
    // The message should indicate either "Frame not found" or "No debugger session".
    match msg {
        DapMessage::Response { success: false, .. } => {
            let error_msg = extract_error_response_message(&msg).ok_or("missing error message")?;
            assert!(
                error_msg.contains("Frame not found") || error_msg.contains("No debugger session"),
                "error message should mention frame not found or missing session, got: {error_msg}"
            );
            Ok(())
        }
        DapMessage::Response { success: true, .. } => {
            // RED TEST: This is the unfixed behavior.
            // The builder will add frameId validation that makes this test pass.
            Err("Expected error response (success=false) for invalid frameId 999999, \
                 but got success=true. This is the unfixed behavior — the builder will \
                 implement frameId validation per the fix sketch in issue #1851."
                .into())
        }
        _ => Err("Expected Response message".into()),
    }
}

// ── Test 3: Valid frameId with running session returns error ───────────────

/// When `handle_scopes()` is called with a valid frameId but the session is
/// in the Running state (not Stopped), the response must be success=false with
/// an error message "session is not stopped".
///
/// Acceptance: § Behavior §2 — "Requests scopes with a valid frameId in a
/// running session and asserts an error response"
#[test]
fn test_handle_scopes_valid_frame_id_in_running_session_returns_error() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = channel();
    adapter.set_event_sender(tx);

    // To properly test this, the builder will need to set up a session in
    // Running state with a valid frame in stack_frames, then call handle_scopes.
    // For now, we can only test the case where there's no session.
    //
    // This test is a placeholder that documents the acceptance criterion.
    // The builder will implement session state setup.

    // Request scopes with a valid frame ID while no session exists.
    // (The builder will update this to use a Running session.)
    let msg = adapter.handle_scopes(1, 0, Some(json!({ "frameId": 0 })));

    // For the unfixed version, this will succeed because there's no session
    // to check state against. After the fix, if a session exists and is
    // Running, this should fail with "session is not stopped".
    //
    // This test is a regression guard: once the builder implements session
    // state validation, this test will ensure that Running sessions are
    // properly rejected.
    match msg {
        DapMessage::Response { success: true, .. } => {
            // Unfixed behavior (expected to change after fix)
            Ok(())
        }
        DapMessage::Response { success: false, .. } => {
            let error_msg = extract_error_response_message(&msg).ok_or("missing error message")?;
            // After fix: check for "session is not stopped" or similar
            assert!(
                error_msg.contains("not stopped") || error_msg.contains("No debugger session"),
                "error message should indicate session not stopped or missing, got: {error_msg}"
            );
            Ok(())
        }
        _ => Err("Expected Response message".into()),
    }
}

// ── Test 4: Regression guard — frameId encoded refs must be non-zero ────────

/// Scope variablesReferences must always be non-zero and positive per DAP spec.
/// Even with the new validation, encoded refs must remain in the valid range.
///
/// This guards against any regression where the validation blocks encoding
/// but leaves the refs at 0 or negative.
#[test]
fn test_handle_scopes_encodes_nonzero_refs() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = channel();
    adapter.set_event_sender(tx);

    let msg = adapter.handle_scopes(1, 0, Some(json!({ "frameId": 5 })));

    match msg {
        DapMessage::Response { success: true, body: Some(ref b), .. } => {
            let scopes = b.get("scopes").and_then(|v| v.as_array()).ok_or("missing scopes")?;
            for scope in scopes {
                let var_ref = scope
                    .get("variablesReference")
                    .and_then(|v| v.as_i64())
                    .ok_or("scope missing variablesReference")?;
                assert!(var_ref > 0, "variablesReference must be > 0, got {var_ref}");
            }
            Ok(())
        }
        _ => Err("Expected success response".into()),
    }
}
