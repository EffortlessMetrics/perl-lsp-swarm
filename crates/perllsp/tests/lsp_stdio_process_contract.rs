//! Required real-process contract for Cargo's exact public `perllsp` binary.
//!
//! The transport, framing, timeout, event classification, stderr capture, and
//! child cleanup implementation is shared with the `perl-lsp-rs` process suite.
//! This target proves that the installable facade—not a compatibility binary or
//! PATH fallback—is the process completing the public contract.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, bail, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

fn explain_trace(client: &mut RealProcessClient, provider: &str) -> Result<Value> {
    let id = json!("explain-trace");
    let response = client.request(
        id.clone(),
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{"provider": provider}]
        }),
        timeout(),
    )?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("error").is_none(), "explanation request failed: {response}");
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing explanation: {response}"))
}

fn explain_trace_for_id(
    client: &mut RealProcessClient,
    provider: &str,
    request_id: Value,
) -> Result<Value> {
    let explain_id = json!(format!("explain-trace-{request_id}"));
    let response = client.request(
        explain_id.clone(),
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{"provider": provider, "request_id": request_id}]
        }),
        timeout(),
    )?;
    assert_response_id(&response, &explain_id)?;
    ensure!(response.get("error").is_none(), "explanation request failed: {response}");
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing explanation: {response}"))
}

fn open_trace_fixture(client: &mut RealProcessClient) -> Result<&'static str> {
    let uri = "file:///workspace/provider-trace.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1,
            "text": "my $value = 1;\nprint $value;\n"
        }}),
    )?;
    Ok(uri)
}

#[test]
fn generic_trace_success_does_not_invent_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace"))?;
    let uri = open_trace_fixture(&mut client)?;
    let hover = client.request(
        json!(801),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": uri}, "position": {"line": 1, "character": 2}
        }),
        timeout(),
    )?;
    assert_response_id(&hover, &json!(801))?;
    ensure!(hover.get("error").is_none(), "valid hover failed: {hover}");
    ensure!(
        hover.pointer("/result/contents").is_some_and(|value| value.to_string().contains("print")),
        "builtin hover control must describe print: {hover}"
    );
    let explanation = explain_trace(&mut client, "hover")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("unknown")),
        "dispatch shape alone cannot establish freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/source_backed_state")
            == Some(&json!("not_proven_by_dispatch_trace")),
        "expected the actual generic dispatch trace: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/live_provider_result_count") == Some(&json!(1)),
        "successful hover trace must still record the actual result: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace"))
}

