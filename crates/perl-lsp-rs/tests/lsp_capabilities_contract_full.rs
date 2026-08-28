#![allow(unused_imports)]

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

#[test]
#[cfg(not(feature = "lsp-ga-lock"))]
fn full_capabilities_match_contract() -> Result<(), Box<dyn std::error::Error>> {
    let srv = LspServer::new();
    let init = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1)),
        method: "initialize".into(),
        params: Some(json!({"capabilities":{}})),
    };
    let res = srv.handle_request(init).ok_or("Failed to handle initialize request")?;
    let result = res.result.ok_or("Response missing result field")?;
    let caps = &result["capabilities"];

    // Always-on capabilities
    assert_eq!(caps["positionEncoding"], json!("utf-16"));
    assert!(caps["textDocumentSync"].is_object());
    assert_eq!(caps["hoverProvider"], json!(true));
    assert_eq!(caps["definitionProvider"], json!(true));
    assert_eq!(caps["declarationProvider"], json!(true));
    assert_eq!(caps["referencesProvider"], json!(true));
    assert_eq!(caps["documentSymbolProvider"], json!(true));
    assert_eq!(caps["foldingRangeProvider"], json!(true));

    // Full set now that features are implemented & tested
    assert!(
        caps["workspaceSymbolProvider"].is_object(),
        "workspaceSymbolProvider should be object"
    );
    assert_eq!(caps["workspaceSymbolProvider"]["resolveProvider"], json!(true));
    // renameProvider can be bool or object with prepareProvider
    assert!(
        caps["renameProvider"] == json!(true)
            || caps["renameProvider"] == json!({"prepareProvider": true}),
        "renameProvider should be true or object with prepareProvider"
    );
    // codeActionProvider can be bool or object
    assert!(
        caps["codeActionProvider"] == json!(true) || caps["codeActionProvider"].is_object(),
        "codeActionProvider should be true or object"
    );

    let st = &caps["semanticTokensProvider"];
    assert!(st.is_object());
    // Per LSP 3.17, `SemanticTokensOptions.full` is `boolean | { delta?: boolean }`.
    // The server advertises the object form `{ delta: true }` because it implements
    // `textDocument/semanticTokens/full/delta` (see runtime/language/semantic_tokens.rs).
    // Accept the bare `true` or an object whose `delta` is `true` — mirroring the
    // `renameProvider` / `codeActionProvider` assertions above — without weakening to a
    // bare `is_object()` check that would pass on a delta-less or malformed object.
    assert!(
        st["full"] == json!(true) || st["full"]["delta"] == json!(true),
        "semanticTokensProvider.full should be true or {{ delta: true }}, got {}",
        st["full"]
    );

    let ih = &caps["inlayHintProvider"];
    assert!(ih.is_object());
    assert_eq!(ih["resolveProvider"], json!(true));

    let dl = &caps["documentLinkProvider"];
    assert!(dl.is_object());
    // Server now supports documentLink/resolve (v0.8.8)
    assert_eq!(dl["resolveProvider"], json!(true));

    assert_eq!(caps["selectionRangeProvider"], json!(true));
    // Withdrawn route (#11955): on-type formatting must not be advertised
    // until #9320 lands the proven cutover.
    assert!(
        caps["documentOnTypeFormattingProvider"].is_null(),
        "documentOnTypeFormattingProvider is withdrawn and must NOT be advertised"
    );

    // Call and type hierarchy should now be advertised
    assert!(!caps["callHierarchyProvider"].is_null(), "callHierarchyProvider must be advertised");
    assert!(!caps["typeHierarchyProvider"].is_null(), "typeHierarchyProvider must be advertised");

    // Pull diagnostics is now advertised (v0.8.5)
    assert!(caps["diagnosticProvider"].is_object(), "diagnosticProvider must be advertised");
    let diag = &caps["diagnosticProvider"];
    assert_eq!(diag["interFileDependencies"], json!(false));
    assert_eq!(diag["workspaceDiagnostics"], json!(true));

    // Code lens should now be advertised
    assert!(caps["codeLensProvider"].is_object(), "codeLensProvider must be advertised");
    assert_eq!(
        caps["codeLensProvider"]["resolveProvider"],
        json!(true),
        "codeLensProvider.resolveProvider must be true"
    );
    // ExecuteCommand is now implemented in v0.8.6
    assert!(
        !caps["executeCommandProvider"].is_null(),
        "executeCommandProvider must be advertised (implemented in v0.8.6)"
    );

    Ok(())
}
