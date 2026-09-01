//! setExpression capability floor tests (#9568).
//!
//! `supportsSetExpression` is a promise that a `setExpression` request performs
//! an exact current-frame l-value assignment with bounded admission, broker
//! acknowledgement, and read-back currentness. That proof does not exist yet
//! (#9570 owns the promotion boundary), so the capability must be advertised
//! false and every request must be refused after envelope validation only —
//! before expression screening, session lookup, or any debugger write.
//!
//! These tests bind the wire behavior to the single advertised-value authority
//! so catalog files, handler presence, or `setVariable`/`evaluate` evidence
//! cannot widen the field.

use perl_dap::backend::capabilities::{
    SET_EXPRESSION_UNSUPPORTED_MESSAGE, advertises_set_expression,
};
use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::sync::mpsc::sync_channel;

fn create_test_adapter() -> DebugAdapter {
    let (tx, _rx) = sync_channel(64);
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    adapter
}

fn extract_initialize_response(msg: DapMessage) -> Result<Value, Box<dyn std::error::Error>> {
    match msg {
        DapMessage::Response { success, command, body, .. } => {
            if command == "initialize" && success {
                body.ok_or_else(|| "initialize response missing body".into())
            } else {
                Err("initialize response not successful".into())
            }
        }
        other => Err(format!("expected initialize response, got {other:?}").into()),
    }
}

fn capability(caps: &Value, name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    caps.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing boolean capability `{name}`").into())
}

fn refused_set_expression(response: DapMessage) -> Result<String, Box<dyn std::error::Error>> {
    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert_eq!(command, "setExpression");
            assert!(!success, "setExpression must fail while the floor is closed");
            assert!(body.is_none(), "a refused request must not allocate a result body");
            message.ok_or_else(|| "refused setExpression must explain why".into())
        }
        other => Err(format!("expected setExpression response, got {other:?}").into()),
    }
}

/// The initialize wire value comes from the single setExpression authority.
///
/// Discriminating against the previous wiring: `dap.core` is advertised, so
/// `supportsSetExpression: supports_core` would report true and fail both
/// assertions here.
#[test]
fn initialize_advertises_set_expression_from_the_single_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let caps = extract_initialize_response(adapter.handle_request(1, "initialize", None))?;

    assert_eq!(
        capability(&caps, "supportsSetExpression")?,
        advertises_set_expression(),
        "supportsSetExpression must be the single-authority value (#9568)"
    );
    assert!(
        !capability(&caps, "supportsSetExpression")?,
        "setExpression stays closed until the #9570 promotion boundary passes"
    );
    assert_ne!(
        capability(&caps, "supportsSetExpression")?,
        perl_dap::feature_catalog::has_feature("dap.core"),
        "the wire value must not ride on the dap.core catalog row anymore"
    );
    Ok(())
}

/// A well-formed request is refused with the exact authority message.
#[test]
fn well_formed_set_expression_is_refused_deterministically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let response = adapter.handle_request(
        1,
        "setExpression",
        Some(json!({"expression": "$x", "value": "42"})),
    );
    let message = refused_set_expression(response)?;
    assert_eq!(
        message, SET_EXPRESSION_UNSUPPORTED_MESSAGE,
        "the refusal must be the single deterministic authority message"
    );
    Ok(())
}

