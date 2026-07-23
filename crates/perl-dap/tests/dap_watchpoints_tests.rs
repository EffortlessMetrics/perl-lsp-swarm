use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::must;
use serde_json::json;
use std::sync::LazyLock;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Mirror of the production `is_valid_set_variable_name` validator so tests can
/// predict the expected `verified` flag for an arbitrary data_id without
/// depending on a private crate item.  Matches the production regex:
/// `^[\$\@\%](?:[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*|\d+|_)$`
/// with a 512-char length cap, plus the newline defense-in-depth check.
static VALID_NAME_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    must(regex::Regex::new(
        r"^[\$\@\%](?:[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*|\d+|_)$",
    ))
});

fn is_valid_set_variable_name_helper(data_id: &str) -> bool {
    if data_id.contains('\n') || data_id.contains('\r') {
        return false;
    }
    data_id.trim().len() <= 512 && VALID_NAME_RE.is_match(data_id.trim())
}

/// Helper: send a dataBreakpointInfo request and return the response body.
fn data_breakpoint_info_request(
    adapter: &mut DebugAdapter,
    name: &str,
) -> Result<serde_json::Value, String> {
    let response = adapter.handle_request(2, "dataBreakpointInfo", Some(json!({ "name": name })));
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        DapMessage::Response { success: false, message, .. } => {
            Err(message.unwrap_or_else(|| "unknown error".to_string()))
        }
        _ => Err("unexpected response type".to_string()),
    }
}

/// Helper: send a setDataBreakpoints request and return the response body.
fn set_data_breakpoints_request(
    adapter: &mut DebugAdapter,
    breakpoints: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = adapter.handle_request(
        3,
        "setDataBreakpoints",
        Some(json!({ "breakpoints": breakpoints })),
    );
    match response {
        DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
        DapMessage::Response { success: false, message, .. } => {
            Err(message.unwrap_or_else(|| "unknown error".to_string()))
        }
        _ => Err("unexpected response type".to_string()),
    }
}

// === dataBreakpointInfo tests ===

#[test]
fn test_data_breakpoint_info_valid_scalar() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "$x")?;

    let data_id = body.get("dataId").and_then(|v| v.as_str());
    assert_eq!(data_id, Some("$x"), "Valid scalar should have a dataId");

    let access_types = body.get("accessTypes").and_then(|v| v.as_array());
    assert!(access_types.is_some(), "Should include access types");
    assert!(
        access_types.iter().flat_map(|a| a.iter()).any(|v| v.as_str() == Some("write")),
        "Should support write access"
    );

    Ok(())
}

#[test]
fn test_data_breakpoint_info_valid_hash() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "%ENV")?;
    let data_id = body.get("dataId").and_then(|v| v.as_str());
    assert_eq!(data_id, Some("%ENV"), "Valid hash should have a dataId");

    Ok(())
}

#[test]
fn test_data_breakpoint_info_valid_array() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "@ARGV")?;
    let data_id = body.get("dataId").and_then(|v| v.as_str());
    assert_eq!(data_id, Some("@ARGV"), "Valid array should have a dataId");

    Ok(())
}

#[test]
fn test_data_breakpoint_info_qualified_name() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "$Foo::Bar::baz")?;
    let data_id = body.get("dataId").and_then(|v| v.as_str());
    assert_eq!(data_id, Some("$Foo::Bar::baz"), "Qualified name should be watchable");

    Ok(())
}

#[test]
fn test_data_breakpoint_info_invalid_name() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "not a variable")?;
    let data_id = body.get("dataId");
    assert!(
        data_id.is_none() || data_id.is_some_and(|v| v.is_null()),
        "Invalid name should have null dataId"
    );

    Ok(())
}

#[test]
fn test_data_breakpoint_info_injection_attempt() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "$x; system('ls')")?;
    let data_id = body.get("dataId");
    assert!(
        data_id.is_none() || data_id.is_some_and(|v| v.is_null()),
        "Injection attempt should be rejected"
    );

    Ok(())
}

#[test]
fn test_data_breakpoint_info_empty_name() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = data_breakpoint_info_request(&mut adapter, "")?;
    let data_id = body.get("dataId");
    assert!(
        data_id.is_none() || data_id.is_some_and(|v| v.is_null()),
        "Empty name should not be watchable"
    );

    Ok(())
}