#[test]
fn references_trace_selector_refuses_overwritten_request_and_keeps_latest_id() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-references-selector"))?;
    let uri = open_trace_fixture(&mut client)?;
    let params = json!({
        "textDocument": {"uri": uri},
        "position": {"line": 0, "character": 4},
        "context": {"includeDeclaration": false}
    });

    let first = client.request(json!(41), "textDocument/references", params.clone(), timeout())?;
    assert_response_id(&first, &json!(41))?;
    ensure!(first.get("error").is_none(), "first references request failed: {first}");
    ensure!(
        first.pointer("/result").and_then(Value::as_array).is_some_and(|locations| {
            locations
                == &[json!({"uri": uri, "range": {
                    "start": {"line": 1, "character": 6},
                    "end": {"line": 1, "character": 12}
                }})]
        }),
        "first references response must contain the exact expected usage location: {first}"
    );
    let first_explanation = explain_trace_for_id(&mut client, "references", json!(41))?;
    ensure!(
        first_explanation.pointer("/request_receipt/request_id") == Some(&json!(41))
            && first_explanation.pointer("/request_receipt/uri") == Some(&json!(uri))
            && first_explanation.pointer("/request_receipt/line") == Some(&json!(0))
            && first_explanation.pointer("/request_receipt/character") == Some(&json!(4))
            && first_explanation.pointer("/request_receipt/result_count") == Some(&json!(1)),
        "request 41 must expose its own receipt before overwrite: {first_explanation}"
    );

    let failed_same_id =
        client.request(json!(41), "textDocument/references", json!({}), timeout())?;
    assert_response_id(&failed_same_id, &json!(41))?;
    ensure!(
        failed_same_id.pointer("/error/code") == Some(&json!(-32602)),
        "reused request 41 must return invalid params: {failed_same_id}"
    );
    let failed_same_id_explanation = explain_trace_for_id(&mut client, "references", json!(41))?;
    ensure!(
        failed_same_id_explanation.get("request_receipt").is_none()
            && failed_same_id_explanation
                .get("user_message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("No request evidence is attached")),
        "failed reused request 41 must not expose the earlier successful receipt: {failed_same_id_explanation}"
    );

    let second = client.request(
        json!("41"),
        "textDocument/references",
        json!({
            "textDocument": {"uri": uri},
            "position": {"line": 1, "character": 8},
            "context": {"includeDeclaration": false}
        }),
        timeout(),
    )?;
    assert_response_id(&second, &json!("41"))?;
    ensure!(second.get("error").is_none(), "second references request failed: {second}");
    ensure!(
        second.pointer("/result").and_then(Value::as_array).is_some_and(|locations| {
            locations
                == &[json!({"uri": uri, "range": {
                    "start": {"line": 1, "character": 6},
                    "end": {"line": 1, "character": 12}
                }})]
        }),
        "second references response must contain the exact expected usage location: {second}"
    );

    let overwritten = explain_trace_for_id(&mut client, "references", json!(41))?;
    ensure!(
        overwritten.get("request_receipt").is_none()
            && overwritten
                .get("user_message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("No request evidence is attached")),
        "numeric request 41 must refuse overwritten string request evidence: {overwritten}"
    );
    let latest = explain_trace_for_id(&mut client, "references", json!("41"))?;
    ensure!(
        latest.pointer("/request_receipt/request_id") == Some(&json!("41"))
            && latest.pointer("/request_receipt/uri") == Some(&json!(uri))
            && latest.pointer("/request_receipt/line") == Some(&json!(1))
            && latest.pointer("/request_receipt/character") == Some(&json!(8))
            && latest.pointer("/request_receipt/result_count") == Some(&json!(1)),
        "latest string request must retain its exact ID: {latest}"
    );

    let failed = client.request(json!(99), "textDocument/references", json!({}), timeout())?;
    assert_response_id(&failed, &json!(99))?;
    ensure!(
        failed.pointer("/error/code") == Some(&json!(-32602)),
        "invalid references request must fail: {failed}"
    );
    let failed_explanation = explain_trace_for_id(&mut client, "references", json!(99))?;
    ensure!(
        failed_explanation.get("request_receipt").is_none()
            && failed_explanation
                .get("user_message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("No request evidence is attached")),
        "failed request 99 must not invent trace evidence: {failed_explanation}"
    );
    let too_large = Value::Number(
        serde_json::Number::from_u128(9_223_372_036_854_775_808_u128)
            .ok_or_else(|| anyhow::anyhow!("failed to construct out-of-range JSON-RPC ID"))?,
    );
    for selector in [json!(1.5), too_large] {
        let invalid = client.request(
            json!(format!("invalid-selector-{selector}")),
            "workspace/executeCommand",
            json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": "references", "request_id": selector}]
            }),
            timeout(),
        )?;
        ensure!(
            invalid.pointer("/error/code") == Some(&json!(-32602)),
            "invalid numeric selector must be rejected: {invalid}"
        );
    }
    shutdown_and_exit(&mut client, json!("shutdown-references-selector"))
}

#[test]
fn generic_trace_error_does_not_invent_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace-error"))?;
    let response = client.request(json!("801"), "textDocument/hover", json!({}), timeout())?;
    assert_response_id(&response, &json!("801"))?;
    ensure!(
        response.pointer("/error/code") == Some(&json!(-32602)),
        "expected invalid params: {response}"
    );
    let explanation = explain_trace(&mut client, "hover")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("unknown")),
        "provider error without accepted-state evidence cannot claim freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/provider_error/code") == Some(&json!(-32602)),
        "trace must retain the provider error: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace-error"))
}

