use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

/// Test Pull Diagnostics support (LSP 3.17)
#[test]
fn test_document_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    // Initialize server
    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
    };
    let _ = server.handle_request(init_request);

    // Send initialized notification (required after successful initialize)
    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(initialized_notification);

    // Open a document with errors
    let uri = "file:///test.pl";
    let content = r#"#!/usr/bin/perl
use strict;
use warnings;

my $x = 1;
print $y;  # Undefined variable
"#;

    // Send didOpen notification
    let did_open_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": content
            }
        })),
    };
    let _ = server.handle_request(did_open_request);

    // Request diagnostics
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "textDocument/diagnostic".into(),
        params: Some(json!({
            "textDocument": { "uri": uri }
        })),
    };

    let response = server.handle_request(request).ok_or("Failed to get response from server")?;
    let result = response.result.ok_or("Response missing result field")?;

    assert_eq!(result["kind"], "full");
    assert!(result["resultId"].is_string(), "Should have a result ID");

    let items = result["items"].as_array().ok_or("Expected items to be an array")?;
    assert!(!items.is_empty(), "Should have at least one diagnostic");

    // Check first diagnostic structure
    let first = items.first().ok_or("Expected at least one diagnostic item")?;
    assert!(first["range"].is_object());
    assert!(first["severity"].is_number());
    assert_eq!(first["source"], "perl-lsp");
    assert!(first["message"].is_string());

    Ok(())
}

#[test]
fn test_document_diagnostic_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    // Initialize server
    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
    };
    let _ = server.handle_request(init_request);

    // Send initialized notification (required after successful initialize)
    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(initialized_notification);

    // Open a document
    let uri = "file:///test.pl";
    let content = r#"#!/usr/bin/perl
print "Hello, World!\n";
"#;

    let did_open_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": content
            }
        })),
    };
    let _ = server.handle_request(did_open_request);

    // First request - get full diagnostics
    let request1 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "textDocument/diagnostic".into(),
        params: Some(json!({
            "textDocument": { "uri": uri }
        })),
    };

    let response1 = server
        .handle_request(request1)
        .ok_or("Failed to get response from first diagnostic request")?;
    let result1 = response1.result.ok_or("First response missing result field")?;
    let result_id = result1["resultId"].as_str().ok_or("Expected resultId to be a string")?;

    // Second request with previous result ID - should be unchanged
    let request2 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(3_i64)),
        method: "textDocument/diagnostic".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "previousResultId": result_id
        })),
    };

    let response2 = server
        .handle_request(request2)
        .ok_or("Failed to get response from second diagnostic request")?;
    let result2 = response2.result.ok_or("Second response missing result field")?;

    assert_eq!(result2["kind"], "unchanged");
    assert_eq!(result2["resultId"], result_id);

    Ok(())
}

#[test]
fn test_workspace_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    // Initialize server
    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
    };
    let _ = server.handle_request(init_request);

    // Send initialized notification (required after successful initialize)
    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(initialized_notification);

    // Open multiple documents
    let uri1 = "file:///test1.pl";
    let content1 = r#"use strict;
my $x = 1;
print $y;  # Error
"#;

    let uri2 = "file:///test2.pl";
    let content2 = r#"#!/usr/bin/perl
print "OK\n";
"#;

    // Open first document
    let did_open1 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri1,
                "languageId": "perl",
                "version": 1,
                "text": content1
            }
        })),
    };
    let _ = server.handle_request(did_open1);

    // Open second document
    let did_open2 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri2,
                "languageId": "perl",
                "version": 1,
                "text": content2
            }
        })),
    };
    let _ = server.handle_request(did_open2);

    // Request workspace diagnostics
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "workspace/diagnostic".into(),
        params: Some(json!({})),
    };

    let response =
        server.handle_request(request).ok_or("Failed to get workspace diagnostic response")?;
    let result = response.result.ok_or("Workspace diagnostic response missing result field")?;

    let items = result["items"].as_array().ok_or("Expected items to be an array")?;
    assert_eq!(items.len(), 2, "Should have diagnostics for both documents");

    // Check structure of workspace diagnostic reports
    for item in items {
        assert!(item["uri"].is_string());
        assert!(item["version"].is_number());
        assert!(item["kind"].is_string());
        if item["kind"] == "full" {
            assert!(item["items"].is_array());
        }
    }

    Ok(())
}

#[test]
fn test_diagnostic_provider_capability() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
    };

    let response =
        server.handle_request(init_request).ok_or("Failed to get initialize response")?;
    let result = response.result.ok_or("Initialize response missing result field")?;
    let caps = &result["capabilities"];

    // Diagnostic provider should be advertised in non-lock mode
    if !cfg!(feature = "lsp-ga-lock") {
        assert!(caps["diagnosticProvider"].is_object(), "diagnosticProvider should be advertised");
        let diag_provider = &caps["diagnosticProvider"];
        assert_eq!(diag_provider["interFileDependencies"], json!(false));
        assert_eq!(diag_provider["workspaceDiagnostics"], json!(true));
    }

    Ok(())
}

