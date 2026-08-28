//! Real tests for Document Lifecycle Events
//! Tests didSave, willSave, and willSaveWaitUntil notifications

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

mod support;

/// Helper to set up an initialized LSP server with a document
fn setup_server_with_document() -> (LspServer, String) {
    let server = LspServer::new();

    // 1. Send initialize request with JsonRpcRequest
    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(json!({
            "processId": null,
            "capabilities": {},
            "rootUri": "file:///test"
        })),
    };
    let init_response = server.handle_request(init_request);
    assert!(init_response.is_some(), "Initialize should return a response");

    // 2. CRITICAL: Send initialized notification
    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None, // Notifications have no id
        method: "initialized".to_string(),
        params: Some(json!({})),
    };
    server.handle_request(initialized_notification);

    // 3. Open document
    let uri = "file:///test.pl".to_string();
    let open_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didOpen".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "my $foo = 42;\nuse strict;"
            }
        })),
    };
    server.handle_request(open_notification);

    (server, uri)
}

#[test]
fn test_did_save_notification() {
    let (server, uri) = setup_server_with_document();

    // Send didSave notification
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None, // Notification has no id
        method: "textDocument/didSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "version": 2
            }
        })),
    };
    let response = server.handle_request(request);

    // Notifications should not return a response
    assert!(response.is_none(), "Notifications should not return a response");
}

#[test]
fn test_did_save_with_text() {
    let (server, uri) = setup_server_with_document();

    // Send didSave notification with text
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "version": 3
            },
            "text": "my $bar = 123;\nuse warnings;"
        })),
    };
    let response = server.handle_request(request);

    assert!(response.is_none(), "Notifications should not return a response");
}

#[test]
fn test_will_save_notification() {
    let (server, uri) = setup_server_with_document();

    // Send willSave notification - Manual save (reason = 1)
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/willSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri.clone()
            },
            "reason": 1 // Manual = 1
        })),
    };
    let response = server.handle_request(request);
    assert!(response.is_none(), "willSave notification should not return a response");

    // Test AfterDelay reason
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/willSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri.clone()
            },
            "reason": 2 // AfterDelay = 2
        })),
    };
    let response = server.handle_request(request);
    assert!(response.is_none());

    // Test FocusOut reason
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/willSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri
            },
            "reason": 3 // FocusOut = 3
        })),
    };
    let response = server.handle_request(request);
    assert!(response.is_none());
}

/// Withdrawal control (#11955): formatter-owned `willSaveWaitUntil` is
/// withdrawn until #8092 selects one proven save owner. Direct requests must
/// receive the truthful method-not-advertised refusal — never edits, and never
/// a successful empty standing in for refusal.
#[test]
fn test_will_save_wait_until_is_withdrawn() -> Result<(), Box<dyn std::error::Error>> {
    let (server, uri) = setup_server_with_document();

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(10_i64)),
        method: "textDocument/willSaveWaitUntil".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri
            },
            "reason": 1 // Manual
        })),
    };
    let response = server.handle_request(request);

    let response = response.ok_or("willSaveWaitUntil must return a response envelope")?;
    let error = response.error.ok_or("withdrawn willSaveWaitUntil must return an error")?;
    assert_eq!(error.code, -32601, "refusal must be MethodNotFound (-32601)");

    Ok(())
}

/// Withdrawal control (#11955): even with messy unformatted content that the
/// old save-owner path would rewrite, `willSaveWaitUntil` refuses instead of
/// producing a second save owner's edits.
#[test]
fn test_will_save_wait_until_refuses_despite_unformatted_content()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, uri) = setup_server_with_document();

    // Update document with poorly formatted code
    let change_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didChange".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri.clone(),
                "version": 2
            },
            "contentChanges": [{
                "text": "sub test{my$foo=42;return$foo;}\n"
            }]
        })),
    };
    server.handle_request(change_notification);

    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(11_i64)),
        method: "textDocument/willSaveWaitUntil".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri
            },
            "reason": 1 // Manual
        })),
    };
    let response = server.handle_request(request);

    let response =
        response.ok_or("willSaveWaitUntil must return a response envelope even when messy")?;
    let error = response.error.ok_or("withdrawn willSaveWaitUntil must return an error")?;
    assert_eq!(error.code, -32601, "refusal must be MethodNotFound (-32601)");
    assert!(
        response.result.is_none(),
        "refusal must not carry an edit payload for unformatted content"
    );

    Ok(())
}

#[test]
fn test_did_close_after_save() {
    let (server, uri) = setup_server_with_document();

    // Send didSave notification
    let save_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri.clone(),
                "version": 2
            }
        })),
    };
    server.handle_request(save_notification);

    // Send didClose notification
    let close_notification = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didClose".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri
            }
        })),
    };
    let response = server.handle_request(close_notification);

    assert!(response.is_none(), "didClose notification should not return a response");
}

#[test]
fn test_save_events_sequence() {
    let (server, uri) = setup_server_with_document();

    // Simulate typical save sequence: willSave -> willSaveWaitUntil -> didSave

    // 1. willSave notification
    let will_save = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/willSave".to_string(),
        params: Some(json!({
            "textDocument": { "uri": uri.clone() },
            "reason": 1
        })),
    };
    server.handle_request(will_save);

    // 2. willSaveWaitUntil request
    let will_save_wait_until = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(20_i64)),
        method: "textDocument/willSaveWaitUntil".to_string(),
        params: Some(json!({
            "textDocument": { "uri": uri.clone() },
            "reason": 1
        })),
    };
    let edits_response = server.handle_request(will_save_wait_until);
    let error = edits_response.as_ref().and_then(|response| response.error.as_ref());
    assert!(edits_response.is_some(), "willSaveWaitUntil must return a response envelope");
    assert!(error.is_some(), "withdrawn willSaveWaitUntil must return an error");
    assert_eq!(error.map(|err| err.code), Some(-32601));
    assert!(
        edits_response.as_ref().is_some_and(|response| response.result.is_none()),
        "withdrawn willSaveWaitUntil must not return edits"
    );

    // 3. didSave notification
    let did_save = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "version": 3
            }
        })),
    };
    server.handle_request(did_save);

    // If we reach here without panics, the sequence completed successfully
}

#[test]
fn test_save_with_invalid_uri() {
    let (server, _uri) = setup_server_with_document();

    // Try to save a document that was never opened
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "textDocument/didSave".to_string(),
        params: Some(json!({
            "textDocument": {
                "uri": "file:///nonexistent.pl",
                "version": 1
            }
        })),
    };

    // Should handle gracefully without crashing
    let _response = server.handle_request(request);
    // Just checking it doesn't panic - the server may or may not return a response
}