#[test]
fn generic_trace_preserves_provider_owned_full_token_freshness() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-trace-tokens"))?;
    let uri = open_trace_fixture(&mut client)?;
    let response = client.request(
        json!(802),
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": {"uri": uri}
        }),
        timeout(),
    )?;
    assert_response_id(&response, &json!(802))?;
    ensure!(response.get("error").is_none(), "valid full-token request failed: {response}");
    ensure!(
        response
            .pointer("/result/data")
            .and_then(Value::as_array)
            .is_some_and(|data| !data.is_empty()),
        "full-token control must execute the provider: {response}"
    );
    let explanation = explain_trace(&mut client, "semantic_tokens")?;
    ensure!(
        explanation.pointer("/request_receipt/freshness") == Some(&json!("fresh")),
        "dispatcher must preserve provider-owned freshness: {explanation}"
    );
    ensure!(
        explanation.pointer("/request_receipt/provider_action")
            == Some(&json!("textDocument/semanticTokens/full")),
        "expected the provider-owned full-token trace: {explanation}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-trace-tokens"))
}

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn check_policy_message(receipt: Option<Value>, expected_evidence: &str) -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-policy"))?;
    let expected_freshness = receipt
        .as_ref()
        .map(|value| value.get("freshness").cloned().unwrap_or_else(|| json!("unknown")));
    let expected_detail = receipt
        .as_ref()
        .and_then(|value| value.get("user_message"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut argument = json!({"provider": "hover"});
    if expected_detail.is_some() {
        let fields =
            argument.as_object_mut().ok_or_else(|| anyhow::anyhow!("expected argument object"))?;
        fields.insert("receipt_id".to_string(), json!("caller-receipt-marker"));
        fields.insert("scenario".to_string(), json!("caller-scenario-marker"));
    }
    if let Some(receipt) = receipt {
        let prior =
            client.request(json!("prior-hover"), "textDocument/hover", json!({}), timeout())?;
        assert_response_id(&prior, &json!("prior-hover"))?;
        ensure!(
            prior.pointer("/error/code") == Some(&json!(-32602)),
            "expected prior hover error: {prior}"
        );
        argument
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("expected argument object"))?
            .insert("request_receipt".to_string(), receipt);
    }
    let response = client.request(
        json!("policy-message"),
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision", "arguments": [argument]
        }),
        timeout(),
    )?;
    assert_response_id(&response, &json!("policy-message"))?;
    ensure!(response.get("error").is_none(), "explanation failed: {response}");
    let result =
        response.get("result").ok_or_else(|| anyhow::anyhow!("missing result: {response}"))?;
    let message = result
        .get("user_message")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing user message: {result}"))?;
    ensure!(
        message.contains("\nProvider policy summary:\n"),
        "static defaults must be identified as policy: {message}"
    );
    ensure!(
        message.starts_with(expected_evidence),
        "request evidence must lead the message: {message}"
    );
    if let Some(detail) = expected_detail {
        let (evidence, policy) = message
            .split_once("\nProvider policy summary:\n")
            .ok_or_else(|| anyhow::anyhow!("missing policy boundary: {message}"))?;
        ensure!(
            evidence.contains(&format!("Request detail: {detail}")),
            "request detail must be in the evidence section: {message}"
        );
        ensure!(
            !policy.contains(&detail) && message.matches(&detail).count() == 1,
            "request detail must not be duplicated or labeled as policy: {message}"
        );
        for (field, marker) in
            [("receipt_id", "caller-receipt-marker"), ("scenario", "caller-scenario-marker")]
        {
            ensure!(
                evidence.contains(marker) && !policy.contains(marker),
                "caller context must remain outside static policy: {message}"
            );
            ensure!(
                result.get(field).and_then(Value::as_str) == Some(marker),
                "structured caller context must be preserved: {result}"
            );
        }
        ensure!(
            result.pointer("/request_receipt/user_message").and_then(Value::as_str)
                == Some(detail.as_str()),
            "structured request detail must be preserved: {result}"
        );
    }
    ensure!(
        result.get("freshness") == Some(&json!("fresh")),
        "message repair must preserve the structured policy default: {result}"
    );
    if let Some(expected_freshness) = expected_freshness {
        ensure!(
            result.pointer("/request_receipt/freshness") == Some(&expected_freshness),
            "caller receipt must retain precedence and its structured freshness: {result}"
        );
        ensure!(
            result.pointer("/request_receipt/provider_error").is_none(),
            "prior hover error must not leak into the caller receipt: {result}"
        );
        ensure!(
            result.pointer("/copyable_payload/request_receipt") == result.get("request_receipt"),
            "copyable request receipt must match the structured evidence: {result}"
        );
    } else {
        ensure!(
            result.get("request_receipt").is_none(),
            "no request receipt should be invented: {result}"
        );
        ensure!(
            result.pointer("/copyable_payload/request_receipt") == Some(&Value::Null),
            "copyable payload must retain its explicit absence representation: {result}"
        );
    }
    ensure!(
        result.pointer("/copyable_payload/user_message").and_then(Value::as_str) == Some(message),
        "copyable message must match the displayed message: {result}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-policy"))
}

