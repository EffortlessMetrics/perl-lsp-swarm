//! DAP Feature Flag Coverage Tests
//!
//! Explicit test coverage for DAP features that previously had no dedicated
//! feature-gate validation:
//!
//! - AC0: dap.core
//! - AC1: dap.breakpoints.basic
//! - AC2: dap.breakpoints.hit_condition
//! - AC2b: dap.breakpoints.function (function breakpoints with condition expressions)
//! - AC3: dap.breakpoints.logpoints
//! - AC4: dap.completions
//! - AC5: dap.exceptions.die
//! - AC6: dap.inline_values
//! - AC7: dap.modules
//! - AC10: dap.goto_targets (fail-closed standard goto, #9064)
//!
//! Each feature gets:
//! 1. Feature gate test: `has_feature("dap.X")` returns true
//! 2. Capability test: initialize response advertises the feature correctly
//! 3. Functional test: feature-gated code path works when enabled
//!
//! Related issues: #2784, #435, #2783, #5498

use perl_dap::feature_catalog::has_feature;
use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;
use std::io::Write;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn initialize_adapter() -> DebugAdapter {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    adapter
}

fn get_initialize_body() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    match adapter.handle_request(1, "initialize", None) {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        _ => Err("Expected successful initialize response with body".into()),
    }
}

fn expect_success_body(
    response: DapMessage,
    command: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match response {
        DapMessage::Response { success: true, command: actual, body: Some(body), .. }
            if actual == command =>
        {
            Ok(body)
        }
        DapMessage::Response { success: true, command: actual, .. } if actual == command => {
            Ok(serde_json::Value::Null)
        }
        DapMessage::Response { success: false, message, command: actual, .. } => Err(format!(
            "command `{actual}` failed: {}",
            message.unwrap_or_else(|| "<no message>".to_string())
        )
        .into()),
        _ => Err(format!("Expected response for `{command}`").into()),
    }
}

/// Expect the command to be refused, returning its refusal message.
fn expect_error(response: DapMessage, command: &str) -> Result<String, Box<dyn std::error::Error>> {
    match response {
        DapMessage::Response { success: false, command: actual, message, .. }
            if actual == command =>
        {
            Ok(message.unwrap_or_default())
        }
        DapMessage::Response { success, command: actual, .. } => {
            Err(format!("expected `{actual}` to be refused, got success={success}").into())
        }
        _ => Err(format!("Expected response for `{command}`").into()),
    }
}

// ---------------------------------------------------------------------------
// AC0: dap.core
// ---------------------------------------------------------------------------

/// Feature gate: dap.core is registered in the catalog.
#[test]
fn test_feature_gate_dap_core() {
    assert!(has_feature("dap.core"), "dap.core must be registered in the feature catalog");
}

/// Capability test: initialize advertises core capabilities when dap.core is enabled.
#[test]
fn test_capability_dap_core_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    if has_feature("dap.core") {
        let supports_config_done =
            body.get("supportsConfigurationDoneRequest").and_then(|v| v.as_bool()).unwrap_or(false);
        assert!(
            supports_config_done,
            "supportsConfigurationDoneRequest must be true when dap.core is enabled"
        );

        // #9573: hover is NOT a `dap.core` consequence. The catalog flag cannot
        // widen it, because the promise hover makes (pure inspection of the
        // selected frame) is narrower than anything `dap.core` compiles in.
        let supports_evaluate_for_hovers =
            body.get("supportsEvaluateForHovers").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !supports_evaluate_for_hovers,
            "supportsEvaluateForHovers must stay false even when dap.core is enabled (#9573)"
        );

        // #8354: setVariable is NOT a `dap.core` consequence either. The exact
        // mutation proof does not exist, so the catalog flag cannot widen the
        // field and initialize must advertise false even with dap.core enabled.
        let supports_set_variable =
            body.get("supportsSetVariable").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !supports_set_variable,
            "supportsSetVariable must stay false even when dap.core is enabled (#8354)"
        );
    }
    Ok(())
}

/// Functional test: configurationDone succeeds (core DAP lifecycle).
#[test]
fn test_functional_dap_core_configuration_done() -> TestResult {
    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(2, "configurationDone", None);

    match response {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "configurationDone");
        }
        _ => return Err("Expected configurationDone response".into()),
    }
    Ok(())
}

/// Functional test: threads request succeeds (core DAP capability).
///
/// Without an active debug session, threads returns an empty array — this is
/// correct DAP behavior. The test validates that the command is dispatched and
/// the response body has the expected shape.
#[test]
fn test_functional_dap_core_threads() -> TestResult {
    let mut adapter = initialize_adapter();
    let body = expect_success_body(adapter.handle_request(2, "threads", None), "threads")?;

    // threads array must be present (may be empty without an active session)
    let _threads = body.get("threads").and_then(|v| v.as_array()).ok_or("missing threads array")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// AC1: dap.breakpoints.basic
// ---------------------------------------------------------------------------

/// Feature gate: dap.breakpoints.basic is registered in the catalog.
#[test]
fn test_feature_gate_dap_breakpoints_basic() {
    assert!(
        has_feature("dap.breakpoints.basic"),
        "dap.breakpoints.basic must be registered in the feature catalog"
    );
}

/// Capability test: initialize keeps conditional breakpoint support false.
///
/// #9578: the four optional breakpoint capability rows fail closed from the
/// single breakpoint authority. Even while `dap.breakpoints.basic` is
/// registered AND advertised in the catalog, the runtime contract (exact
/// condition installation and enforcement, #8988) is unproven, so the catalog
/// row cannot widen the wire value.
#[test]
fn test_capability_dap_breakpoints_basic_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    if has_feature("dap.breakpoints.basic") {
        let supports_conditional =
            body.get("supportsConditionalBreakpoints").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !supports_conditional,
            "supportsConditionalBreakpoints must stay false while the catalog row is advertised (#9578)"
        );
    }
    Ok(())
}