#[test]
fn test_data_breakpoint_info_missing_arguments() {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(2, "dataBreakpointInfo", None);

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Expected failure for missing arguments");
            assert!(message.is_some(), "Expected error message");
        }
        _ => must(Err::<(), _>("Expected response message")),
    }
}

// === setDataBreakpoints tests ===

#[test]
fn test_set_data_breakpoints_empty_list() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(&mut adapter, json!([]))?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    assert!(breakpoints.is_empty(), "Empty input should produce empty output");

    Ok(())
}

#[test]
fn test_set_data_breakpoints_single() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([{ "dataId": "$x", "accessType": "write" }]),
    )?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    assert_eq!(breakpoints.len(), 1, "Should have one breakpoint");

    let bp = &breakpoints[0];
    assert!(
        bp.get("verified").and_then(|v| v.as_bool()).unwrap_or(false),
        "Breakpoint should be verified"
    );

    Ok(())
}

#[test]
fn test_set_data_breakpoints_multiple() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([
            { "dataId": "$x", "accessType": "write" },
            { "dataId": "%ENV", "accessType": "write" },
            { "dataId": "@ARGV", "accessType": "write" }
        ]),
    )?;

    let breakpoints =
        body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    assert_eq!(breakpoints.len(), 3, "Should have three breakpoints, order preserved");

    // Verify IDs are sequential
    for (i, bp) in breakpoints.iter().enumerate() {
        let id = bp.get("id").and_then(|v| v.as_i64()).ok_or("missing id")?;
        assert_eq!(id, (i as i64) + 1, "IDs should be sequential");
    }

    Ok(())
}

#[test]
fn test_set_data_breakpoints_replace_semantics() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    // First request: set two breakpoints
    let body1 = set_data_breakpoints_request(
        &mut adapter,
        json!([
            { "dataId": "$x", "accessType": "write" },
            { "dataId": "$y", "accessType": "write" }
        ]),
    )?;
    let bps1 = body1.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    assert_eq!(bps1.len(), 2);

    // Second request: replace with just one
    let body2 = set_data_breakpoints_request(
        &mut adapter,
        json!([{ "dataId": "$z", "accessType": "write" }]),
    )?;
    let bps2 = body2.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    assert_eq!(bps2.len(), 1, "Second request should replace, not append");

    Ok(())
}

#[test]
fn test_set_data_breakpoints_missing_arguments() {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(3, "setDataBreakpoints", None);

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Expected failure for missing arguments");
            assert!(message.is_some(), "Expected error message");
        }
        _ => must(Err::<(), _>("Expected response message")),
    }
}

// === setDataBreakpoints data_id validation (Issue #4637) ===
//
// A hostile dataId is interpolated raw into the Perl debugger `w {dataId}`
// command.  Invalid or injection-laden dataIds must be reported as
// verified:false rather than passed through to the debugger.

/// Helper: extract the verified flag and message for the n-th breakpoint.
fn breakpoint_verified_at(
    body: &serde_json::Value,
    index: usize,
) -> Result<(bool, Option<String>), Box<dyn std::error::Error>> {
    let bps = body.get("breakpoints").and_then(|v| v.as_array()).ok_or("missing breakpoints")?;
    let bp = bps.get(index).ok_or("breakpoint index out of range")?;
    let verified = bp.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
    let message = bp.get("message").and_then(|v| v.as_str()).map(ToString::to_string);
    Ok((verified, message))
}

#[test]
fn test_set_data_breakpoints_rejects_injection_data_id() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([{ "dataId": "$x; system('id')", "accessType": "write" }]),
    )?;

    let (verified, message) = breakpoint_verified_at(&body, 0)?;
    assert!(!verified, "injection dataId must be verified:false");
    assert!(message.is_some(), "rejected breakpoint should carry an explanatory message");
    Ok(())
}

#[test]
fn test_set_data_breakpoints_rejects_newline_data_id() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([{ "dataId": "$x\ndie('inject')", "accessType": "write" }]),
    )?;

    let (verified, message) = breakpoint_verified_at(&body, 0)?;
    assert!(!verified, "newline dataId must be verified:false");
    let msg = message.unwrap_or_default();
    assert!(
        msg.contains("newline"),
        "newline rejection message should mention newlines, got: {msg:?}"
    );
    Ok(())
}

#[test]
fn test_set_data_breakpoints_rejects_data_id_without_sigil() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([{ "dataId": "system('id')", "accessType": "write" }]),
    )?;

    let (verified, _message) = breakpoint_verified_at(&body, 0)?;
    assert!(!verified, "dataId without a sigil must be verified:false");
    Ok(())
}