#[test]
fn policy_message_without_request_evidence_is_explicit() -> Result<()> {
    check_policy_message(None, "No request evidence is attached.")
}

#[test]
fn policy_message_keeps_request_detail_out_of_policy() -> Result<()> {
    check_policy_message(
        Some(json!({
            "freshness": "unknown", "user_message": "The recorded request returned no result."
        })),
        "Attached request freshness: unknown.",
    )
}

#[test]
fn policy_message_preserves_unknown_request_evidence() -> Result<()> {
    check_policy_message(
        Some(json!({"freshness": "unknown"})),
        "Attached request freshness: unknown.",
    )
}

#[test]
fn policy_message_preserves_stale_request_evidence() -> Result<()> {
    check_policy_message(Some(json!({"freshness": "stale"})), "Attached request freshness: stale.")
}

#[test]
fn policy_message_preserves_fresh_request_evidence() -> Result<()> {
    check_policy_message(Some(json!({"freshness": "fresh"})), "Attached request freshness: fresh.")
}

#[test]
fn policy_message_preserves_not_applicable_request_evidence() -> Result<()> {
    check_policy_message(
        Some(json!({"freshness": "not_applicable"})),
        "Attached request freshness: not applicable.",
    )
}

#[test]
fn policy_message_missing_request_freshness_is_unknown() -> Result<()> {
    check_policy_message(Some(json!({})), "Attached request freshness: unknown.")
}

#[test]
fn policy_message_invalid_request_freshness_is_unknown() -> Result<()> {
    check_policy_message(
        Some(json!({"freshness": "invented"})),
        "Attached request freshness: unknown.",
    )
}

fn assert_public_candidate(client: &RealProcessClient) -> Result<()> {
    ensure!(
        client.candidate_name() == "perllsp",
        "public process target selected {}; env={}; path={}",
        client.candidate_name(),
        client.candidate_environment(),
        client.candidate_path().display()
    );
    ensure!(
        client.candidate_environment() == "CARGO_BIN_EXE_perllsp",
        "public process target used the wrong Cargo identity: {}",
        client.candidate_environment()
    );
    ensure!(
        client.candidate_path().is_file(),
        "exact public candidate path disappeared: {}",
        client.candidate_path().display()
    );
    ensure!(
        client.candidate_path().file_stem().and_then(|stem| stem.to_str()) == Some("perllsp"),
        "exact public candidate was not named perllsp: {}",
        client.candidate_path().display()
    );
    Ok(())
}

fn initialize(client: &mut RealProcessClient, id: Value) -> Result<Value> {
    client.request(
        id,
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perllsp-public-process-contract",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            },
            "workspaceFolders": null
        }),
        timeout(),
    )
}

fn assert_response_id(response: &Value, expected: &Value) -> Result<()> {
    ensure!(response.get("jsonrpc") == Some(&json!("2.0")), "missing JSON-RPC version: {response}");
    ensure!(
        response.get("id") == Some(expected),
        "response ID mismatch: expected={expected}; response={response}"
    );
    Ok(())
}

fn initialize_and_notify(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response = initialize(client, id.clone())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize omitted capabilities: {response}"
    );
    client.notify("initialized", json!({}))
}

fn shutdown_and_exit(client: &mut RealProcessClient, id: Value) -> Result<()> {
    let response = client.request(id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&response, &id)?;
    ensure!(
        response.get("result").is_some_and(Value::is_null),
        "shutdown must return null: {response}"
    );
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "clean public shutdown exited with {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

#[test]
fn exact_public_candidate_completes_legal_lifecycle() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-public"))?;
    shutdown_and_exit(&mut client, json!("shutdown-public"))
}

