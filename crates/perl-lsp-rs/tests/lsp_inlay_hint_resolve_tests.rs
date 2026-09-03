use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

/// Test that inlayHint/resolve adds tooltip when requested
#[test]
fn lsp_inlay_hint_resolve_adds_tooltip() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "capabilities": {
                "textDocument": {
                    "inlayHint": {
                        "resolveSupport": {
                            "properties": ["tooltip"]
                        }
                    }
                }
            }
        })),
    };
    srv.handle_request(init);

    let initialized = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    srv.handle_request(initialized);

    // Resolve a parameter hint
    let hint = json!({
        "position": {"line": 0, "character": 10},
        "label": "expr:",
        "kind": 2,
        "paddingLeft": false,
        "paddingRight": true,
        "data": {
            "uri": "file:///test.pl",
            "function": "substr",
            "paramIndex": 0
        }
    });

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "inlayHint/resolve".into(),
        params: Some(hint.clone()),
    };

    let res = srv.handle_request(req).ok_or("Failed to handle inlayHint/resolve request")?;
    let result = res.result.ok_or("No result in inlayHint/resolve response")?;

    // Should add tooltip
    assert!(result.get("tooltip").is_some(), "should add tooltip");

    // Should preserve original fields
    assert_eq!(result["label"], "expr:");
    assert_eq!(result["kind"], 2);
    assert_eq!(result["data"]["uri"], "file:///test.pl");

    Ok(())
}

/// Test that resolve preserves data field
#[test]
fn lsp_inlay_hint_resolve_preserves_data() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({"capabilities":{}})),
    };
    srv.handle_request(init);

    let initialized = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    srv.handle_request(initialized);

    let hint = json!({
        "position": {"line": 5, "character": 20},
        "label": ": Str",
        "kind": 1,
        "paddingLeft": true,
        "paddingRight": false,
        "data": {
            "uri": "file:///test.pl",
            "type": "String",
            "custom": "preserved"
        }
    });

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "inlayHint/resolve".into(),
        params: Some(hint.clone()),
    };

    let res = srv.handle_request(req).ok_or("Failed to handle inlayHint/resolve request")?;
    let result = res.result.ok_or("No result in inlayHint/resolve response")?;

    // Data field should be preserved
    assert_eq!(result["data"], hint["data"]);
    assert_eq!(result["data"]["custom"], "preserved");

    Ok(())
}

/// Test that resolve returns same hint if already has tooltip
#[test]
fn lsp_inlay_hint_resolve_no_op_when_complete() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({"capabilities":{}})),
    };
    srv.handle_request(init);

    let initialized = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    srv.handle_request(initialized);

    // Hint already has tooltip
    let hint = json!({
        "position": {"line": 0, "character": 10},
        "label": "param:",
        "kind": 2,
        "paddingLeft": false,
        "paddingRight": true,
        "tooltip": "Already has tooltip",
        "data": {"uri": "file:///test.pl"}
    });

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "inlayHint/resolve".into(),
        params: Some(hint.clone()),
    };

    let res = srv.handle_request(req).ok_or("Failed to handle inlayHint/resolve request")?;
    let result = res.result.ok_or("No result in inlayHint/resolve response")?;

    // Should return same hint
    assert_eq!(result["tooltip"], "Already has tooltip");
    assert_eq!(result["label"], "param:");

    Ok(())
}

/// Test that inlayHint/resolve adds labelDetails.location for click-to-definition
///
/// When a client advertises "label.location" in resolveSupport and the item was
/// issued by this server's own `textDocument/inlayHint` response, the resolved
/// hint must include labelDetails.location with uri and range fields.
///
/// This test used to hand-write `data` and never call the parent provider, which
/// is precisely the fabricated-item path #14672 closes. It now performs the real
/// list→resolve round trip; the refusal of a fabricated item is asserted in
/// `lsp_inlay_hint_resolve_identity_tests.rs`.
#[test]
fn lsp_inlay_hint_resolve_adds_label_location() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();

    // Initialize with both tooltip and label.location in resolveSupport
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "capabilities": {
                "textDocument": {
                    "inlayHint": {
                        "resolveSupport": {
                            "properties": ["tooltip", "label.location"]
                        }
                    }
                }
            }
        })),
    };
    srv.handle_request(init);

    let initialized = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    srv.handle_request(initialized);

    // Open a document with a named subroutine definition and a call site, so
    // the provider emits a parameter hint for it.
    let doc_uri = "file:///test_label_location.pl";
    let text = "sub my_func($first, $second) { return $first; }\nmy_func(1, 2);\n";
    let open = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": doc_uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })),
    };
    srv.handle_request(open);

    // Ask the parent provider first and keep the exact item it returned.
    let hints_req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(10_i64)),
        method: "textDocument/inlayHint".into(),
        params: Some(json!({
            "textDocument": {"uri": doc_uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 999, "character": 0}
            }
        })),
    };
    let hints = srv
        .handle_request(hints_req)
        .and_then(|r| r.result)
        .and_then(|r| r.as_array().cloned())
        .ok_or("textDocument/inlayHint returned no hints")?;

    let hint = hints
        .into_iter()
        .find(|h| {
            h.pointer("/data/functionName").and_then(|v| v.as_str()) == Some("my_func")
                && h.pointer("/data/resolveEnvelope").is_some()
        })
        .ok_or("expected a resolvable parameter hint for my_func")?;

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "inlayHint/resolve".into(),
        params: Some(hint),
    };

    let res = srv.handle_request(req).ok_or("Failed to handle inlayHint/resolve request")?;
    let result = res.result.ok_or("No result in inlayHint/resolve response")?;

    // Must have labelDetails with a location field
    let label_details = result.get("labelDetails").ok_or("labelDetails field missing")?;
    let location = label_details.get("location").ok_or("labelDetails.location field missing")?;

    // location must have uri and range
    assert!(location.get("uri").is_some(), "labelDetails.location must have uri, got: {location}");
    assert!(
        location.get("range").is_some(),
        "labelDetails.location must have range, got: {location}"
    );

    // Tooltip should still be present (no regression)
    assert!(result.get("tooltip").is_some(), "tooltip should still be populated");

    Ok(())
}

/// Test that resolve handles missing params gracefully
#[test]
fn lsp_inlay_hint_resolve_handles_invalid_params() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({"capabilities":{}})),
    };
    srv.handle_request(init);

    let initialized = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    srv.handle_request(initialized);

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "inlayHint/resolve".into(),
        params: None,
    };

    let res = srv.handle_request(req).ok_or("Failed to handle inlayHint/resolve request")?;

    // Should return error for invalid params
    assert!(res.error.is_some());
    let error = res.error.ok_or("Expected error for invalid params")?;
    assert_eq!(error.code, -32602); // InvalidParams

    Ok(())
}
