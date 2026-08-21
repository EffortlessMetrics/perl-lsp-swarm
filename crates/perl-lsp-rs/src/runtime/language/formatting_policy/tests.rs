use super::*;
use crate::protocol::{JsonRpcId, JsonRpcRequest};
use perl_subprocess_runtime::mock::MockSubprocessRuntime;
use std::sync::Arc;

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
    assert_eq!(receipt["requested_mode"], "native");
    assert_eq!(receipt["effective_mode"], "off");
    Ok(())
}

#[test]
fn generic_external_formatter_alias_is_contained_before_formatting()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let runtime = Arc::new(MockSubprocessRuntime::new());
    server.test_install_formatter_runtime(runtime.clone());
    server.test_handle_did_change_configuration(Some(json!({
        "settings": { "perl": { "formatting": { "engine": "external-legacy" } } }
    })));
    assert_eq!(server.config.lock().formatting_engine, FormatterMode::Native);

    let uri = "file:///generic-external-alias.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;
    let result = server.handle_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;
    assert!(result.is_some(), "native formatting should return a response");
    assert!(
        runtime.invocations().is_empty(),
        "native formatting must not invoke the external runtime"
    );
    let receipt = receipt(&server)?;
    assert_eq!(receipt["actual_engine"], "native");
    assert_eq!(receipt["effective_mode"], "native");
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
fn native_refusal_is_not_no_change() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let uri = "file:///native-refusal.pl";
    server.test_apply_did_open(uri, "if (\n", 1)?;

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
    assert_ne!(receipt["reason"], "already_formatted");
    Ok(())
}

#[test]
fn disabled_on_type_does_not_run() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::OnType);
    server.config.lock().formatting_engine = FormatterMode::Off;
    let uri = "file:///disabled-on-type.pl";
    server.test_apply_did_open(uri, "if ($ok) {\n\n", 1)?;

    let result = server.handle_on_type_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;

    assert_eq!(result, Some(json!([])));
    let receipt = receipt(&server)?;
    assert_eq!(receipt["reason"], "formatter_disabled");
    assert_eq!(receipt["actual_engine"], "disabled");
    Ok(())
}

#[test]
fn tab_indentation_on_type_is_a_typed_refusal() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::OnType);
    server.config.lock().perltidy_tabs = Some(true);
    let uri = "file:///tabs-on-type.pl";
    server.test_apply_did_open(uri, "if ($ok) {\n\n", 1)?;

    let result = server.handle_on_type_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;

    assert_eq!(result, Some(json!([])));
    let receipt = receipt(&server)?;
    assert_eq!(receipt["decision"], "blocked");
    assert_eq!(receipt["reason"], "unsupported_syntax");
    assert_eq!(receipt["result_count"], 0);
    Ok(())
}

#[test]
fn heredoc_on_type_is_a_typed_suppression() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::OnType);
    let uri = "file:///heredoc-on-type.pl";
    server.test_apply_did_open(uri, "my $x = <<END;\nbody\nEND\n", 1)?;

    let result = server.handle_on_type_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "position": { "line": 1, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;

    assert_eq!(result, Some(json!([])));
    let receipt = receipt(&server)?;
    assert_eq!(receipt["decision"], "blocked");
    assert_eq!(receipt["reason"], "inside_heredoc");
    assert_eq!(receipt["result_count"], 0);
    Ok(())
}

#[test]
fn malformed_options_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let uri = "file:///malformed-options.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;

    let error = server
        .handle_formatting_policy(
            Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": "not-an-object",
            })),
            None,
        )
        .err()
        .ok_or("expected invalid params")?;

    assert_eq!(error.code, -32602);
    Ok(())
}

#[test]
fn missing_on_type_trigger_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::OnType);
    let uri = "file:///missing-trigger.pl";
    server.test_apply_did_open(uri, "if ($ok) {\n\n", 1)?;

    let error = server
        .handle_on_type_formatting_policy(
            Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 1, "character": 0 },
                "options": { "tabSize": 4, "insertSpaces": true },
            })),
            None,
        )
        .err()
        .ok_or("expected invalid params")?;

    assert_eq!(error.code, -32602);
    Ok(())
}