/// Functional test: setBreakpoints responds successfully with at least one breakpoint record.
#[test]
fn test_functional_dap_breakpoints_basic_set_breakpoints() -> TestResult {
    let mut adapter = initialize_adapter();
    let body = expect_success_body(
        adapter.handle_request(
            2,
            "setBreakpoints",
            Some(json!({
                "source": { "path": "/tmp/test.pl", "name": "test.pl" },
                "breakpoints": [{ "line": 5 }]
            })),
        ),
        "setBreakpoints",
    )?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints array")?;
    assert_eq!(breakpoints.len(), 1, "setBreakpoints must return one record per requested BP");
    Ok(())
}

/// Functional test: clearing breakpoints (replace semantics) returns empty array.
#[test]
fn test_functional_dap_breakpoints_basic_clear_breakpoints() -> TestResult {
    let mut adapter = initialize_adapter();

    // Set breakpoints
    adapter.handle_request(
        2,
        "setBreakpoints",
        Some(json!({
            "source": { "path": "/tmp/test.pl", "name": "test.pl" },
            "breakpoints": [{ "line": 5 }, { "line": 10 }]
        })),
    );

    // Clear (replace with empty list)
    let body = expect_success_body(
        adapter.handle_request(
            3,
            "setBreakpoints",
            Some(json!({
                "source": { "path": "/tmp/test.pl", "name": "test.pl" },
                "breakpoints": []
            })),
        ),
        "setBreakpoints",
    )?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints array")?;
    assert!(breakpoints.is_empty(), "clearing breakpoints must return an empty array");
    Ok(())
}

// ---------------------------------------------------------------------------
// AC2: dap.breakpoints.hit_condition
// ---------------------------------------------------------------------------

/// Feature gate: dap.breakpoints.hit_condition is registered in the catalog.
#[test]
fn test_feature_gate_dap_breakpoints_hit_condition() {
    assert!(
        has_feature("dap.breakpoints.hit_condition"),
        "dap.breakpoints.hit_condition must be registered in the feature catalog"
    );
}

/// Capability test: initialize keeps hit-conditional breakpoint support false.
///
/// #9578: attributed hit counting with serialized auto-continue is unproven,
/// so `dap.breakpoints.hit_condition` being registered and advertised cannot
/// widen the wire value.
#[test]
fn test_capability_dap_breakpoints_hit_condition_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    if has_feature("dap.breakpoints.hit_condition") {
        let supports_hit_conditional =
            body.get("supportsHitConditionalBreakpoints").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !supports_hit_conditional,
            "supportsHitConditionalBreakpoints must stay false while the catalog row is advertised (#9578)"
        );
    }
    Ok(())
}

/// Functional test: setBreakpoints accepts hitCondition field and stores the breakpoint.
#[test]
fn test_functional_dap_breakpoints_hit_condition_accepted() -> TestResult {
    let store = perl_dap::breakpoints::BreakpointStore::new();
    let args = perl_dap::protocol::SetBreakpointsArguments {
        source: perl_dap::protocol::Source {
            path: Some("/tmp/test.pl".to_string()),
            name: Some("test.pl".to_string()),
        },
        breakpoints: Some(vec![perl_dap::protocol::SourceBreakpoint {
            line: 5,
            column: None,
            condition: None,
            hit_condition: Some(">= 3".to_string()),
            log_message: None,
        }]),
        source_modified: None,
    };

    let breakpoints = store.set_breakpoints(&args);
    assert_eq!(breakpoints.len(), 1, "hit_condition breakpoint must return one record");
    Ok(())
}

/// Functional test: hit-condition `>= 2` — first hit does not stop, second does.
///
/// Uses a real temp Perl file so AST validation can verify the breakpoint.
#[test]
fn test_functional_dap_breakpoints_hit_condition_gte_two() -> TestResult {
    use tempfile::NamedTempFile;
    let mut temp = NamedTempFile::with_suffix(".pl").map_err(|e| e.to_string())?;
    let perl_src = b"my $x = 0;\n$x++;\n$x++;\nprint $x;\n";
    temp.write_all(perl_src).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    let path = temp.path().to_string_lossy().to_string();

    let store = perl_dap::breakpoints::BreakpointStore::new();
    let args = perl_dap::protocol::SetBreakpointsArguments {
        source: perl_dap::protocol::Source {
            path: Some(path.clone()),
            name: Some("hit_test.pl".to_string()),
        },
        breakpoints: Some(vec![perl_dap::protocol::SourceBreakpoint {
            line: 2,
            column: None,
            condition: None,
            hit_condition: Some(">= 2".to_string()),
            log_message: None,
        }]),
        source_modified: None,
    };

    let bps = store.set_breakpoints(&args);
    assert_eq!(bps.len(), 1, "expected one breakpoint record");
    assert!(bps[0].verified, "breakpoint on executable line must be verified");

    let first = store.register_breakpoint_hit(&path, 2);
    assert!(first.matched, "first hit must match the breakpoint");
    assert!(!first.should_stop, "first hit must not stop for `>= 2`");

    let second = store.register_breakpoint_hit(&path, 2);
    assert!(second.matched, "second hit must match the breakpoint");
    assert!(second.should_stop, "second hit must stop for `>= 2`");
    Ok(())
}

