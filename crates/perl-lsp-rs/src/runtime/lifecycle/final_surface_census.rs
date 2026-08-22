//! Final-surface census proof against the exact process-emitted surface
//! (#9662, #8032 train stage S01).
//!
//! The ledger lives in `perl-lsp-rs-core::protocol::
//! final_surface_inventory`; these tests drive `handle_initialize` for
//! representative client shapes and enforce that:
//!
//! 1. every pointer in every emitted initialize response is covered by a
//!    ledger row (unregistered capability fields fail);
//! 2. every pointer added beyond the static-builder census is owned by a
//!    mutation row (hidden post-hoc JSON mutations fail);
//! 3. the known mutation sites, dynamic registrations, refresh requests,
//!    compatibility branches and suppression arms are represented and
//!    behave exactly as their rows claim.
//!
//! Test-only module; no production behavior change.
//!
//! Assertions here intentionally use `expect`/`unwrap_or_else(panic!)`;
//! the production bans do not apply to this cfg(test) module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::super::LspServer;
use super::capabilities::apply_disabled_feature_id;
use perl_lsp_rs_core::features::flags::BuildFlags;
use perl_lsp_rs_core::protocol::final_surface_inventory::{
    SurfaceKind, census_pointer_union, covered_final_surface_pointers, final_surface_rows,
    flatten_surface_pointers, ids,
};

/// Representative initialize-params shapes exercising every runtime branch
/// relevant to the final surface.
fn representative_client_shapes() -> Vec<(&'static str, Value)> {
    vec![
        ("minimal", json!({})),
        (
            "maximal-static",
            json!({
                "clientInfo": {"name": "vscode"},
                "workspaceFolders": [{"uri": "file:///repo"}],
                "capabilities": {
                    "workspace": {
                        "workspaceFolders": true,
                        "fileOperations": {
                            "willCreate": true,
                            "didCreate": true,
                            "willRename": true,
                            "didRename": true,
                            "willDelete": true,
                            "didDelete": true
                        }
                    },
                    "textDocument": {
                        "inlineCompletion": {}
                    }
                }
            }),
        ),
        (
            "inline-dynamic",
            json!({
                "clientInfo": {"name": "neovim"},
                "capabilities": {
                    "workspace": {"fileOperations": {"willRename": true}},
                    "textDocument": {
                        "inlineCompletion": {"dynamicRegistration": true}
                    }
                }
            }),
        ),
        (
            "code-action-doc",
            json!({
                "clientInfo": {"name": "helix"},
                "capabilities": {
                    "textDocument": {
                        "codeAction": {"documentationSupport": true},
                        "diagnostic": {}
                    }
                }
            }),
        ),
        (
            "jetbrains",
            json!({
                "clientInfo": {"name": "IntelliJ IDEA"},
                "capabilities": {
                    "workspace": {
                        "didChangeWatchedFiles": {"dynamicRegistration": true}
                    },
                    "textDocument": {"inlineCompletion": {}}
                }
            }),
        ),
        (
            "opencode",
            json!({
                "clientInfo": {"name": "OpenCode"},
                "capabilities": {
                    "textDocument": {"diagnostic": {}, "inlineCompletion": {}}
                }
            }),
        ),
    ]
}

fn emitted_capabilities(server: &LspServer, shape: &Value) -> Value {
    let response = server.handle_initialize(Some(shape.clone())).expect("initialize must succeed");
    let result = response.expect("initialize must return a result");
    result.get("capabilities").cloned().expect("initialize result must carry serverCapabilities")
}

#[test]
fn every_emitted_surface_pointer_is_ledgered() {
    let covered = covered_final_surface_pointers();
    for (shape_name, params) in representative_client_shapes() {
        let server = LspServer::new();
        let capabilities = emitted_capabilities(&server, &params);
        let pointers = flatten_surface_pointers(&capabilities);
        let unledgered: Vec<&String> =
            pointers.iter().filter(|pointer| !covered.contains(*pointer)).collect();
        assert!(
            unledgered.is_empty(),
            "shape {shape_name} emitted final-surface pointers with no inventory row: {unledgered:?}"
        );
    }
}

