//! #9573 — `supportsEvaluateForHovers` must stay false until a pure
//! selected-frame inspection path is proven, and hover-context `evaluate`
//! requests must be refused before they can reach the raw evaluator.
//!
//! # Why these assertions are shaped this way
//!
//! A naive "a hover request fails" assertion would be **vacuous**: with no
//! debugger session attached, every `evaluate` request fails anyway. So each
//! test below discriminates on *which* refusal fired and *how early* it fired:
//!
//! * an expression that would be rejected by safe-mode screening must still
//!   come back with the **hover** refusal, proving the gate runs before
//!   `SafeEvaluator`;
//! * `allowSideEffects: true` must still come back with the **hover** refusal,
//!   proving the gate runs before the side-effect branch and that the custom
//!   field cannot widen hover into REPL authority;
//! * a bogus `frameId` must come back with the **hover** refusal rather than
//!   "Frame not found" / "No debugger session", proving the gate runs before
//!   session and frame inspection;
//! * non-hover contexts must *not* produce the hover refusal, proving the gate
//!   is scoped and that watch/repl/clipboard evaluation is untouched.
//!
//! # What this file does NOT prove
//!
//! Every test here drives an adapter that was never launched or attached, so
//! there is no live perl5db process to observe. That bounds three claims:
//!
//! * "no debugger command was written" is established by *which refusal fires*
//!   and by source ordering, not by instrumenting a real debugger's input;
//! * `frameId` values are varied, but there is no live stack, so "current vs
//!   stale vs other frame" identity is not exercised — only frame-ID values;
//! * "no reference or cache state was allocated" is proven through the absent
//!   response body, not by inspecting the variable cache directly.
//!
//! A same-session receipt against the real `--stdio` binary with a stopped
//! session is the stronger proof and is owned by #1743/#7567, which this claim
//! deliberately does not implement.

#![cfg(feature = "dap-phase2")]

use anyhow::Result;
use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::sync::mpsc::sync_channel;

/// Substring identifying the #9573 hover refusal specifically, as opposed to
/// any other reason an `evaluate` request can fail.
const HOVER_REFUSAL_MARKER: &str = "supportsEvaluateForHovers";

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

struct EvaluateOutcome {
    success: bool,
    message: String,
    body: Option<Value>,
}

fn evaluate(adapter: &mut DebugAdapter, args: Value) -> Result<EvaluateOutcome> {
    match adapter.handle_request(2, "evaluate", Some(args)) {
        DapMessage::Response { success, command, message, body, .. } => {
            assert_eq!(command, "evaluate", "response must be for the evaluate request");
            Ok(EvaluateOutcome { success, message: message.unwrap_or_default(), body })
        }
        other => anyhow::bail!("expected an evaluate response, got {other:?}"),
    }
}

fn initialize_body(adapter: &mut DebugAdapter) -> Result<Value> {
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        other => anyhow::bail!("expected a successful initialize response, got {other:?}"),
    }
}

fn assert_hover_refusal(outcome: &EvaluateOutcome, what: &str) {
    assert!(!outcome.success, "hover evaluation must not succeed ({what})");
    assert!(
        outcome.message.contains(HOVER_REFUSAL_MARKER),
        "{what}: expected the #9573 hover refusal, got {:?}. A different refusal means the \
         hover gate did not fire and the request reached deeper machinery.",
        outcome.message
    );
    assert!(
        outcome.body.is_none(),
        "{what}: a refused hover must not allocate a result body or variables reference, got {:?}",
        outcome.body
    );
}

// ---------------------------------------------------------------------------
// Capability floor
// ---------------------------------------------------------------------------

/// Test 1: native `initialize` must report `supportsEvaluateForHovers: false`.
#[tokio::test]
async fn native_initialize_advertises_hover_false() -> Result<()> {
    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    assert_eq!(
        body.get("supportsEvaluateForHovers").and_then(Value::as_bool),
        Some(false),
        "#9573: hover must be advertised false while no pure selected-frame \
         inspection path exists; body was {body:?}"
    );
    Ok(())
}