/// Functional test: hit-condition `%3` stops on every third hit.
///
/// Uses a real temp Perl file so AST validation can verify the breakpoint.
#[test]
fn test_functional_dap_breakpoints_hit_condition_modulo() -> TestResult {
    use tempfile::NamedTempFile;
    let mut temp = NamedTempFile::with_suffix(".pl").map_err(|e| e.to_string())?;
    let perl_src = b"my $x = 0;\n$x++;\n$x++;\n$x++;\nprint $x;\n";
    temp.write_all(perl_src).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    let path = temp.path().to_string_lossy().to_string();

    let store = perl_dap::breakpoints::BreakpointStore::new();
    let args = perl_dap::protocol::SetBreakpointsArguments {
        source: perl_dap::protocol::Source {
            path: Some(path.clone()),
            name: Some("modulo_test.pl".to_string()),
        },
        breakpoints: Some(vec![perl_dap::protocol::SourceBreakpoint {
            line: 2,
            column: None,
            condition: None,
            hit_condition: Some("%3".to_string()),
            log_message: None,
        }]),
        source_modified: None,
    };

    let bps = store.set_breakpoints(&args);
    assert_eq!(bps.len(), 1, "expected one breakpoint record");
    assert!(bps[0].verified, "breakpoint on executable line must be verified");

    let h1 = store.register_breakpoint_hit(&path, 2);
    let h2 = store.register_breakpoint_hit(&path, 2);
    let h3 = store.register_breakpoint_hit(&path, 2);

    assert!(!h1.should_stop, "hit 1 must not stop for `%3`");
    assert!(!h2.should_stop, "hit 2 must not stop for `%3`");
    assert!(h3.should_stop, "hit 3 must stop for `%3`");
    Ok(())
}

// ---------------------------------------------------------------------------
// AC2b: dap.breakpoints.function
// ---------------------------------------------------------------------------

/// Feature gate: dap.breakpoints.function is registered in the catalog.
#[test]
fn test_feature_gate_dap_breakpoints_function() {
    assert!(
        has_feature("dap.breakpoints.function"),
        "dap.breakpoints.function must be registered in the feature catalog"
    );
}

/// Capability test: initialize keeps supportsFunctionBreakpoints false.
///
/// #9578: a syntactically valid function name is not runtime resolution or
/// engine installation (#8645 re-enable gate), so the wire value stays false
/// regardless of the catalog or `dap.core`.
#[test]
fn test_capability_dap_breakpoints_function_initialize_response() -> TestResult {
    let body = get_initialize_body()?;
    let supports =
        body.get("supportsFunctionBreakpoints").and_then(|v| v.as_bool()).unwrap_or(true);
    assert!(
        !supports,
        "supportsFunctionBreakpoints must stay false until exact runtime proof exists (#9578)"
    );
    Ok(())
}

/// Functional test: setFunctionBreakpoints is refused while the capability is
/// floored (#9578).
///
/// A function name with a Perl-variable condition used to be accepted and
/// stored. #8645 owns the runtime resolution and engine install/remove/hit
/// proof; until then every shape — including a well-formed one — must receive
/// the identical deterministic refusal with no stored record.
#[test]
fn test_functional_dap_function_breakpoints_with_condition() -> TestResult {
    if !has_feature("dap.breakpoints.function") {
        return Ok(());
    }

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        2,
        "setFunctionBreakpoints",
        Some(json!({
            "breakpoints": [{
                "name": "test_func",
                "condition": "$debug_flag"
            }]
        })),
    );

    match response {
        DapMessage::Response { success: false, command, message: Some(message), .. }
            if command == "setFunctionBreakpoints" =>
        {
            assert!(
                message.contains("supportsFunctionBreakpoints") && message.contains("#9578"),
                "expected the #9578 floor refusal, got {message:?}"
            );
            Ok(())
        }
        other => Err(format!("unexpected response: {other:?}").into()),
    }
}

/// Functional test: setFunctionBreakpoints is refused for a scalar variable
/// condition shape ($count) while the capability is floored (#9578).
#[test]
fn test_functional_dap_function_breakpoints_scalar_condition() -> TestResult {
    if !has_feature("dap.breakpoints.function") {
        return Ok(());
    }

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        3,
        "setFunctionBreakpoints",
        Some(json!({
            "breakpoints": [{
                "name": "my_sub",
                "condition": "$count"
            }]
        })),
    );

    match response {
        DapMessage::Response { success: false, command, message: Some(message), .. }
            if command == "setFunctionBreakpoints" =>
        {
            assert!(
                message.contains("supportsFunctionBreakpoints") && message.contains("#9578"),
                "expected the #9578 floor refusal, got {message:?}"
            );
            Ok(())
        }
        other => Err(format!("unexpected response: {other:?}").into()),
    }
}

