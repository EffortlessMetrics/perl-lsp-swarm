//! #9578 — the optional breakpoint capabilities
//! (`supportsFunctionBreakpoints`, `supportsConditionalBreakpoints`,
//! `supportsHitConditionalBreakpoints`, `supportsLogPoints`) must stay false
//! until their exact runtime contracts are proven, and every request shape
//! that depends on them must be refused before it can mutate state.
//!
//! # Why these assertions are shaped this way
//!
//! The negative direction is live, not simulated: the shipped catalog
//! registers AND advertises `dap.core` and every `dap.breakpoints.*` row this
//! claim floors, so each wire-false assertion below runs against a catalog
//! that authorizes the capability. If the advertisement ever re-derives from
//! a catalog flag, handler presence, or another mode's receipt, these tests
//! fail.
//!
//! A naive "a function breakpoint request fails" assertion would be vacuous:
//! with no debugger session attached, almost any stateful request can fail.
//! So each refusal test discriminates on *which* response fires:
//!
//! * a request mixing previously-valid and previously-invalid function names
//!   must receive the **identical** capability refusal, proving the gate runs
//!   ahead of the name-validation loop;
//! * a plain source-breakpoint entry in the same adapter must keep its
//!   independent contract, proving the floor did not widen into the base cell;
//! * a `condition` entry must come back unverified with the **conditional**
//!   refusal marker — never silently stripped into an installed unconditional
//!   breakpoint;
//! * rejected entries must occupy their request slot with the exact
//!   field-specific reason, preserving one response breakpoint per input in
//!   request order.
//!
//! # What this file does NOT prove
//!
//! Every test here drives an adapter that was never launched or attached, so
//! there is no live perl5db process to observe. "No debugger command was
//! written" is established by which response fires and by source ordering,
//! not by instrumenting a real debugger's stdin; store-level non-mutation for
//! fully-rejected requests is pinned by the crate-internal tests in
//! `debug_adapter::tests`. A same-session receipt against the real `--stdio`
//! binary is the stronger proof and is owned by #7366, which this claim
//! deliberately does not implement.

#![cfg(feature = "dap-phase2")]

#[path = "common/dap_core_capability_witnesses.rs"]
mod dap_core_capability_witnesses;

use anyhow::Result;
use dap_core_capability_witnesses::{
    DAP_CORE_DERIVED_TRUE_SIBLINGS, FORMER_TRUE_SIBLINGS_NOW_FLOORED, VALUE_FORMAT_FLOOR_FIELD,
    assert_capability_bool, assert_capability_is_json_boolean,
};
use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use perl_dap::feature_catalog::has_feature;
use serde_json::{Value, json};
use std::io::Write;
use std::sync::mpsc::sync_channel;
use tempfile::NamedTempFile;

/// Substring identifying the #9578 function-breakpoints refusal specifically.
const FUNCTION_FLOOR_MARKER: &str = "supportsFunctionBreakpoints";
/// Substring identifying the #9578 conditional-breakpoints per-item refusal.
const CONDITION_FLOOR_MARKER: &str = "supportsConditionalBreakpoints";
/// Substring identifying the #9578 hit-condition per-item refusal.
const HIT_CONDITION_FLOOR_MARKER: &str = "supportsHitConditionalBreakpoints";
/// Substring identifying the #9578 logpoint per-item refusal.
const LOG_MESSAGE_FLOOR_MARKER: &str = "supportsLogPoints";

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

fn initialize_body(adapter: &mut DebugAdapter) -> Result<Value> {
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        other => anyhow::bail!("expected a successful initialize response, got {other:?}"),
    }
}

fn write_perl_file() -> Result<(tempfile::TempPath, String)> {
    let mut temp = NamedTempFile::with_suffix(".pl").map_err(|e| anyhow::anyhow!("{e}"))?;
    temp.write_all(b"my $x = 1;\nmy $y = 2;\nmy $z = $x + $y;\nprint $z;\n")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let path = temp.into_temp_path();
    let path_string = path.to_string_lossy().to_string();
    Ok((path, path_string))
}

