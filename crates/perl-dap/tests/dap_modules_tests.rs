//! Modules is a #9581 secondary-capability floor row.
//!
//! `supportsModulesRequest` is an explicit `false` cell in the initialize
//! response until the modules re-enable gate passes (#8581 + #7667/#8668 +
//! #9585 + #9586). While it is false, every `modules` request must return the
//! explicit unsupported disposition *before* any `%INC` query, pagination, or
//! module-ID allocation, and a missing/unavailable session can never become an
//! unexplained successful empty list (#9581).

use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Send a `modules` request and return `(success, message, body)`.
fn modules_request(
    adapter: &mut DebugAdapter,
    arguments: Option<serde_json::Value>,
) -> Result<(bool, String, Option<serde_json::Value>), String> {
    match adapter.handle_request(2, "modules", arguments) {
        DapMessage::Response { success, command, body, message, .. } if command == "modules" => {
            Ok((success, message.unwrap_or_default(), body))
        }
        DapMessage::Response { command, .. } => Err(format!("unexpected command `{command}`")),
        _ => Err("unexpected response type".to_string()),
    }
}

fn assert_floor_rejection(success: bool, message: &str, body: &Option<serde_json::Value>) {
    assert!(!success, "modules is floored (#9581) and must fail");
    assert!(
        message.contains("unsupported") && message.contains("supportsModulesRequest"),
        "expected the explicit #9581 unsupported disposition, got: {message}"
    );
    assert!(body.is_none(), "a floored modules request must not carry a modules/totalModules body");
}

#[test]
fn initialize_does_not_advertise_modules() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let init = adapter.handle_request(1, "initialize", None);
    let body = match init {
        DapMessage::Response { success: true, body: Some(body), .. } => body,
        _ => return Err("Expected successful initialize response".into()),
    };
    let supported = body
        .get("supportsModulesRequest")
        .and_then(serde_json::Value::as_bool)
        .ok_or("supportsModulesRequest must be a boolean")?;
    assert!(
        !supported,
        "supportsModulesRequest must be false while the modules gate is open (#9581)"
    );
    Ok(())
}

#[test]
fn modules_without_a_session_is_unsupported_not_an_empty_success() -> TestResult {
    // The discriminating no-masquerade row (#9581): a request before any
    // launch must fail explicitly, never succeed with an empty list.
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let (success, message, body) = modules_request(&mut adapter, Some(json!({ "startModule": 0 })))
        .map_err(|e| e.to_string())?;
    assert_floor_rejection(success, &message, &body);
    assert!(
        message.contains("#9581"),
        "the rejection must name the capability floor for traceability: {message}"
    );
    Ok(())
}

#[test]
fn modules_pagination_arguments_are_not_processed() -> TestResult {
    // Pagination is part of the floored handler computation: every argument
    // shape gets the same explicit rejection with no module-ID allocation.
    let cases = [
        Some(json!({ "startModule": 0, "moduleCount": 10 })),
        Some(json!({ "startModule": 0, "moduleCount": 0 })),
        Some(json!({ "startModule": -1 })),
        Some(json!({ "startModule": 0, "moduleCount": -1 })),
        Some(json!({ "startModule": 999_999 })),
        None,
    ];
    for arguments in cases {
        let mut adapter = DebugAdapter::new();
        adapter.handle_request(1, "initialize", None);
        let (success, message, body) =
            modules_request(&mut adapter, arguments).map_err(|e| e.to_string())?;
        assert_floor_rejection(success, &message, &body);
    }
    Ok(())
}

#[test]
fn modules_rejection_is_stable_across_repeated_requests() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let first = modules_request(&mut adapter, Some(json!({ "startModule": 0 })))
        .map_err(|e| e.to_string())?;
    let second = modules_request(&mut adapter, Some(json!({ "startModule": 0 })))
        .map_err(|e| e.to_string())?;
    assert_eq!(first.0, second.0, "floor disposition must not flip between requests");
    assert_eq!(first.1, second.1, "floor message must be deterministic");
    assert_floor_rejection(first.0, &first.1, &first.2);
    Ok(())
}
