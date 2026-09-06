//! Parity between the shipped initialize path and the typed
//! [`EffectiveLspSurface`] authority (#9665, train #8032 stage S02).
//!
//! These tests are the S03 migration evidence: every subject proves the model
//! independently derives the exact final surface the runtime emits, and pins
//! the runtime suppression twin to the canonical model table. No production
//! behavior changes here.
//!
//! The params→inputs mapping in this module is deliberately test-owned: it
//! re-derives typed facts from the wire shape using the same rules as the
//! runtime parser. If either side drifts, a parity assertion fails — that is
//! the discrimination working.

#![cfg(test)]
use super::super::{LspServer, json};
use super::capabilities::{apply_disabled_feature_id, disabled_feature_ids_from_init_options};
use perl_lsp_rs_core::features::policy::FeatureProfile;
use perl_lsp_rs_core::protocol::capabilities::{BuildFlags, get_supported_commands};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use perl_lsp_rs_core::protocol::effective_surface::{
    CapabilityFamily, ClientFact, EffectiveLspSurface, FamilyOutcome, FileOperationFacts,
    KnownException, PositionEncoding, RefreshSupportFacts, RuntimeAvailability, SurfaceInputs,
    apply_disabled_feature_id_model,
};

/// Map a JSON boolean capability pointer to its normalized fact class,
/// mirroring the runtime parser's wire rules (`as_bool` semantics).
fn fact_at(params: &Value, pointer: &str) -> ClientFact {
    match params.pointer(pointer) {
        None => ClientFact::Absent,
        Some(Value::Bool(true)) => ClientFact::Supported,
        Some(Value::Bool(false)) => ClientFact::DeclaredFalse,
        Some(_) => ClientFact::Malformed,
    }
}

/// Presence-only selectors admit any present payload.
fn presence_at(params: &Value, pointer: &str) -> ClientFact {
    if params.pointer(pointer).is_some() { ClientFact::Supported } else { ClientFact::Absent }
}

/// Model input for the wire encoding, derived from the server's ACCEPTED
/// text-sync session contract (#9378) — never re-parsed from the raw params.
/// Re-parsing here would create a second, free-standing negotiation seam that
/// can disagree with the stored session the response was built from.
fn accepted_encoding_from(server: &LspServer) -> Option<PositionEncoding> {
    server.accepted_text_sync_session().map(|session| {
        match session.contract().position_encoding() {
            super::session_contract::AcceptedPositionEncoding::Utf16 => PositionEncoding::Utf16,
        }
    })
}