#[test]
fn external_partial_range_never_substitutes_native() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Range);
    server.config.lock().formatting_engine = FormatterMode::ExternalLegacy;
    let uri = "file:///external-range.pl";
    server.test_apply_did_open(uri, "my$x=1;\nmy$y=2;\n", 1)?;

    let result = server.handle_range_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 7 }
            },
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    )?;

    assert_eq!(result, Some(json!([])));
    let receipt = receipt(&server)?;
    assert_eq!(receipt["reason"], "unsafe_range");
    assert_eq!(receipt["actual_engine"], "unknown");
    assert_eq!(receipt["requested_mode"], "external-legacy");
    Ok(())
}

#[test]
fn invalid_multi_range_geometry_rejects_atomically() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Ranges);
    let uri = "file:///multi-range-refusal.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;

    let error = server
        .handle_ranges_formatting_policy(
            Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "ranges": [
                    {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 7 }
                    },
                    {
                        "start": { "line": 99, "character": 0 },
                        "end": { "line": 99, "character": 1 }
                    }
                ],
                "options": { "tabSize": 4, "insertSpaces": true },
            })),
            None,
        )
        .err()
        .ok_or("invalid range geometry was admitted")?;

    assert_eq!(error.code, -32602);
    let data = error.data.ok_or("missing invalid-plan evidence")?;
    assert_eq!(data["reason"], "invalid_position");
    let receipt = receipt(&server)?;
    assert_eq!(receipt["decision"], "blocked");
    assert_eq!(receipt["reason"], "invalid_position");
    assert_eq!(receipt["result_count"], 0);
    Ok(())
}

#[test]
fn overlapping_multi_ranges_are_rejected_before_formatting()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Ranges);
    let uri = "file:///overlapping-ranges.pl";
    server.test_apply_did_open(uri, "my$x=1;\n", 1)?;

    let error = server
        .handle_ranges_formatting_policy(
            Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "ranges": [
                    {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 5 }
                    },
                    {
                        "start": { "line": 0, "character": 3 },
                        "end": { "line": 0, "character": 7 }
                    }
                ],
                "options": { "tabSize": 4, "insertSpaces": true },
            })),
            None,
        )
        .err()
        .ok_or("overlapping ranges were admitted")?;

    assert_eq!(error.code, -32602);
    let data = error.data.ok_or("missing invalid-plan evidence")?;
    assert_eq!(data["reason"], "overlapping_ranges");
    let receipt = receipt(&server)?;
    assert_eq!(receipt["decision"], "blocked");
    assert_eq!(receipt["reason"], "overlapping_ranges");
    assert_eq!(receipt["actual_engine"], "not_started");
    assert_eq!(receipt["result_count"], 0);
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

    // A source-current snapshot whose configuration changed after admission
    // must report the distinct stale_configuration reason, so a selector
    // hardcoded to stale_source cannot pass this contract.
    let fresh_params = json!({
        "textDocument": { "uri": uri, "version": 2 },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let fresh = server.admit(Surface::Document, &fresh_params)?;
    server.config.lock().perltidy_maximum_line_length = Some(96);
    let error = server.ensure_current(&fresh).err().ok_or("expected stale-configuration error")?;
    assert_eq!(error.code, CONTENT_MODIFIED);
    assert_eq!(receipt(&server)?["reason"], "stale_configuration");

    Ok(())
}

#[test]
fn stale_unknown_range_decision_preserves_unknown_receipt_engine()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Range);
    server.config.lock().formatting_engine = FormatterMode::ExternalLegacy;
    let uri = "file:///stale-unknown-range-formatting.pl";
    server.test_apply_did_open(uri, "my$x=1;\nmy$y=2;\n", 1)?;
    let params = json!({
        "textDocument": { "uri": uri, "version": 1 },
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 7 }
        },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let snapshot = server.admit(Surface::Range, &params)?;
    let formatter =
        CodeFormatter::with_config_and_mode(snapshot.config.perltidy.clone(), snapshot.config.mode);
    let context = FormatContext::new(Some(snapshot.uri.clone()), Some(snapshot.generation));
    let decision = formatter.format_range_decision(
        &snapshot.text,
        &parse_range(params.get("range").ok_or("missing range")?, "range")?,
        &snapshot.options,
        &context,
    )?;
    assert_eq!(decision.outcome.identity.actual_engine, FormatEngine::Unknown);
    let actual_engine = actual_engine_for_decision(&decision);

    {
        let mut documents = server.documents.lock();
        let document = server.get_document_mut(&mut documents, uri).ok_or("missing document")?;
        document.update_content("my $x = 2;\nmy$y=2;\n", 2);
    }
    let error = server
        .ensure_current_with_engine(&snapshot, Some(actual_engine))
        .err()
        .ok_or("expected stale-source error")?;
    assert_eq!(error.code, CONTENT_MODIFIED);
    let trace = receipt(&server)?;
    assert_eq!(trace["reason"], "stale_source");
    assert_eq!(trace["actual_engine"], "unknown");

    let fresh_params = json!({
        "textDocument": { "uri": uri, "version": 2 },
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 7 }
        },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let fresh = server.admit(Surface::Range, &fresh_params)?;
    let fresh_context = FormatContext::new(Some(fresh.uri.clone()), Some(fresh.generation));
    let fresh_decision = formatter.format_range_decision(
        &fresh.text,
        &parse_range(fresh_params.get("range").ok_or("missing fresh range")?, "range")?,
        &fresh.options,
        &fresh_context,
    )?;
    assert_eq!(fresh_decision.outcome.identity.actual_engine, FormatEngine::Unknown);
    server.config.lock().perltidy_maximum_line_length = Some(96);
    let error = server
        .ensure_current_with_engine(&fresh, Some(actual_engine_for_decision(&fresh_decision)))
        .err()
        .ok_or("expected stale-configuration error")?;
    assert_eq!(error.code, CONTENT_MODIFIED);
    let trace = receipt(&server)?;
    assert_eq!(trace["reason"], "stale_configuration");
    assert_eq!(trace["actual_engine"], "unknown");

    Ok(())
}