#[test]
fn cancellation_of_unknown_or_completed_ids_does_not_poison_reuse() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-cancel-reuse"))?;
    let uri = open_trace_fixture(&mut client)?;

    for id in [json!(71003), json!("71003")] {
        for phase in ["unknown", "completed"] {
            client.notify("$/cancelRequest", json!({"id": id.clone()}))?;
            let response = client.request(
                id.clone(),
                "textDocument/hover",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 1, "character": 2}
                }),
                timeout(),
            )?;
            assert_response_id(&response, &id)?;
            ensure!(
                response.get("error").is_none(),
                "cancellation of {phase} ID {id} poisoned a later request: {response}"
            );
            ensure!(
                response
                    .pointer("/result/contents")
                    .is_some_and(|contents| contents.to_string().contains("print")),
                "later request must execute the real hover provider after {phase} cancellation: {response}"
            );
        }
    }

    shutdown_and_exit(&mut client, json!("shutdown-cancel-reuse"))
}

#[test]
fn signature_help_empty_results_complete_exact_requests() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    let initialized = initialize(&mut client, json!("initialize-signature-empty"))?;
    ensure!(
        initialized.pointer("/result/capabilities/signatureHelpProvider").is_some(),
        "signature help must be advertised for this proof: {initialized}"
    );
    client.notify("initialized", json!({}))?;
    let uri = "file:///signature-empty.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": "my $value = 1;\n"
        }}),
    )?;

    // Numeric and string IDs with the same spelling must each settle. A
    // legitimate empty result is an explicit JSON null, not a missing frame.
    for id in [json!(42), json!("42")] {
        let response = client.request(
            id.clone(),
            "textDocument/signatureHelp",
            json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 3}}),
            timeout(),
        )?;
        assert_response_id(&response, &id)?;
        ensure!(response.get("error").is_none(), "empty help returned an error: {response}");
        ensure!(
            response.get("result").is_some_and(Value::is_null),
            "empty signature help must return explicit null: {response}"
        );
    }

    client.notify("textDocument/didClose", json!({"textDocument": {"uri": uri}}))?;
    let closed_id = json!("closed-signature-document");
    let closed = client.request(
        closed_id.clone(),
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 3}}),
        timeout(),
    )?;
    assert_response_id(&closed, &closed_id)?;
    ensure!(closed.get("result").is_some_and(Value::is_null), "closed help: {closed}");
    shutdown_and_exit(&mut client, json!("shutdown-signature-empty"))
}

#[test]
fn signature_help_success_and_invalid_params_remain_distinct() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-signature-control"))?;
    let uri = "file:///signature-control.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1,
            "text": "substr(\"hello\", 0, 1);\n"
        }}),
    )?;
    let success_id = json!("signature-substr");
    let success = client.request(
        success_id.clone(),
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 7}}),
        timeout(),
    )?;
    assert_response_id(&success, &success_id)?;
    ensure!(
        success
            .pointer("/result/signatures/0/label")
            .and_then(Value::as_str)
            .is_some_and(|label| label.starts_with("substr")),
        "known builtin must retain its signature: {success}"
    );
    let invalid_id = json!("signature-invalid-params");
    let invalid =
        client.request(invalid_id.clone(), "textDocument/signatureHelp", json!({}), timeout())?;
    assert_response_id(&invalid, &invalid_id)?;
    ensure!(invalid.pointer("/error/code") == Some(&json!(-32602)), "invalid help: {invalid}");
    shutdown_and_exit(&mut client, json!("shutdown-signature-control"))
}

