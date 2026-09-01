//! #9581 secondary-capability floor — discriminating wire contract.
//!
//! Seven `initialize` capability fields are explicit `false` rows in every
//! mode until each field's own exact-behavior receipt passes (#9581):
//!
//! - `supportsCompletionsRequest`     (gate: #9021 + #9046 + #9050 + #8581 + #9582 + #9584)
//! - `supportsModulesRequest`         (gate: #8581 + #7667/#8668 + #9585 + #9586)
//! - `supportsLoadedSourcesRequest`   (gate: #8581 + #7667/#8668 + #9585 + #9586)
//! - `supportsRestartRequest`         (gate: #9051 + #8691/#8703 + #8974 + #9587 + #8726 + #7568)
//! - `supportsValueFormattingOptions` (gate: #9050 + #8364 + #9070 + #7342/#7345 + #9588 + #9590)
//! - `supportsBreakpointLocationsRequest` (gate: #10524 + #2300 + #9021 + #7566)
//! - `supportsCancelRequest`          (gate: #9074 + #8712 + #7568)
//!
//! While a row is false, its request must return the explicit unsupported
//! disposition with no debugger I/O, no process action, and no state mutation;
//! a missing session can never masquerade as a successful empty result; and a
//! non-default `format` option on the four ValueFormat families is rejected
//! before any debugger/value mutation while requests without one keep their
//! independent contract (#9581). One field's future receipt never widens
//! another: every disposition names exactly its own capability row.

use perl_dap::{DapMessage, DebugAdapter};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The seven floored wire rows and the capability each disposition must name.
const FLOOR_ROWS: [(&str, &str); 7] = [
    ("supportsCompletionsRequest", "completions"),
    ("supportsModulesRequest", "modules"),
    ("supportsLoadedSourcesRequest", "loadedSources"),
    ("supportsRestartRequest", "restart"),
    ("supportsValueFormattingOptions", ""),
    ("supportsBreakpointLocationsRequest", "breakpointLocations"),
    ("supportsCancelRequest", "cancel"),
];

/// The six floored requests with minimal valid arguments.
fn floored_requests() -> Vec<(&'static str, Option<Value>)> {
    vec![
        ("completions", Some(json!({ "text": "pr", "column": 2 }))),
        ("modules", Some(json!({ "startModule": 0 }))),
        ("loadedSources", None),
        ("restart", None),
        ("breakpointLocations", Some(json!({ "source": { "path": "/tmp/x.pl" }, "line": 1 }))),
        ("cancel", None),
    ]
}

fn initialize_body(adapter: &mut DebugAdapter) -> Result<Value, Box<dyn std::error::Error>> {
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        other => Err(format!("expected successful initialize response, got {other:?}").into()),
    }
}

fn response_parts(
    msg: DapMessage,
    command: &str,
) -> Result<(bool, Option<String>, Option<Value>), Box<dyn std::error::Error>> {
    match msg {
        DapMessage::Response { success, command: actual, message, body, .. } => {
            if actual != command {
                return Err(format!("expected {command} response, got {actual}").into());
            }
            Ok((success, message, body))
        }
        other => Err(format!("expected Response for {command}, got {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// 1. The initialize response reports all seven fields false (native mode)
// ---------------------------------------------------------------------------

#[test]
fn initialize_reports_all_seven_secondary_fields_false() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = initialize_body(&mut adapter)?;

    for (capability, _) in FLOOR_ROWS {
        let value = body
            .get(capability)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("`{capability}` must be present as a boolean"))?;
        assert!(!value, "`{capability}` must be false under the #9581 floor");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Core positive controls are untouched by the floor
// ---------------------------------------------------------------------------

#[test]
fn core_launch_breakpoint_and_control_cells_stay_advertised() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let body = initialize_body(&mut adapter)?;

    for capability in [
        "supportsConfigurationDoneRequest",
        "supportsFunctionBreakpoints",
        "supportsConditionalBreakpoints",
        "supportsSetVariable",
        "supportsGotoTargetsRequest",
        "supportTerminateDebuggee",
        "supportsTerminateRequest",
    ] {
        let value = body
            .get(capability)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("`{capability}` must be present as a boolean"))?;
        assert!(value, "`{capability}` is outside the #9581 floor and must stay advertised");
    }
    Ok(())
}

#[test]
fn core_breakpoints_still_succeed_alongside_the_floor() -> TestResult {
    // Plain source breakpoints are the proven core: they must keep working in
    // the same adapter session where every secondary request is rejected.
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": "/tmp/floor_9581_core.pl", "name": "floor_9581_core.pl" },
            "breakpoints": [{ "line": 5 }]
        })),
    );
    let (success, _, body) = response_parts(response, "setBreakpoints")?;
    assert!(success, "setBreakpoints is core and must still succeed");
    let breakpoints = body
        .and_then(|b| b.get("breakpoints").and_then(Value::as_array).cloned())
        .ok_or("setBreakpoints body must include breakpoints")?;
    assert_eq!(breakpoints.len(), 1, "one record per requested breakpoint");
    Ok(())
}

