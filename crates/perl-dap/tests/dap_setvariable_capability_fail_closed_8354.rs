//! #8354 — `supportsSetVariable` must stay false until an exact native-launch
//! mutation path is proven, and every `setVariable` request must be refused by
//! the early capability gate before it can reach target screening, the scalar
//! RHS parser, the broker, or any debugger bytes.
//!
//! # Why these assertions are shaped this way
//!
//! A naive "a setVariable request fails" assertion would be **vacuous**: with
//! no debugger session attached, every `setVariable` request fails anyway. So
//! each test below discriminates on *which* refusal fired and *how early*:
//!
//! * a perfectly well-formed request must come back with the **#8354**
//!   refusal, not a session error — proving the gate fires before backend work;
//! * a hostile or structured-looking value (`1; system('id')`, newlines,
//!   non-sigil names) must come back with the **same** refusal — proving the
//!   gate runs before name/value screening and that input shape cannot select
//!   a deeper path;
//! * a request whose arguments are missing entirely must get the **same**
//!   refusal — proving the gate runs before `parse_dap_arguments`, i.e. only
//!   the protocol envelope work needed to identify the request happens;
//! * refusal must be unconditional on the argument contents, so a request that
//!   would fail the sigil check cannot be used as a side channel to confirm
//!   that screening code still runs.
//!
//! # What this file does NOT prove
//!
//! Every test here drives an adapter that was never launched or attached, so
//! there is no live perl5db process to observe. That bounds three claims:
//!
//! * "zero broker/debugger bytes" is established by *which refusal fires* and
//!   by source ordering (the gate precedes every parser/broker call in
//!   `handle_set_variable`), not by instrumenting a real debugger's stdin;
//! * same-session receipts against the real `--stdio` binary with a stopped
//!   session (issue tests 5, 9) belong to the promotion train (#8368/#7366)
//!   and are deliberately not implemented here;
//! * per-mode cell independence for external peers is asserted at the
//!   negotiation layer (`capabilities.rs`), not against a live peer bridge.

#![cfg(feature = "dap-phase2")]

use anyhow::Result;
use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::sync::mpsc::sync_channel;

/// Substring identifying the #8354 setVariable refusal specifically, as
/// opposed to any other reason a `setVariable` request can fail.
const SET_VARIABLE_REFUSAL_MARKER: &str = "supportsSetVariable";

fn adapter() -> DebugAdapter {
    let (tx, _rx) = sync_channel(64);
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    adapter
}

fn initialized_adapter() -> DebugAdapter {
    let mut adapter = adapter();
    let _ = adapter.handle_request(1, "initialize", None);
    adapter
}

struct SetVariableOutcome {
    success: bool,
    message: String,
    body: Option<Value>,
}

fn set_variable(adapter: &mut DebugAdapter, args: Option<Value>) -> Result<SetVariableOutcome> {
    match adapter.handle_request(2, "setVariable", args) {
        DapMessage::Response { success, command, message, body, .. } => {
            assert_eq!(command, "setVariable", "response must be for the setVariable request");
            Ok(SetVariableOutcome { success, message: message.unwrap_or_default(), body })
        }
        other => anyhow::bail!("expected a setVariable response, got {other:?}"),
    }
}

fn initialize_body(adapter: &mut DebugAdapter) -> Result<Value> {
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        other => anyhow::bail!("expected a successful initialize response, got {other:?}"),
    }
}

fn assert_set_variable_refusal(outcome: &SetVariableOutcome, what: &str) {
    assert!(!outcome.success, "setVariable must not succeed ({what})");
    assert!(
        outcome.message.contains(SET_VARIABLE_REFUSAL_MARKER),
        "{what}: expected the #8354 setVariable refusal, got {:?}. A different refusal means \
         the capability gate did not fire and the request reached deeper machinery.",
        outcome.message
    );
    assert!(
        outcome.body.is_none(),
        "{what}: a refused setVariable must not allocate a result body or variables reference, \
         got {:?}",
        outcome.body
    );
}

