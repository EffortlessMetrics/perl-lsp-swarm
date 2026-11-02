//! Shipped-process proof for in-pattern regex capture navigation (#7030).
#![cfg(feature = "expose_lsp_test_api")]

use perl_lsp::LspServer;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fresh_server() -> LspServer {
    let server = LspServer::new();
    let _ = server.test_handle_initialize_dispatch(Some(json!({
        "capabilities": {},
        "rootUri": null,
        "workspaceFolders": null
    })));
    server
}

#[test]
fn definition_on_named_backreference_jumps_to_capture_declaration() -> TestResult {
    let source = "my $re = qr/(?<name>foo)\\k<name>/;\n";
    let uri = "file:///regex7030.pl";
    let server = fresh_server();
    server.test_apply_did_open(uri, source, 1)?;

    let character = u32::try_from(source.find("\\k").expect("backreference"))?;
    let result = server
        .test_handle_definition(Some(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": character}
        })))?
        .unwrap_or(Value::Null);

    assert!(!result.is_null(), "expected a definition for \\k<name>, got {result:?}");
    Ok(())
}

#[test]
fn references_on_capture_declaration_include_use_site() -> TestResult {
    let source = "my $re = qr/(?<x>a)\\k<x>/;\n";
    let uri = "file:///regex7030refs.pl";
    let server = fresh_server();
    server.test_apply_did_open(uri, source, 1)?;

    let character = u32::try_from(source.find("<x>").expect("capture name") + 1)?;
    let result = server
        .test_handle_references(Some(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": character},
            "context": {"includeDeclaration": true}
        })))?
        .unwrap_or(Value::Null);

    let locations =
        result.as_array().ok_or_else(|| format!("expected reference array, got {result:?}"))?;
    assert!(locations.len() >= 2, "declaration and use should both appear");
    Ok(())
}

#[test]
fn dynamic_interpolation_returns_no_guessed_navigation() -> TestResult {
    let source = "my $part = 'a'; my $re = qr/$part/;\n";
    let uri = "file:///regex7030dyn.pl";
    let server = fresh_server();
    server.test_apply_did_open(uri, source, 1)?;

    let pattern_offset = source.find("qr/").expect("regex") + 3;
    let character = u32::try_from(
        source[pattern_offset..]
            .find("$part")
            .map(|index| pattern_offset + index)
            .expect("interpolation in pattern"),
    )?;
    let definition = server
        .test_handle_definition(Some(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": character}
        })))?
        .unwrap_or(Value::Null);
    assert!(definition.is_null(), "dynamic pattern must not guess");

    let references = server
        .test_handle_references(Some(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": character},
            "context": {"includeDeclaration": true}
        })))?
        .unwrap_or(Value::Null);
    assert!(
        references.is_null() || references.as_array().is_some_and(|items| items.is_empty()),
        "dynamic pattern must not guess references"
    );
    Ok(())
}