#[test]
fn test_workspace_diagnostic_with_previous_ids() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();

    // Initialize server
    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "processId": 1,
            "capabilities": {}
        })),
    };
    let _ = server.handle_request(init_request);

    // Send initialized notification (required after successful initialize)
    let initialized_notification = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(initialized_notification);

    // Open a document
    let uri = "file:///test.pl";
    let content = r#"print "Hello\n";"#;

    let did_open = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": content
            }
        })),
    };
    let _ = server.handle_request(did_open);

    // First workspace diagnostic request
    let request1 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "workspace/diagnostic".into(),
        params: Some(json!({})),
    };

    let response1 = server
        .handle_request(request1)
        .ok_or("Failed to get first workspace diagnostic response")?;
    let result1 =
        response1.result.ok_or("First workspace diagnostic response missing result field")?;
    let items1 =
        result1["items"].as_array().ok_or("Expected first response items to be an array")?;

    // Get result IDs from first request
    let mut previous_ids = Vec::new();
    for item in items1 {
        if let Some(result_id) = item["resultId"].as_str() {
            previous_ids.push(json!({
                "uri": item["uri"],
                "value": result_id
            }));
        }
    }

    // Second request with previous IDs
    let request2 = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(3_i64)),
        method: "workspace/diagnostic".into(),
        params: Some(json!({
            "previousResultIds": previous_ids
        })),
    };

    let response2 = server
        .handle_request(request2)
        .ok_or("Failed to get second workspace diagnostic response")?;
    let result2 =
        response2.result.ok_or("Second workspace diagnostic response missing result field")?;
    let items2 =
        result2["items"].as_array().ok_or("Expected second response items to be an array")?;

    // At least one should be unchanged
    let has_unchanged = items2.iter().any(|item| item["kind"] == "unchanged");
    assert!(has_unchanged, "Should have at least one unchanged result");

    Ok(())
}

#[test]
fn test_pull_diagnostics_suppressed_until_full_resync() -> Result<(), Box<dyn std::error::Error>> {
    // #17752: after a ranged didChange is rejected under the Full sync
    // envelope, pull diagnostics must not reparse retained last-good text.
    // The report is a full empty report with no reusable resultId until an
    // explicit full resync restores currentness.
    let server = LspServer::new();
    let _ = server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".into(),
        params: Some(json!({"processId": 1, "capabilities": {}})),
    });
    let _ = server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    });

    let uri = "file:///desync.pl";
    let content = "use strict;\nuse warnings;\nmy $x = 1;\nprint $y;\n";
    let _ = server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": content}
        })),
    });

    let pull = |id: i64| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let response = server
            .handle_request(JsonRpcRequest {
                _jsonrpc: "2.0".into(),
                id: Some(perl_lsp::protocol::JsonRpcId::Integer(id)),
                method: "textDocument/diagnostic".into(),
                params: Some(json!({"textDocument": {"uri": uri}})),
            })
            .ok_or("diagnostic request failed")?;
        Ok(response.result.ok_or("response missing result")?)
    };

    let before = pull(2).map_err(|e| format!("initial pull failed: {e}"))?;
    assert_eq!(before["kind"], "full");
    assert!(before["resultId"].is_string(), "current document must mint a resultId");
    assert!(
        !before["items"].as_array().ok_or("items array")?.is_empty(),
        "current document must report its diagnostics"
    );

    // Ranged change: rejected under the advertised Full envelope.
    let _ = server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didChange".into(),
        params: Some(json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{
                "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 8}},
                "text": "z"
            }]
        })),
    });

    let desynced = pull(3).map_err(|e| format!("pull during desync failed: {e}"))?;
    assert_eq!(desynced["kind"], "full");
    assert!(
        desynced["items"].as_array().ok_or("items array")?.is_empty(),
        "desynchronized pull must not reparse retained text, got: {desynced}"
    );
    assert!(desynced["resultId"].is_null(), "desynchronized pull must not mint a resultId");

    // Explicit full resync restores current diagnostics.
    let _ = server.handle_request(JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didChange".into(),
        params: Some(json!({
            "textDocument": {"uri": uri, "version": 3},
            "contentChanges": [{"text": content}]
        })),
    });

    let recovered = pull(4).map_err(|e| format!("pull after recovery failed: {e}"))?;
    assert!(recovered["resultId"].is_string(), "resync must restore resultId minting");
    assert!(
        !recovered["items"].as_array().ok_or("items array")?.is_empty(),
        "resync must restore real diagnostics, not blanket suppression"
    );
    Ok(())
}