#[test]
fn core_state_queries_still_respond_alongside_the_floor() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    // threads/stackTrace/gotoTargets succeed without a session on current
    // main; they are outside the floor and must stay untouched.
    for command in ["threads", "stackTrace", "gotoTargets"] {
        let arguments = match command {
            "gotoTargets" => {
                Some(json!({ "source": { "path": "/tmp/floor_9581_core.pl" }, "line": 1 }))
            }
            _ => None,
        };
        let response = adapter.handle_request(2, command, arguments);
        let (success, _, _) = response_parts(response, command)?;
        assert!(success, "`{command}` is outside the #9581 floor and must succeed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Every floored request returns explicit unsupported — no masquerade
// ---------------------------------------------------------------------------

#[test]
fn floored_requests_reject_before_any_session_or_backend_work() -> TestResult {
    // Sent BEFORE launch: there is no session at all, so any success body
    // (an empty module/source/completion list) would be a masquerade (#9581).
    for (command, arguments) in floored_requests() {
        let mut adapter = DebugAdapter::new();
        adapter.handle_request(1, "initialize", None);
        let response = adapter.handle_request(2, command, arguments);
        let (success, message, body) = response_parts(response, command)?;
        assert!(!success, "`{command}` is floored and must fail");
        let message = message.ok_or_else(|| format!("`{command}` rejection must explain why"))?;
        assert!(
            message.contains("unsupported"),
            "`{command}` must return the explicit unsupported disposition, got: {message}"
        );
        assert!(body.is_none(), "`{command}` rejection must not carry a plausible result body");
    }
    Ok(())
}

#[test]
fn each_floor_disposition_names_exactly_its_own_capability() -> TestResult {
    // One field's gate evidence must never widen another: the disposition for
    // each request names its own capability row and no sibling's (#9581).
    for (capability, command) in FLOOR_ROWS {
        let Some(command) = (!command.is_empty()).then_some(command) else { continue };
        let mut adapter = DebugAdapter::new();
        adapter.handle_request(1, "initialize", None);
        let arguments =
            floored_requests().into_iter().find_map(|(c, a)| (c == command).then_some(a)).flatten();
        let response = adapter.handle_request(2, command, arguments);
        let (_, message, _) = response_parts(response, command)?;
        let message = message.unwrap_or_default();
        assert!(
            message.contains(capability),
            "`{command}` disposition must name its own row `{capability}`: {message}"
        );
        for (other_capability, other_command) in FLOOR_ROWS {
            if other_command == command || other_command.is_empty() {
                continue;
            }
            assert!(
                !message.contains(other_capability),
                "`{command}` disposition must not name sibling row `{other_capability}`: {message}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Per-request falsifiers: no handler computation leaks through the floor
// ---------------------------------------------------------------------------

#[test]
fn restart_rejects_even_with_fresh_launch_arguments() -> TestResult {
    // handle_restart would otherwise tear down state and re-launch with the
    // provided arguments; the floor must reject before ANY of that (#9581).
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let response = adapter.handle_request(
        2,
        "restart",
        Some(json!({ "arguments": { "program": "/tmp/floor_9581_restart.pl" } })),
    );
    let (success, message, body) = response_parts(response, "restart")?;
    assert!(!success, "restart with fresh arguments is still floored");
    assert!(message.is_some_and(|m| m.contains("unsupported")));
    assert!(body.is_none());

    // The adapter stays fully usable afterwards (no half-spawned state).
    let (threads_ok, _, _) = response_parts(adapter.handle_request(3, "threads", None), "threads")?;
    assert!(threads_ok, "threads must respond after the floored restart");
    Ok(())
}

#[test]
fn breakpoint_locations_rejects_a_real_file_without_reading_it() -> TestResult {
    // A valid, existing source path must still be floored-rejected: the gate
    // fires before the source read/parser work (#9581 falsifier).
    let mut temp = tempfile::NamedTempFile::with_suffix(".pl")?;
    std::io::Write::write_all(&mut temp, b"my $x = 1;\nprint $x;\n")?;
    let path = temp.path().to_string_lossy().to_string();

    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let response = adapter.handle_request(
        2,
        "breakpointLocations",
        Some(json!({ "source": { "path": path }, "line": 1, "endLine": 2 })),
    );
    let (success, message, body) = response_parts(response, "breakpointLocations")?;
    assert!(!success, "breakpointLocations is floored even for a real file");
    assert!(message.is_some_and(|m| m.contains("unsupported")));
    assert!(body.is_none(), "no locations may be computed while floored");
    Ok(())
}

#[test]
fn cancel_never_mutates_the_shared_cancellation_flag() -> TestResult {
    // Two cancels return the identical disposition (a mutated flag would
    // change subsequent cancel behavior), and an unrelated control request on
    // the same adapter is unaffected (#9581).
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let (first_ok, first_message, _) =
        response_parts(adapter.handle_request(2, "cancel", None), "cancel")?;
    let (second_ok, second_message, _) =
        response_parts(adapter.handle_request(3, "cancel", None), "cancel")?;
    assert!(!first_ok && !second_ok, "cancel stays floored");
    assert_eq!(
        first_message, second_message,
        "the cancel disposition must be deterministic (no flag mutation)"
    );

    let (goto_ok, _, _) = response_parts(
        adapter.handle_request(
            4,
            "gotoTargets",
            Some(json!({ "source": { "path": "/tmp/floor_9581_cancel.pl" }, "line": 1 })),
        ),
        "gotoTargets",
    )?;
    assert!(goto_ok, "gotoTargets (not floored) must keep working after floored cancels");
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. ValueFormat option floor: reject non-default `format`, pass default
// ---------------------------------------------------------------------------

#[test]
fn non_default_format_is_rejected_on_all_four_families() -> TestResult {
    let cells: [(&str, Value); 4] = [
        ("variables", json!({ "variablesReference": 1, "format": { "hex": true } })),
        (
            "setVariable",
            json!({ "variablesReference": 1, "name": "$x", "value": "1", "format": { "hex": true } }),
        ),
        ("evaluate", json!({ "expression": "$x", "format": { "hex": true } })),
        ("setExpression", json!({ "expression": "$x", "value": "1", "format": { "hex": true } })),
    ];
    for (command, arguments) in cells {
        let mut adapter = DebugAdapter::new();
        adapter.handle_request(1, "initialize", None);
        let response = adapter.handle_request(2, command, Some(arguments));
        let (success, message, body) = response_parts(response, command)?;
        assert!(!success, "`{command}` with hex:true must be rejected (#9581)");
        let message = message.ok_or("rejection must explain the format floor")?;
        assert!(
            message.contains("unsupported") && message.contains("supportsValueFormattingOptions"),
            "`{command}` must name the ValueFormat floor, got: {message}"
        );
        assert!(body.is_none(), "a floored format request must not carry a result body");
    }
    Ok(())
}

#[test]
fn requests_without_format_keep_their_independent_contract() -> TestResult {
    // Without `format`, the four families route to their own handlers: a
    // no-session `evaluate` fails with its own no-session disposition, not the
    // format floor (#9581: unformatted requests are untouched).
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let (ok, message, _) = response_parts(
        adapter.handle_request(2, "evaluate", Some(json!({ "expression": "$x" }))),
        "evaluate",
    )?;
    assert!(!ok, "evaluate without a session still fails for its own reason");
    let message = message.unwrap_or_default();
    assert!(
        !message.contains("supportsValueFormattingOptions"),
        "an unformatted request must not hit the format floor, got: {message}"
    );

    // Default-equivalent formats request nothing non-default and pass through.
    for format in [json!({}), json!({ "hex": false })] {
        let (ok, message, _) = response_parts(
            adapter.handle_request(
                3,
                "evaluate",
                Some(json!({ "expression": "$x", "format": format })),
            ),
            "evaluate",
        )?;
        assert!(!ok);
        assert!(
            !message.unwrap_or_default().contains("supportsValueFormattingOptions"),
            "default-equivalent format must keep the independent contract"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Mode independence: the mirror peer surface floors the same rows on its own
// ---------------------------------------------------------------------------

#[test]
fn mirror_surface_floors_the_same_rows_independently() -> TestResult {
    let caps = perl_dap::backend::static_mirror_capabilities();
    for (capability, _) in FLOOR_ROWS {
        let value = caps
            .get(capability)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("mirror profile must carry an explicit `{capability}` row"))?;
        assert!(!value, "mirror `{capability}` must be false under the #9581 floor");
    }
    // Core cells stay on for the mirror surface too.
    assert_eq!(caps.get("supportsConfigurationDoneRequest").and_then(Value::as_bool), Some(true));
    assert_eq!(caps.get("supportsTerminateRequest").and_then(Value::as_bool), Some(true));
    Ok(())
}

#[test]
fn mirror_bridge_rejects_floored_requests_before_peer_or_oracle_work() -> TestResult {
    // A pending bridge (no peer connected) must still reject every floored
    // request with the explicit disposition — the floor does not depend on
    // reaching (or waiting for) the peer (#9581).
    let mut bridge = perl_dap::backend::MirrorPeerBridge::new_pending(
        perl_dap::backend::capabilities::ControlMode::Mirror,
    );
    // The floored editor entry applies the floor before the canonical route
    // match, so a pending bridge (no peer) refuses without peer or oracle work.
    bridge.dispatch_with_capability_floor(1, "initialize", Some(json!({ "adapterID": "perl" })));

    let floored: [(&str, Option<Value>); 6] = [
        ("completions", Some(json!({ "text": "pr", "column": 2 }))),
        ("modules", None),
        ("loadedSources", None),
        ("restart", None),
        ("breakpointLocations", Some(json!({ "source": { "path": "/tmp/x.pl" }, "line": 1 }))),
        ("cancel", None),
    ];
    for (command, arguments) in floored {
        let out = bridge.dispatch_with_capability_floor(2, command, arguments);
        assert_eq!(out.len(), 1, "mirror `{command}` floor must not poll peer events");
        let first = out.first().ok_or_else(|| format!("{command}: no response"))?;
        match first {
            DapMessage::Response { success, message, body, .. } => {
                assert!(!success, "mirror `{command}` must be floored");
                assert!(
                    message.as_deref().is_some_and(|m| m.contains("unsupported")),
                    "mirror `{command}` must return the explicit unsupported disposition"
                );
                assert!(body.is_none(), "mirror `{command}` must not carry a body");
            }
            other => {
                return Err(format!("mirror `{command}`: expected Response, got {other:?}").into());
            }
        }
    }
    Ok(())
}

#[test]
fn mirror_bridge_rejects_non_default_format_on_proxied_families() -> TestResult {
    // variables/evaluate are proxied to the peer on the mirror surface; a
    // hex request must be rejected by the floor before any peer I/O (#9581).
    let mut bridge = perl_dap::backend::MirrorPeerBridge::new_pending(
        perl_dap::backend::capabilities::ControlMode::Mirror,
    );
    bridge.dispatch_with_capability_floor(1, "initialize", Some(json!({ "adapterID": "perl" })));

    for (command, arguments) in [
        ("variables", json!({ "variablesReference": 1, "format": { "hex": true } })),
        ("evaluate", json!({ "expression": "$x", "format": { "hex": true } })),
    ] {
        let out = bridge.dispatch_with_capability_floor(2, command, Some(arguments));
        let first = out.first().ok_or_else(|| format!("{command}: no response"))?;
        match first {
            DapMessage::Response { success, message, .. } => {
                assert!(!success, "mirror `{command}` hex request must be floored");
                assert!(
                    message
                        .as_deref()
                        .is_some_and(|m| m.contains("supportsValueFormattingOptions")),
                    "mirror `{command}` must name the ValueFormat floor"
                );
            }
            other => {
                return Err(format!("mirror `{command}`: expected Response, got {other:?}").into());
            }
        }
    }
    Ok(())
}
