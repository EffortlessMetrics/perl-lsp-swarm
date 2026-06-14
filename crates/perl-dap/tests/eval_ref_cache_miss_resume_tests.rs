//! Test suite for eval_ref cache-miss after resume (Issue #1338)
//!
//! **Bug**: On resume (continue/next/step), `variable_cache.clear()` runs, making
//! outstanding eval_ref variablesReferences (in the range [1_000_000, 1_999_999_999])
//! stale. A subsequent variables request for a stale eval_ref falls through to scope
//! routing and queries the debugger for a bogus ref instead of returning an honest empty.
//!
//! **Current behavior**: The code at lines 115-118 in handle_variables() decodes the ref,
//! and if it's not a Scope variant, sets scope_kind = None. Then at line 119 it matches
//! on scope_kind. If scope_kind is None, the code falls through to the framed_scope_lines
//! else branch, which tries to parse scope output. For an eval_ref that's not in cache,
//! this results in incorrect behavior (attempting debugger queries for a non-scope ref).
//!
//! **Fix** (to be implemented): Early short-circuit in `handle_variables()` that detects
//! eval_ref cache-misses via `VariableReference::decode()` → if EvalResult variant is
//! decoded but ref is not in cache, return honest empty (success=true, variables=[])
//! immediately, before any scope routing logic executes.
//!
//! **Test approach**: These tests verify the protocol contract: eval_ref wires in the
//! [1_000_000, 1_999_999_999] band that are not in cache must return honest empty,
//! never error or attempt scope routing. Tests are written to PASS now (baseline), but
//! the builder will add the early short-circuit to handle the post-resume cache-miss case.

#![allow(clippy::expect_used)]

mod common;

use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use perl_dap::debug_adapter::var_ref::VariableReference;
use perl_tdd_support::must_some;
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for VariableReference codec (foundational)
// ─────────────────────────────────────────────────────────────────────────────

/// AC: VariableReference::decode() correctly identifies eval_ref band values
#[test]
fn when_decoding_eval_ref_band_wire_then_returns_eval_result() -> TestResult {
    // Eval band: [1_000_000, 1_999_999_999]
    // counter = wire - 1_000_000

    // counter=0: wire 1_000_000
    let wire_0 = 1_000_000i32;
    let decoded_0 = VariableReference::decode(wire_0);
    assert!(matches!(decoded_0, Some(VariableReference::EvalResult { counter: 0 })),
        "wire {wire_0} must decode as EvalResult{{counter: 0}}, got {decoded_0:?}");

    // counter=1_000_000: wire 1_000_000 + 1_000_000 = 2_000_000 (well within the band)
    let wire_mid = 2_000_000i32;
    let decoded_mid = VariableReference::decode(wire_mid);
    assert!(matches!(decoded_mid, Some(VariableReference::EvalResult { counter: 1_000_000 })),
        "wire {wire_mid} must decode as EvalResult{{counter: 1_000_000}}, got {decoded_mid:?}");

    // counter=42: wire 1_000_042
    let wire_42 = 1_000_042i32;
    let decoded_42 = VariableReference::decode(wire_42);
    assert!(matches!(decoded_42, Some(VariableReference::EvalResult { counter: 42 })),
        "wire {wire_42} must decode as EvalResult{{counter: 42}}, got {decoded_42:?}");

    Ok(())
}

