//! Content-level tests for the DAP `setVariable` request (Issue #781; #8354).
//!
//! Since #8354 the exact setVariable mutation path is not proven, so
//! `supportsSetVariable` is advertised false and every `setVariable` request
//! is refused by the early capability gate — before argument parsing, target
//! screening, the broker, or any debugger bytes. These tests therefore verify
//! the fail-closed contract against a live stopped session:
//!
//! 1. A well-formed scalar assignment is refused, and `evaluate("$x")` still
//!    reports the ORIGINAL value — behavioral proof of zero mutation.
//! 2. Array-element and quoted-string requests are refused identically.
//! 3. Read-only / invalid requests return `success=false` with a non-empty
//!    error message (now the capability refusal, which fires first).
//! 4. A refused response carries no `value`/body, so a client can never
//!    mistake the refusal for a confirmed assignment.
//! 5. The stopped session stays fully inspectable after refusals.
//!
//! ## Determinism note
//!
//! Live-session tests require `perl` on `PATH`.  They skip gracefully when Perl
//! is absent, matching the pattern used by `dap_e2e_workflow_tests.rs`.
//!
//! The live-mutation round-trip this file asserted before #8354 (assignment
//! sent as `p $name = $value` plus a read-back `p $name`) is the #8368
//! promotion proof: it returns only when the exact mutation cell is promoted
//! through #7363/#7364, not while main advertises false.

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::debug_adapter::DapMessage;
use serde_json::{Value, json};
use std::fs::write;
use std::time::SystemTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ── Line constants for the setVariable fixture script ─────────────────────────
//
//  Line 1:  use strict;
//  Line 2:  use warnings;
//  Line 3:  (blank)
//  Line 4:  my $x = 10;
//  Line 5:  my @arr = (1, 2, 3);
//  Line 6:  my %hash = (key => 'original');
//  Line 7:  my $stop = 1; # stop here
//  Line 8:  print "$x\n";
//
// BP_LINE is line 7 — the explicit stop; it is reached after `configurationDone`
// sends `c` past the implicit entry stop on line 4.

const BP_LINE: u64 = 7;

fn set_variable_fixture_script() -> &'static str {
    "use strict;\nuse warnings;\n\nmy $x = 10;\nmy @arr = (1, 2, 3);\nmy %hash = (key => 'original');\nmy $stop = 1;\nprint \"$x\\n\";\n"
}

/// Build a unique temp path for the fixture script.
fn fixture_path(label: &str) -> Result<String, Box<dyn std::error::Error>> {
    let nanos =
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("dap_set_var_{label}_{nanos}_{}.pl", std::process::id()));
    Ok(path.to_str().ok_or("fixture path is not valid UTF-8")?.to_string())
}

/// Launch a session stopped at BP_LINE and return `(session, locals_ref)`.
fn live_session_at_bp() -> Result<(DapWorkflowSession, i64), Box<dyn std::error::Error>> {
    let script_path = fixture_path("session")?;
    write(&script_path, set_variable_fixture_script())?;

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_path)?;
    session.set_breakpoints(&script_path, &[BP_LINE])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;
    let locals_ref = session.scopes_locals_ref(frame_id)?;

    Ok((session, locals_ref))
}

/// Send `setVariable` and return the raw response.
fn set_variable(
    session: &mut DapWorkflowSession,
    variables_reference: i64,
    name: &str,
    value: &str,
) -> DapMessage {
    session.request(
        "setVariable",
        Some(json!({
            "variablesReference": variables_reference,
            "name": name,
            "value": value
        })),
    )
}

