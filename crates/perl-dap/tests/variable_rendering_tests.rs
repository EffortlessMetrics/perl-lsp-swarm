//! Variable Rendering Tests (AC8.6)
//!
//! Comprehensive tests for DAP variable rendering including:
//! - Hierarchical scope retrieval (Locals, Package, Globals)
//! - Lazy child expansion for arrays and hashes
//! - Scalar value truncation
//! - Complex nested structures
//!
//! Specification: GitHub Issue #452 - AC8.1, AC8.3, AC8.4, AC8.6

use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::json;

/// Helper to create a test adapter
fn create_test_adapter() -> DebugAdapter {
    DebugAdapter::new()
}

#[test]
// AC:8.1
fn test_threads_main_thread_only() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    // Threads request without session should return empty
    let response = adapter.handle_request(1, "threads", None);

    if let DapMessage::Response { body, .. } = response {
        let body_val = body.ok_or("Expected body in response")?;
        let threads = body_val
            .get("threads")
            .ok_or("Expected threads field")?
            .as_array()
            .ok_or("Expected threads array")?;
        assert!(threads.is_empty());
    }
    Ok(())
}

#[test]
// AC:8.3
fn test_scopes_hierarchy() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "frameId": 1 });
    let response = adapter.handle_request(1, "scopes", Some(args));

    if let DapMessage::Response { success, body, .. } = response {
        assert!(success);
        let body_val = body.ok_or("Expected body in response")?;
        let scopes = body_val
            .get("scopes")
            .ok_or("Expected scopes field")?
            .as_array()
            .ok_or("Expected scopes array")?;

        // Without a live debuggee session the adapter must not fabricate the
        // Locals/Package/Globals frame state — same no-fabrication law as the
        // variables tests above (#7275). Scope enumeration is exercised by the
        // live-session suites.
        assert!(scopes.is_empty(), "session-less scopes must be empty; got: {scopes:?}");
    }
    Ok(())
}

#[test]
// AC:8.4
// Without a live debuggee session the adapter must not fabricate DB-internal
// placeholders (`@_`, `$self`): the Locals flow deliberately returns nothing
// rather than reconstructing "unrelated session history" — same law as the
// Globals scope test above (#7275). Lazy-expansion indicators are exercised
// where they are produced: the renderer unit tests (indexed/named variables)
// and the live-session suites.
fn test_variables_lazy_expansion_indicators() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();

    // Request variables for Locals scope (ref 11 for frame 1)
    let args = json!({ "variablesReference": 11 });
    let response = adapter.handle_request(1, "variables", Some(args));

    if let DapMessage::Response { body, .. } = response {
        let body_val = body.ok_or("Expected body in response")?;
        let vars = body_val
            .get("variables")
            .ok_or("Expected variables field")?
            .as_array()
            .ok_or("Expected variables array")?;

        assert!(
            vars.iter().all(|v| v.get("name").and_then(|n| n.as_str()) != Some("@_")
                && v.get("name").and_then(|n| n.as_str()) != Some("$self")),
            "session-less Locals must not fabricate DB-internal placeholders: {vars:?}"
        );
    }
    Ok(())
}

#[test]
// AC:8.6
fn test_variables_globals_scope() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "variablesReference": 13 }); // Globals for frame 1
    let response = adapter.handle_request(1, "variables", Some(args));

    let DapMessage::Response { body, .. } = response else {
        return Err("Expected a response to the variables request".into());
    };
    let body_val = body.ok_or("Expected body in response")?;
    let vars = body_val
        .get("variables")
        .ok_or("Expected variables field")?
        .as_array()
        .ok_or("Expected variables array")?;

    // Without a live debugger the adapter cannot observe `$_`, so it must report
    // nothing rather than a hard-coded `$_ = undef` the client would render
    // identically to a real observation (#7275).
    assert!(vars.is_empty(), "Globals scope without a live session must be empty; got: {vars:?}");
    Ok(())
}

#[test]
// AC8.4: Scalar truncation itself is covered at the rendering layer
// (variables/renderer.rs: test_string_truncation, test_string_truncation_zero_max_length,
// test_string_truncation_utf8_boundary_safety). Here we pin the session-less
// contract that feeds the renderer: no fabricated Locals content.
fn test_scalar_truncation() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();

    // Request variables for Locals scope
    let args = json!({ "variablesReference": 11 });
    let response = adapter.handle_request(1, "variables", Some(args));

    if let DapMessage::Response { success, body, .. } = response {
        assert!(success);
        let body_val = body.ok_or("Expected body in response")?;
        let vars = body_val
            .get("variables")
            .ok_or("Expected variables field")?
            .as_array()
            .ok_or("Expected variables array")?;

        // Session-less Locals must be empty rather than fabricated (#7275);
        // every emitted variable still carries the required DAP fields.
        for var in vars {
            assert!(var.get("name").is_some());
            assert!(var.get("value").is_some());
            assert!(var.get("variablesReference").is_some());
        }
    }
    Ok(())
}