#[test]
fn preinitialize_duplicate_initialize_and_post_shutdown_are_deterministic() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;

    let before_id = json!("before-initialize");
    let before = client.request(
        before_id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///not-open.pl" },
            "position": { "line": 0, "character": 0 }
        }),
        timeout(),
    )?;
    assert_response_id(&before, &before_id)?;
    ensure!(
        before.pointer("/error/code") == Some(&json!(-32002)),
        "expected ServerNotInitialized: {before}"
    );

    initialize_and_notify(&mut client, json!(1))?;
    let duplicate_id = json!("duplicate-initialize");
    let duplicate = initialize(&mut client, duplicate_id.clone())?;
    assert_response_id(&duplicate, &duplicate_id)?;
    ensure!(
        duplicate.pointer("/error/code") == Some(&json!(-32600)),
        "duplicate initialize must return InvalidRequest: {duplicate}"
    );

    let shutdown = client.request(json!(2), "shutdown", Value::Null, timeout())?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null), "shutdown failed: {shutdown}");
    let after_id = json!("after-shutdown");
    let after = client.request(after_id.clone(), "$/perl-lsp/watchdog", json!({}), timeout())?;
    assert_response_id(&after, &after_id)?;
    ensure!(
        after.pointer("/error/code") == Some(&json!(-32600)),
        "request after shutdown must return InvalidRequest: {after}"
    );

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn public_candidate_preserves_fragmented_coalesced_and_utf8_frames() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;

    let initialize_id = json!(7);
    let initialize_message = json!({
        "jsonrpc": "2.0",
        "id": initialize_id.clone(),
        "method": "initialize",
        "params": {
            "processId": null,
            "clientInfo": {
                "name": "perllsp-fragmented-client",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {},
            "workspaceFolders": null
        }
    });
    let frame = RealProcessClient::encode_message(&initialize_message);
    let first = frame.len().min(9);
    let second = frame.len().min(first.saturating_add(17));
    client.send_raw_chunks(&[&frame[..first], &frame[first..second], &frame[second..]])?;
    let response = client.receive_response(&initialize_id, timeout())?;
    assert_response_id(&response, &initialize_id)?;
    client.notify("initialized", json!({}))?;

    let numeric_id = json!(42);
    let string_id = json!("utf8-✓");
    let numeric = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": numeric_id.clone(),
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "first" }
    }));
    let string = RealProcessClient::encode_message(&json!({
        "jsonrpc": "2.0",
        "id": string_id.clone(),
        "method": "$/perl-lsp/watchdog",
        "params": { "payload": "π and ✓ prove byte lengths" }
    }));
    let mut coalesced = numeric;
    coalesced.extend_from_slice(&string);
    client.send_raw_bytes(&coalesced)?;

    let numeric_response = client.receive_response(&numeric_id, timeout())?;
    let string_response = client.receive_response(&string_id, timeout())?;
    assert_response_id(&numeric_response, &numeric_id)?;
    assert_response_id(&string_response, &string_id)?;
    ensure!(numeric_response.get("result").is_some_and(Value::is_null));
    ensure!(string_response.get("result").is_some_and(Value::is_null));
    shutdown_and_exit(&mut client, json!(43))
}

#[test]
fn serialized_notification_receives_no_response() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!(1))?;

    client.notify("$/setTrace", json!({ "value": "off" }))?;
    let shutdown_id = json!("serialized-public-barrier");
    let shutdown = client.request(shutdown_id.clone(), "shutdown", Value::Null, timeout())?;
    assert_response_id(&shutdown, &shutdown_id)?;
    ensure!(shutdown.get("result").is_some_and(Value::is_null));
    client.assert_no_response_pending()?;

    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(status.success(), "shutdown then exit failed: {status}");
    client.assert_transport_clean()
}

#[test]
fn exit_without_shutdown_returns_status_one() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!(1))?;
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(!status.success(), "exit without shutdown succeeded");
    ensure!(status.code() == Some(1), "expected status 1, got {status}");
    client.assert_transport_clean()
}

fn require_missing_resolve_params_error(
    method: &str,
    id: Value,
    explicit_null: bool,
) -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-resolve-error"))?;
    let request = if explicit_null {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": null})
    } else {
        json!({"jsonrpc": "2.0", "id": id, "method": method})
    };
    client.send_raw_bytes(&RealProcessClient::encode_message(&request))?;
    let response = client.receive_response(&id, timeout())?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("result").is_none(), "invalid resolve returned a result: {response}");
    ensure!(
        response.pointer("/error/code") == Some(&json!(-32602)),
        "missing resolve parameters must return InvalidParams: {response}"
    );
    ensure!(
        response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        "invalid resolve must explain the error: {response}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-resolve-error"))
}

#[test]
fn resolve_completion_without_params_returns_error() -> Result<()> {
    require_missing_resolve_params_error("completionItem/resolve", json!(701), false)
}