/// Functional test: setFunctionBreakpoints is refused for a compound boolean
/// expression shape while the capability is floored (#9578).
///
/// Complex Perl expressions (e.g., `defined($ENV{DEBUG}) && $ENV{DEBUG} > 0`)
/// are syntactically valid conditions, but no acceptance path exists while the
/// capability is floored: the same deterministic refusal applies.
#[test]
fn test_functional_dap_function_breakpoints_complex_condition() -> TestResult {
    if !has_feature("dap.breakpoints.function") {
        return Ok(());
    }

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        4,
        "setFunctionBreakpoints",
        Some(json!({
            "breakpoints": [{
                "name": "handler",
                "condition": "defined($ENV{DEBUG}) && $ENV{DEBUG} > 0"
            }]
        })),
    );

    match response {
        DapMessage::Response { success: false, command, message: Some(message), .. }
            if command == "setFunctionBreakpoints" =>
        {
            assert!(
                message.contains("supportsFunctionBreakpoints") && message.contains("#9578"),
                "expected the #9578 floor refusal, got {message:?}"
            );
            Ok(())
        }
        other => Err(format!("unexpected response: {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// AC3: dap.breakpoints.logpoints
// ---------------------------------------------------------------------------

/// Feature gate: dap.breakpoints.logpoints is registered in the catalog.
#[test]
fn test_feature_gate_dap_breakpoints_logpoints() {
    assert!(
        has_feature("dap.breakpoints.logpoints"),
        "dap.breakpoints.logpoints must be registered in the feature catalog"
    );
}

/// Capability test: initialize keeps logpoint support false.
///
/// #9578: install → hit → correlated lookup → output → continue is unproven
/// (#9000 re-enable gate), so `dap.breakpoints.logpoints` being registered and
/// advertised cannot widen the wire value.
#[test]
fn test_capability_dap_breakpoints_logpoints_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    if has_feature("dap.breakpoints.logpoints") {
        let supports_log_points =
            body.get("supportsLogPoints").and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !supports_log_points,
            "supportsLogPoints must stay false while the catalog row is advertised (#9578)"
        );
    }
    Ok(())
}

/// Functional test: logpoint emits log_messages and does not stop execution.
///
/// Uses a real temp Perl file so AST validation can verify the breakpoint.
#[test]
fn test_functional_dap_breakpoints_logpoint_emits_and_continues() -> TestResult {
    use tempfile::NamedTempFile;
    let mut temp = NamedTempFile::with_suffix(".pl").map_err(|e| e.to_string())?;
    let perl_src = b"my $x = 1;\nmy $y = 2;\nmy $z = $x + $y;\nprint $z;\n";
    temp.write_all(perl_src).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    let path = temp.path().to_string_lossy().to_string();

    let store = perl_dap::breakpoints::BreakpointStore::new();
    let args = perl_dap::protocol::SetBreakpointsArguments {
        source: perl_dap::protocol::Source {
            path: Some(path.clone()),
            name: Some("logpoint_test.pl".to_string()),
        },
        breakpoints: Some(vec![perl_dap::protocol::SourceBreakpoint {
            line: 3,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: Some("z = {$z}".to_string()),
        }]),
        source_modified: None,
    };

    let bps = store.set_breakpoints(&args);
    assert_eq!(bps.len(), 1, "expected one breakpoint record");
    assert!(bps[0].verified, "logpoint on executable line must be verified");

    let outcome = store.register_breakpoint_hit(&path, 3);
    assert!(outcome.matched, "logpoint must match the breakpoint");
    assert!(!outcome.should_stop, "logpoint must not stop execution");
    assert!(!outcome.log_messages.is_empty(), "logpoint must emit at least one log message");
    Ok(())
}

/// Functional test: setBreakpoints with logMessage field stores the breakpoint.
#[test]
fn test_functional_dap_breakpoints_logpoint_stored_via_set_breakpoints() -> TestResult {
    let mut adapter = initialize_adapter();
    let body = expect_success_body(
        adapter.handle_request(
            2,
            "setBreakpoints",
            Some(json!({
                "source": { "path": "/tmp/logtest.pl", "name": "logtest.pl" },
                "breakpoints": [{ "line": 4, "logMessage": "Reached line 4: {$x}" }]
            })),
        ),
        "setBreakpoints",
    )?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints array")?;
    assert_eq!(breakpoints.len(), 1, "logpoint must appear in setBreakpoints response");
    Ok(())
}

// ---------------------------------------------------------------------------
// AC4: dap.completions
// ---------------------------------------------------------------------------

/// Feature gate: dap.completions is registered in the catalog.
#[test]
fn test_feature_gate_dap_completions() {
    assert!(
        has_feature("dap.completions"),
        "dap.completions must be registered in the feature catalog"
    );
}

/// Capability test: initialize keeps `supportsCompletionsRequest` false.
///
/// #9581 secondary-capability floor: the wire row is an explicit `false`,
/// independent of the `dap.completions` catalog registration, until the
/// completions re-enable gate passes.
#[test]
fn test_capability_dap_completions_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    let supports_completions =
        body.get("supportsCompletionsRequest").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(
        !supports_completions,
        "supportsCompletionsRequest must be false while the #9581 completions floor holds"
    );
    Ok(())
}

/// Functional test: completions rejects explicitly before any completion work.
#[test]
fn test_functional_dap_completions_returns_keywords() -> TestResult {
    let mut adapter = initialize_adapter();
    let response =
        adapter.handle_request(2, "completions", Some(json!({ "text": "pri", "column": 3 })));

    match response {
        DapMessage::Response {
            success: false,
            command,
            body: None,
            message: Some(message),
            ..
        } if command == "completions" => {
            assert!(
                message.contains("unsupported") && message.contains("supportsCompletionsRequest"),
                "expected the explicit #9581 unsupported disposition, got: {message}"
            );
            Ok(())
        }
        other => Err(format!("expected floored completions rejection, got {other:?}").into()),
    }
}

