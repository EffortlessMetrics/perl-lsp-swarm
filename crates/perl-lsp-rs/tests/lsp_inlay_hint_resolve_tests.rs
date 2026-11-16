mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn init_with_capabilities(harness: &mut LspHarness, capabilities: Value) -> Result<(), String> {
    harness.initialize(Some(capabilities))?;
    Ok(())
}

fn resolve_hint(
    harness: &mut LspHarness,
    hint: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    harness.request("inlayHint/resolve", hint).map_err(|e| e.into())
}

/// Test that inlayHint/resolve adds tooltip when requested
#[test]
fn lsp_inlay_hint_resolve_adds_tooltip() -> TestResult {
    let mut harness = LspHarness::new();
    init_with_capabilities(
        &mut harness,
        json!({
            "textDocument": {
                "inlayHint": {
                    "resolveSupport": {
                        "properties": ["tooltip"]
                    }
                }
            }
        }),
    )?;

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

    let result = resolve_hint(&mut harness, hint)?;

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
fn lsp_inlay_hint_resolve_preserves_data() -> TestResult {
    let mut harness = LspHarness::new();
    init_with_capabilities(&mut harness, json!({"textDocument": {}}))?;

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

    let result = resolve_hint(&mut harness, hint.clone())?;

    // Data field should be preserved
    assert_eq!(result["data"], hint["data"]);
    assert_eq!(result["data"]["custom"], "preserved");

    Ok(())
}

/// Test that resolve returns same hint if already has tooltip
#[test]
fn lsp_inlay_hint_resolve_no_op_when_complete() -> TestResult {
    let mut harness = LspHarness::new();
    init_with_capabilities(&mut harness, json!({"textDocument": {}}))?;

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

    let result = resolve_hint(&mut harness, hint)?;

    // Should return same hint
    assert_eq!(result["tooltip"], "Already has tooltip");
    assert_eq!(result["label"], "param:");

    Ok(())
}

/// Test that inlayHint/resolve adds an InlayHintLabelPart.location for click-to-definition
///
/// When a client advertises "label.location" in resolveSupport and the item was
/// issued by this server's own `textDocument/inlayHint` response, the resolved
/// hint must expose the location through its label part with uri and range fields.
///
/// This test used to hand-write `data` and never call the parent provider, which
/// is precisely the fabricated-item path #14672 closes. It now performs the real
/// list→resolve round trip; the refusal of a fabricated item is asserted in
/// `lsp_inlay_hint_resolve_identity_tests.rs`.
#[test]
fn lsp_inlay_hint_resolve_adds_label_location() -> TestResult {
    let mut harness = LspHarness::new();

    // Initialize with both tooltip and label.location in resolveSupport
    init_with_capabilities(
        &mut harness,
        json!({
            "textDocument": {
                "inlayHint": {
                    "resolveSupport": {
                        "properties": ["tooltip", "label.location"]
                    }
                }
            }
        }),
    )?;

    // Open a document with a named subroutine definition and a call site, so
    // the provider emits a parameter hint for it.
    let doc_uri = "file:///test_label_location.pl";
    let text = "sub my_func($first, $second) { return $first; }\nmy_func(1, 2);\n";
    harness.open(doc_uri, text)?;
    harness.barrier();

    // Ask the parent provider first and keep the exact item it returned.
    let hints = harness
        .request_with_timeout(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": doc_uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 999, "character": 0}
                }
            }),
            Duration::from_secs(5),
        )?
        .as_array()
        .cloned()
        .ok_or("textDocument/inlayHint returned no hints")?;

    let hint = hints
        .into_iter()
        .find(|h| {
            h.pointer("/data/functionName").and_then(|v| v.as_str()) == Some("my_func")
                && h.pointer("/data/resolveEnvelope").is_some()
        })
        .ok_or("expected a resolvable parameter hint for my_func")?;
    let listed_label = perl_lsp_rs_core::providers::inlay_hints::inlay_hint_label_str(&hint)
        .ok_or("listed hint has no label text")?
        .to_string();

    let result = resolve_hint(&mut harness, hint)?;

    let resolved: lsp_types::InlayHint = serde_json::from_value(result.clone())?;
    let label_parts = match resolved.label {
        lsp_types::InlayHintLabel::LabelParts(parts) => parts,
        lsp_types::InlayHintLabel::String(label) => {
            return Err(format!("resolved label remained a string: {label}").into());
        }
    };
    let location = label_parts
        .iter()
        .find_map(|part| part.location.as_ref())
        .ok_or("resolved label part location missing")?;

    // location must have uri and range
    assert_eq!(
        label_parts.iter().map(|part| part.value.as_str()).collect::<String>(),
        listed_label,
        "resolve must preserve the displayed label text"
    );
    assert_eq!(location.uri.as_str(), doc_uri);
    assert!(location.range.start.line <= location.range.end.line);
    assert!(result.get("labelDetails").is_none(), "legacy labelDetails must be absent");

    // Tooltip should still be present (no regression)
    assert!(result.get("tooltip").is_some(), "tooltip should still be populated");

    Ok(())
}

/// Test that resolve handles missing params gracefully
#[test]
fn lsp_inlay_hint_resolve_handles_invalid_params() -> TestResult {
    let mut harness = LspHarness::new();
    init_with_capabilities(&mut harness, json!({"textDocument": {}}))?;

    let response = harness.request_raw(json!({
        "jsonrpc": "2.0",
        "method": "inlayHint/resolve"
    }));

    // Should return error for invalid params
    assert!(response.get("error").is_some());
    let error = response.get("error").ok_or("Expected error for invalid params")?;
    assert_eq!(error.get("code").and_then(Value::as_i64), Some(-32602)); // InvalidParams

    Ok(())
}
