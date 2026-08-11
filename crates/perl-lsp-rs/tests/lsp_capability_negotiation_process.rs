//! Real-process capability-negotiation matrix.
//!
//! This suite exercises sparse, explicit-false, supported, and malformed
//! capability declarations through Cargo's exact `perl-lsp` candidate. It
//! focuses on initialize-time shape selection and registration-request emission;
//! activation and rollback outcomes remain owned by #6724. The required
//! `lsp_smoke` gate includes this file from `semantic_definition.rs`.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn assert_success_response(response: &Value, id: &Value, context: &str) -> Result<()> {
    ensure!(
        response.get("jsonrpc") == Some(&json!("2.0")),
        "{context} response omitted JSON-RPC 2.0: {response}"
    );
    ensure!(response.get("id") == Some(id), "{context} response ID mismatch: {response}");
    ensure!(
        response.get("error").is_none(),
        "{context} response unexpectedly contained an error: {response}"
    );
    ensure!(response.get("result").is_some(), "{context} response omitted result: {response}");
    Ok(())
}

fn initialize(
    client: &mut RealProcessClient,
    client_name: &str,
    capabilities: Value,
    initialization_options: Option<Value>,
) -> Result<Value> {
    let mut params = json!({
        "processId": null,
        "clientInfo": {
            "name": client_name,
            "version": "1"
        },
        "rootUri": null,
        "capabilities": capabilities,
        "workspaceFolders": null
    });
    if let Some(options) = initialization_options {
        params["initializationOptions"] = options;
    }

    let id = json!("initialize");
    let response = client.request(id.clone(), "initialize", params, timeout())?;
    assert_success_response(&response, &id, "initialize")?;
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize result omitted capabilities: {response}"
    );
    client.notify("initialized", json!({}))?;
    Ok(response)
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let id = json!("shutdown");
    let response = client.request(id.clone(), "shutdown", Value::Null, timeout())?;
    assert_success_response(&response, &id, "shutdown")?;
    ensure!(
        response.get("result").is_some_and(Value::is_null),
        "shutdown must return null: {response}"
    );
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "candidate exited unsuccessfully: {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

fn initialize_once(
    client_name: &str,
    capabilities: Value,
    initialization_options: Option<Value>,
) -> Result<Value> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(&mut client, client_name, capabilities, initialization_options)?;
    finish(&mut client)?;
    Ok(response)
}

#[test]
fn workspace_folder_support_requires_boolean_true() -> Result<()> {
    let cases = [
        (json!({}), false, "absent"),
        (json!({ "workspace": { "workspaceFolders": false } }), false, "explicit false"),
        (json!({ "workspace": { "workspaceFolders": "true" } }), false, "malformed string"),
        (json!({ "workspace": { "workspaceFolders": true } }), true, "supported true"),
    ];

    for (capabilities, expected, label) in cases {
        let response = initialize_once("capability-matrix", capabilities, None)?;
        ensure!(
            response.pointer("/result/capabilities/workspace/workspaceFolders/supported")
                == Some(&json!(expected)),
            "workspaceFolders negotiation mismatch for {label}: {response}"
        );
    }
    Ok(())
}

#[test]
fn inline_completion_initialize_shape_selects_static_or_dynamic_mode() -> Result<()> {
    // This proves the initialize shape and exact dynamic request emission. The
    // success reply below exists only to leave the process terminally clean;
    // #6724 owns whether success activates durable registration state and how
    // failure, timeout, retry, or unregistration changes that state.
    let cases = [
        (
            json!({
                "textDocument": {
                    "inlineCompletion": { "dynamicRegistration": true }
                }
            }),
            false,
            true,
            "dynamic true",
        ),
        (
            json!({
                "textDocument": {
                    "inlineCompletion": { "dynamicRegistration": false }
                }
            }),
            true,
            false,
            "dynamic false",
        ),
        (
            json!({
                "textDocument": {
                    "inlineCompletion": { "dynamicRegistration": "true" }
                }
            }),
            true,
            false,
            "malformed dynamic flag",
        ),
    ];

    for (capabilities, expect_static, expect_dynamic_request, label) in cases {
        let mut client = RealProcessClient::spawn_exact()?;
        let response = initialize(&mut client, "capability-matrix", capabilities, None)?;
        let static_provider = response.pointer("/result/capabilities/inlineCompletionProvider");
        if expect_static {
            ensure!(
                static_provider == Some(&json!({})),
                "static inline-completion provider must be exactly an empty object for {label}: {response}"
            );
        } else {
            ensure!(
                static_provider.is_none(),
                "dynamic clients must not receive a static inline-completion provider for {label}: {response}"
            );
        }

        if expect_dynamic_request {
            let registration =
                client.receive_server_request("client/registerCapability", timeout())?;
            let request_id = registration.get("id").cloned().ok_or_else(|| {
                anyhow::anyhow!("registration request omitted id: {registration}")
            })?;
            let registrations = registration
                .pointer("/params/registrations")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    anyhow::anyhow!("registration request omitted registrations: {registration}")
                })?;
            let inline_registrations = registrations
                .iter()
                .filter(|entry| {
                    entry.get("method").and_then(Value::as_str)
                        == Some("textDocument/inlineCompletion")
                })
                .collect::<Vec<_>>();
            ensure!(
                inline_registrations.len() == 1,
                "expected exactly one inline-completion registration for {label}, got {}: {registration}",
                inline_registrations.len()
            );

            let inline_registration = inline_registrations[0];
            ensure!(
                inline_registration.get("id") == Some(&json!("perl-inlineCompletion")),
                "inline-completion registration ID drifted for {label}: {inline_registration}"
            );
            ensure!(
                inline_registration.pointer("/registerOptions/documentSelector")
                    == Some(&json!([
                        { "language": "perl" },
                        { "language": "perl5" }
                    ])),
                "inline-completion document selector drifted for {label}: {inline_registration}"
            );

            let mut registration_ids = std::collections::BTreeSet::new();
            for entry in registrations {
                let id = entry.get("id").and_then(Value::as_str).ok_or_else(|| {
                    anyhow::anyhow!("dynamic registration omitted string id: {entry}")
                })?;
                ensure!(!id.is_empty(), "dynamic registration id must not be empty: {entry}");
                ensure!(
                    registration_ids.insert(id),
                    "duplicate dynamic registration id {id:?}: {registration}"
                );
            }
            client.respond(request_id, Value::Null)?;
        }

        finish(&mut client)?;
    }
    Ok(())
}