#[test]
fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    let document_uri = "file:///live-formatting.pl";
    let on_type_uri = "file:///live-on-type.pl";
    server.test_apply_did_open(document_uri, "my$x=1;\nmy$y=2;\n", 1)?;
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
                "ranges": [
                    {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 7 }
                    },
                    {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 7 }
                    }
                ],
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
        assert!(
            trace["source_generation"].is_u64(),
            "{method} receipt must carry a numeric source_generation"
        );
        assert!(
            trace["config_fingerprint"].is_string(),
            "{method} receipt must carry a config_fingerprint string"
        );
        if method == "textDocument/rangesFormatting" {
            let edits = response
                .result
                .as_ref()
                .and_then(Value::as_array)
                .ok_or("rangesFormatting must return an edit array")?;
            assert_eq!(edits.len(), 2);
            assert_eq!(
                edits[0],
                json!({
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 7 }
                    },
                    "newText": "my $x = 1;"
                })
            );
            assert_eq!(
                edits[1],
                json!({
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 7 }
                    },
                    "newText": "my $y = 2;"
                })
            );
            assert_eq!(trace["result_count"], 2);
        }
    }

    Ok(())
}

#[test]
fn live_external_partial_range_returns_typed_refusal_not_native_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    server.config.lock().formatting_engine = FormatterMode::ExternalLegacy;
    let uri = "file:///live-external-range.pl";
    server.test_apply_did_open(uri, "my$x=1;\nmy$y=2;\n", 1)?;

    let response = server
        .handle_request(request(
            200,
            "textDocument/rangeFormatting",
            json!({
                "textDocument": { "uri": uri, "version": 1 },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ))
        .ok_or("rangeFormatting returned no response")?;

    assert!(response.error.is_none(), "range formatting should return a typed refusal");
    assert_eq!(response.result, Some(json!([])));
    let trace = receipt(&server)?;
    assert_eq!(trace["decision"], "blocked");
    assert_eq!(trace["reason"], "unsafe_range");
    assert_eq!(trace["requested_mode"], "external-legacy");
    assert_eq!(trace["actual_engine"], "unknown");
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

/// The formatting receipt must never contain the raw source URI.
///
/// `source_id` is replaced with a deterministic FNV-1a hex hash before the
/// receipt is recorded and forwarded to any observer. This contract exists so
/// that provider receipts do not leak workspace-local file paths into logs,
/// telemetry, or support packets that may leave the local machine.
///
/// Separately, the hash must be a fixed-width lowercase hex string (16 chars)
/// and must not equal the URI itself — confirming that hashing actually ran.
#[test]
fn receipt_source_id_is_always_hashed_never_raw() -> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    let uri = "file:///private-path/secret-workspace/my-module.pl";
    server.test_apply_did_open(uri, "my $x = 1;\n", 1)?;

    let response = server
        .handle_request(request(
            401,
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ))
        .ok_or("formatting returned no response")?;
    // The request must succeed so the receipt below reflects a completed decision.
    assert!(
        response.error.is_none(),
        "formatting must not fail for this fixture; got error={:?}",
        response.error
    );
    assert!(response.result.is_some(), "formatting must return an edit array");
    let trace = receipt(&server)?;

    // source_id (the raw URI) must never appear as a key in the receipt.
    assert!(
        trace.get("source_id").is_none(),
        "raw source_id must not appear in the receipt; got trace={trace}"
    );
    // source_id_hash must always be present.
    let hash = trace["source_id_hash"].as_str().ok_or("source_id_hash must be a string")?;
    // Hash is a 16-character lowercase hexadecimal string (FNV-1a 64-bit).
    assert_eq!(hash.len(), 16, "source_id_hash must be 16 hex chars; got {hash:?}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "source_id_hash must be lowercase hex; got {hash:?}"
    );
    // The hash must not equal the URI string itself.
    assert_ne!(hash, uri, "source_id_hash must not equal the raw URI");
    // The hash must be the exact deterministic FNV-1a 64 digest of the URI,
    // computed independently here so an algorithm swap or constant cannot pass.
    let mut expected: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in uri.as_bytes() {
        expected ^= u64::from(*byte);
        expected = expected.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(
        hash,
        format!("{expected:016x}"),
        "source_id_hash must be the FNV-1a digest of the URI"
    );
    // The raw URI must never appear anywhere in the serialized receipt, and no
    // nested `source_id` key may survive in any outcome object.
    let serialized = serde_json::to_string(&trace)?;
    assert!(!serialized.contains(uri), "raw URI leaked into the serialized receipt: {serialized}");
    fn has_raw_source_id_key(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                map.iter().any(|(key, inner)| key == "source_id" || has_raw_source_id_key(inner))
            }
            serde_json::Value::Array(items) => items.iter().any(has_raw_source_id_key),
            _ => false,
        }
    }
    assert!(
        !has_raw_source_id_key(&trace),
        "a nested source_id key survived sanitization: {trace}"
    );
    Ok(())
}

