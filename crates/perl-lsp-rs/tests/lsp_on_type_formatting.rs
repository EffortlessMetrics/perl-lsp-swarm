//! Withdrawal controls for `textDocument/onTypeFormatting` (#11955).
//!
//! The route is fail-closed until #9320 lands the proven cutover: every
//! direct request must receive the repository's truthful method-not-advertised
//! refusal — never edits and never a successful empty — regardless of trigger
//! character, options, or formatting configuration.

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::json;

fn initialized_server() -> LspServer {
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
        params: None,
    };
    srv.handle_request(initialized);
    srv
}

fn open(srv: &LspServer, uri: &str, text: &str) {
    let open = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })),
    };
    srv.handle_request(open);
}

fn on_type_response(
    srv: &LspServer,
    id: i64,
    uri: &str,
    line: i64,
    character: i64,
    ch: &str,
) -> Result<perl_lsp::JsonRpcResponse, Box<dyn std::error::Error>> {
    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(id)),
        method: "textDocument/onTypeFormatting".into(),
        params: Some(json!({
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
            "ch": ch,
            "options": {"tabSize": 4, "insertSpaces": true}
        })),
    };
    let response =
        srv.handle_request(req).ok_or("onTypeFormatting request returned no response")?;
    Ok(response)
}

fn assert_withdrawn_refusal(
    response: perl_lsp::JsonRpcResponse,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let error = response
        .error
        .as_ref()
        .ok_or_else(|| format!("{label} must be refused, got {response:?}"))?;
    assert_eq!(error.code, -32601, "{label} refusal must be MethodNotFound (-32601)");
    assert!(response.result.is_none(), "{label} refusal must not carry a successful edit payload");
    Ok(())
}

#[test]
fn on_type_braces_indent_is_withdrawn() -> Result<(), Box<dyn std::error::Error>> {
    let srv = initialized_server();
    open(&srv, "file:///fmt.pl", "sub f {\n\n}\n");
    let response = on_type_response(&srv, 2, "file:///fmt.pl", 0, 7, "{")?;
    assert_withdrawn_refusal(response, "on-type '{' request")
}

#[test]
fn on_type_closing_brace_dedent_is_withdrawn() -> Result<(), Box<dyn std::error::Error>> {
    let srv = initialized_server();
    open(&srv, "file:///dedent.pl", "sub f {\n    my $x = 1;\n    }");
    let response = on_type_response(&srv, 2, "file:///dedent.pl", 2, 5, "}")?;
    assert_withdrawn_refusal(response, "on-type '}' request")
}

#[test]
fn on_type_newline_after_open_brace_is_withdrawn() -> Result<(), Box<dyn std::error::Error>> {
    let srv = initialized_server();
    open(&srv, "file:///newline_indent.pl", "sub foo {\n\n}\n");
    let response = on_type_response(&srv, 2, "file:///newline_indent.pl", 1, 0, "\n")?;
    assert_withdrawn_refusal(response, "on-type newline request")
}

#[test]
fn on_type_tab_size_4_cannot_produce_indent_edits() -> Result<(), Box<dyn std::error::Error>> {
    // Even a fully specified tabSize:4 indent scenario must refuse rather than
    // emit edits while the route is withdrawn (#11955, restoration #9320).
    let srv = initialized_server();
    open(&srv, "file:///tab_size_4.pl", "if ($x) {\n\n}\n");
    let response = on_type_response(&srv, 2, "file:///tab_size_4.pl", 1, 0, "\n")?;
    assert_withdrawn_refusal(response, "on-type tabSize:4 request")
}

#[test]
fn manual_formatting_stays_live_beside_withdrawn_on_type_route()
-> Result<(), Box<dyn std::error::Error>> {
    // The proven manual whole-document route remains available for the same
    // server session after withdrawn on-type refusals.
    let srv = initialized_server();
    open(&srv, "file:///manual_after_refusal.pl", "sub messy{my$x=1;return$x;}\n");
    let refused = on_type_response(&srv, 2, "file:///manual_after_refusal.pl", 0, 9, "\n")?;
    assert_withdrawn_refusal(refused, "on-type pre-manual request")?;

    let manual = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(3_i64)),
        method: "textDocument/formatting".into(),
        params: Some(json!({
            "textDocument": {"uri": "file:///manual_after_refusal.pl"},
            "options": {"tabSize": 4, "insertSpaces": true}
        })),
    };
    let res = srv.handle_request(manual).ok_or("manual formatting request failed")?;
    let edits = res.result.ok_or("manual formatting response missing result")?;
    let edits_array = edits.as_array().ok_or("manual formatting must return an edit array")?;
    assert!(
        !edits_array.is_empty(),
        "manual whole-document formatting must remain available beside withdrawn routes"
    );
    Ok(())
}