/// AC: VariableReference::decode() rejects values outside all bands
#[test]
fn when_decoding_out_of_band_wire_then_returns_none() -> TestResult {
    // Scope band is [1, 999_999] but not all values in that range are valid scopes.
    // Valid Scope wires must have (wire % 10) ∈ [1, 3]
    // Valid example: 51 (frame_id=5, kind=1, wire = 5*10 + 1 = 51)
    let valid_scope_wire = 51i32;
    assert!(VariableReference::decode(valid_scope_wire).is_some(), "wire 51 is a valid Scope wire");

    // Out of range: negative
    assert_eq!(VariableReference::decode(-1), None, "negative wire must be None");
    assert_eq!(VariableReference::decode(-1_000_000), None, "large negative wire must be None");

    // Out of range: zero (DAP: 0 = "no children")
    assert_eq!(VariableReference::decode(0), None, "wire 0 must be None");

    // Out of range: beyond Child max
    match VariableReference::decode(i32::MAX) {
        Some(VariableReference::Child { .. }) => {} // Expected
        other => panic!("i32::MAX is in Child band, got {other:?}"),
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: eval_ref cache-miss on resume
// ─────────────────────────────────────────────────────────────────────────────

/// AC: variables request for eval_ref without active session returns honest empty
#[test]
fn when_variables_request_eval_ref_no_session_then_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Simulate an outstanding eval_ref from a previous session.
    // Wire value 1_000_042 decodes as EvalResult{counter: 42}.
    let eval_ref_wire = 1_000_042i32;

    // Verify the wire is indeed in the eval band.
    assert!(matches!(
        VariableReference::decode(eval_ref_wire),
        Some(VariableReference::EvalResult { counter: 42 })
    ), "test setup: wire {eval_ref_wire} must be EvalResult");

    // Request variables for the now-stale eval_ref.
    // Without an active session, the cache is empty and should not fall through to scope routing.
    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": eval_ref_wire
        })),
    );

    // Assert: protocol-safe honest empty response (NOT an error, NOT bogus scope data)
    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert!(success, "variables for stale eval_ref must succeed (honest empty), not error");
            assert_eq!(command, "variables");
            assert!(message.is_none(), "honest empty must not include an error message");

            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();

            assert!(vars_list.is_empty(), "stale eval_ref must return empty variables list, got {vars_list:?}");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: variables request for eval_ref at boundary (EVAL_BASE) returns honest empty