#[test]
// AC8.6: Test variable type indicators
fn test_variable_type_indicators() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "variablesReference": 11 });
    let response = adapter.handle_request(1, "variables", Some(args));

    if let DapMessage::Response { body, .. } = response {
        let body_val = body.ok_or("Expected body in response")?;
        let vars = body_val
            .get("variables")
            .ok_or("Expected variables field")?
            .as_array()
            .ok_or("Expected variables array")?;

        // Arrays should have indexedVariables
        let arrays = vars
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("array"))
            .collect::<Vec<_>>();

        for array in arrays {
            assert!(array.get("indexedVariables").is_some(), "Array should have indexedVariables");
            assert!(
                array
                    .get("variablesReference")
                    .and_then(|r| r.as_i64())
                    .map(|r| r > 0)
                    .unwrap_or(false),
                "Array should have non-zero variablesReference for lazy expansion"
            );
        }

        // Hashes should have namedVariables
        let hashes = vars
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("hash"))
            .collect::<Vec<_>>();

        for hash in hashes {
            assert!(hash.get("namedVariables").is_some(), "Hash should have namedVariables");
            assert!(
                hash.get("variablesReference")
                    .and_then(|r| r.as_i64())
                    .map(|r| r > 0)
                    .unwrap_or(false),
                "Hash should have non-zero variablesReference for lazy expansion"
            );
        }
    }
    Ok(())
}

#[test]
// AC8.6: Test package scope variables
fn test_package_scope_variables() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "variablesReference": 12 }); // Package for frame 1
    let response = adapter.handle_request(1, "variables", Some(args));

    let DapMessage::Response { success, body, .. } = response else {
        return Err("Expected a response to the variables request".into());
    };
    assert!(success);
    let body_val = body.ok_or("Expected body in response")?;
    let vars = body_val
        .get("variables")
        .ok_or("Expected variables field")?
        .as_array()
        .ok_or("Expected variables array")?;

    // Without a live debugger the adapter cannot observe package state, so it must
    // report nothing rather than a representative `$VERSION = "1.0.0"` the client
    // would render identically to a real observation (#7275).
    assert!(vars.is_empty(), "Package scope without a live session must be empty; got: {vars:?}");
    Ok(())
}

#[test]
// AC8.4: Test lazy expansion reference allocation
fn test_lazy_expansion_references() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "variablesReference": 11 });
    let response = adapter.handle_request(1, "variables", Some(args));

    if let DapMessage::Response { body, .. } = response {
        let body_val = body.ok_or("Expected body in response")?;
        let vars = body_val
            .get("variables")
            .ok_or("Expected variables field")?
            .as_array()
            .ok_or("Expected variables array")?;

        // Verify that expandable structures have unique references
        let refs: Vec<i64> = vars
            .iter()
            .filter_map(|v| v.get("variablesReference").and_then(|r| r.as_i64()))
            .filter(|&r| r > 0)
            .collect();

        // Check uniqueness of non-zero references
        let unique_refs: std::collections::HashSet<_> = refs.iter().collect();
        assert_eq!(refs.len(), unique_refs.len(), "All lazy expansion references should be unique");
    }
    Ok(())
}

#[test]
// AC8.3: Expensive-flag classification is exercised where scopes are
// produced (live-session suites); session-less scopes are empty.
fn test_scope_expensive_flags() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "frameId": 1 });
    let response = adapter.handle_request(1, "scopes", Some(args));

    if let DapMessage::Response { body, .. } = response {
        let body_val = body.ok_or("Expected body in response")?;
        let scopes = body_val
            .get("scopes")
            .ok_or("Expected scopes field")?
            .as_array()
            .ok_or("Expected scopes array")?;

        assert!(scopes.is_empty(), "session-less scopes must be empty; got: {scopes:?}");
    }
    Ok(())
}

#[test]
fn test_variables_placeholder_pagination() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = create_test_adapter();
    let args = json!({ "variablesReference": 11, "start": 1, "count": 1 });
    let response = adapter.handle_request(1, "variables", Some(args));

    if let DapMessage::Response { success, body, .. } = response {
        assert!(success);
        let body_val = body.ok_or("Expected body in response")?;
        let vars = body_val
            .get("variables")
            .ok_or("Expected variables field")?
            .as_array()
            .ok_or("Expected variables array")?;

        // Pagination over a session-less Locals scope has no fabricated
        // placeholders to paginate (see #7275 no-fabrication law above).
        assert!(vars.is_empty(), "session-less Locals pagination must be empty; got: {vars:?}");
    }
    Ok(())
}
