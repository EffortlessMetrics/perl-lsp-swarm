use super::*;
use crate::protocol::{JsonRpcId, JsonRpcRequest};

fn advertise(server: &LspServer, surface: Surface) {
    server.advertised_feature_ids.lock().push(surface.feature_id());
}

fn receipt(server: &LspServer) -> Result<Value, Box<dyn std::error::Error>> {
    server
        .provider_decision_traces
        .lock()
        .get(PROVIDER)
        .cloned()
        .ok_or_else(|| "missing formatting receipt".into())
}

fn request(id: i64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(JsonRpcId::Integer(id)),
        method: method.to_string(),
        params: Some(params),
    }
}

fn request_without_params(id: i64, method: &str) -> JsonRpcRequest {
    JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(JsonRpcId::Integer(id)),
        method: method.to_string(),
        params: None,
    }
}

fn initialize(server: &LspServer) -> Result<(), Box<dyn std::error::Error>> {
    let response = server
        .handle_request(request(1, "initialize", json!({})))
        .ok_or("initialize returned no response")?;
    if let Some(error) = response.error {
        return Err(format!("initialize failed: {error:?}").into());
    }
    Ok(())
}

#[test]
fn disabled_is_a_typed_refusal() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    server.config.lock().perltidy_enabled = false;
    let uri = "file:///disabled-formatting.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;

    let result = server.handle_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;

    assert_eq!(result, Some(json!([])));
    let receipt = receipt(&server)?;
    assert_eq!(receipt["decision"], "blocked");
    assert_eq!(receipt["reason"], "formatter_disabled");
    assert_eq!(receipt["actual_engine"], "disabled");
    Ok(())
}

#[test]
fn cancellation_records_a_typed_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let uri = "file:///cancelled-formatting.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;
    let params = json!({
        "textDocument": { "uri": uri, "version": 1 },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let snapshot = server.admit(Surface::Document, &params)?;
    let request_id = JsonRpcId::Integer(77);
    let token =
        PerlLspCancellationToken::new(request_id.clone(), Surface::Document.method().to_string());
    GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone())?;
    let _ = GLOBAL_CANCELLATION_REGISTRY.cancel_request(&request_id)?;

    let error = server
        .ensure_not_cancelled(Surface::Document, Some(&token), Some(&snapshot), None)
        .err()
        .ok_or("expected cancellation error")?;
    GLOBAL_CANCELLATION_REGISTRY.remove_request(&request_id);

    assert_eq!(error.code, REQUEST_CANCELLED);
    assert_eq!(receipt(&server)?["reason"], "request_cancelled");
    Ok(())
}

#[test]
fn stale_snapshot_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let uri = "file:///stale-formatting.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;
    let params = json!({
        "textDocument": { "uri": uri, "version": 1 },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let snapshot = server.admit(Surface::Document, &params)?;
    {
        let mut documents = server.documents.lock();
        let document = server.get_document_mut(&mut documents, uri).ok_or("missing document")?;
        document.update_content("my $x = 2;\n", 2);
    }

    let error = server.ensure_current(&snapshot).err().ok_or("expected stale error")?;
    assert_eq!(error.code, CONTENT_MODIFIED);
    assert_eq!(receipt(&server)?["reason"], "stale_source");
    Ok(())
}

#[test]
fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    let document_uri = "file:///live-formatting.pl";
    let on_type_uri = "file:///live-on-type.pl";
    server.test_apply_did_open(document_uri, "my$x=1;\n", 1)?;
    server.test_apply_did_open(on_type_uri, "if ($ok) {\n\n", 1)?;

    let cases = [
        (
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": document_uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ),
        (
            "textDocument/rangeFormatting",
            json!({
                "textDocument": { "uri": document_uri, "version": 1 },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ),
        (
            "textDocument/rangesFormatting",
            json!({
                "textDocument": { "uri": document_uri, "version": 1 },
                "ranges": [{
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                }],
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ),
        (
            "textDocument/onTypeFormatting",
            json!({
                "textDocument": { "uri": on_type_uri, "version": 1 },
                "position": { "line": 1, "character": 0 },
                "ch": "\n",
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ),
    ];

    for (offset, (method, params)) in cases.into_iter().enumerate() {
        let response = server
            .handle_request(request(100 + offset as i64, method, params))
            .ok_or_else(|| format!("{method} returned no response"))?;
        if let Some(error) = response.error {
            return Err(format!("{method} failed: {error:?}").into());
        }
        assert!(response.result.is_some(), "{method} must return an edit array");
        let trace = receipt(&server)?;
        assert_eq!(trace["provider"], PROVIDER);
        assert_eq!(trace["provider_action"], method);
        assert!(trace["source_generation"].is_u64());
        assert!(trace["config_fingerprint"].is_string());
    }

    Ok(())
}

#[test]
fn disabled_formatting_surfaces_return_method_not_advertised_even_without_params()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    {
        let mut features = server.advertised_features.lock();
        features.formatting = false;
        features.range_formatting = false;
    }
    server.advertised_feature_ids.lock().retain(|id| {
        *id != perl_lsp_rs_core::features::ids::LSP_FORMATTING
            && *id != perl_lsp_rs_core::features::ids::LSP_RANGE_FORMATTING
            && *id != perl_lsp_rs_core::features::ids::LSP_RANGES_FORMATTING
            && *id != perl_lsp_rs_core::features::ids::LSP_ON_TYPE_FORMATTING
    });

    for (offset, method) in [
        "textDocument/formatting",
        "textDocument/rangeFormatting",
        "textDocument/rangesFormatting",
        "textDocument/onTypeFormatting",
    ]
    .into_iter()
    .enumerate()
    {
        let response = server
            .handle_request(request_without_params(300 + offset as i64, method))
            .ok_or_else(|| format!("{method} returned no response"))?;
        let code = response.error.as_ref().map(|error| error.code).unwrap_or(0);
        assert_eq!(
            code, -32601,
            "{method} with missing params must still return method-not-advertised (-32601), got {code}"
        );
    }
    Ok(())
}

#[test]
fn live_stale_request_returns_content_modified_not_successful_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    let uri = "file:///live-stale-formatting.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 2)?;

    let response = server
        .handle_request(request(
            300,
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ))
        .ok_or("formatting returned no response")?;

    assert!(response.result.is_none(), "stale formatting must not return a result");
    let error = response.error.ok_or("expected ContentModified")?;
    assert_eq!(error.code, CONTENT_MODIFIED);
    assert_eq!(receipt(&server)?["reason"], "stale_source");
    Ok(())
}