/// Receipts from a disabled formatter carry the correct static invariants.
///
/// When perltidy is disabled (`perltidy_enabled = false`) the formatter mode
/// resolves to `FormatterMode::Off` and the actual engine is "disabled". The
/// receipt contract requires:
/// - `dynamic_boundary` is always `false` (a fixed, non-negotiable seam marker)
/// - `source_backed` is `false` for non-native engines
/// - `fact_source` is `"provider_runtime"` for non-native engines
/// - `confidence` is `"low"` for blocked decisions
/// - `freshness` is `"fresh"` for reasons that do not start with `"stale_"`
#[test]
fn receipt_static_invariants_hold_for_disabled_formatter() -> Result<(), Box<dyn std::error::Error>>
{
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    server.config.lock().perltidy_enabled = false;
    let uri = "file:///disabled-formatter-invariants.pl";
    server.test_apply_did_open(uri, "my $x = 1;\n", 1)?;

    let _ = server.handle_formatting_policy(
        Some(json!({
            "textDocument": { "uri": uri, "version": 1 },
            "options": { "tabSize": 4, "insertSpaces": true },
        })),
        None,
    );
    let trace = receipt(&server)?;

    // Static seam marker — never negotiated or inferred from runtime state.
    assert_eq!(
        trace["dynamic_boundary"],
        json!(false),
        "dynamic_boundary must always be false; got trace={trace}"
    );
    // Disabled engine is not source-backed.
    assert_eq!(
        trace["source_backed"],
        json!(false),
        "source_backed must be false for non-native engine; got trace={trace}"
    );
    // Non-native engine routes facts through the provider runtime, not the parser.
    assert_eq!(
        trace["fact_source"], "provider_runtime",
        "fact_source must be 'provider_runtime' for disabled engine; got trace={trace}"
    );
    // Blocked decision → low confidence.
    assert_eq!(
        trace["decision"], "blocked",
        "disabled formatter must produce decision='blocked'; got trace={trace}"
    );
    assert_eq!(
        trace["confidence"], "low",
        "confidence must be 'low' for blocked decisions; got trace={trace}"
    );
    // Reason "formatter_disabled" does not start with "stale_" → fresh.
    assert_eq!(
        trace["freshness"], "fresh",
        "freshness must be 'fresh' when reason does not start with 'stale_'; got trace={trace}"
    );
    Ok(())
}