/// Functional test: completions with any prefix keeps the floor disposition.
#[test]
fn test_functional_dap_completions_empty_prefix() -> TestResult {
    let mut adapter = initialize_adapter();
    let response =
        adapter.handle_request(2, "completions", Some(json!({ "text": "", "column": 0 })));

    match response {
        DapMessage::Response {
            success: false,
            command,
            body: None,
            message: Some(message),
            ..
        } if command == "completions" => {
            assert!(
                message.contains("unsupported"),
                "expected the explicit #9581 unsupported disposition, got: {message}"
            );
            Ok(())
        }
        other => Err(format!("expected floored completions rejection, got {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// AC5: dap.exceptions.die
// ---------------------------------------------------------------------------

/// Feature gate: dap.exceptions.die is registered in the catalog.
#[test]
fn test_feature_gate_dap_exceptions_die() {
    assert!(
        has_feature("dap.exceptions.die"),
        "dap.exceptions.die must be registered in the feature catalog"
    );
}

/// Capability test: initialize advertises the `die` exception filter.
#[test]
fn test_capability_dap_exceptions_die_filter_in_initialize() -> TestResult {
    let body = get_initialize_body()?;

    let filters = body
        .get("exceptionBreakpointFilters")
        .and_then(|v| v.as_array())
        .ok_or("Missing exceptionBreakpointFilters")?;

    let has_die = filters.iter().any(|f| f.get("filter").and_then(|v| v.as_str()) == Some("die"));

    if has_feature("dap.exceptions.die") {
        assert!(
            has_die,
            "die filter must appear in exceptionBreakpointFilters when feature is enabled"
        );

        let die_filter = filters
            .iter()
            .find(|f| f.get("filter").and_then(|v| v.as_str()) == Some("die"))
            .ok_or("die filter not found")?;

        assert_eq!(
            die_filter.get("label").and_then(|v| v.as_str()),
            Some("Perl die() and uncaught exceptions"),
            "die filter must have correct label"
        );
    } else {
        assert!(!has_die, "die filter must not appear when dap.exceptions.die is disabled");
    }
    Ok(())
}

/// Functional test: setExceptionBreakpoints with `die` filter succeeds.
#[test]
fn test_functional_dap_exceptions_die_set_filter() -> TestResult {
    let mut adapter = initialize_adapter();
    let response =
        adapter.handle_request(2, "setExceptionBreakpoints", Some(json!({ "filters": ["die"] })));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(success, "setExceptionBreakpoints with die filter must succeed");
            assert_eq!(command, "setExceptionBreakpoints");
        }
        _ => return Err("Expected setExceptionBreakpoints response".into()),
    }
    Ok(())
}

/// Functional test: exceptionInfo request returns the `die` category details.
#[test]
fn test_functional_dap_exceptions_die_exception_info() -> TestResult {
    let mut adapter = initialize_adapter();
    adapter.handle_request(2, "setExceptionBreakpoints", Some(json!({ "filters": ["die"] })));

    let response = adapter.handle_request(3, "exceptionInfo", Some(json!({ "threadId": 1 })));

    // exceptionInfo may return a graceful failure when no session is active,
    // but the command must be recognized (not treated as unknown).
    match response {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "exceptionInfo", "exceptionInfo command must be dispatched");
        }
        _ => return Err("Expected exceptionInfo response".into()),
    }
    Ok(())
}

/// Functional test: `die` and `all` filters both present in exceptionBreakpointFilters.
#[test]
fn test_capability_dap_exceptions_die_and_all_filters() -> TestResult {
    if !has_feature("dap.exceptions.die") {
        return Ok(());
    }

    let body = get_initialize_body()?;
    let filters = body
        .get("exceptionBreakpointFilters")
        .and_then(|v| v.as_array())
        .ok_or("Missing exceptionBreakpointFilters")?;

    let has_all = filters.iter().any(|f| f.get("filter").and_then(|v| v.as_str()) == Some("all"));
    assert!(has_all, "`all` filter must appear alongside `die` in exceptionBreakpointFilters");
    Ok(())
}

// ---------------------------------------------------------------------------
// AC6: dap.inline_values — fail-closed extension floor (#9089)
// ---------------------------------------------------------------------------

/// Feature gate: dap.inline_values is registered but not advertised.
///
/// #9089: the routed inlineValues extension is a project extension kept
/// outside standard DAP capability accounting. The catalog row stays
/// registered for inventory honesty while `advertised = false`, and the wire
/// value comes from the single #9089 negotiation authority, which is false
/// until a versioned negotiation contract is proven.
#[test]
fn test_feature_gate_dap_inline_values() {
    assert!(
        !has_feature("dap.inline_values"),
        "dap.inline_values must not be advertised until #9089's negotiation gate passes"
    );
}

/// Capability test: initialize must not advertise inline values.
#[test]
fn test_capability_dap_inline_values_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    // #9089: supportsInlineValues is authority-bound false — the routed
    // `inlineValues` request is a project extension, not standard DAP, and no
    // versioned negotiation contract exists yet. It must not ride on the
    // catalog row either way.
    let supports_inline =
        body.get("supportsInlineValues").and_then(|v| v.as_bool()).unwrap_or(true);
    assert!(
        !supports_inline,
        "supportsInlineValues must stay false until #9089's negotiation gate passes"
    );
    assert_eq!(
        supports_inline,
        perl_dap::backend::capabilities::advertises_inline_values_extension(),
        "supportsInlineValues must mirror the #9089 negotiation authority, not the \
         catalog row — the row stays unadvertised while the authority alone owns \
         promotion"
    );
    Ok(())
}

/// Functional test: inlineValues is refused with the deterministic #9089
/// refusal — the extension cannot serve source-derived occurrences or runtime
/// values while it is unnegotiated.
#[test]
fn test_functional_dap_inline_values_scalar_extraction() -> TestResult {
    use std::fs::write;
    use tempfile::tempdir;

    let dir = tempdir()?;
    let script = dir.path().join("inline_test.pl");
    write(&script, "my $foo = 42;\nmy $bar = $foo + 1;\n")?;

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        2,
        "inlineValues",
        Some(json!({
            "source": { "path": script.to_str().ok_or("path error")? },
            "startLine": 1,
            "endLine": 2
        })),
    );
    let err = expect_error(response, "inlineValues")?;
    assert!(
        err.contains("inlineValues"),
        "inlineValues refusal must carry the #9089 gate reason, got: {err}"
    );
    Ok(())
}