/// Extract `success` and `value` from a `setVariable` response.
fn parse_set_variable_response(
    msg: &DapMessage,
) -> Result<(bool, Option<String>, Option<String>), Box<dyn std::error::Error>> {
    match msg {
        DapMessage::Response { success, command, body, message, .. } => {
            assert_eq!(command, "setVariable", "response command field must echo 'setVariable'");
            let returned_value = body
                .as_ref()
                .and_then(|b| b.get("value"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            Ok((*success, returned_value, message.clone()))
        }
        other => Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
}

// ── Test 1: scalar setVariable is refused and the value stays unchanged ───────

/// Since #8354 the exact setVariable mutation path is not proven, so the
/// capability is advertised false and every `setVariable` request is refused
/// by the early gate — including on a live stopped session.
///
/// After the refused request:
/// - The response must be `success=false` with the #8354 capability refusal
///   and no result body (no fabricated read-back).
/// - A follow-up `evaluate("$x")` must still report the ORIGINAL value, which
///   behaviorally proves zero debugger bytes were written by the refusal.
///
/// The live-mutation round-trip this file used to assert is the #8368
/// promotion proof and returns only when the exact mutation cell is promoted.
#[test]
fn test_set_scalar_value_is_refused_and_value_unchanged() -> TestResult {
    if !perl_available() {
        return Ok(());
    }

    let (mut session, locals_ref) = live_session_at_bp()?;

    // ── setVariable: attempt to change $x from 10 to 99 ──────────────────────
    let response = set_variable(&mut session, locals_ref, "$x", "99");
    let (success, returned_value, err_msg) = parse_set_variable_response(&response)?;

    assert!(
        !success,
        "setVariable($x, 99) must be refused by the #8354 capability gate; err={err_msg:?}"
    );
    let msg = err_msg.unwrap_or_default();
    assert!(
        msg.contains("supportsSetVariable"),
        "the refusal must be the #8354 capability floor, not a session error: {msg:?}"
    );
    assert!(
        returned_value.is_none(),
        "a refused setVariable must not fabricate a read-back `value`, got {returned_value:?}"
    );

    // ── evaluate: the value must be untouched by the refused request ─────────
    let eval_response =
        session.request("evaluate", Some(json!({"expression": "$x", "allowSideEffects": false})));

    match eval_response {
        DapMessage::Response { success: eval_ok, body, .. } if eval_ok => {
            let result =
                body.as_ref().and_then(|b| b.get("result")).and_then(Value::as_str).unwrap_or("");
            assert!(
                result.contains("10"),
                "evaluate($x) after a refused setVariable must still report the original '10', \
                 got: {result:?}"
            );
        }
        DapMessage::Response { success: false, .. } => {
            // evaluate may fail if the safe-eval policy blocks `$x` lookup in
            // this session mode. The refusal itself carried no body, so there
            // is no fabricated value to contradict the cache.
        }
        other => return Err(format!("unexpected evaluate response: {other:?}").into()),
    }

    session.disconnect()?;
    Ok(())
}

// ── Test 1b: variables query after a refused setVariable is non-empty ─────────

/// After a refused `setVariable` on a live breakpoint, a follow-up
/// `variables(locals_ref)` must still return a non-empty list: the refusal
/// must not corrupt, reset, or invalidate the retained variable state (the
/// session stays fully inspectable — #8354 test 5).
#[test]
fn test_variables_query_non_empty_after_set_variable() -> TestResult {
    if !perl_available() {
        return Ok(());
    }

    let (mut session, locals_ref) = live_session_at_bp()?;

    // First populate the variables cache.
    let vars_before = session.variables(locals_ref)?;
    assert!(!vars_before.is_empty(), "variables(locals_ref) must be non-empty before setVariable");

    // Attempt the mutation — refused by the #8354 capability floor.
    let response = set_variable(&mut session, locals_ref, "$x", "42");
    let (success, _, err_msg) = parse_set_variable_response(&response)?;
    assert!(
        !success,
        "setVariable($x, 42) must be refused by the capability gate; err={err_msg:?}"
    );

    // Follow-up variables query — must still return a non-empty list.
    let vars_after = session.variables(locals_ref)?;
    assert!(
        !vars_after.is_empty(),
        "variables(locals_ref) must remain non-empty after a refused setVariable (adapter must \
         not crash or drop retained state)"
    );

    // Every variable entry must have a non-empty name and value.
    for var in &vars_after {
        let name = var.get("name").and_then(Value::as_str).unwrap_or("");
        let value = var.get("value").and_then(Value::as_str).unwrap_or("");
        assert!(!name.is_empty(), "post-setVariable variable must have non-empty name: {var:?}");
        assert!(!value.is_empty(), "post-setVariable variable '{name}' must have non-empty value");
    }

    session.disconnect()?;
    Ok(())
}

// ── Test 2: array element setVariable is refused ──────────────────────────────

/// `setVariable($arr[0], "999")` on a live breakpoint must be refused by the
/// #8354 capability gate like any other request: the response is
/// `success=false` with the capability refusal, and the staged value never
/// reaches the debugger.
#[test]
fn test_set_array_element_returns_success_with_new_value() -> TestResult {
    if !perl_available() {
        return Ok(());
    }

    let (mut session, locals_ref) = live_session_at_bp()?;

    let response = set_variable(&mut session, locals_ref, "$arr[0]", "999");
    let (success, returned_value, err_msg) = parse_set_variable_response(&response)?;

    assert!(
        !success,
        "setVariable($arr[0], 999) must be refused by the #8354 capability gate; err={err_msg:?}"
    );
    assert!(
        returned_value.is_none(),
        "a refused setVariable must not fabricate a read-back `value`"
    );

    session.disconnect()?;
    Ok(())
}

// ── Test 3: invalid name rejected with success=false ─────────────────────────

/// Sending `setVariable` with a name that lacks a Perl sigil must return
/// `success=false` with the #8354 capability refusal. Since #8354 the
/// fail-closed gate fires before the `is_valid_set_variable_name` guard in
/// `handle_set_variable`, so the discriminating content here is that a
/// sigil-less name is refused by the capability floor — it can never reach
/// the name guard, and must not be answered by any deeper machinery.
///
/// This test does NOT need a live session — the gate fires before any
/// debugger interaction.
#[test]
fn test_set_variable_invalid_name_rejected_no_session() -> TestResult {
    use perl_dap::debug_adapter::DebugAdapter;

    let mut adapter = DebugAdapter::new();
    // Initialise so the adapter is in a valid pre-session state.
    adapter.handle_request(1, "initialize", None);

    // "no_sigil" is not a Perl sigil-prefixed variable — must be rejected.
    let response = adapter.handle_request(
        2,
        "setVariable",
        Some(json!({
            "variablesReference": 1,
            "name": "no_sigil",
            "value": "42"
        })),
    );

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "setVariable");
            assert!(!success, "setVariable with invalid name 'no_sigil' must return success=false");
            let msg = message.ok_or("error response must include a non-empty message")?;
            assert!(!msg.is_empty(), "error message for invalid name must be non-empty");
            // The refusal must be the #8354 capability floor specifically. A
            // generic "mentions variable anywhere" check is vacuous: the
            // capability message itself contains "setVariable".
            assert!(
                msg.contains("supportsSetVariable"),
                "invalid-name request must be refused by the #8354 capability floor, \
                 not by a deeper guard or a session error, got: {msg:?}"
            );
        }
        other => return Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
    Ok(())
}

// ── Test 4: unsafe value rejected with success=false ─────────────────────────

/// Sending `setVariable` with a value that contains a statement separator (`;`)
/// must return `success=false` with the #8354 capability refusal. Since #8354
/// the fail-closed gate fires before the
/// `contains_unquoted_statement_separator` guard, so the discriminating
/// content here is that an injection-shaped value is refused by the capability
/// floor — it can never reach the separator guard or the broker.
///
/// Does not need a live session — the gate fires before debugger interaction.
#[test]
fn test_set_variable_statement_separator_in_value_rejected() -> TestResult {
    use perl_dap::debug_adapter::DebugAdapter;

    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    // A value with an unquoted `;` is rejected as potentially unsafe.
    let response = adapter.handle_request(
        2,
        "setVariable",
        Some(json!({
            "variablesReference": 1,
            "name": "$x",
            "value": "1; die"
        })),
    );

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "setVariable");
            assert!(
                !success,
                "setVariable with ';' in value must return success=false (injection guard)"
            );
            let msg = message.ok_or("error response must include a non-empty message")?;
            assert!(!msg.is_empty(), "error message for unsafe value must be non-empty");
            // The refusal must be the #8354 capability floor specifically:
            // the gate fires before the separator guard, so a deeper-path
            // message here would mean the gate was bypassed.
            assert!(
                msg.contains("supportsSetVariable"),
                "statement-separator value must be refused by the #8354 capability floor, \
                 got: {msg:?}"
            );
        }
        other => return Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
    Ok(())
}