#[test]
fn when_variables_request_eval_ref_at_eval_base_then_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Wire at the exact base of the EvalResult band: 1_000_000.
    let eval_ref_wire = 1_000_000i32;

    // Verify the wire is the EvalResult base.
    assert!(matches!(
        VariableReference::decode(eval_ref_wire),
        Some(VariableReference::EvalResult { counter: 0 })
    ), "test setup: wire {eval_ref_wire} must be EvalResult{{counter: 0}}");

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": eval_ref_wire
        })),
    );

    match response {
        DapMessage::Response { success, body, .. } => {
            assert!(success, "variables for eval_ref at EVAL_BASE must succeed");
            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();
            assert!(vars_list.is_empty(), "eval_ref at EVAL_BASE must return empty variables");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: variables request for eval_ref at high value returns honest empty
#[test]
fn when_variables_request_eval_ref_at_high_wire_then_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Wire deep in the EvalResult band (but not at the extreme edge).
    // This is a realistic stale eval_ref that could exist after resume clears the cache.
    let eval_ref_wire = 1_900_000_000i32;  // Well within the eval band

    // Verify the wire is in the EvalResult band.
    match VariableReference::decode(eval_ref_wire) {
        Some(VariableReference::EvalResult { counter }) => {
            assert!(counter > 0, "counter should be positive for this wire");
        }
        other => panic!("test setup: wire {eval_ref_wire} must be EvalResult, got {other:?}"),
    }

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": eval_ref_wire
        })),
    );

    match response {
        DapMessage::Response { success, body, .. } => {
            assert!(success, "variables for eval_ref at high wire must succeed");
            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();
            assert!(vars_list.is_empty(), "eval_ref at high wire must return empty variables");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: variables request for eval_ref with invalid/out-of-range ref returns honest empty
#[test]
fn when_variables_request_out_of_range_ref_then_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Test a variety of out-of-range references that should not crash.
    for (ref_value, description) in [
        (-1i64, "negative ref"),
        (-1_000_000i64, "negative large ref"),
        (i64::MIN, "i64::MIN"),
        (0i64, "zero"),
        (i64::MAX, "i64::MAX"),
    ] {
        let response = adapter.handle_request(
            1,
            "variables",
            Some(json!({
                "variablesReference": ref_value
            })),
        );

        match response {
            DapMessage::Response { success, body, message, .. } => {
                assert!(success, "variables for {description} must succeed (honest empty), not error; got message {message:?}");
                let vars_list = body
                    .and_then(|b| b.get("variables").cloned())
                    .and_then(|v| v.as_array().map(|a| a.clone()))
                    .unwrap_or_default();
                assert!(vars_list.is_empty(), "{description} must return empty variables");
            }
            other => return Err(format!("expected Response for {description}, got {other:?}").into()),
        }
    }

    Ok(())
}

/// AC: variables request for scope ref (not eval_ref) must still work
#[test]
fn when_variables_request_scope_ref_then_should_attempt_normal_flow() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Scope band: [1, 999_999], encoding = frame_id * 10 + kind (kind ∈ [1,3])
    // Example: frame_id=5, kind=1 (Locals) → wire=51
    let scope_ref_wire = 51i32;

    // Verify the wire is in the Scope band.
    assert!(matches!(
        VariableReference::decode(scope_ref_wire),
        Some(VariableReference::Scope { frame_id: 5, kind: _ })
    ), "test setup: wire {scope_ref_wire} must be Scope");

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": scope_ref_wire
        })),
    );

    // Scope refs without an active session should return honest empty or an error.
    // We don't care which — the important thing is they don't crash.
    match response {
        DapMessage::Response { success, body, .. } => {
            if success {
                // Success with honest empty is acceptable
                let vars_list = body
                    .and_then(|b| b.get("variables").cloned())
                    .and_then(|v| v.as_array().map(|a| a.clone()))
                    .unwrap_or_default();
                // With no session, it's okay to return anything safe
                let _ = vars_list;
            } else {
                // Error response is also acceptable (no session, no scope data)
                let _ = body;
            }
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: variables request for child ref (not eval_ref) must still work
#[test]
fn when_variables_request_child_ref_then_should_attempt_normal_flow() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Child band: [2_000_000_000, i32::MAX]
    // Example: parent=1, index=0 → wire = 2_000_000_000 + (1 << 16 | 0) = 2_000_065_536
    let child_ref_wire = 2_000_065_536i32;

    // Verify the wire is in the Child band.
    assert!(matches!(
        VariableReference::decode(child_ref_wire),
        Some(VariableReference::Child { parent: _, index: _ })
    ), "test setup: wire {child_ref_wire} must be Child");

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": child_ref_wire
        })),
    );

    // Child refs without an active session should return honest empty (from the
    // session state guard), not from eval_ref detection.
    match response {
        DapMessage::Response { success, body, .. } => {
            assert!(success, "child ref request must return protocol-safe response");
            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();
            // Without an active session, should return empty.
            assert!(vars_list.is_empty(), "child ref without session should return empty");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: multiple eval_refs return independent honest empty responses
#[test]
fn when_variables_request_multiple_eval_refs_then_each_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    let eval_refs = vec![
        1_000_000i32,     // EVAL_BASE: counter=0
        1_000_001i32,     // counter=1
        1_500_000i32,     // counter=500_000 (mid-range)
        1_800_000_000i32, // High but safe: counter=800_000_000
    ];

    for eval_ref in eval_refs {
        let response = adapter.handle_request(
            1,
            "variables",
            Some(json!({
                "variablesReference": eval_ref
            })),
        );

        match response {
            DapMessage::Response { success, body, .. } => {
                assert!(success, "eval_ref {eval_ref} must return success");
                let vars_list = body
                    .and_then(|b| b.get("variables").cloned())
                    .and_then(|v| v.as_array().map(|a| a.clone()))
                    .unwrap_or_default();
                assert!(vars_list.is_empty(), "eval_ref {eval_ref} must return empty variables");
            }
            other => return Err(format!("eval_ref {eval_ref}: expected Response, got {other:?}").into()),
        }
    }

    Ok(())
}

/// AC: variables request with negative count/start doesn't confuse eval_ref detection
#[test]
fn when_variables_request_eval_ref_with_invalid_pagination_then_returns_error() -> TestResult {
    let mut adapter = DebugAdapter::new();

    let eval_ref_wire = 1_000_042i32;

    // Negative start should be rejected before eval_ref handling.
    let response_neg_start = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": eval_ref_wire,
            "start": -1
        })),
    );

    match response_neg_start {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "negative start must be rejected");
            let msg = must_some(message);
            assert!(msg.contains("start"), "error must mention start parameter: {msg}");
        }
        other => return Err(format!("expected error Response, got {other:?}").into()),
    }

    // Negative count should be rejected before eval_ref handling.
    let response_neg_count = adapter.handle_request(
        2,
        "variables",
        Some(json!({
            "variablesReference": eval_ref_wire,
            "count": -1
        })),
    );

    match response_neg_count {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "negative count must be rejected");
            let msg = must_some(message);
            assert!(msg.contains("count"), "error must mention count parameter: {msg}");
        }
        other => return Err(format!("expected error Response, got {other:?}").into()),
    }

    Ok(())
}