#[test]
fn test_set_data_breakpoints_mixed_batch_partial_verification() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([
            { "dataId": "$x", "accessType": "write" },
            { "dataId": "$y; system('id')", "accessType": "write" }
        ]),
    )?;

    let (verified_first, _) = breakpoint_verified_at(&body, 0)?;
    assert!(verified_first, "legitimate first breakpoint must be verified:true");

    let (verified_second, _) = breakpoint_verified_at(&body, 1)?;
    assert!(!verified_second, "injection second breakpoint must be verified:false");
    Ok(())
}

#[test]
fn test_set_data_breakpoints_legitimate_batch_all_verified() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let body = set_data_breakpoints_request(
        &mut adapter,
        json!([
            { "dataId": "$x", "accessType": "write" },
            { "dataId": "%ENV", "accessType": "write" },
            { "dataId": "@ARGV", "accessType": "write" }
        ]),
    )?;

    for i in 0..3 {
        let (verified, message) = breakpoint_verified_at(&body, i)?;
        assert!(verified, "legitimate breakpoint {i} must be verified:true");
        assert!(message.is_none(), "verified breakpoint {i} should not carry a rejection message");
    }
    Ok(())
}

#[test]
fn test_data_breakpoint_info_response_is_success() {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);

    let response = adapter.handle_request(2, "dataBreakpointInfo", Some(json!({ "name": "$x" })));

    match response {
        DapMessage::Response { success, command, .. } => {
            assert!(success);
            assert_eq!(command, "dataBreakpointInfo");
        }
        _ => must(Err::<(), _>("Expected response message")),
    }
}

mod proptest_watchpoints {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_data_breakpoint_info_arbitrary_names(
            name in "\\PC{0,100}"
        ) {
            let mut adapter = DebugAdapter::new();
            adapter.handle_request(1, "initialize", None);

            // Must never panic regardless of input
            let response = adapter.handle_request(
                2,
                "dataBreakpointInfo",
                Some(json!({ "name": name })),
            );

            match response {
                DapMessage::Response { success, command, .. } => {
                    assert!(success, "dataBreakpointInfo should always succeed");
                    assert_eq!(command, "dataBreakpointInfo");
                }
                _ => must(Err::<(), _>("Expected response")),
            }
        }

        #[test]
        fn test_set_data_breakpoints_arbitrary_lists_preserve_shape(
            breakpoints in vec(
                (
                    "\\PC{0,64}",
                    proptest::option::of("read|write|readWrite"),
                    proptest::option::of("\\PC{0,64}")
                ),
                0..40
            )
        ) {
            let mut adapter = DebugAdapter::new();
            adapter.handle_request(1, "initialize", None);

            let request_breakpoints: Vec<serde_json::Value> = breakpoints
                .iter()
                .map(|(data_id, access_type, condition)| {
                    let mut bp = json!({ "dataId": data_id });
                    if let Some(access_type) = access_type {
                        bp["accessType"] = json!(access_type);
                    }
                    if let Some(condition) = condition {
                        bp["condition"] = json!(condition);
                    }
                    bp
                })
                .collect();

            let body = set_data_breakpoints_request(&mut adapter, json!(request_breakpoints))
                .map_err(|e| proptest::test_runner::TestCaseError::fail(e.to_string()))?;

            let response_breakpoints = body
                .get("breakpoints")
                .and_then(|value| value.as_array())
                .ok_or_else(|| {
                    proptest::test_runner::TestCaseError::fail("missing response breakpoints")
                })?;

            prop_assert_eq!(response_breakpoints.len(), breakpoints.len());

            for (index, breakpoint) in response_breakpoints.iter().enumerate() {
                prop_assert_eq!(
                    breakpoint.get("id").and_then(|value| value.as_i64()),
                    Some((index as i64) + 1)
                );
                // Verification is per-breakpoint: valid Perl variable names are
                // verified:true, anything else (including arbitrary strings that
                // may contain injection payloads) is verified:false.
                let data_id = &breakpoints[index].0;
                let expected_verified = is_valid_set_variable_name_helper(data_id);
                prop_assert_eq!(
                    breakpoint.get("verified").and_then(|value| value.as_bool()),
                    Some(expected_verified),
                    "verified flag mismatch for dataId {:?}",
                    data_id
                );
            }
        }
    }
}