/// Every request shape that passes envelope validation is refused identically.
///
/// Input type, frame, format, emptiness, hostility — none of it may widen or
/// reshape the gate.
#[test]
fn every_input_shape_receives_the_same_refusal() -> Result<(), Box<dyn std::error::Error>> {
    let shapes = [
        json!({"expression": "$x", "value": "42"}),
        json!({"expression": "$x", "value": "42", "frameId": 0}),
        json!({"expression": "$x", "value": "42", "frameId": 7}),
        json!({"expression": "$x", "value": "42", "format": {"hex": true}}),
        json!({"expression": "", "value": "42"}),
        json!({"expression": "$x", "value": ""}),
        json!({"expression": "$x\nsystem('id')", "value": "42"}),
        json!({"expression": "$x; system('id')", "value": "42"}),
        json!({"expression": "$hash{key}", "value": "$other"}),
    ];

    for shape in shapes {
        let mut adapter = create_test_adapter();
        let response = adapter.handle_request(1, "setExpression", Some(shape.clone()));
        let message = refused_set_expression(response)
            .map_err(|error| format!("shape {shape} was not refused uniformly: {error}"))?;
        assert_eq!(
            message, SET_EXPRESSION_UNSUPPORTED_MESSAGE,
            "shape {shape} must receive the deterministic authority refusal"
        );
    }
    Ok(())
}

/// Envelope validation still owns malformed arguments.
///
/// The gate sits *after* envelope validation: a request with no arguments at
/// all fails the envelope layer, not the capability gate.
#[test]
fn missing_arguments_still_fail_at_the_envelope_layer() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let response = adapter.handle_request(1, "setExpression", None);
    let message = refused_set_expression(response)?;
    assert_eq!(
        message, "Missing arguments",
        "malformed envelopes are an envelope-layer failure, not a capability refusal"
    );
    Ok(())
}

/// A refused request leaves the adapter state untouched.
///
/// `threads` reports the identical no-session shape before and after a refused
/// `setExpression`, proving the gate allocated no session, no frame identity,
/// and no reference as a side effect.
#[test]
fn refused_request_does_not_mutate_adapter_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();

    fn no_session_threads_body(
        adapter: &mut DebugAdapter,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        match adapter.handle_request(99, "threads", None) {
            DapMessage::Response { success, body, .. } => {
                assert!(success, "threads without a session must still answer");
                body.ok_or_else(|| "threads response must carry a body".into())
            }
            other => Err(format!("expected threads response, got {other:?}").into()),
        }
    }

    let before = no_session_threads_body(&mut adapter)?;
    let response = adapter.handle_request(
        1,
        "setExpression",
        Some(json!({"expression": "$x", "value": "42"})),
    );
    refused_set_expression(response)?;
    let after = no_session_threads_body(&mut adapter)?;

    assert_eq!(
        before, after,
        "a refused setExpression must not create a session or move any state"
    );
    assert_eq!(
        before.get("threads").and_then(Value::as_array).map(Vec::len),
        Some(0),
        "no synthetic thread may appear before a session exists"
    );
    Ok(())
}

/// Refusal is idempotent: repeated requests cannot drift the adapter.
#[test]
fn repeated_refusals_are_identical() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({"expression": "$x", "value": "42"});

    let first =
        refused_set_expression(adapter.handle_request(1, "setExpression", Some(args.clone())))?;
    let second = refused_set_expression(adapter.handle_request(2, "setExpression", Some(args)))?;
    assert_eq!(first, second, "repeated refusals must be byte-identical");
    Ok(())
}

/// Only the setExpression cell moved to the authority; sibling mutation and
/// data-breakpoint cells keep their existing catalog-driven wiring.
#[test]
fn sibling_capability_cells_are_independent() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let caps = extract_initialize_response(adapter.handle_request(1, "initialize", None))?;

    assert_eq!(
        capability(&caps, "supportsSetVariable")?,
        perl_dap::feature_catalog::has_feature("dap.core"),
        "supportsSetVariable keeps its catalog-driven cell; this PR must not move it"
    );
    assert_eq!(
        capability(&caps, "supportsDataBreakpoints")?,
        perl_dap::feature_catalog::has_feature("dap.watchpoints"),
        "supportsDataBreakpoints keeps its own cell (#9091 owns that floor)"
    );
    assert!(
        !capability(&caps, "supportsEvaluateForHovers")?,
        "the hover floor (#9573) is untouched by the setExpression floor"
    );
    Ok(())
}