#[test]
fn resolve_completion_with_null_params_returns_error() -> Result<()> {
    require_missing_resolve_params_error("completionItem/resolve", json!("701"), true)
}

#[test]
fn resolve_code_action_without_params_returns_error() -> Result<()> {
    require_missing_resolve_params_error("codeAction/resolve", json!(702), false)
}

#[test]
fn resolve_code_action_with_null_params_returns_error() -> Result<()> {
    require_missing_resolve_params_error("codeAction/resolve", json!("702"), true)
}

#[test]
fn resolve_valid_items_retain_documentation_and_edits() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-resolve-valid"))?;
    let completion = client.request(
        json!(703),
        "completionItem/resolve",
        json!({"label": "print", "kind": 3, "extension": {"opaque": [1, "β"]}}),
        timeout(),
    )?;
    assert_response_id(&completion, &json!(703))?;
    ensure!(completion.get("error").is_none(), "valid completion failed: {completion}");
    ensure!(
        completion.pointer("/result/extension") == Some(&json!({"opaque": [1, "β"]})),
        "completion lost extension data: {completion}"
    );
    ensure!(
        completion.pointer("/result/label") == Some(&json!("print")),
        "completion changed: {completion}"
    );
    ensure!(
        completion
            .pointer("/result/documentation/value")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        "builtin completion must retain documentation: {completion}"
    );
    let uri = "file:///workspace/resolve-valid.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": "print 1;\n"
        }}),
    )?;
    let action = client.request(
        json!("703"),
        "codeAction/resolve",
        json!({
            "title": "Add use strict", "kind": "quickfix",
            "data": {"uri": uri, "pragma": "use strict;"},
            "extension": {"opaque": [1, "β"]}
        }),
        timeout(),
    )?;
    assert_response_id(&action, &json!("703"))?;
    ensure!(action.get("error").is_none(), "valid code action failed: {action}");
    ensure!(
        action.pointer("/result/extension") == Some(&json!({"opaque": [1, "β"]})),
        "code action lost extension data: {action}"
    );
    let edits = action
        .pointer("/result/edit/changes")
        .and_then(|changes| changes.get(uri))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("resolved code action omitted document edits: {action}"))?;
    ensure!(edits.len() == 1, "expected one pragma edit: {action}");
    ensure!(
        edits.first()
            == Some(&json!({
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                "newText": "use strict;\n"
            })),
        "wrong resolved pragma edit: {action}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-resolve-valid"))
}

#[test]
fn malformed_resolve_notifications_do_not_emit_errors_or_poison_requests() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-resolve-notifications"))?;
    for method in ["completionItem/resolve", "codeAction/resolve"] {
        let missing = json!({"jsonrpc": "2.0", "method": method});
        client.send_raw_bytes(&RealProcessClient::encode_message(&missing))?;
        client.notify(method, Value::Null)?;
        client.notify(method, json!({}))?;
    }
    let id = json!("resolve-after-notifications");
    let response = client.request(
        id.clone(),
        "completionItem/resolve",
        json!({"label": "print", "kind": 3}),
        timeout(),
    )?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("error").is_none(), "valid resolve failed: {response}");
    ensure!(response.pointer("/result/label") == Some(&json!("print")));
    ensure!(
        response
            .pointer("/result/documentation/value")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        "valid resolve lost documentation: {response}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-resolve-notifications"))
}

#[test]
fn resolve_completion_rejects_invalid_supplied_shapes() -> Result<()> {
    require_resolve_shape_errors("completionItem/resolve", "label")
}

#[test]
fn resolve_code_action_rejects_invalid_supplied_shapes() -> Result<()> {
    require_resolve_shape_errors("codeAction/resolve", "title")
}