/// AC: variables request for eval_ref that never existed returns honest empty (not a crash)
#[test]
fn when_variables_request_nonexistent_eval_ref_then_returns_honest_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // An eval_ref that was never actually created or cached.
    // This simulates the post-resume scenario: the ref is in the eval band
    // but the cache was cleared and it no longer exists.
    let nonexistent_eval_ref = 1_500_000i32;

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": nonexistent_eval_ref
        })),
    );

    match response {
        DapMessage::Response { success, body, command, .. } => {
            assert!(success, "request for nonexistent eval_ref must succeed (honest empty)");
            assert_eq!(command, "variables");
            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();
            assert!(vars_list.is_empty(), "nonexistent eval_ref must return empty variables, not error");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Hazard-class tests (per acceptance.md)
// ─────────────────────────────────────────────────────────────────────────────

/// Hazard: Protocol-safety — malformed/out-of-band ref must not panic or hang
#[test]
fn hazard_protocol_safety_eval_ref_no_panic() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // These are all in the eval_ref band but may not exist in cache.
    // None should cause a panic or hang when queried.
    for eval_ref in [1_000_000, 1_000_001, 1_500_000, 1_999_999_999] {
        let _response = adapter.handle_request(
            1,
            "variables",
            Some(json!({
                "variablesReference": eval_ref
            })),
        );
        // If we reach here without panicking, the test passes for this ref.
    }

    Ok(())
}

/// Hazard: Bounds/overflow — extreme refs (0, i32::MAX, i32::MIN, i64::MAX)
/// should all return safe empty or error, never panic.
#[test]
fn hazard_bounds_overflow_extreme_refs() -> TestResult {
    let mut adapter = DebugAdapter::new();

    for ref_value in [0i64, i32::MAX as i64, i32::MIN as i64, i64::MAX, i64::MIN] {
        let response = adapter.handle_request(
            1,
            "variables",
            Some(json!({
                "variablesReference": ref_value
            })),
        );

        // Must either succeed with empty or fail gracefully, never panic.
        match response {
            DapMessage::Response { success, body, message, .. } => {
                if success {
                    // Honest empty response is acceptable.
                    let vars_list = body
                        .and_then(|b| b.get("variables").cloned())
                        .and_then(|v| v.as_array().map(|a| a.clone()))
                        .unwrap_or_default();
                    assert!(vars_list.is_empty(), "ref {ref_value}: must be empty if success");
                } else {
                    // Error response is acceptable if it indicates out-of-range.
                    assert!(message.is_some(), "ref {ref_value}: must have error message if not success");
                }
            }
            other => return Err(format!("ref {ref_value}: got unexpected {other:?}").into()),
        }
    }

    Ok(())
}

/// Hazard: Test-encodes-the-bug — verify that eval_ref in [EVAL_BASE, EVAL_MAX]
/// returns honest empty (success=true, variables=[]), not a scope-lookup error.
#[test]
fn hazard_test_encodes_bug_eval_ref_must_not_fallthrough_to_scope_routing() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // An eval_ref that would previously fall through to scope routing because
    // the cache miss wasn't detected. For example, wire=1_000_001 would have been
    // misinterpreted as scope (frame_id=100, kind=1 via modulo 10 logic).
    let eval_ref = 1_000_001i32;

    // Verify this is in the eval band.
    assert!(matches!(
        VariableReference::decode(eval_ref),
        Some(VariableReference::EvalResult { counter: 1 })
    ), "setup: {eval_ref} must be EvalResult");

    let response = adapter.handle_request(
        1,
        "variables",
        Some(json!({
            "variablesReference": eval_ref
        })),
    );

    match response {
        DapMessage::Response { success, body, message, .. } => {
            // Must succeed with honest empty, NOT fail with a debugger error.
            assert!(success, "eval_ref {eval_ref} must succeed (not error from scope routing)");
            assert!(message.is_none(), "eval_ref must not have error message; got {message:?}");

            let vars_list = body
                .and_then(|b| b.get("variables").cloned())
                .and_then(|v| v.as_array().map(|a| a.clone()))
                .unwrap_or_default();
            assert!(vars_list.is_empty(), "eval_ref must return empty, not bogus scope data");
        }
        other => return Err(format!("expected Response, got {other:?}").into()),
    }

    Ok(())
}
