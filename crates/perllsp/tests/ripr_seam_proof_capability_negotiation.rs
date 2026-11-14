//! Real-process capability-negotiation matrix.
//!
//! This suite exercises sparse, explicit-false, supported, and malformed
//! capability declarations through Cargo's exact public `perllsp` candidate. It
//! focuses on initialize-time shape selection and registration-request emission;
//! activation and rollback outcomes remain owned by #6724. The required
//! `lsp_smoke` gate includes this file from `semantic_definition.rs`.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, ensure};
use perl_lsp::features::policy::FeatureProfile;
use perl_lsp::protocol::capabilities::{BuildFlags, capabilities_json};
use perl_lsp::protocol::effective_surface::{
    CapabilityFamily, EffectiveLspSurface, FamilyOutcome, SuppressionReason, SurfaceInputs,
};
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
fn workspace_folder_server_support_is_client_independent() -> Result<()> {
    // #8161: `workspaceFolders.supported` describes whether the server
    // implements workspace-folder semantics, so the exact process must
    // advertise `true` for every client shape. The client's own bit (absent,
    // explicit false, malformed, or supported) is a client limitation and
    // never rewrites server truth.
    let cases = [
        (json!({}), "absent"),
        (json!({ "workspace": { "workspaceFolders": false } }), "explicit false"),
        (json!({ "workspace": { "workspaceFolders": "true" } }), "malformed string"),
        (json!({ "workspace": { "workspaceFolders": true } }), "supported true"),
    ];

    for (capabilities, label) in cases {
        let response = initialize_once("capability-matrix", capabilities, None)?;
        ensure!(
            response.pointer("/result/capabilities/workspace/workspaceFolders/supported")
                == Some(&json!(true)),
            "server workspaceFolders.supported must stay true for client shape ({label}): \
             {response}"
        );
        ensure!(
            response.pointer("/result/capabilities/workspace/workspaceFolders/changeNotifications")
                == Some(&json!(true)),
            "changeNotifications must agree with the compiled-in dispatch route ({label}): \
             {response}"
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
    // A client that never declares `textDocument.inlineCompletion` cannot
    // consume either the standard provider or the custom stream, so the
    // initialize result must omit both rather than advertise a surface the
    // negotiated client has no way to use (#7682).
    let cases = [
        (json!({}), false, false, "absent inline-completion declaration"),
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
                "clients that negotiate dynamic registration or declare no inline-completion support must not receive a static inline-completion provider for {label}: {response}"
            );
        }

        // The custom stream extension is only consumable alongside the standard
        // capability, so it must track the client's declaration rather than the
        // build feature alone.
        let stream_provider =
            response.pointer("/result/capabilities/experimental/perlInlineCompletionStream");
        if expect_static || expect_dynamic_request {
            ensure!(
                stream_provider.is_some(),
                "custom inline-completion stream must be advertised to declaring clients for {label}: {response}"
            );
        } else {
            ensure!(
                stream_provider.is_none(),
                "custom inline-completion stream must not be advertised to clients that omit textDocument.inlineCompletion for {label}: {response}"
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
fn workspace_symbol_disabled_parent_withdraws_resolve_advertisement() -> Result<()> {
    for options in [
        json!({ "disabledFeatures": ["lsp.workspace_symbol"] }),
        json!({ "perl-lsp": { "disabledFeatures": ["lsp.workspace_symbol"] } }),
        json!({ "perl_lsp": { "disabledFeatures": ["lsp.workspace_symbol"] } }),
    ] {
        let mut client = RealProcessClient::spawn_exact()?;
        let response = initialize(&mut client, "capability-matrix", json!({}), Some(options))?;
        let id = json!("disabled-workspace-symbol");
        let symbols =
            client.request(id.clone(), "workspace/symbol", json!({ "query": "" }), timeout())?;
        ensure!(symbols.get("id") == Some(&id), "workspace-symbol ID mismatch: {symbols}");
        ensure!(
            symbols.pointer("/error/code") == Some(&json!(-32601)),
            "disabled workspace-symbol route must return MethodNotFound: {symbols}"
        );
        ensure!(
            response.pointer("/result/capabilities/workspaceSymbolProvider").is_none(),
            "disabled workspace-symbol parent remained advertised: {response}"
        );
        // #18464: resolve enriches items from the parent request; with the
        // parent withdrawn it must refuse with the same MethodNotFound shape
        // instead of executing the un-advertised feature.
        let resolve_id = json!("disabled-workspace-symbol-resolve");
        let resolve = client.request(
            resolve_id.clone(),
            "workspace/symbol/resolve",
            json!({
                "name": "main",
                "kind": 12,
                "location": { "uri": "file:///not-open.pl", "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 4 }
                } }
            }),
            timeout(),
        )?;
        ensure!(resolve.get("id") == Some(&resolve_id), "resolve ID mismatch: {resolve}");
        ensure!(
            resolve.pointer("/error/code") == Some(&json!(-32601)),
            "resolve must refuse while the workspace-symbol parent is withdrawn: {resolve}"
        );
        finish(&mut client)?;
    }
    Ok(())
}

#[test]
fn workspace_symbol_resolve_disable_id_selects_simple_true_advertisement() -> Result<()> {
    // #18464: `lsp.workspace_symbol_resolve` is a cataloged, suppressible ID.
    // Disabling it must downgrade the advertisement to the simple-true shape
    // the builder already implements while the parent route stays live.
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(
        &mut client,
        "capability-matrix",
        json!({}),
        Some(json!({ "disabledFeatures": ["lsp.workspace_symbol_resolve"] })),
    )?;
    ensure!(
        response.pointer("/result/capabilities/workspaceSymbolProvider") == Some(&json!(true)),
        "resolve-off advertisement must be the simple-true shape: {response}"
    );

    // The parent flat query stays enabled and must keep answering.
    let id = json!("resolve-off-flat-query");
    let symbols =
        client.request(id.clone(), "workspace/symbol", json!({ "query": "" }), timeout())?;
    assert_success_response(&symbols, &id, "workspace/symbol under resolve-off")?;

    // Resolve itself still executes: the simple-true shape means the client
    // will not call resolve, but the runtime lever must not disable the route
    // (only `lsp.workspace_symbol` withdraws the parent).
    let resolve_id = json!("resolve-off-resolve");
    let resolve = client.request(
        resolve_id.clone(),
        "workspace/symbol/resolve",
        json!({
            "name": "main",
            "kind": 12,
            "location": { "uri": "file:///not-open.pl", "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 4 }
            } }
        }),
        timeout(),
    )?;
    assert_success_response(&resolve, &resolve_id, "resolve under resolve-off")?;
    ensure!(
        resolve.pointer("/error").is_none(),
        "resolve must stay live under lsp.workspace_symbol_resolve: {resolve}"
    );

    finish(&mut client)
}

#[test]
fn workspace_symbol_enabled_parent_retains_resolve_advertisement() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = initialize(&mut client, "capability-matrix", json!({}), None)?;
    ensure!(
        response.pointer("/result/capabilities/workspaceSymbolProvider/resolveProvider")
            == Some(&json!(true)),
        "enabled workspace-symbol parent lost resolve advertisement: {response}"
    );
    let id = json!("enabled-workspace-symbol");
    let symbols =
        client.request(id.clone(), "workspace/symbol", json!({ "query": "" }), timeout())?;
    assert_success_response(&symbols, &id, "workspace/symbol")?;
    ensure!(
        symbols.get("result").is_some_and(Value::is_array),
        "enabled workspace-symbol route must return its result array: {symbols}"
    );
    finish(&mut client)
}

#[test]
fn workspace_symbol_builder_requires_parent_for_every_resolve_flag() {
    for (parent, resolve, expected) in [
        (false, false, None),
        (false, true, None),
        (true, false, Some(json!(true))),
        (true, true, Some(json!({ "resolveProvider": true }))),
    ] {
        let mut flags = BuildFlags::all();
        flags.workspace_symbol = parent;
        flags.workspace_symbol_resolve = resolve;
        let capabilities = capabilities_json(flags);
        assert_eq!(
            capabilities.get("workspaceSymbolProvider"),
            expected.as_ref(),
            "workspace-symbol advertisement for parent={parent}, resolve={resolve}"
        );
    }
}

#[test]
fn workspace_symbol_model_follows_parent_suppression() -> Result<()> {
    for profile in [FeatureProfile::GaLock, FeatureProfile::Production, FeatureProfile::All] {
        for disabled in [false, true] {
            let mut inputs = SurfaceInputs::new_subject(profile);
            if disabled {
                inputs.disabled_feature_ids.insert("lsp.workspace_symbol".to_string());
            }
            let surface = EffectiveLspSurface::build(&inputs)
                .map_err(|error| anyhow::anyhow!("model construction refused: {error:?}"))?;
            let expected = (!disabled).then(|| json!({ "resolveProvider": true }));
            assert_eq!(
                surface.server_capabilities.get("workspaceSymbolProvider"),
                expected.as_ref(),
                "workspace-symbol model advertisement for {profile:?}, disabled={disabled}"
            );
            let expected_family = if disabled {
                FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
                    feature_id: "lsp.workspace_symbol".to_string(),
                })
            } else {
                FamilyOutcome::Static
            };
            assert_eq!(
                surface.families.get(&CapabilityFamily::WorkspaceSymbol),
                Some(&expected_family),
                "workspace-symbol model disposition for {profile:?}, disabled={disabled}"
            );
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