#[test]
fn code_action_documentation_requires_boolean_true() -> Result<()> {
    let cases = [
        (
            json!({
                "textDocument": {
                    "codeAction": { "documentationSupport": true }
                }
            }),
            true,
            "supported true",
        ),
        (
            json!({
                "textDocument": {
                    "codeAction": { "documentationSupport": false }
                }
            }),
            false,
            "explicit false",
        ),
        (
            json!({
                "textDocument": {
                    "codeAction": { "documentationSupport": ["true"] }
                }
            }),
            false,
            "malformed array",
        ),
    ];

    for (capabilities, expected, label) in cases {
        let response = initialize_once("capability-matrix", capabilities, None)?;
        let documentation = response
            .pointer("/result/capabilities/codeActionProvider/documentation")
            .and_then(Value::as_array);
        ensure!(
            documentation.is_some() == expected,
            "code-action documentation negotiation mismatch for {label}: {response}"
        );
        if let Some(entries) = documentation {
            ensure!(!entries.is_empty(), "advertised documentation must have entries");
        }
    }
    Ok(())
}

#[test]
fn disabled_feature_removes_capability_and_rejects_route() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(
        &mut client,
        "capability-matrix",
        json!({}),
        Some(json!({
            "disabledFeatures": ["lsp.hover"]
        })),
    )?;
    ensure!(
        response.pointer("/result/capabilities/hoverProvider").is_none(),
        "disabled hover remained advertised: {response}"
    );

    let hover_id = json!("disabled-hover");
    let hover = client.request(
        hover_id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///not-open.pl" },
            "position": { "line": 0, "character": 0 }
        }),
        timeout(),
    )?;
    ensure!(
        hover.get("jsonrpc") == Some(&json!("2.0")),
        "hover response omitted JSON-RPC 2.0: {hover}"
    );
    ensure!(hover.get("id") == Some(&hover_id), "hover ID mismatch: {hover}");
    ensure!(
        hover.pointer("/error/code") == Some(&json!(-32601)),
        "disabled hover route must return MethodNotFound: {hover}"
    );

    finish(&mut client)
}

#[test]
fn malformed_disabled_feature_list_leaves_hover_route_enabled() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(
        &mut client,
        "capability-matrix",
        json!({}),
        Some(json!({
            "disabledFeatures": "lsp.hover"
        })),
    )?;
    ensure!(
        response.pointer("/result/capabilities/hoverProvider").is_some(),
        "malformed disabledFeatures value changed advertised feature state: {response}"
    );

    let hover_id = json!("malformed-disabled-hover");
    let hover = client.request(
        hover_id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///not-open.pl" },
            "position": { "line": 0, "character": 0 }
        }),
        timeout(),
    )?;
    ensure!(
        hover.get("jsonrpc") == Some(&json!("2.0")),
        "hover response omitted JSON-RPC 2.0: {hover}"
    );
    ensure!(hover.get("id") == Some(&hover_id), "hover ID mismatch: {hover}");
    ensure!(
        hover.pointer("/error/code") != Some(&json!(-32601)),
        "malformed disabledFeatures value removed the live hover route: {hover}"
    );

    finish(&mut client)
}