#[test]
fn runtime_added_pointers_are_owned_by_mutation_rows() {
    let static_census = census_pointer_union();
    let rows = final_surface_rows();
    let mutation_owned: BTreeSet<String> = rows
        .iter()
        .filter(|row| row.kind == SurfaceKind::Mutation)
        .flat_map(|row| {
            let mut owned = vec![row.protocol_field.to_string()];
            owned
                .extend(row.additional_owned_pointers.iter().map(|pointer| (*pointer).to_string()));
            owned
        })
        .collect();

    let mut runtime_added = BTreeSet::new();
    for (_, params) in representative_client_shapes() {
        let server = LspServer::new();
        let pointers = flatten_surface_pointers(&emitted_capabilities(&server, &params));
        for pointer in pointers {
            if !static_census.contains(&pointer) {
                runtime_added.insert(pointer);
            }
        }
    }

    let unowned: Vec<&String> =
        runtime_added.iter().filter(|pointer| !mutation_owned.contains(*pointer)).collect();
    assert!(
        unowned.is_empty(),
        "runtime-added pointers not owned by any mutation row (hidden JSON mutation): {unowned:?}"
    );

    // Completeness of exercise: each structural mutation-owned pointer must
    // actually appear under some representative shape.
    let required = [
        "positionEncoding",
        "textDocumentSync.willSave",
        "textDocumentSync.willSaveWaitUntil",
        "textDocumentSync.save.includeText",
        "workspace.workspaceFolders.supported",
        "workspace.workspaceFolders.changeNotifications",
        "workspace.textDocumentContent.schemes[]",
        "workspace.fileOperations.willRename.filters[]",
        "workspace.fileOperations.willRename.filters[].pattern.glob",
        "codeActionProvider.documentation[]",
        "experimental.perlInlineCompletionStream",
    ];
    let missing: Vec<&str> =
        required.iter().copied().filter(|p| !runtime_added.contains(*p)).collect();
    assert!(
        missing.is_empty(),
        "mutation-owned pointers never exercised by representative shapes: {missing:?}"
    );
}

#[test]
fn inline_completion_tri_state_matches_its_row() {
    let static_only = LspServer::new();
    let static_caps = emitted_capabilities(
        &static_only,
        &representative_client_shapes()
            .into_iter()
            .find(|(name, _)| *name == "maximal-static")
            .expect("maximal-static shape exists")
            .1,
    );
    assert!(
        static_caps.get("inlineCompletionProvider").is_some(),
        "static-only inline clients must keep the statically advertised provider ({})",
        ids::CAP_INLINE_COMPLETION_PROVIDER
    );

    let dynamic = LspServer::new();
    let dynamic_caps = emitted_capabilities(
        &dynamic,
        &representative_client_shapes()
            .into_iter()
            .find(|(name, _)| *name == "inline-dynamic")
            .expect("inline-dynamic shape exists")
            .1,
    );
    assert!(
        dynamic_caps.get("inlineCompletionProvider").is_none(),
        "dynamically registered inline completion must remove the static provider ({})",
        ids::MUT_INLINE_COMPLETION_TRI_STATE
    );
}

#[test]
fn registrations_refreshes_and_compat_rows_are_ledgered() {
    let rows = final_surface_rows();
    let find = |surface_id: &str| {
        rows.iter()
            .find(|row| row.surface_id == surface_id)
            .unwrap_or_else(|| panic!("ledger row {surface_id} missing"))
    };

    let watchers = find(ids::REG_DID_CHANGE_WATCHED_FILES);
    assert!(watchers.protocol_field.contains("workspace/didChangeWatchedFiles"));
    assert!(watchers.protocol_field.contains("perl-didChangeWatchedFiles"));
    assert_eq!(
        watchers.disposition,
        perl_lsp_rs_core::protocol::final_surface_inventory::Disposition::Dynamic
    );

    let inline = find(ids::REG_INLINE_COMPLETION);
    assert!(inline.protocol_field.contains("textDocument/inlineCompletion"));

    for refresh_method in [
        "workspace/codeLens/refresh",
        "workspace/semanticTokens/refresh",
        "workspace/inlayHint/refresh",
        "workspace/inlineValue/refresh",
        "workspace/diagnostic/refresh",
        "workspace/foldingRange/refresh",
    ] {
        assert!(
            rows.iter().any(|row| row.protocol_field == refresh_method),
            "refresh request {refresh_method} has no inventory row"
        );
    }

    // Compatibility branches behave exactly as their rows claim.
    let jetbrains = LspServer::new();
    let params = representative_client_shapes()
        .into_iter()
        .find(|(name, _)| *name == "jetbrains")
        .expect("jetbrains shape exists")
        .1;
    let _ = jetbrains.handle_initialize(Some(params));
    assert!(
        !jetbrains.client_capabilities.lock().dynamic_registration_support,
        "compat row compat.client.jetbrains.watcherForceDisable: dynamic registration must be forced off"
    );
    assert!(
        jetbrains.pending_startup_log.lock().is_some(),
        "compat row compat.client.jetbrains.watcherForceDisable: override logMessage must be queued"
    );
}