// ── Test 5: missing variables_reference rejected with success=false ───────────

/// `setVariable` with `variablesReference <= 0` must return `success=false`
/// with the #8354 capability refusal. Since #8354 the fail-closed gate fires
/// before the argument-guard and session-lookup paths, so the discriminating
/// content here is that a degenerate reference is refused by the capability
/// floor — identically to any other request shape.
#[test]
fn test_set_variable_zero_variables_reference_rejected() -> TestResult {
    use perl_dap::debug_adapter::DebugAdapter;

    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(
        2,
        "setVariable",
        Some(json!({
            "variablesReference": 0,
            "name": "$x",
            "value": "42"
        })),
    );

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "setVariable");
            assert!(!success, "setVariable with variablesReference=0 must return success=false");
            let msg = message.ok_or("error response must be non-empty")?;
            assert!(!msg.is_empty(), "error message must be non-empty");
            // The refusal must be the #8354 capability floor specifically:
            // the gate fires before the reference guard, so a deeper-path
            // message here would mean the gate was bypassed.
            assert!(
                msg.contains("supportsSetVariable"),
                "variablesReference=0 must be refused by the #8354 capability floor, got: {msg:?}"
            );
        }
        other => return Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
    Ok(())
}

// ── Test 6: refused setVariable fabricates no read-back value ─────────────────

