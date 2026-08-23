//! Final-surface census proof against the exact process-emitted surface
//! (#9662, #8032 train stage S01).
//!
//! The checked-in generated artifact is the test-only handoff from the
//! crate-private core ledger; these tests drive `handle_initialize` for
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

const INVENTORY_ARTIFACT: &str =
    include_str!("../../../../../docs/specs/lsp-final-surface-inventory.json");

fn inventory_artifact() -> Value {
    serde_json::from_str(INVENTORY_ARTIFACT).expect("checked-in inventory artifact must be JSON")
}

fn inventory_rows() -> Vec<Value> {
    inventory_artifact()
        .get("rows")
        .and_then(Value::as_array)
        .expect("inventory artifact must contain rows")
        .clone()
}

fn inventory_static_census() -> BTreeSet<String> {
    let mut pointers = BTreeSet::new();
    let artifact = inventory_artifact();
    let profiles = artifact
        .get("static_surface_census")
        .and_then(Value::as_object)
        .expect("inventory artifact must contain static_surface_census");
    for profile in profiles.values() {
        for pointer in profile.as_array().expect("census profile must be an array") {
            pointers.insert(pointer.as_str().expect("census pointer must be a string").to_string());
        }
    }
    pointers
}

fn inventory_covered_pointers() -> BTreeSet<String> {
    let mut covered = inventory_static_census();
    for row in inventory_rows() {
        let kind = row.get("kind").and_then(Value::as_str);
        if matches!(kind, Some("capability-field") | Some("mutation")) {
            if let Some(pointer) = row.get("protocol_field").and_then(Value::as_str) {
                covered.insert(pointer.to_string());
            }
            if let Some(additional) = row.get("additional_owned_pointers").and_then(Value::as_array)
            {
                for pointer in additional {
                    covered.insert(
                        pointer.as_str().expect("owned pointer must be a string").to_string(),
                    );
                }
            }
        }
    }
    covered
}

fn inventory_row(surface_id: &str) -> Value {
    inventory_rows()
        .into_iter()
        .find(|row| row.get("surface_id").and_then(Value::as_str) == Some(surface_id))
        .unwrap_or_else(|| panic!("ledger row {surface_id} missing"))
}

fn row_string<'a>(row: &'a Value, field: &str) -> &'a str {
    row.get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("inventory row is missing string field {field}: {row}"))
}

fn flatten_surface_pointers(value: &Value) -> BTreeSet<String> {
    fn walk(prefix: &str, value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if map.is_empty() {
                    out.insert(prefix.to_string());
                    return;
                }
                for (key, child) in map {
                    let child_path = format!("{prefix}.{key}");
                    match child {
                        Value::Object(inner) if inner.is_empty() => {
                            out.insert(child_path);
                        }
                        Value::Array(items) => {
                            let array_path = format!("{child_path}[]");
                            out.insert(array_path.clone());
                            for item in items {
                                walk(&array_path, item, out);
                            }
                        }
                        other => walk(&child_path, other, out),
                    }
                }
            }
            Value::Array(items) => {
                if items.is_empty() {
                    out.insert(prefix.to_string());
                    return;
                }
                for item in items {
                    walk(prefix, item, out);
                }
            }
            _ => {
                out.insert(prefix.to_string());
            }
        }
    }

    let mut out = BTreeSet::new();
    if let Value::Object(map) = value {
        for (key, child) in map {
            match child {
                Value::Object(inner) if inner.is_empty() => {
                    out.insert(key.clone());
                }
                Value::Array(items) => {
                    let array_path = format!("{key}[]");
                    out.insert(array_path.clone());
                    for item in items {
                        walk(&array_path, item, &mut out);
                    }
                }
                other => walk(key, other, &mut out),
            }
        }
    }
    out
}

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
    let covered = inventory_covered_pointers();
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
    let static_census = inventory_static_census();
    let rows = inventory_rows();
    let mutation_owned: BTreeSet<String> = rows
        .iter()
        .filter(|row| row.get("kind").and_then(Value::as_str) == Some("mutation"))
        .flat_map(|row| {
            let mut owned = vec![row_string(row, "protocol_field").to_string()];
            owned.extend(
                row.get("additional_owned_pointers")
                    .and_then(Value::as_array)
                    .expect("mutation row must list additional pointers")
                    .iter()
                    .map(|pointer| {
                        pointer.as_str().expect("owned pointer must be a string").to_string()
                    }),
            );
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
        "cap.inlineCompletionProvider"
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
        "mut.handle_initialize.inlineCompletionTriState"
    );
}

#[test]
fn registrations_refreshes_and_compat_rows_are_ledgered() {
    let find = inventory_row;

    let watchers = find("reg.perl-didChangeWatchedFiles");
    assert!(row_string(&watchers, "protocol_field").contains("workspace/didChangeWatchedFiles"));
    assert!(row_string(&watchers, "protocol_field").contains("perl-didChangeWatchedFiles"));
    assert_eq!(row_string(&watchers, "disposition"), "dynamic");

    let inline = find("reg.perl-inlineCompletion");
    assert!(row_string(&inline, "protocol_field").contains("textDocument/inlineCompletion"));

    for refresh_method in [
        "workspace/codeLens/refresh",
        "workspace/semanticTokens/refresh",
        "workspace/inlayHint/refresh",
        "workspace/inlineValue/refresh",
        "workspace/diagnostic/refresh",
        "workspace/foldingRange/refresh",
        "workspace/textDocumentContent/refresh",
    ] {
        assert!(
            inventory_rows().iter().any(|row| row_string(row, "protocol_field") == refresh_method),
            "refresh request {refresh_method} has no inventory row"
        );
    }

    let virtual_content_refresh = find("ref.workspace/textDocumentContent/refresh");
    assert!(
        virtual_content_refresh
            .get("client_capability_inputs")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    );
    assert!(
        row_string(&virtual_content_refresh, "builder_mutator_path")
            .contains("virtual_content.rs request_text_document_content_refresh")
    );

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
    for row in inventory_rows() {
        let Some(effect) = row.get("build_flag_effect") else {
            continue;
        };
        let Some(feature_id) = row_string(&row, "protocol_field").strip_prefix(prefix) else {
            continue;
        };
        let effect_flag = row_string(effect, "flag");

        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, feature_id);

        let before_all_true =
            ALL_FLAG_NAMES.iter().all(|name| flag_value(&BuildFlags::all(), name) == Some(true));
        assert!(before_all_true, "BuildFlags::all() baseline changed; update flag list");

        for name in ALL_FLAG_NAMES {
            let expected = if *name == effect_flag { Some(false) } else { Some(true) };
            assert_eq!(
                flag_value(&flags, name),
                expected,
                "suppression row {} claims flag {} but applying {feature_id} diverged on {name}",
                row_string(&row, "surface_id"),
                effect_flag
            );
        }
    }
}