/// Functional test: the refusal is input-independent — unnegotiated requests
/// are refused as a class, whatever variables the source contains.
#[test]
fn test_functional_dap_inline_values_skips_special_vars() -> TestResult {
    use std::fs::write;
    use tempfile::tempdir;

    let dir = tempdir()?;
    let script = dir.path().join("special_vars_test.pl");
    write(&script, "print $_; warn $!; my $val = 1;\n")?;

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        2,
        "inlineValues",
        Some(json!({
            "source": { "path": script.to_str().ok_or("path error")? },
            "startLine": 1,
            "endLine": 1
        })),
    );
    let err = expect_error(response, "inlineValues")?;
    assert!(
        err.contains("inlineValues"),
        "inlineValues refusal must carry the #9089 gate reason, got: {err}"
    );
    Ok(())
}

/// Functional test: inlineValues for array and hash sources is refused with
/// the same deterministic #9089 refusal — array/hash content cannot widen the
/// floor either.
#[test]
fn test_functional_dap_inline_values_array_and_hash() -> TestResult {
    use std::fs::write;
    use tempfile::tempdir;

    let dir = tempdir()?;
    let script = dir.path().join("array_hash_test.pl");
    write(&script, "my @items = (1, 2, 3);\nmy %config = (a => 1);\n")?;

    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(
        2,
        "inlineValues",
        Some(json!({
            "source": { "path": script.to_str().ok_or("path error")? },
            "startLine": 1,
            "endLine": 2
        })),
    );
    let err = expect_error(response, "inlineValues")?;
    assert!(
        err.contains("inlineValues"),
        "inlineValues refusal must carry the #9089 gate reason, got: {err}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// AC7: dap.modules
// ---------------------------------------------------------------------------

/// Feature gate: dap.modules is registered in the catalog.
#[test]
fn test_feature_gate_dap_modules() {
    assert!(has_feature("dap.modules"), "dap.modules must be registered in the feature catalog");
}

/// Capability test: initialize keeps `supportsModulesRequest` false.
///
/// #9581 secondary-capability floor: the wire row is an explicit `false`,
/// independent of the `dap.modules` catalog registration, until the modules
/// re-enable gate passes.
#[test]
fn test_capability_dap_modules_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    let supports_modules =
        body.get("supportsModulesRequest").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(
        !supports_modules,
        "supportsModulesRequest must be false while the #9581 modules floor holds"
    );
    Ok(())
}

/// Functional test: modules with no session is an explicit unsupported, never
/// a successful empty list (#9581 masquerade falsifier).
#[test]
fn test_functional_dap_modules_no_session() -> TestResult {
    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(2, "modules", Some(json!({ "startModule": 0 })));

    match response {
        DapMessage::Response {
            success: false,
            command,
            body: None,
            message: Some(message),
            ..
        } if command == "modules" => {
            assert!(
                message.contains("unsupported") && message.contains("supportsModulesRequest"),
                "expected the explicit #9581 unsupported disposition, got: {message}"
            );
            Ok(())
        }
        other => Err(format!("expected floored modules rejection, got {other:?}").into()),
    }
}

/// Functional test: pagination arguments are not processed while floored.
#[test]
fn test_functional_dap_modules_with_count_limit() -> TestResult {
    let mut adapter = initialize_adapter();
    let response =
        adapter.handle_request(2, "modules", Some(json!({ "startModule": 0, "moduleCount": 10 })));

    match response {
        DapMessage::Response { success, .. } => {
            assert!(!success, "modules must be floored regardless of arguments (#9581)");
            Ok(())
        }
        _ => Err("Expected modules response".into()),
    }
}

/// Functional test: modules without arguments keeps the floor disposition.
#[test]
fn test_functional_dap_modules_without_arguments() -> TestResult {
    let mut adapter = initialize_adapter();
    let response = adapter.handle_request(2, "modules", None);

    match response {
        DapMessage::Response {
            success: false,
            command,
            body: None,
            message: Some(message),
            ..
        } if command == "modules" => {
            assert!(
                message.contains("unsupported"),
                "expected the explicit #9581 unsupported disposition, got: {message}"
            );
            Ok(())
        }
        other => Err(format!("expected floored modules rejection, got {other:?}").into()),
    }
}

// ---------------------------------------------------------------------------
// AC8: dap.exceptions.warn
// ---------------------------------------------------------------------------

/// Feature gate: dap.exceptions.warn is registered in the catalog.
#[test]
fn test_feature_gate_dap_exceptions_warn() {
    assert!(
        has_feature("dap.exceptions.warn"),
        "dap.exceptions.warn must be registered in the feature catalog"
    );
}

/// Capability test: initialize advertises the `warn` exception filter when feature is enabled.
#[test]
fn test_capability_dap_exceptions_warn_filter_in_initialize() -> TestResult {
    let body = get_initialize_body()?;

    let filters = body
        .get("exceptionBreakpointFilters")
        .and_then(|v| v.as_array())
        .ok_or("Missing exceptionBreakpointFilters")?;

    let has_warn = filters.iter().any(|f| f.get("filter").and_then(|v| v.as_str()) == Some("warn"));

    if has_feature("dap.exceptions.warn") {
        assert!(
            has_warn,
            "warn filter must appear in exceptionBreakpointFilters when feature is enabled"
        );

        let warn_filter = filters
            .iter()
            .find(|f| f.get("filter").and_then(|v| v.as_str()) == Some("warn"))
            .ok_or("warn filter not found")?;

        assert_eq!(
            warn_filter.get("label").and_then(|v| v.as_str()),
            Some("Perl warn() and Carp warnings"),
            "warn filter must have correct label"
        );
        assert_eq!(
            warn_filter.get("default").and_then(|v| v.as_bool()),
            Some(false),
            "warn filter default must be false (non-intrusive by default)"
        );
    } else {
        assert!(!has_warn, "warn filter must not appear when dap.exceptions.warn is disabled");
    }
    Ok(())
}

/// Functional test: setExceptionBreakpoints with `warn` filter succeeds.
#[test]
fn test_functional_dap_exceptions_warn_set_filter() -> TestResult {
    let mut adapter = initialize_adapter();
    let response =
        adapter.handle_request(2, "setExceptionBreakpoints", Some(json!({ "filters": ["warn"] })));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(success, "setExceptionBreakpoints with warn filter must succeed");
            assert_eq!(command, "setExceptionBreakpoints");
        }
        _ => return Err("Expected setExceptionBreakpoints response".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AC9: dap.watchpoints
// ---------------------------------------------------------------------------

/// Feature gate: native data breakpoints are fail-closed (#9091), so the
/// dap.watchpoints row is retired from the advertised set.
#[test]
fn test_feature_gate_dap_watchpoints() {
    assert!(
        !has_feature("dap.watchpoints"),
        "dap.watchpoints must not be advertised while native watchpoint identity/install/hit proof is absent (#9091)"
    );
}

/// Capability test: native data breakpoints are fail-closed (#9091), so the
/// catalog row stays registered in features.toml but is no longer advertised,
/// and initialize must not claim supportsDataBreakpoints.
#[test]
fn test_capability_dap_watchpoints_initialize_response() -> TestResult {
    let body = get_initialize_body()?;

    let supports_data_breakpoints =
        body.get("supportsDataBreakpoints").and_then(|v| v.as_bool()).unwrap_or(false);

    assert!(
        !supports_data_breakpoints,
        "supportsDataBreakpoints must be false while watchpoint identity/install/hit proof is absent (#9091)"
    );
    assert!(
        !has_feature("dap.watchpoints"),
        "dap.watchpoints must be unadvertised while native data breakpoints are unsupported (#9091)"
    );
    Ok(())
}

/// Functional test: the data-breakpoint surface answers honestly without any
/// debugger mutation while native watchpoints are unsupported (#9091).
#[test]
fn test_functional_dap_watchpoints_data_breakpoint_roundtrip() -> TestResult {
    let mut adapter = initialize_adapter();

    // dataBreakpointInfo stays a successful DAP response but must not mint a dataId
    let info_response =
        adapter.handle_request(2, "dataBreakpointInfo", Some(json!({ "name": "$x" })));

    match info_response {
        DapMessage::Response { success, command, body: Some(body), .. } => {
            assert!(success, "dataBreakpointInfo must respond");
            assert_eq!(command, "dataBreakpointInfo");
            assert!(
                body.get("dataId").is_some_and(|value| value.is_null()),
                "no persistent native dataId may be minted (#9091)"
            );
        }
        _ => return Err("Expected dataBreakpointInfo response".into()),
    }

    // setDataBreakpoints must succeed at the protocol layer with one
    // unverified entry per input and zero debugger mutation (#9091)
    let set_response = adapter.handle_request(
        3,
        "setDataBreakpoints",
        Some(json!({ "breakpoints": [{ "dataId": "$x", "accessType": "write" }] })),
    );

    match set_response {
        DapMessage::Response { success, command, body: Some(body), .. } => {
            assert!(success, "setDataBreakpoints must respond");
            assert_eq!(command, "setDataBreakpoints");
            let bps = body
                .get("breakpoints")
                .and_then(|v| v.as_array())
                .ok_or("missing breakpoints array")?;
            assert_eq!(bps.len(), 1, "setDataBreakpoints must return one record per input");
            let verified = bps[0].get("verified").and_then(|v| v.as_bool()).unwrap_or(true);
            assert!(!verified, "returned watchpoint must be unverified while unsupported (#9091)");
        }
        _ => return Err("Expected setDataBreakpoints response with body".into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AC10: dap.goto_targets — fail-closed standard goto (#9064)
// ---------------------------------------------------------------------------

/// Feature gate: `dap.goto_targets` is not advertised while the native backend
/// only offers run-to-line, which is not standard DAP goto.
#[test]
fn test_feature_gate_dap_goto_targets_not_advertised() {
    assert!(
        !has_feature("dap.goto_targets"),
        "dap.goto_targets must not be advertised until a backend proves a real \
         next-statement relocation primitive (#9064)"
    );
}

/// Capability test: initialize must not advertise goto targets while the
/// catalog row is unadvertised.
#[test]
fn test_capability_dap_goto_targets_not_advertised_in_initialize() -> TestResult {
    let body = get_initialize_body()?;
    let supports_goto =
        body.get("supportsGotoTargetsRequest").and_then(|v| v.as_bool()).unwrap_or(false);
    assert_eq!(
        supports_goto,
        has_feature("dap.goto_targets") && has_feature("dap.goto"),
        "supportsGotoTargetsRequest must require the complete dap.goto_targets + dap.goto contract"
    );
    assert!(
        !supports_goto,
        "supportsGotoTargetsRequest must be false while dap.goto_targets is unadvertised"
    );
    Ok(())
}

/// Functional test: both goto requests are explicitly refused while unsupported.
#[test]
fn test_functional_dap_goto_targets_requests_fail_closed() -> TestResult {
    let mut adapter = initialize_adapter();

    for (command, args) in [
        ("gotoTargets", json!({ "source": { "path": "script.pl" }, "line": 3 })),
        ("goto", json!({ "threadId": 1, "targetId": 1 })),
    ] {
        let response = adapter.handle_request(2, command, Some(args));
        match response {
            DapMessage::Response { success, command: actual, message, .. } => {
                assert_eq!(actual, command);
                assert!(!success, "{command} must fail closed while unadvertised (#9064)");
                let err = message.unwrap_or_default();
                assert!(
                    err.to_lowercase().contains("unsupported"),
                    "{command} rejection must explain that standard goto is unsupported: {err}"
                );
            }
            _ => return Err(format!("Expected {command} response").into()),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cross-feature: feature catalog completeness
// ---------------------------------------------------------------------------

/// All DAP features that must be advertised are registered in the feature
/// catalog.
///
/// `dap.inline_values` is deliberately excluded from this advertised set
/// (#9089): its catalog row stays registered for inventory honesty while
/// `advertised = false`, and `test_feature_gate_dap_inline_values` pins that
/// value. The goto rows (#9064) are likewise unadvertised and likewise live
/// outside this list.
///
/// Advertised DAP features must be registered in the feature catalog.
///
/// #9091: `dap.watchpoints` is deliberately excluded — the row remains in
/// features.toml with full maturity metadata, but it is no longer advertised
/// until watchpoint identity/install/hit proof exists.
#[test]
fn test_all_dap_features_registered_in_catalog() {
    let dap_features = [
        "dap.core",
        "dap.breakpoints.basic",
        "dap.breakpoints.hit_condition",
        "dap.breakpoints.logpoints",
        "dap.completions",
        "dap.exceptions.die",
        "dap.exceptions.warn",
        "dap.modules",
    ];

    let missing: Vec<&str> = dap_features.iter().filter(|&&f| !has_feature(f)).copied().collect();

    assert!(
        missing.is_empty(),
        "The following DAP features are missing from the catalog: {missing:?}"
    );

    // The floored extension must not quietly rejoin the advertised set.
    assert!(
        !has_feature("dap.inline_values"),
        "dap.inline_values must stay unadvertised until #9089's negotiation gate passes"
    );
}

/// The initialize response must not advertise capabilities for disabled features.
///
/// #9581: `supportsBreakpointLocationsRequest`, `supportsCompletionsRequest`,
/// and `supportsModulesRequest` are excluded from the catalog-mirror table —
/// they are explicit `false` floor rows that no catalog registration may
/// widen (asserted in the capability tests above and in
/// dap_capability_floor_9581_tests.rs).
#[test]
fn test_initialize_does_not_advertise_disabled_features() -> TestResult {
    let body = get_initialize_body()?;

    // Each capability must be false if its feature is disabled.
    // Derived from handle_initialize in debug_adapter/process.rs:
    //   supportsConditionalBreakpoints      = supports_basic_breakpoints
    //   supportsHitConditionalBreakpoints   = supports_hit_conditions
    //   supportsLogPoints                   = supports_log_points
    //   supportsInlineValues                = supports_inline_values
    //   supportsBreakpointLocationsRequest  = supports_basic_breakpoints
    //   supportsInlineValues                = #9089 negotiation authority (not the catalog row)
    //   supportsCompletionsRequest          = supports_completions
    //   supportsModulesRequest              = supports_modules
    //   supportsDataBreakpoints             = supports_watchpoints
    //
    // `supportsInlineValues` is deliberately absent from this catalog-mirror
    // loop (#9089): its wire value comes from the negotiation authority, not
    // the `dap.inline_values` row, and is pinned against the authority in
    // `test_capability_dap_inline_values_initialize_response`.
    //   #9578: supportsConditionalBreakpoints / supportsHitConditionalBreakpoints /
    //   supportsLogPoints no longer mirror catalog rows at all (they are pinned
    //   false by the breakpoint authority and pinned again below), and
    //   supportsFunctionBreakpoints never mirrored `dap.breakpoints.basic`.
    let feature_to_cap = [
        // #9578/#9581: supportsConditionalBreakpoints, supportsHitConditional
        // Breakpoints, supportsLogPoints, supportsBreakpointLocationsRequest,
        // supportsCompletionsRequest, and supportsModulesRequest are floored
        // rows now (pinned false below regardless of catalog state), so they
        // are excluded from the mirror contract.
        ("dap.watchpoints", "supportsDataBreakpoints"),
        // #9064: goto capability mirrors its own catalog row, not broad core
        // state; while the row is unadvertised the flag must stay false.
        ("dap.goto_targets", "supportsGotoTargetsRequest"),
    ];

    for (feature, capability) in feature_to_cap {
        let advertised = body.get(capability).and_then(|v| v.as_bool()).unwrap_or(false);
        let enabled = has_feature(feature);

        assert_eq!(
            advertised, enabled,
            "Capability `{capability}` must mirror feature `{feature}`: enabled={enabled}, advertised={advertised}"
        );
    }

    // #9581 floor rows: false regardless of catalog state.
    for capability in [
        "supportsBreakpointLocationsRequest",
        "supportsCompletionsRequest",
        "supportsModulesRequest",
    ] {
        let advertised = body.get(capability).and_then(|v| v.as_bool()).unwrap_or(false);
        assert!(
            !advertised,
            "Capability `{capability}` must stay false under the #9581 secondary-capability floor"
        );
    }
    // #9578: the four optional breakpoint capability rows must stay false even
    // while their catalog rows are registered and advertised — a catalog or
    // core flag change cannot make the fields true.
    let floored_rows = [
        "supportsFunctionBreakpoints",
        "supportsConditionalBreakpoints",
        "supportsHitConditionalBreakpoints",
        "supportsLogPoints",
    ];
    for capability in floored_rows {
        let advertised = body.get(capability).and_then(|v| v.as_bool()).unwrap_or(true);
        assert!(
            !advertised,
            "Capability `{capability}` must stay false despite its advertised catalog row (#9578)"
        );
    }
    Ok(())
}