// ---------------------------------------------------------------------------
// Capability floor
// ---------------------------------------------------------------------------

/// Test 1: native `initialize` must report `supportsSetVariable: false`.
#[tokio::test]
async fn native_initialize_advertises_set_variable_false() -> Result<()> {
    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    assert_eq!(
        body.get("supportsSetVariable").and_then(Value::as_bool),
        Some(false),
        "#8354: setVariable must be advertised false while no exact mutation path exists; \
         body was {body:?}"
    );
    Ok(())
}

/// Test 7: sibling mutation surfaces cannot widen setVariable.
///
/// `supportsSetExpression` stays available (its own #7363 evidence row); that
/// must not drag setVariable up with it.
#[tokio::test]
async fn set_expression_capability_does_not_promote_set_variable() -> Result<()> {
    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    let set_expression = body.get("supportsSetExpression").and_then(Value::as_bool);
    assert_eq!(
        set_expression,
        Some(true),
        "precondition: setExpression is advertised, so the setVariable assertion below is \
         discriminating"
    );
    assert_eq!(
        body.get("supportsSetVariable").and_then(Value::as_bool),
        Some(false),
        "setVariable must not ride along with the setExpression capability (#8354)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Request gate — the ordering proofs
// ---------------------------------------------------------------------------

/// Test 2: a well-formed request is refused, and refused as *setVariable*.
#[tokio::test]
async fn set_variable_request_is_refused_with_the_set_variable_reason() -> Result<()> {
    let mut adapter = initialized_adapter();
    let outcome = set_variable(
        &mut adapter,
        Some(json!({ "variablesReference": 1, "name": "$x", "value": "42" })),
    )?;
    assert_set_variable_refusal(&outcome, "well-formed scalar assignment");
    Ok(())
}

/// Test 3: the gate runs before name/value screening.
///
/// Each of these would be rejected by the handler's own admission checks
/// (sigil check, newline check, statement-separator check). Getting the
/// *#8354* message instead proves the capability gate fired first, so no
/// target screening or RHS parsing ran at all.
#[tokio::test]
async fn set_variable_gate_precedes_target_and_value_screening() -> Result<()> {
    let mut adapter = initialized_adapter();

    let hostile = [
        ("non-sigil name", json!({ "variablesReference": 1, "name": "notasigil", "value": "1" })),
        (
            "statement-separator value",
            json!({ "variablesReference": 1, "name": "$x", "value": "1; system('id')" }),
        ),
        ("backtick value", json!({ "variablesReference": 1, "name": "$x", "value": "`id`" })),
        (
            "newline in value",
            json!({ "variablesReference": 1, "name": "$x", "value": "1\nevil()" }),
        ),
        (
            "structured hash replacement",
            json!({ "variablesReference": 999_999, "name": "%ENV", "value": "{}" }),
        ),
        (
            "object-shaped format rider",
            json!({
                "variablesReference": 1,
                "name": "$x",
                "value": "1",
                "format": { "hex": true, "line": false }
            }),
        ),
    ];

    for (what, args) in hostile {
        let outcome = set_variable(&mut adapter, Some(args))?;
        assert_set_variable_refusal(&outcome, what);
        assert!(
            !outcome.message.contains("Invalid variable name")
                && !outcome.message.contains("unsafe value rejected")
                && !outcome.message.contains("cannot contain newlines")
                && !outcome.message.contains("No debugger session"),
            "{what}: admission screening or session lookup ran before the #8354 capability \
             gate; the gate must be first so setVariable never reaches any deeper machinery"
        );
    }
    Ok(())
}

/// Test 2/4 falsifier: arguments are not even parsed before the gate.
///
/// A missing arguments payload cannot satisfy `parse_dap_arguments`, so the
/// generic parse failure would be a different refusal. Receiving the #8354
/// refusal proves only the envelope work needed to *identify* the request ran.
#[tokio::test]
async fn set_variable_gate_precedes_argument_parsing() -> Result<()> {
    let mut adapter = initialized_adapter();

    let outcome = set_variable(&mut adapter, None)?;
    assert_set_variable_refusal(&outcome, "missing arguments");

    let outcome = set_variable(&mut adapter, Some(json!({})))?;
    assert_set_variable_refusal(&outcome, "empty arguments object");

    let outcome = set_variable(&mut adapter, Some(json!("not-an-object")))?;
    assert_set_variable_refusal(&outcome, "malformed arguments payload");

    Ok(())
}

/// Test 6: backend/mode cells stay independent of the refusal.
///
/// The refusal is a pure function of the advertised capability, not of any
/// backend state, so two adapters that never launched anything produce byte
/// identical refusals. A session-shaped or backend-specific refusal here would
/// mean the gate leaked mode state into the floor.
#[tokio::test]
async fn refusal_is_identical_across_adapters_and_requests() -> Result<()> {
    let mut first = initialized_adapter();
    let mut second = initialized_adapter();

    let a = set_variable(
        &mut first,
        Some(json!({ "variablesReference": 1, "name": "$a", "value": "1" })),
    )?;
    let b = set_variable(
        &mut second,
        Some(json!({ "variablesReference": 42, "name": "@arr", "value": "(1, 2)" })),
    )?;

    assert_eq!(a.message, b.message, "the refusal must be the one deterministic floor message");
    Ok(())
}

/// Test 5: a refused setVariable allocates no reference and leaves the
/// session usable.
#[tokio::test]
async fn refused_set_variable_leaves_the_adapter_usable() -> Result<()> {
    let mut adapter = initialized_adapter();

    for i in 0..5 {
        let outcome = set_variable(
            &mut adapter,
            Some(json!({ "variablesReference": i + 1, "name": format!("$x{i}"), "value": "1" })),
        )?;
        assert_set_variable_refusal(&outcome, "reference allocation probe");
        assert!(
            outcome.body.is_none(),
            "a refused setVariable must not return a variablesReference"
        );
    }

    // The adapter still answers subsequent requests normally rather than being
    // wedged or terminated by the refusals.
    match adapter.handle_request(3, "threads", Some(json!({}))) {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "threads", "adapter still routes requests after refusals");
        }
        other => {
            anyhow::bail!("expected a threads response after refused setVariable, got {other:?}")
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls — the gate must be scoped
// ---------------------------------------------------------------------------

/// Falsifier: sibling mutation surfaces must not be swept up by the gate.
///
/// These requests still fail (no debugger session is attached), but they must
/// fail for their *own* reason, never with the setVariable refusal.
#[tokio::test]
async fn sibling_requests_are_not_refused_as_set_variable() -> Result<()> {
    let mut adapter = initialized_adapter();

    let set_expression = adapter.handle_request(
        2,
        "setExpression",
        Some(json!({ "frameId": 0, "expression": "$x", "value": "1" })),
    );
    match set_expression {
        DapMessage::Response { message, .. } => {
            let message = message.unwrap_or_default();
            assert!(
                !message.contains(SET_VARIABLE_REFUSAL_MARKER),
                "setExpression must keep its own admission path, got {message:?}"
            );
        }
        other => anyhow::bail!("expected a setExpression response, got {other:?}"),
    }

    let variables =
        adapter.handle_request(3, "variables", Some(json!({ "variablesReference": 1 })));
    match variables {
        DapMessage::Response { message, .. } => {
            let message = message.unwrap_or_default();
            assert!(
                !message.contains(SET_VARIABLE_REFUSAL_MARKER),
                "read-only variables must keep its own path, got {message:?}"
            );
        }
        other => anyhow::bail!("expected a variables response, got {other:?}"),
    }
    Ok(())
}
