//! Completions is a #9581 secondary-capability floor row.
//!
//! `supportsCompletionsRequest` is an explicit `false` cell in the initialize
//! response until the completions re-enable gate passes (#9021 + #9046 +
//! #9050 + #8581 + #9582 + #9584). While it is false, every `completions`
//! request must return the explicit unsupported disposition *before* any
//! variable-cache, `%INC`, source-text, or debugger work, and must never
//! masquerade a static keyword list as proven debug-console completion
//! support (#9581).

use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::must;
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn floor_response(
    adapter: &mut DebugAdapter,
    arguments: Option<serde_json::Value>,
) -> Result<(bool, String, Option<serde_json::Value>), String> {
    match adapter.handle_request(2, "completions", arguments) {
        DapMessage::Response { success, command, body, message, .. }
            if command == "completions" =>
        {
            Ok((success, message.unwrap_or_default(), body))
        }
        DapMessage::Response { command, .. } => Err(format!("unexpected command `{command}`")),
        _ => Err("unexpected response type".to_string()),
    }
}

fn assert_floor_rejection(success: bool, message: &str, body: &Option<serde_json::Value>) {
    assert!(!success, "completions is floored (#9581) and must fail");
    assert!(
        message.contains("unsupported") && message.contains("supportsCompletionsRequest"),
        "expected the explicit #9581 unsupported disposition, got: {message}"
    );
    assert!(
        body.is_none(),
        "a floored completions request must not return a plausible target list"
    );
}

#[test]
fn initialize_does_not_advertise_completions() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let init = adapter.handle_request(1, "initialize", None);
    let body = match init {
        DapMessage::Response { success: true, body: Some(body), .. } => body,
        _ => return Err("Expected successful initialize response".into()),
    };
    let supported = body
        .get("supportsCompletionsRequest")
        .and_then(serde_json::Value::as_bool)
        .ok_or("supportsCompletionsRequest must be a boolean")?;
    assert!(
        !supported,
        "supportsCompletionsRequest must be false while the completions gate is open (#9581)"
    );
    Ok(())
}

#[test]
fn completions_without_a_session_is_unsupported_not_an_empty_success() -> TestResult {
    // Discriminates the no-missing-session masquerade: a request before any
    // launch must fail explicitly rather than return an empty-but-successful
    // list (#9581).
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let (success, message, body) =
        floor_response(&mut adapter, Some(json!({ "text": "pr", "column": 2 })))
            .map_err(|e| e.to_string())?;
    assert_floor_rejection(success, &message, &body);
    assert!(
        message.contains("#9581"),
        "the rejection must name the capability floor for traceability: {message}"
    );
    Ok(())
}

#[test]
fn completions_returns_no_keyword_list_for_any_prefix() -> TestResult {
    for (text, column) in [("pr", 2), ("", 0), ("zzz", 3), ("my $x = pri", 11), ("s", 1)] {
        let mut adapter = DebugAdapter::new();
        adapter.handle_request(1, "initialize", None);
        let (success, message, body) =
            floor_response(&mut adapter, Some(json!({ "text": text, "column": column })))
                .map_err(|e| e.to_string())?;
        assert_floor_rejection(success, &message, &body);
    }
    Ok(())
}

#[test]
fn completions_missing_arguments_still_get_the_floor_disposition() -> TestResult {
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let (success, message, body) = floor_response(&mut adapter, None).map_err(|e| e.to_string())?;
    assert_floor_rejection(success, &message, &body);
    Ok(())
}

#[test]
fn completions_rejection_is_stable_across_repeated_requests() -> TestResult {
    // The floor must be a constant disposition, not state-dependent: repeated
    // requests return the same explicit rejection and never mutate session
    // state as a side effect.
    let mut adapter = DebugAdapter::new();
    adapter.handle_request(1, "initialize", None);
    let first = floor_response(&mut adapter, Some(json!({ "text": "un", "column": 2 })))
        .map_err(|e| e.to_string())?;
    let second = floor_response(&mut adapter, Some(json!({ "text": "un", "column": 2 })))
        .map_err(|e| e.to_string())?;
    assert_eq!(first.0, second.0, "floor disposition must not flip between requests");
    assert_eq!(first.1, second.1, "floor message must be deterministic");
    assert_floor_rejection(first.0, &first.1, &first.2);
    Ok(())
}

mod proptest_completions {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// The floor must never panic regardless of client input, and every
        /// arbitrary input gets the same explicit unsupported disposition.
        #[test]
        fn test_completions_arbitrary_input(
            text in "\\PC{0,200}",
            column in -10i64..300
        ) {
            let mut adapter = DebugAdapter::new();
            adapter.handle_request(1, "initialize", None);

            let response = adapter.handle_request(
                2,
                "completions",
                Some(json!({ "text": text, "column": column })),
            );

            match response {
                DapMessage::Response { success, command, body, message, .. } => {
                    assert_eq!(command, "completions");
                    assert!(!success, "completions is floored (#9581)");
                    assert!(message.is_some_and(|m| m.contains("unsupported")));
                    assert!(body.is_none(), "no plausible target list may leak");
                }
                _ => must(Err::<(), _>("Expected response for completions")),
            }
        }
    }
}