#[test]
fn suppression_rows_flip_exactly_their_claimed_build_flag() {
    fn flag_value(flags: &BuildFlags, name: &str) -> Option<bool> {
        Some(match name {
            "completion" => flags.completion,
            "hover" => flags.hover,
            "definition" => flags.definition,
            "type_definition" => flags.type_definition,
            "implementation" => flags.implementation,
            "references" => flags.references,
            "document_symbol" => flags.document_symbol,
            "workspace_symbol" => flags.workspace_symbol,
            "inlay_hints" => flags.inlay_hints,
            "pull_diagnostics" => flags.pull_diagnostics,
            "workspace_symbol_resolve" => flags.workspace_symbol_resolve,
            "semantic_tokens" => flags.semantic_tokens,
            "code_actions" => flags.code_actions,
            "execute_command" => flags.execute_command,
            "rename" => flags.rename,
            "document_links" => flags.document_links,
            "selection_ranges" => flags.selection_ranges,
            "on_type_formatting" => flags.on_type_formatting,
            "code_lens" => flags.code_lens,
            "call_hierarchy" => flags.call_hierarchy,
            "type_hierarchy" => flags.type_hierarchy,
            "linked_editing" => flags.linked_editing,
            "inline_completion" => flags.inline_completion,
            "inline_values" => flags.inline_values,
            "notebook_document_sync" => flags.notebook_document_sync,
            "notebook_cell_execution" => flags.notebook_cell_execution,
            "moniker" => flags.moniker,
            "document_color" => flags.document_color,
            "formatting" => flags.formatting,
            "range_formatting" => flags.range_formatting,
            "folding_range" => flags.folding_range,
            "signature_help" => flags.signature_help,
            "document_highlight" => flags.document_highlight,
            "declaration" => flags.declaration,
            _ => return None,
        })
    }

    const ALL_FLAG_NAMES: &[&str] = &[
        "completion",
        "hover",
        "definition",
        "type_definition",
        "implementation",
        "references",
        "document_symbol",
        "workspace_symbol",
        "inlay_hints",
        "pull_diagnostics",
        "workspace_symbol_resolve",
        "semantic_tokens",
        "code_actions",
        "execute_command",
        "rename",
        "document_links",
        "selection_ranges",
        "on_type_formatting",
        "code_lens",
        "call_hierarchy",
        "type_hierarchy",
        "linked_editing",
        "inline_completion",
        "inline_values",
        "notebook_document_sync",
        "notebook_cell_execution",
        "moniker",
        "document_color",
        "formatting",
        "range_formatting",
        "folding_range",
        "signature_help",
        "document_highlight",
        "declaration",
    ];

    let prefix = "initializationOptions.disabledFeatures:";
    for row in final_surface_rows() {
        let Some(effect) = row.build_flag_effect else {
            continue;
        };
        let Some(feature_id) = row.protocol_field.strip_prefix(prefix) else {
            continue;
        };

        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, feature_id);

        let before_all_true =
            ALL_FLAG_NAMES.iter().all(|name| flag_value(&BuildFlags::all(), name) == Some(true));
        assert!(before_all_true, "BuildFlags::all() baseline changed; update flag list");

        for name in ALL_FLAG_NAMES {
            let expected = if *name == effect.flag { Some(false) } else { Some(true) };
            assert_eq!(
                flag_value(&flags, name),
                expected,
                "suppression row {} claims flag {} but applying {feature_id} diverged on {name}",
                row.surface_id,
                effect.flag
            );
        }
    }
}