/// Build model inputs from initialize params exactly as the runtime consumes
/// them (test-owned adapter; no production parsing moved).
fn inputs_from_params(params: &Value) -> SurfaceInputs {
    let mut inputs = SurfaceInputs::new_subject(FeatureProfile::current());
    if let Some(init_opts) = params.get("initializationOptions") {
        for id in disabled_feature_ids_from_init_options(init_opts) {
            inputs.disabled_feature_ids.insert(id.to_string());
        }
    }
    let client_name = params
        .pointer("/clientInfo/name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if client_name.contains("opencode")
        && params.pointer("/capabilities/textDocument/diagnostic").is_some()
    {
        inputs.compatibility_exceptions.insert(KnownException::OpenCodePushDiagnosticsRetention);
    }
    if ["jetbrains", "intellij", "idea"].iter().any(|marker| client_name.contains(marker)) {
        inputs.compatibility_exceptions.insert(KnownException::JetBrainsWatcherForceDisable);
    }

    let diagnostic_refresh = {
        let plural = fact_at(params, "/capabilities/workspace/diagnostics/refreshSupport");
        let singular = fact_at(params, "/capabilities/workspace/diagnostic/refreshSupport");
        if plural == ClientFact::Absent { singular } else { plural }
    };

    let client = &mut inputs.client;
    client.inline_completion = presence_at(params, "/capabilities/textDocument/inlineCompletion");
    client.inline_completion_dynamic_registration =
        fact_at(params, "/capabilities/textDocument/inlineCompletion/dynamicRegistration");
    client.dynamic_file_watcher_registration =
        fact_at(params, "/capabilities/workspace/didChangeWatchedFiles/dynamicRegistration");
    client.file_watcher_relative_pattern =
        fact_at(params, "/capabilities/workspace/didChangeWatchedFiles/relativePatternSupport");
    client.code_action_documentation =
        fact_at(params, "/capabilities/textDocument/codeAction/documentationSupport");
    client.workspace_folders = fact_at(params, "/capabilities/workspace/workspaceFolders");
    client.diagnostic_pull = presence_at(params, "/capabilities/textDocument/diagnostic");
    client.file_operations = {
        let mut operations = FileOperationFacts::default();
        operations.will_create =
            fact_at(params, "/capabilities/workspace/fileOperations/willCreate");
        operations.did_create = fact_at(params, "/capabilities/workspace/fileOperations/didCreate");
        operations.will_rename =
            fact_at(params, "/capabilities/workspace/fileOperations/willRename");
        operations.did_rename = fact_at(params, "/capabilities/workspace/fileOperations/didRename");
        operations.will_delete =
            fact_at(params, "/capabilities/workspace/fileOperations/willDelete");
        operations.did_delete = fact_at(params, "/capabilities/workspace/fileOperations/didDelete");
        operations
    };
    client.refresh_supports = {
        let mut refreshes = RefreshSupportFacts::default();
        refreshes.code_lens = fact_at(params, "/capabilities/workspace/codeLens/refreshSupport");
        refreshes.semantic_tokens =
            fact_at(params, "/capabilities/workspace/semanticTokens/refreshSupport");
        refreshes.inlay_hint = fact_at(params, "/capabilities/workspace/inlayHint/refreshSupport");
        refreshes.inline_value =
            fact_at(params, "/capabilities/workspace/inlineValue/refreshSupport");
        refreshes.diagnostic = diagnostic_refresh;
        refreshes.folding_range =
            fact_at(params, "/capabilities/workspace/foldingRange/refreshSupport");
        refreshes
    };
    inputs.runtime = RuntimeAvailability::default();
    // Keep command descriptors canonical for every subject.
    inputs.command_ids = get_supported_commands();
    inputs
}

/// Core discriminator: the model must reproduce the EXACT emitted initialize
/// capabilities for the given subject, plus effective advertised identities.
fn assert_initialize_matches_model(params: Value) -> Result<EffectiveLspSurface, String> {
    let server = LspServer::new();
    let response = server
        .handle_initialize(Some(params.clone()))
        .map_err(|error| format!("initialize failed: {error}"))?
        .ok_or_else(|| "initialize returned no payload".to_string())?;
    let mut inputs = inputs_from_params(&params);
    inputs.client.negotiated_position_encoding = accepted_encoding_from(&server);
    let surface = EffectiveLspSurface::build(&inputs)
        .map_err(|error| format!("model refused subject: {error}"))?;

    assert_eq!(
        response.get("capabilities"),
        Some(&surface.server_capabilities),
        "model projection must equal the shipped initialize capabilities"
    );
    // The model derives identities from final family outcomes (#9665 item
    // 5); the shipped runtime still persists post-configuration flag IDs
    // (`capabilities.rs`). The one permitted live disagreement is the
    // inline-completion tri-state row (MUT_INLINE_COMPLETION_TRI_STATE,
    // #9662): without a client declaration the wire omits the provider while
    // the runtime ID list keeps it. S03 owns removing this twin delta; any
    // other divergence fails here.
    let runtime_ids: BTreeSet<&str> =
        server.advertised_feature_ids.lock().clone().into_iter().collect();
    let model_ids: BTreeSet<&str> = surface.advertised_feature_ids.iter().copied().collect();
    let disagreements: Vec<&str> = runtime_ids.symmetric_difference(&model_ids).copied().collect();
    assert!(
        disagreements.iter().all(|id| *id == "lsp.inline_completion"),
        "only the inline-completion tri-state twin may disagree with the \
         model's effective identities (S03 owns removal): {disagreements:?}"
    );
    Ok(surface)
}

// ---------------------------------------------------------------------------
// Suppression-table twin pinning
// ---------------------------------------------------------------------------

/// Whether suppressing `id` on `base` changes any flag.
fn suppression_has_effect(base: &BuildFlags, id: &str) -> bool {
    let mut suppressed = base.clone();
    apply_disabled_feature_id_model(&mut suppressed, id);
    suppressed != *base
}

#[test]
fn suppression_table_matches_the_canonical_model_table() {
    let mut ids: Vec<String> =
        BuildFlags::all().to_feature_ids().into_iter().map(str::to_string).collect();
    ids.push("lsp.ranges_formatting".to_string());
    assert!(ids.len() >= 20, "expected the full feature-ID denominator");
    for id in &ids {
        for base in [BuildFlags::ga_lock(), BuildFlags::production(), BuildFlags::all()] {
            let mut via_runtime = base.clone();
            apply_disabled_feature_id(&mut via_runtime, id);
            let mut via_model = base.clone();
            apply_disabled_feature_id_model(&mut via_model, id);
            assert_eq!(
                via_runtime, via_model,
                "runtime suppression diverges from the canonical model table for {id}"
            );
            // A no-op is legitimate when the profile already excluded the
            // feature (e.g. lsp.inline_value on production); require an
            // effect only where the flag started enabled.
            if suppression_has_effect(&base, id) {
                assert_ne!(via_runtime, base, "suppression id {id} must have an effect");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Full-subject parity against handle_initialize
// ---------------------------------------------------------------------------

#[test]
fn minimal_client_surface_matches() -> Result<(), String> {
    assert_initialize_matches_model(json!({ "capabilities": {} }))?;
    Ok(())
}

#[test]
fn vscode_like_client_surface_matches() -> Result<(), String> {
    assert_initialize_matches_model(json!({
        "clientInfo": { "name": "Visual Studio Code" },
        "capabilities": {
            "workspace": {
                "workspaceFolders": true,
                "didChangeWatchedFiles": {
                    "dynamicRegistration": true,
                    "relativePatternSupport": true
                },
                "fileOperations": { "willRename": true, "didRename": true },
                "codeLens": { "refreshSupport": true },
                "diagnostics": { "refreshSupport": true },
                "foldingRange": { "refreshSupport": true }
            },
            "textDocument": {
                "completion": { "completionItem": { "snippetSupport": true } },
                "codeAction": { "documentationSupport": true },
                "diagnostic": {}
            },
            "general": { "positionEncodings": ["utf-16"] }
        },
        "initializationOptions": { "disabledFeatures": ["lsp.moniker"] }
    }))?;
    Ok(())
}

#[test]
fn lsp4ij_like_dynamic_inline_completion_surface_matches() -> Result<(), String> {
    let surface = assert_initialize_matches_model(json!({
        "clientInfo": { "name": "LSP4IJ" },
        "capabilities": {
            "workspace": {
                "workspaceFolders": true,
                "didChangeWatchedFiles": { "dynamicRegistration": true }
            },
            "textDocument": {
                "inlineCompletion": { "dynamicRegistration": true }
            }
        }
    }))?;

    let methods: Vec<&str> =
        surface.registration_plan.registrations.iter().map(|plan| plan.method).collect();
    assert!(methods.contains(&"textDocument/inlineCompletion"));
    assert!(methods.contains(&"workspace/didChangeWatchedFiles"));
    assert!(
        surface.server_capabilities.get("inlineCompletionProvider").is_none(),
        "static provider withdrawn while the plan owns the selector"
    );
    let family = surface
        .families
        .get(&CapabilityFamily::InlineCompletion)
        .ok_or_else(|| "inline completion family missing".to_string())?;
    assert!(
        matches!(family, FamilyOutcome::Downgraded(_, _)),
        "inline completion downgraded to planned dynamic: {family:?}"
    );
    Ok(())
}

#[test]
fn opencode_push_retention_surface_matches() -> Result<(), String> {
    let surface = assert_initialize_matches_model(json!({
        "clientInfo": { "name": "opencode" },
        "capabilities": {
            "textDocument": { "diagnostic": {} }
        }
    }))?;
    assert_eq!(
        surface.diagnostic_transport,
        perl_lsp_rs_core::protocol::effective_surface::DiagnosticTransport::PushOnly(
            perl_lsp_rs_core::protocol::effective_surface::PushTransportReason::ClientCompatibility(
                KnownException::OpenCodePushDiagnosticsRetention
            ),
        ),
        "OpenCode keeps push publishing through the typed exception"
    );
    Ok(())
}

#[test]
fn jetbrains_watcher_override_surface_matches() -> Result<(), String> {
    let surface = assert_initialize_matches_model(json!({
        "clientInfo": { "name": "IntelliJ IDEA" },
        "capabilities": {
            "workspace": {
                "didChangeWatchedFiles": { "dynamicRegistration": true }
            }
        }
    }))?;
    assert!(
        surface.registration_plan.registrations.is_empty(),
        "JetBrains exception must suppress the watcher registration plan"
    );
    Ok(())
}

#[test]
fn malformed_unknown_future_and_sparse_facts_match_runtime_collapse() -> Result<(), String> {
    assert_initialize_matches_model(json!({
        "capabilities": {
            "workspace": {
                "workspaceFolders": false,
                "didChangeWatchedFiles": {
                    "dynamicRegistration": "yes",
                    "relativePatternSupport": null,
                    "unknownFutureKey": { "nested": true }
                },
                "fileOperations": { "willDelete": 1 }
            },
            "textDocument": {
                "inlineCompletion": { "futureVerb": true },
                "codeAction": { "documentationSupport": [] }
            }
        }
    }))?;
    Ok(())
}

#[test]
fn pull_diagnostic_client_with_refresh_supports_and_non_utf16_first_preference_matches()
-> Result<(), String> {
    // The v0.18 envelope (#8129 `full_document_utf16`, #9378) accepts this
    // subject only because the offer still contains utf-16; selection is
    // contract-owned UTF-16 regardless of offer order.
    assert_initialize_matches_model(json!({
        "clientInfo": { "name": "neovim" },
        "capabilities": {
            "workspace": {
                "semanticTokens": { "refreshSupport": true },
                "inlayHint": { "refreshSupport": true },
                "inlineValue": { "refreshSupport": true },
                "diagnostic": { "refreshSupport": true }
            },
            "textDocument": { "diagnostic": {} },
            "general": { "positionEncodings": ["utf-32", "utf-8", "utf-16"] }
        }
    }))?;
    Ok(())
}

#[test]
fn pull_diagnostic_client_with_no_common_offer_fails_initialize() -> Result<(), String> {
    // LSP-FS16-004: a client whose offer excludes utf-16 is rejected before
    // any state mutation, so there is no surface for the model to match.
    let server = LspServer::new();
    let error = server
        .handle_initialize(Some(json!({
            "clientInfo": { "name": "neovim" },
            "capabilities": {
                "textDocument": { "diagnostic": {} },
                "general": { "positionEncodings": ["utf-32", "utf-8"] }
            }
        })))
        .err()
        .ok_or("no-common offer must fail initialize")?;
    assert_eq!(error.code, -32602, "no-common offer must be typed InvalidParams");
    assert!(
        server.accepted_text_sync_session().is_none(),
        "rejected initialize must not publish a session contract"
    );
    Ok(())
}

#[test]
fn pull_gating_side_effect_agrees_with_transport_selection() -> Result<(), String> {
    let server = LspServer::new();
    let params = json!({
        "clientInfo": { "name": "vscode" },
        "capabilities": { "textDocument": { "diagnostic": {} } }
    });
    let _ = server.handle_initialize(Some(params.clone()));
    assert!(
        server.client_supports_pull_diags.load(Ordering::Relaxed),
        "non-opencode declaring clients enable pull gating"
    );
    let mut inputs = inputs_from_params(&params);
    inputs.client.negotiated_position_encoding = accepted_encoding_from(&server);
    let surface = EffectiveLspSurface::build(&inputs)
        .map_err(|error| format!("model refused subject: {error}"))?;
    assert_eq!(
        surface.diagnostic_transport,
        perl_lsp_rs_core::protocol::effective_surface::DiagnosticTransport::PullPreferred,
    );
    Ok(())
}