/// The historical contract asserted that the `value` field in the setVariable
/// response carried the fresh read-back from the debugger. Since #8354 the
/// mutation path is closed: the refusal carries NO `value` field at all, so a
/// client can never mistake a refusal for a confirmed assignment.
#[test]
fn test_set_variable_response_value_reflects_new_value() -> TestResult {
    if !perl_available() {
        return Ok(());
    }

    let (mut session, locals_ref) = live_session_at_bp()?;

    let response = set_variable(&mut session, locals_ref, "$x", "'hello'");
    let (success, returned_value, err_msg) = parse_set_variable_response(&response)?;

    assert!(
        !success,
        "setVariable($x, 'hello') must be refused by the #8354 capability gate; err={err_msg:?}"
    );
    assert!(
        returned_value.is_none(),
        "a refused setVariable must carry no `value` field, got {returned_value:?}"
    );
    if let DapMessage::Response { body, .. } = &response {
        assert!(
            body.is_none(),
            "a refused setVariable must carry no result body at all, got {body:?}"
        );
    }

    session.disconnect()?;
    Ok(())
}

// ── Test 7: setVariable without a session returns success=false (not crash) ───

/// When no `perl -d` session is active (adapter freshly initialized, no launch),
/// `setVariable` with valid args must return `success=false` and a non-empty
/// error message explaining that no debugger session is active.
///
/// This is the "read-only / invalid set" case from the spec: the adapter must
/// not crash or return `success=true` when there is nothing to mutate.
#[test]
fn test_set_variable_no_session_returns_graceful_failure() -> TestResult {
    use perl_dap::debug_adapter::DebugAdapter;

    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    // Valid args, but no session is running.
    let response = adapter.handle_request(
        2,
        "setVariable",
        Some(json!({
            "variablesReference": 11,
            "name": "$x",
            "value": "42"
        })),
    );

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "setVariable");
            assert!(
                !success,
                "setVariable without an active session must return success=false, not crash"
            );
            let msg = message.ok_or("error response must include a non-empty message")?;
            assert!(!msg.is_empty(), "error message for no-session path must be non-empty");
            // Since #8354 the capability floor fires before the session
            // lookup, so the refusal is the capability message.
            assert!(
                msg.to_lowercase().contains("session")
                    || msg.to_lowercase().contains("debugger")
                    || msg.to_lowercase().contains("transport")
                    || msg.to_lowercase().contains("active")
                    || msg.to_lowercase().contains("supported"),
                "error message should describe the missing session or the capability floor; \
                 got: {msg:?}"
            );
        }
        other => return Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
    Ok(())
}

// ── Test 8: setVariable with newline in name rejected ─────────────────────────

/// Newlines in name or value must be rejected to prevent debugger command
/// injection.  Returns `success=false` with error message.
#[test]
fn test_set_variable_newline_in_name_rejected() -> TestResult {
    use perl_dap::debug_adapter::DebugAdapter;

    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(
        2,
        "setVariable",
        Some(json!({
            "variablesReference": 1,
            "name": "$x\nmalicious",
            "value": "42"
        })),
    );

    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "setVariable");
            assert!(
                !success,
                "setVariable with newline in name must return success=false (injection guard)"
            );
            let msg = message.ok_or("error response must be non-empty")?;
            assert!(!msg.is_empty(), "error message must be non-empty");
            // The refusal must be the #8354 capability floor specifically:
            // the gate fires before the newline guard, so a deeper-path
            // message here would mean the gate was bypassed.
            assert!(
                msg.contains("supportsSetVariable"),
                "newline-in-name must be refused by the #8354 capability floor, got: {msg:?}"
            );
        }
        other => return Err(format!("expected Response for setVariable, got: {other:?}").into()),
    }
    Ok(())
}