fn response_breakpoints(body: &Value) -> Result<&Vec<Value>> {
    body.get("breakpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("setBreakpoints body must carry a breakpoints array"))
}

// ---------------------------------------------------------------------------
// Advertisement floor
// ---------------------------------------------------------------------------

/// All four optional capability rows must be false even though the shipped
/// catalog registers and advertises their backing rows (#9578).
#[test]
fn all_four_optional_breakpoint_capabilities_are_false_while_catalog_advertises() -> Result<()> {
    // Negative-direction precondition: the catalog authorizes every floored
    // row. If a catalog row is ever un-advertised, this precondition (not the
    // floor) is what changed.
    assert!(has_feature("dap.core"), "precondition: dap.core is advertised");
    assert!(has_feature("dap.breakpoints.basic"), "precondition: basic breakpoints advertised");
    assert!(
        has_feature("dap.breakpoints.hit_condition"),
        "precondition: hit-condition breakpoints advertised"
    );
    assert!(has_feature("dap.breakpoints.logpoints"), "precondition: logpoints advertised");
    assert!(
        has_feature("dap.breakpoints.function"),
        "precondition: function breakpoints advertised"
    );

    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    let rows = [
        "supportsFunctionBreakpoints",
        "supportsConditionalBreakpoints",
        "supportsHitConditionalBreakpoints",
        "supportsLogPoints",
    ];
    for row in rows {
        assert_capability_is_json_boolean(&body, row);
        assert_capability_bool(&body, row, false, "must be advertised false (#9578)");
    }
    Ok(())
}

/// The floor must not be achieved by weakening neighboring capability rows:
/// the surviving `dap.core`-derived siblings keep their values, so a
/// regression that simply deletes advertisement rows wholesale, or flattens
/// every boolean to false, is visible.
///
/// #9581 closed the previous true-witness (`supportsValueFormattingOptions`)
/// and the previous `dap.breakpoints.basic` sibling
/// (`supportsBreakpointLocationsRequest`). Re-pointing at either, or promoting
/// either to keep this test green, is forbidden (#14933).
#[test]
fn optional_floor_does_not_widen_or_flatten_neighboring_capability_rows() -> Result<()> {
    assert!(
        has_feature("dap.core"),
        "precondition: dap.core is advertised; without it the true-sibling table is not a dap.core witness"
    );
    assert!(
        !DAP_CORE_DERIVED_TRUE_SIBLINGS.is_empty(),
        "anti-flattening is vacuous without at least one surviving dap.core-derived true sibling; \
         if none remain, replace this table with an explicit alternate proof rather than an empty list"
    );

    let mut adapter = adapter();
    let body = initialize_body(&mut adapter)?;

    for name in DAP_CORE_DERIVED_TRUE_SIBLINGS {
        assert_capability_is_json_boolean(&body, name);
        assert_capability_bool(
            &body,
            name,
            true,
            "surviving dap.core-derived sibling must stay true (#9578 anti-flattening; #14933)",
        );
    }

    for (name, floor) in FORMER_TRUE_SIBLINGS_NOW_FLOORED {
        assert_capability_is_json_boolean(&body, name);
        assert_capability_bool(
            &body,
            name,
            false,
            &format!(
                "former true-sibling now carries the {floor} floor; do not promote it to satisfy anti-flattening"
            ),
        );
    }

    // The same catalog row that used to keep breakpointLocations true cannot
    // be re-derived into conditional support (#9578: no capability inherits
    // another's receipt).
    assert_capability_bool(
        &body,
        "supportsConditionalBreakpoints",
        false,
        "dap.breakpoints.basic must not widen supportsConditionalBreakpoints",
    );
    Ok(())
}

/// Each surviving true-sibling is an independent flattening detector: dropping
/// one from the table would let a later floor close that row unnoticed while
/// the remaining two still pass. The table must keep distinct names.
#[test]
fn anti_flattening_true_siblings_are_distinct_named_rows() {
    let mut seen = std::collections::BTreeSet::new();
    for name in DAP_CORE_DERIVED_TRUE_SIBLINGS {
        assert!(seen.insert(*name), "duplicate anti-flattening witness {name}");
    }
    for (name, _) in FORMER_TRUE_SIBLINGS_NOW_FLOORED {
        assert!(
            !seen.contains(name),
            "{name} cannot be both a surviving true-sibling and a former-sibling floor pin"
        );
    }
    assert!(
        FORMER_TRUE_SIBLINGS_NOW_FLOORED
            .iter()
            .any(|(name, floor)| { *name == VALUE_FORMAT_FLOOR_FIELD && *floor == "#9581" }),
        "{VALUE_FORMAT_FLOOR_FIELD} must remain a #9581 former-sibling pin, not a true-sibling"
    );
}

// ---------------------------------------------------------------------------
// setFunctionBreakpoints floor
// ---------------------------------------------------------------------------

fn assert_function_floor_refusal(message: &str, what: &str) {
    assert!(
        message.contains(FUNCTION_FLOOR_MARKER) && message.contains("#9578"),
        "{what}: expected the #9578 function floor refusal naming the capability, got {message:?}"
    );
}

/// Every request shape — well-formed, malformed, and empty — receives the
/// identical deterministic refusal, proving the gate runs before argument
/// parsing and name validation (#9578 test 3).
#[test]
fn function_breakpoint_requests_receive_one_identical_refusal_for_every_shape() -> Result<()> {
    let mut adapter = initialized_adapter();

    let shapes = [
        (
            "well-formed valid names",
            Some(json!({
                "breakpoints": [
                    { "name": "main::entry" },
                    { "name": "Foo::bar" },
                ]
            })),
        ),
        (
            "invalid names mixed with injection",
            Some(json!({
                "breakpoints": [
                    { "name": "Invalid Name With Spaces" },
                    { "name": "bad\ninjection" },
                ]
            })),
        ),
        ("empty list no-op", Some(json!({ "breakpoints": [] }))),
        ("missing arguments", None),
    ];

    let mut reference: Option<(bool, Option<Value>, String)> = None;
    for (what, arguments) in shapes {
        let response = adapter.handle_request(2, "setFunctionBreakpoints", arguments);
        match response {
            DapMessage::Response { success, command, body, message, .. } => {
                assert_eq!(command, "setFunctionBreakpoints", "{what}");
                assert!(!success, "{what}: a floored request must not succeed");
                assert!(body.is_none(), "{what}: refusal must not carry a breakpoint body");
                let message = message.unwrap_or_default();
                assert_function_floor_refusal(&message, what);

                let shaped = (success, body, message);
                if let Some(reference) = &reference {
                    assert_eq!(
                        &shaped, reference,
                        "{what}: every shape must receive the identical refusal"
                    );
                } else {
                    reference = Some(shaped);
                }
            }
            other => anyhow::bail!("{what}: expected a response, got {other:?}"),
        }
    }
    Ok(())
}

/// The refusal must not poison the base source-breakpoint cell: a plain
/// request against the same adapter keeps its independent contract (#9578
/// test 2 and test 9).
#[test]
fn function_floor_refusal_leaves_plain_source_breakpoints_independent() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let before = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{ "line": 3 }],
        })),
    );
    match before {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1);
        }
        other => anyhow::bail!("plain setBreakpoints must keep its contract, got {other:?}"),
    }

    let refusal = adapter.handle_request(
        3,
        "setFunctionBreakpoints",
        Some(json!({ "breakpoints": [{ "name": "main::run" }] })),
    );
    match refusal {
        DapMessage::Response { success: false, message: Some(message), .. } => {
            assert_function_floor_refusal(&message, "after a stored plain breakpoint");
        }
        other => anyhow::bail!("expected the floor refusal, got {other:?}"),
    }

    // The stored plain breakpoint path still replaces cleanly afterwards.
    let after = adapter.handle_request(
        4,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{ "line": 1 }],
        })),
    );
    match after {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1, "replace semantics must keep working");
        }
        other => anyhow::bail!("plain setBreakpoints after refusal must succeed, got {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Optional fields in setBreakpoints
// ---------------------------------------------------------------------------

/// A `condition` entry must be rejected per item with the conditional refusal
/// marker; it must not be silently stripped so an unconditional breakpoint
/// installs (#9578 test 4).
#[test]
fn condition_entry_is_rejected_not_stripped_into_unconditional_install() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{ "line": 3, "condition": "$x > 10" }],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1, "one response breakpoint per input");
            assert_eq!(
                breakpoints[0].get("verified").and_then(Value::as_bool),
                Some(false),
                "a conditioned entry must not install while the capability is floored"
            );
            let message = breakpoints[0]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("rejected entry must carry a message"))?;
            assert!(
                message.contains(CONDITION_FLOOR_MARKER) && message.contains("#9578"),
                "expected the conditional floor refusal, got {message:?}"
            );
        }
        other => anyhow::bail!("expected a per-item rejection response, got {other:?}"),
    }
    Ok(())
}