fn require_resolve_shape_errors(method: &str, required_field: &str) -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-shapes"))?;
    let malformed =
        [json!([]), json!({}), json!({(required_field): 7}), json!({(required_field): null})];
    let mut unexpected = Vec::new();
    for (index, params) in malformed.into_iter().enumerate() {
        let id = json!(format!("invalid-{index}"));
        let response = client.request(id.clone(), method, params, timeout())?;
        assert_response_id(&response, &id)?;
        if response.get("result").is_some()
            || response.pointer("/error/code") != Some(&json!(-32602))
            || response.pointer("/error/message").and_then(Value::as_str).is_none_or(str::is_empty)
        {
            unexpected.push(response);
        }
    }
    // Empty strings satisfy the required string field; extension data is opaque.
    let valid = json!({(required_field): "", "extension": {"opaque": [1, "β"]}});
    let response = client.request(json!("valid-after-errors"), method, valid.clone(), timeout())?;
    assert_response_id(&response, &json!("valid-after-errors"))?;
    ensure!(response.get("error").is_none(), "valid recovery failed: {response}");
    ensure!(response.get("result") == Some(&valid), "valid item was altered: {response}");
    shutdown_and_exit(&mut client, json!("shutdown-shapes"))?;
    ensure!(unexpected.is_empty(), "invalid {method} shapes returned success: {unexpected:?}");
    Ok(())
}

#[test]
fn strict_stdout_parser_rejects_stray_logs_and_lf_only_frames() -> Result<()> {
    let stray_log = b"starting perllsp on stdout\n";
    let Err(stray_error) = RealProcessClient::parse_stdout_frame_for_test(stray_log) else {
        bail!("stray stdout log must fail strict framing");
    };
    ensure!(
        stray_error.to_string().contains("CRLF")
            || stray_error.to_string().contains("header")
            || stray_error.to_string().contains("non-header"),
        "stray log failed for an unexpected reason: {stray_error:#}"
    );

    let body = br#"{"jsonrpc":"2.0","method":"window/logMessage"}"#;
    let mut lf_only = format!("Content-Length: {}\n\n", body.len()).into_bytes();
    lf_only.extend_from_slice(body);
    let Err(lf_error) = RealProcessClient::parse_stdout_frame_for_test(&lf_only) else {
        bail!("LF-only framing must fail");
    };
    ensure!(
        lf_error.to_string().contains("CRLF"),
        "LF-only framing failed for the wrong reason: {lf_error:#}"
    );
    Ok(())
}

fn open_definition_terminal_fixture(client: &mut RealProcessClient) -> Result<&'static str> {
    let uri = "file:///workspace/definition-terminal.pl";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1,
            "text": "package Foo;\nsub bar { return 1; }\npackage main;\nFoo::bar();\n# Foo::bar is not a call\n"
        }}),
    )?;
    Ok(uri)
}

fn require_empty_definition_response(id: Value, line: u32, character: u32) -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-definition-empty"))?;
    let uri = open_definition_terminal_fixture(&mut client)?;
    let response = client.request(
        id.clone(),
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        timeout(),
    )?;
    assert_response_id(&response, &id)?;
    ensure!(response.get("error").is_none(), "legitimate empty definition errored: {response}");
    ensure!(
        response.get("result").is_some_and(Value::is_null),
        "empty definition must have an explicit null result: {response}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-definition-empty"))
}

#[test]
fn definition_in_comment_completes_with_null() -> Result<()> {
    require_empty_definition_response(json!(51), 4, 7)
}

#[test]
fn definition_on_package_prefix_completes_with_null() -> Result<()> {
    require_empty_definition_response(json!("51"), 3, 1)
}

#[test]
fn definition_on_callable_retains_exact_location() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    assert_public_candidate(&client)?;
    initialize_and_notify(&mut client, json!("initialize-definition-location"))?;
    let uri = open_definition_terminal_fixture(&mut client)?;
    let id = json!("definition-bar");
    let response = client.request(
        id.clone(),
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": {"line": 3, "character": 6}}),
        timeout(),
    )?;
    assert_response_id(&response, &id)?;
    let locations = response
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("callable must return definition locations: {response}"))?;
    ensure!(locations.len() == 1, "expected one definition of Foo::bar: {response}");
    let location = locations.first().ok_or_else(|| anyhow::anyhow!("missing bar location"))?;
    ensure!(location.get("uri") == Some(&json!(uri)), "wrong definition document: {response}");
    ensure!(
        location.pointer("/range/start/line") == Some(&json!(1)),
        "definition must point to the bar declaration on line 1: {response}"
    );
    shutdown_and_exit(&mut client, json!("shutdown-definition-location"))
}