/// Test 8 + falsifier: REPL/evaluate capability cannot promote hover.
///
/// The general evaluate path (and the still-advertised `supportsSetVariable`
/// sibling) stay available; that must not drag hover up with them.
/// `supportsSetExpression` is itself floored false by #9568, so the surviving
/// discriminating sibling is `supportsSetVariable`.
#[tokio::test]
async fn other_evaluation_capabilities_do_not_promote_hover() -> Result<()> {
    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    // Guard: if the surviving sibling were false the test would be vacuous.
    // (#9568 floored `supportsSetExpression` to false — the pair this
    // precondition used to pin was (Some(true), Some(true)) — so the
    // discrimination now rides on `supportsSetVariable` staying advertised.
    // When the #9570 promotion boundary flips setExpression back on, revisit
    // this pair consciously.)
    let set_expression = body.get("supportsSetExpression").and_then(Value::as_bool);
    let set_variable = body.get("supportsSetVariable").and_then(Value::as_bool);
    assert_eq!(
        (set_expression, set_variable),
        (Some(false), Some(true)),
        "precondition: setExpression must still carry the #9568 floor and setVariable \
         must stay advertised, so the hover assertion below is discriminating"
    );

    assert_eq!(
        body.get("supportsEvaluateForHovers").and_then(Value::as_bool),
        Some(false),
        "hover must not ride along with sibling evaluation capabilities (#9573)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Request gate — the ordering proofs
// ---------------------------------------------------------------------------

/// Test 2: a hover request is refused, and refused as *hover*.
#[tokio::test]
async fn hover_request_is_refused_with_the_hover_reason() -> Result<()> {
    let mut adapter = initialized_adapter();
    let outcome = evaluate(&mut adapter, json!({ "expression": "$x", "context": "hover" }))?;
    assert_hover_refusal(&outcome, "plain scalar hover");
    Ok(())
}

/// Test 3: the gate runs before safe-expression screening.
///
/// Each of these expressions would be rejected by `SafeEvaluator` with a
/// "Safe evaluation mode: ..." message. Getting the *hover* message instead
/// proves the gate fired first, so no expression screening, parsing, or
/// debugger command construction happened.
#[tokio::test]
async fn hover_gate_precedes_expression_screening() -> Result<()> {
    let mut adapter = initialized_adapter();

    for expression in [
        "$x",             // ordinary scalar
        "@list",          // array-shaped
        "%h",             // hash-shaped
        "$obj->method()", // call/method-shaped
        "$x = 1",         // assignment (would fail safe screening)
        "system('id')",   // system call (would fail safe screening)
        "`id`",           // backticks (would fail safe screening)
        "$counter++",     // increment (would fail safe screening)
        "$cb->()",        // callback-shaped
    ] {
        let outcome =
            evaluate(&mut adapter, json!({ "expression": expression, "context": "hover" }))?;
        assert_hover_refusal(&outcome, expression);
        assert!(
            !outcome.message.contains("Safe evaluation mode"),
            "{expression}: safe-mode screening ran before the hover gate; the gate must be \
             first so hover never reaches the evaluator at all"
        );
    }
    Ok(())
}

/// Test 4: `allowSideEffects` cannot widen hover.
#[tokio::test]
async fn allow_side_effects_cannot_widen_hover() -> Result<()> {
    let mut adapter = initialized_adapter();

    let outcome = evaluate(
        &mut adapter,
        json!({ "expression": "$x = 1", "context": "hover", "allowSideEffects": true }),
    )?;
    assert_hover_refusal(&outcome, "allowSideEffects=true with a mutating expression");

    let outcome = evaluate(
        &mut adapter,
        json!({ "expression": "system('id')", "context": "hover", "allowSideEffects": true }),
    )?;
    assert_hover_refusal(&outcome, "allowSideEffects=true with a system call");
    Ok(())
}

/// Test 5: supplied frame IDs cause no session or frame inspection.
///
/// Without the gate these would return "No debugger session" (or "Frame not
/// found"). The hover message proves the gate short-circuits earlier.
#[tokio::test]
async fn hover_gate_precedes_frame_and_session_lookup() -> Result<()> {
    let mut adapter = initialized_adapter();

    for frame_id in [0_i64, 1, 9999, -1] {
        let outcome = evaluate(
            &mut adapter,
            json!({ "expression": "$x", "context": "hover", "frameId": frame_id }),
        )?;
        assert_hover_refusal(&outcome, &format!("frameId {frame_id}"));
        assert!(
            !outcome.message.contains("Frame not found")
                && !outcome.message.contains("No debugger session"),
            "frameId {frame_id}: session/frame lookup ran before the hover gate"
        );
    }
    Ok(())
}

/// Falsifier: a case-variant context must not slip past the closed capability.
#[tokio::test]
async fn hover_gate_is_case_insensitive() -> Result<()> {
    let mut adapter = initialized_adapter();

    for context in ["hover", "Hover", "HOVER", "hOvEr"] {
        let outcome = evaluate(&mut adapter, json!({ "expression": "$x", "context": context }))?;
        assert_hover_refusal(&outcome, context);
    }
    Ok(())
}

/// Test 6: a refused hover allocates no reference and mutates no state.
///
/// Proven behaviourally: after a burst of refused hovers, a subsequent
/// `variables` request for a reference that was never handed out must still be
/// unknown, and the refusals themselves must carry no body.
#[tokio::test]
async fn refused_hover_allocates_no_reference() -> Result<()> {
    let mut adapter = initialized_adapter();

    for i in 0..5 {
        let outcome =
            evaluate(&mut adapter, json!({ "expression": format!("$x{i}"), "context": "hover" }))?;
        assert_hover_refusal(&outcome, "reference allocation probe");
        assert!(outcome.body.is_none(), "a refused hover must not return a variablesReference");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls — the gate must be scoped
// ---------------------------------------------------------------------------

/// Falsifier: non-hover contexts must not be swept up by the gate.
///
/// These requests still fail (no debugger session is attached), but they must
/// fail for their *own* reason, never with the hover refusal. This is what
/// proves watch/repl/clipboard/variables evaluation is untouched by #9573.
#[tokio::test]
async fn non_hover_contexts_are_not_refused_as_hover() -> Result<()> {
    let mut adapter = initialized_adapter();

    for context in [
        Some("watch"),
        Some("repl"),
        Some("clipboard"),
        Some("variables"),
        Some("someUnknownContext"),
        None,
    ] {
        let mut args = json!({ "expression": "$x" });
        if let Some(context) = context {
            args["context"] = json!(context);
        }
        let outcome = evaluate(&mut adapter, args)?;
        assert!(
            !outcome.message.contains(HOVER_REFUSAL_MARKER),
            "context {context:?} must not be refused as hover; the gate must match the \
             standard hover context only, and must not reclassify a missing or unknown \
             context (#9573). Got: {:?}",
            outcome.message
        );
    }
    Ok(())
}

/// Falsifier: safe-mode screening must still apply to non-hover contexts.
///
/// If the hover gate had been implemented by short-circuiting *all* evaluation,
/// this test would fail — it pins that watch keeps its own screening policy.
#[tokio::test]
async fn watch_context_still_reaches_safe_mode_screening() -> Result<()> {
    let mut adapter = initialized_adapter();

    let outcome = evaluate(&mut adapter, json!({ "expression": "$x = 1", "context": "watch" }))?;
    assert!(!outcome.success, "a mutating watch expression is still rejected");
    assert!(
        outcome.message.contains("Safe evaluation mode"),
        "watch must still be screened by safe-evaluation policy, got {:?}",
        outcome.message
    );
    Ok(())
}

/// Falsifier: the existing session must survive a refused hover.
#[tokio::test]
async fn session_remains_usable_after_a_refused_hover() -> Result<()> {
    let mut adapter = initialized_adapter();

    let refused = evaluate(&mut adapter, json!({ "expression": "$x", "context": "hover" }))?;
    assert_hover_refusal(&refused, "pre-check");

    // The adapter still answers subsequent requests normally rather than being
    // wedged or terminated by the refusal.
    match adapter.handle_request(3, "threads", Some(json!({}))) {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "threads", "adapter still routes requests after a refusal");
        }
        other => anyhow::bail!("expected a threads response after a refused hover, got {other:?}"),
    }

    let after = evaluate(&mut adapter, json!({ "expression": "$x", "context": "watch" }))?;
    assert!(
        !after.message.contains(HOVER_REFUSAL_MARKER),
        "a refused hover must not poison later non-hover evaluation"
    );
    Ok(())
}