/// A `hitCondition` entry must not be counted locally or installed as
/// unconditional (#9578 test 5).
#[test]
fn hit_condition_entry_is_rejected_not_counted_or_installed() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{ "line": 3, "hitCondition": ">= 3" }],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1);
            assert_eq!(breakpoints[0].get("verified").and_then(Value::as_bool), Some(false));
            let message = breakpoints[0]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(
                message.contains(HIT_CONDITION_FLOOR_MARKER) && message.contains("#9578"),
                "expected the hit-condition floor refusal, got {message:?}"
            );
        }
        other => anyhow::bail!("expected a per-item rejection response, got {other:?}"),
    }
    Ok(())
}

/// A `logMessage` entry must not become an ordinary stopping breakpoint and
/// must not simulate output (#9578 test 6).
#[test]
fn log_message_entry_is_rejected_not_converted_into_stopping_breakpoint() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{ "line": 3, "logMessage": "hit line 3" }],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1);
            assert_eq!(breakpoints[0].get("verified").and_then(Value::as_bool), Some(false));
            let message = breakpoints[0]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(
                message.contains(LOG_MESSAGE_FLOOR_MARKER) && message.contains("#9578"),
                "expected the logpoint floor refusal, got {message:?}"
            );
        }
        other => anyhow::bail!("expected a per-item rejection response, got {other:?}"),
    }
    Ok(())
}

