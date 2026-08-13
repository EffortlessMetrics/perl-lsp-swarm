//! End-to-end capability and dispatch proof for the notebook preview boundary.

use perl_lsp::{JsonRpcId, JsonRpcRequest, LspServer};
use perl_lsp_rs_core::features::policy::FeatureProfile;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn message(id: Option<i64>, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: id.map(JsonRpcId::Integer),
        method: method.to_string(),
        params,
    }
}

fn initialize(server: &LspServer) -> Result<Value, Box<dyn std::error::Error>> {
    let response = server
        .handle_request(message(Some(1), "initialize", Some(json!({"capabilities": {}}))))
        .ok_or("initialize produced no response")?;
    if let Some(error) = response.error {
        return Err(format!("initialize failed: {error:?}").into());
    }
    response
        .result
        .and_then(|result| result.get("capabilities").cloned())
        .ok_or_else(|| "initialize response omitted capabilities".into())
}

fn notebook_open() -> Value {
    json!({
        "notebookDocument": {
            "uri": "file:///workspace/example.ipynb",
            "notebookType": "jupyter-notebook",
            "version": 1,
            "cells": [{"kind": 2, "document": "file:///workspace/example.ipynb#cell1"}]
        },
        "cellTextDocuments": [{
            "uri": "file:///workspace/example.ipynb#cell1",
            "languageId": "perl",
            "version": 1,
            "text": "sub preview_cell {}"
        }]
    })
}

#[test]
fn supported_profiles_omit_notebook_capabilities() -> TestResult {
    for profile in [FeatureProfile::Production, FeatureProfile::GaLock] {
        let capabilities = initialize(&LspServer::new_with_feature_profile(profile))?;
        assert!(
            capabilities.get("notebookDocumentSync").is_none(),
            "{} advertised notebookDocumentSync",
            profile.as_str()
        );
    }
    Ok(())
}

#[test]
fn all_advertises_only_the_intentional_selector_and_save_contract() -> TestResult {
    let capabilities = initialize(&LspServer::new_with_feature_profile(FeatureProfile::All))?;
    let actual =
        capabilities.get("notebookDocumentSync").ok_or("all omitted notebookDocumentSync")?;
    let expected = json!({
        "notebookSelector": [{
            "notebook": "jupyter-notebook",
            "cells": [{"language": "perl"}]
        }],
        "save": true
    });
    assert_eq!(actual, &expected, "unexpected notebook selector: {actual}");
    Ok(())
}

#[test]
fn disabled_notebook_notification_has_no_json_rpc_response() -> TestResult {
    for profile in [FeatureProfile::Production, FeatureProfile::GaLock] {
        let server = LspServer::new_with_feature_profile(profile);
        let _ = initialize(&server)?;
        let response =
            server.handle_request(message(None, "notebookDocument/didOpen", Some(notebook_open())));
        assert!(response.is_none(), "{} emitted a response to a notification", profile.as_str());

        let request_response = server
            .handle_request(message(Some(2), "notebookDocument/didOpen", Some(notebook_open())))
            .ok_or("request-shaped disabled route produced no response")?;
        let error = request_response.error.ok_or("disabled route unexpectedly succeeded")?;
        assert_eq!(error.code, -32601, "unexpected disabled disposition: {error:?}");
    }
    Ok(())
}

#[test]
fn all_notification_reaches_the_preview_handler_without_response() -> TestResult {
    let server = LspServer::new_with_feature_profile(FeatureProfile::All);
    let _ = initialize(&server)?;
    assert!(
        server
            .handle_request(message(None, "notebookDocument/didOpen", Some(notebook_open())))
            .is_none(),
        "preview notification produced a JSON-RPC response"
    );
    Ok(())
}
