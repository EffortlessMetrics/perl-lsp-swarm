//! Inline-values extension floor tests (#9089).
//!
//! The routed `inlineValues` request is a project extension, not standard DAP:
//! it is the one `Extension`-class row in the request authority table, and it
//! is not defined by the pinned upstream schema. Advertising the standard
//! `supportsInlineValues` capability from a catalog row let ordinary DAP
//! clients see a faux-standard capability and conflated source-text occurrences
//! with runtime debugger values.
//!
//! Until a versioned, explicitly negotiated extension identity is proven, the
//! extension stays disabled: the standard cell advertises false and every
//! request is refused after envelope validation only — before path validation,
//! any filesystem read, or any debugger query.

use perl_dap::backend::capabilities::{
    INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE, advertises_inline_values_extension,
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

fn refused_inline_values(response: DapMessage) -> Result<String, Box<dyn std::error::Error>> {
    match response {
        DapMessage::Response { success, command, body, message, .. } => {
            assert_eq!(command, "inlineValues");
            assert!(!success, "the extension is disabled and must refuse every request");
            assert!(body.is_none(), "a refused request must not carry a response body");
            message.ok_or_else(|| "refused inlineValues must explain why".into())
        }
        other => Err(format!("expected inlineValues response, got {other:?}").into()),
    }
}

fn well_formed_inline_values() -> Value {
    json!({
        "source": {"path": "fixture.pl"},
        "line": 1,
        "startLine": 1,
        "endLine": 3
    })
}

/// Ordinary DAP initialize exposes no custom inline-values capability.
///
/// The wire value comes from the single #9089 negotiation authority, never
/// from the catalog row: the row is unadvertised (`advertised = false` since
/// this floor landed) and `supportsInlineValues` is authority-bound false, so
/// both sides are pinned here. Promotion must flip the negotiation authority
/// consciously — re-reading `has_feature("dap.inline_values")` for the wire
/// would quietly re-couple the extension to ordinary catalog state, which is
/// exactly what #9089 removes.
#[test]
fn ordinary_initialize_does_not_advertise_the_extension() -> Result<(), Box<dyn std::error::Error>>
{
    let mut adapter = create_test_adapter();
    let caps = extract_initialize_response(adapter.handle_request(1, "initialize", None))?;

    assert_eq!(
        capability(&caps, "supportsInlineValues")?,
        advertises_inline_values_extension(),
        "supportsInlineValues must be the single-authority value (#9089)"
    );
    assert!(
        !capability(&caps, "supportsInlineValues")?,
        "the extension must not count as standard DAP while its negotiation is unproven"
    );
    assert_eq!(
        (
            capability(&caps, "supportsInlineValues")?,
            perl_dap::feature_catalog::has_feature("dap.inline_values"),
        ),
        (false, false),
        "the wire value stays authority-bound false while the catalog row stays \
         unadvertised (#9089); revisit this pair consciously at promotion"
    );
    Ok(())
}

/// An unnegotiated (i.e. any) request fails explicitly and deterministically.
#[test]
fn unnegotiated_request_fails_explicitly() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let response = adapter.handle_request(1, "inlineValues", Some(well_formed_inline_values()));
    let message = refused_inline_values(response)?;
    assert_eq!(
        message, INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE,
        "the refusal must be the single deterministic authority message"
    );
    Ok(())
}

/// Every request shape that passes envelope validation is refused identically.
///
/// Source, range, and reversal variants must not widen, reshape, or specialize
/// the gate.
#[test]
fn every_input_shape_receives_the_same_refusal() -> Result<(), Box<dyn std::error::Error>> {
    let shapes = [
        well_formed_inline_values(),
        json!({
            "source": {"path": "C:\\absent\\dir\\fixture.pl"},
            "line": 1,
            "startLine": 1,
            "endLine": 3
        }),
        json!({
            "source": {"path": "../../../etc/passwd"},
            "line": 1,
            "startLine": 1,
            "endLine": 3
        }),
        json!({
            "source": {"path": "fixture.pl"},
            "line": 1,
            "startLine": 9,
            "endLine": 3
        }),
        json!({
            "source": {"path": "fixture.pl"},
            "line": 1,
            "startLine": 0,
            "endLine": 0
        }),
        json!({"source": {}, "line": 1, "startLine": 1, "endLine": 3}),
    ];

    for shape in shapes {
        let mut adapter = create_test_adapter();
        let response = adapter.handle_request(1, "inlineValues", Some(shape.clone()));
        let message = refused_inline_values(response)
            .map_err(|error| format!("shape {shape} was not refused uniformly: {error}"))?;
        assert_eq!(
            message, INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE,
            "shape {shape} must receive the deterministic authority refusal"
        );
    }
    Ok(())
}

/// Envelope validation still owns malformed arguments.
///
/// The gate sits *after* envelope validation: missing arguments fail the
/// envelope layer, not the capability gate.
#[test]
fn missing_arguments_still_fail_at_the_envelope_layer() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let response = adapter.handle_request(1, "inlineValues", None);
    let message = refused_inline_values(response)?;
    assert_eq!(
        message, "Missing arguments",
        "malformed envelopes are an envelope-layer failure, not a capability refusal"
    );
    Ok(())
}

/// A refused request performs no filesystem access and no debugger work.
///
/// A path that would fail workspace validation, and a path whose read would
/// fail, both receive the identical negotiation refusal — proving the gate runs
/// ahead of every side effect the old path performed.
#[test]
fn refusal_precedes_filesystem_and_debugger_effects() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();

    // A traversal path would be rejected by path validation if the gate were
    // missing; it must instead receive the negotiation refusal verbatim.
    let traversal = json!({
        "source": {"path": "../outside/fixture.pl"},
        "line": 1,
        "startLine": 1,
        "endLine": 2
    });
    let message =
        refused_inline_values(adapter.handle_request(1, "inlineValues", Some(traversal)))?;
    assert_eq!(
        message, INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE,
        "the negotiation gate must fire before path validation and any read"
    );
    Ok(())
}

/// Refusal is idempotent: repeated requests cannot drift the adapter.
#[test]
fn repeated_refusals_are_identical() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let first = refused_inline_values(adapter.handle_request(
        1,
        "inlineValues",
        Some(well_formed_inline_values()),
    ))?;
    let second = refused_inline_values(adapter.handle_request(
        2,
        "inlineValues",
        Some(well_formed_inline_values()),
    ))?;
    assert_eq!(first, second, "repeated refusals must be byte-identical");
    Ok(())
}

/// Only the extension cell moved to the authority; the completions and hover
/// sibling cells keep their wiring. (The setVariable sibling later moved to
/// its own #8354 authority — asserted below.)
#[test]
fn sibling_capability_cells_are_independent() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let caps = extract_initialize_response(adapter.handle_request(1, "initialize", None))?;

    assert_eq!(
        capability(&caps, "supportsCompletionsRequest")?,
        perl_dap::feature_catalog::has_feature("dap.completions"),
        "supportsCompletionsRequest keeps its catalog-driven cell; this PR must not move it"
    );
    assert_eq!(
        capability(&caps, "supportsSetVariable")?,
        false,
        "supportsSetVariable carried a catalog-driven cell when #9089 landed; #8354 later \
         floored it on the exact-mutation authority, so it must now read false"
    );
    assert!(
        !capability(&caps, "supportsEvaluateForHovers")?,
        "the hover floor (#9573) is untouched by the inline-values floor"
    );
    Ok(())
}