/// The stale-source receipt must report `freshness: "stale"`.
///
/// When a document is edited between admit-time and currentness check, the
/// server detects a generation mismatch. The resulting receipt's `reason`
/// starts with `"stale_"`, which the freshness selector must map to `"stale"`.
/// This proves the freshness branch is data-driven, not hardcoded.
#[test]
fn receipt_freshness_is_stale_when_reason_starts_with_stale_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    advertise(&server, Surface::Document);
    let uri = "file:///freshness-stale-check.pl";
    server.test_apply_did_open(uri, "my $x = 1;\n", 1)?;
    let params = json!({
        "textDocument": { "uri": uri, "version": 1 },
        "options": { "tabSize": 4, "insertSpaces": true },
    });
    let snapshot = server.admit(Surface::Document, &params)?;
    // Advance the document generation to make the snapshot stale.
    {
        let mut documents = server.documents.lock();
        let doc = server.get_document_mut(&mut documents, uri).ok_or("document must be open")?;
        doc.update_content("my $x = 2;\n", 2);
    }

    let error = server.ensure_current(&snapshot).err().ok_or("expected stale-source error")?;
    assert_eq!(error.code, CONTENT_MODIFIED);

    let trace = receipt(&server)?;
    // Reason must start with "stale_" to trigger the freshness selector.
    let reason = trace["reason"].as_str().ok_or("reason must be a string")?;
    assert!(
        reason.starts_with("stale_"),
        "reason must start with 'stale_' for a stale-source receipt; got {reason:?}"
    );
    // Freshness must be "stale" when reason starts with "stale_".
    assert_eq!(
        trace["freshness"], "stale",
        "freshness must be 'stale' when reason starts with 'stale_'; got trace={trace}"
    );
    Ok(())
}

/// The live-dispatch receipt carries the full provider identity.
///
/// When a formatting request is dispatched end-to-end, the receipt must
/// contain the canonical provider identity fields expected by consumers such
/// as the support-packet builder and the provider decision trace index.
#[test]
fn live_dispatch_receipt_carries_canonical_provider_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let server = LspServer::new();
    initialize(&server)?;
    let uri = "file:///provider-identity-receipt.pl";
    server.test_apply_did_open(uri, "my $x = 1;\n", 1)?;

    let response = server
        .handle_request(request(
            500,
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri, "version": 1 },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ))
        .ok_or("formatting returned no response")?;
    assert!(response.error.is_none(), "expected no error; got {:?}", response.error);

    let trace = receipt(&server)?;
    assert_eq!(trace["provider"], PROVIDER, "provider field must match PROVIDER constant");
    assert_eq!(
        trace["provider_action"], "textDocument/formatting",
        "provider_action must be the LSP method name"
    );
    // claim_boundary must be a non-empty explanatory string.
    let boundary = trace["claim_boundary"].as_str().ok_or("claim_boundary must be a string")?;
    assert!(
        !boundary.is_empty(),
        "claim_boundary must be a non-empty explanation; got {boundary:?}"
    );
    // Per the documented text-sync invariant (`handle_did_open` in
    // runtime/text_sync.rs), didOpen always starts at generation 0; only a
    // didChange bumps it. A first-dispatch receipt on a freshly opened
    // document must therefore carry source_generation == 0, and a
    // re-dispatch after an edit must carry a positive generation.
    assert_eq!(
        trace["source_generation"], 0,
        "source_generation must be 0 for a freshly opened document; got trace={trace}"
    );
    server.test_apply_did_change(uri, "my $x = 2;\n", 2)?;
    let edited = server
        .handle_request(request(
            501,
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri, "version": 2 },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ))
        .ok_or("formatting returned no response after edit")?;
    assert!(edited.error.is_none(), "expected no error after edit; got {:?}", edited.error);
    let edited_trace = receipt(&server)?;
    assert!(
        edited_trace["source_generation"].as_u64().is_some_and(|g| g > 0),
        "source_generation must be positive after a didChange; got trace={edited_trace}"
    );
    // config_fingerprint must be a non-empty string.
    let fingerprint =
        trace["config_fingerprint"].as_str().ok_or("config_fingerprint must be a string")?;
    assert!(!fingerprint.is_empty(), "config_fingerprint must not be empty");
    // dynamic_boundary is a static invariant.
    assert_eq!(trace["dynamic_boundary"], json!(false));
    Ok(())
}