/// A mixed request preserves input order and per-item truth: plain entries
/// proceed under their own contract while optional entries reject in place
/// (#9578 test 7).
#[test]
fn mixed_request_preserves_input_order_and_per_item_truth() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [
                { "line": 3 },
                { "line": 1, "condition": "$x" },
                { "line": 2, "hitCondition": "%2" },
                { "line": 4, "logMessage": "done" },
            ],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 4, "one response per input, in order");
            assert_eq!(
                breakpoints[0].get("line").and_then(Value::as_i64),
                Some(3),
                "plain entry keeps its requested line"
            );
            assert_eq!(breakpoints[0].get("verified").and_then(Value::as_bool), Some(true));
            assert_eq!(breakpoints[1].get("line").and_then(Value::as_i64), Some(1));
            let condition_message = breakpoints[1]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(condition_message.contains(CONDITION_FLOOR_MARKER));
            assert_eq!(breakpoints[2].get("line").and_then(Value::as_i64), Some(2));
            let hit_message = breakpoints[2]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(hit_message.contains(HIT_CONDITION_FLOOR_MARKER));
            assert_eq!(breakpoints[3].get("line").and_then(Value::as_i64), Some(4));
            let log_message = breakpoints[3]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(log_message.contains(LOG_MESSAGE_FLOOR_MARKER));
        }
        other => anyhow::bail!("expected a mixed-response, got {other:?}"),
    }
    Ok(())
}

/// A request with no source path keeps exactly one response entry per input:
/// the plain slot stays present as unverified instead of disappearing, so the
/// DAP one-result-per-request shape holds for mixed pathless requests too.
#[test]
fn pathless_mixed_request_keeps_one_response_entry_per_input() -> Result<()> {
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": {},
            "breakpoints": [{ "line": 3 }, { "line": 4, "condition": "$x" }],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 2, "one response per input, in order");
            assert_eq!(breakpoints[0].get("line").and_then(Value::as_i64), Some(3));
            assert_eq!(breakpoints[1].get("line").and_then(Value::as_i64), Some(4));
            let condition_message = breakpoints[1]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            assert!(condition_message.contains(CONDITION_FLOOR_MARKER));
        }
        other => anyhow::bail!("expected a mixed-response, got {other:?}"),
    }
    Ok(())
}

/// Unsupported combinations reject on every offending field and cannot widen
/// another feature (#9578 test 8).
#[test]
fn unsupported_combinations_name_every_offending_field() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": source_path },
            "breakpoints": [{
                "line": 3,
                "condition": "$x",
                "hitCondition": ">= 1",
                "logMessage": "msg",
            }],
        })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let breakpoints = response_breakpoints(&body)?;
            assert_eq!(breakpoints.len(), 1);
            assert_eq!(breakpoints[0].get("verified").and_then(Value::as_bool), Some(false));
            let message = breakpoints[0]
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing message"))?;
            for marker in
                [CONDITION_FLOOR_MARKER, HIT_CONDITION_FLOOR_MARKER, LOG_MESSAGE_FLOOR_MARKER]
            {
                assert!(message.contains(marker), "combination refusal must name {marker}");
            }
        }
        other => anyhow::bail!("expected a per-item rejection response, got {other:?}"),
    }
    Ok(())
}

/// Per-item refusals are idempotent and deterministic (#9578: no
/// state-dependent drift in the false path).
#[test]
fn per_item_refusals_are_idempotent() -> Result<()> {
    let (_keep, source_path) = write_perl_file()?;
    let mut adapter = initialized_adapter();

    let args = json!({
        "source": { "path": source_path },
        "breakpoints": [{ "line": 3, "condition": "$x > 10" }],
    });
    let first = adapter.handle_request(2, "setBreakpoints", Some(args.clone()));
    let second = adapter.handle_request(3, "setBreakpoints", Some(args));
    match (first, second) {
        (
            DapMessage::Response { body: Some(first), .. },
            DapMessage::Response { body: Some(second), .. },
        ) => {
            assert_eq!(
                first, second,
                "the same floored request must produce the identical per-item response"
            );
        }
        other => anyhow::bail!("expected two successful responses, got {other:?}"),
    }
    Ok(())
}
