//! The hand-maintained ledger rows for the final-surface inventory (#9662).
//!
//! Every row cites current-main source paths and function names; stale
//! citations are caught by the coverage checker against the live census.
//! Sorted output is enforced at render time by [`super::
//! render_final_surface_inventory_json`].

use super::{
    BuildFlagEffect, CompetingPath, Disposition, SurfaceKind, SurfaceRow, capability, command,
    compat, mutation, suppression,
};

// ---------------------------------------------------------------------------
// Cited source paths (current main)
// ---------------------------------------------------------------------------

const S_DOC_SYNC: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_document_sync";
const S_NAV: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_navigation_features";
const S_EDIT: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_editing_features";
const S_SYM: &str = "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_symbol_and_workspace_features";
const S_ANALYSIS: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_analysis_features";
const S_CODE_ACTION: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_code_action_features";
const S_MISC: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_misc_features";
const S_EXP: &str =
    "crates/perl-lsp-rs-core/src/protocol/capabilities/experimental.rs apply_experimental_features";
const S_JSON: &str = "crates/perl-lsp-rs-core/src/protocol/capabilities.rs capabilities_json";
const RT_INIT: &str = "crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs handle_initialize";

/// Target issue absorbing this surface on the #8032 train.
const TARGET_S02: &str = "#9665";
/// Shared build/profile input annotation for statically advertised fields.
const PROFILE_FLAGS: &[&str] = &["BuildFlags (ga-lock | production | all census profiles)"];
const NO_CLIENT: &[&str] = &[];
const NO_INPUTS: &[&str] = &[];

fn cap(
    id: &'static str,
    field: &'static str,
    builder: &'static str,
    route: &'static str,
    evidence: &'static str,
) -> SurfaceRow {
    capability(id, field, builder, NO_CLIENT, PROFILE_FLAGS, route, evidence, TARGET_S02)
}

fn mut_row(
    id: &'static str,
    field: &'static str,
    clients: &'static [&'static str],
    evidence: &'static str,
) -> SurfaceRow {
    mutation(
        id,
        field,
        RT_INIT,
        clients,
        "n/a (initialize response assembly)",
        evidence,
        TARGET_S02,
    )
}

fn registration(
    id: &'static str,
    protocol_field: &'static str,
    builder: &'static str,
    clients: &'static [&'static str],
    config_inputs: &'static [&'static str],
    disposition: Disposition,
    evidence: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id: id,
        kind: SurfaceKind::Registration,
        protocol_field,
        builder_mutator_path: builder,
        client_capability_inputs: clients,
        build_profile_config_tool_inputs: config_inputs,
        disposition,
        runtime_route_owner: "initialized notification -> perl-lsp-rs/src/runtime/dispatch/lifecycle.rs",
        evidence_owner: evidence,
        competing_paths: Vec::new(),
        target_issue: TARGET_S02,
        compatibility: None,
        additional_owned_pointers: super::NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_request(
    id: &'static str,
    method: &'static str,
    client_input: &'static [&'static str],
    sender: &'static str,
    evidence: &'static str,
) -> SurfaceRow {
    SurfaceRow {
        surface_id: id,
        kind: SurfaceKind::RefreshRequest,
        protocol_field: method,
        builder_mutator_path: sender,
        client_capability_inputs: client_input,
        build_profile_config_tool_inputs: NO_INPUTS,
        disposition: Disposition::Dynamic,
        runtime_route_owner: "crates/perl-lsp-rs/src/runtime/refresh.rs RefreshController (debounced)",
        evidence_owner: evidence,
        competing_paths: Vec::new(),
        target_issue: TARGET_S02,
        compatibility: None,
        additional_owned_pointers: super::NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    }
}

fn systemic_suppression(
    id: &'static str,
    inputs: &'static [&'static str],
    builder: &'static str,
    flag: Option<&'static str>,
) -> SurfaceRow {
    let input = inputs.first().copied().unwrap_or("");
    SurfaceRow {
        surface_id: id,
        kind: SurfaceKind::Suppression,
        protocol_field: input,
        builder_mutator_path: builder,
        client_capability_inputs: NO_CLIENT,
        build_profile_config_tool_inputs: inputs,
        disposition: Disposition::Unadvertised,
        runtime_route_owner: "crates/perl-lsp-rs/src/runtime/dispatch dispatch gating (-32601 method_not_advertised)",
        evidence_owner: "features.toml catalog; perl-lsp-rs lifecycle tests",
        competing_paths: Vec::new(),
        target_issue: TARGET_S02,
        compatibility: None,
        additional_owned_pointers: super::NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: flag.map(|flag| BuildFlagEffect { flag }),
    }
}

/// The complete ledger. Grouped by kind for review; render sorts by ID.
pub(super) fn rows() -> Vec<SurfaceRow> {
    let mut all = Vec::new();
    all.extend(capability_rows());
    all.extend(mutation_rows());
    all.extend(registration_rows());
    all.extend(refresh_rows());
    all.extend(suppression_rows());
    all.extend(compatibility_rows());
    all.extend(command_rows());
    all
}

// ---------------------------------------------------------------------------
// Capability fields (static builder census)
// ---------------------------------------------------------------------------

fn capability_rows() -> Vec<SurfaceRow> {
    vec![
        // -- text document sync (typed static options; overridden at runtime)
        cap(
            "cap.textDocumentSync.openClose",
            "textDocumentSync.openClose",
            S_DOC_SYNC,
            "textDocument/didOpen + didClose notifications",
            "features.toml#lsp.text_document_sync",
        ),
        cap(
            "cap.textDocumentSync.change",
            "textDocumentSync.change",
            S_DOC_SYNC,
            "textDocument/didChange (Full=1 reparse)",
            "features.toml#lsp.text_document_sync; lifecycle tests text_document_sync_advertises_full_sync_and_open_close",
        ),
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: RT_INIT,
                delta: "runtime replaces the whole textDocumentSync value after initialize \
                        parsing: adds willSave=true, willSaveWaitUntil=true and turns save \
                        from boolean true into {includeText:true} (see \
                        mut.handle_initialize.textDocumentSyncOverride)",
            }],
            ..cap(
                "cap.textDocumentSync.save",
                "textDocumentSync.save",
                S_DOC_SYNC,
                "textDocument/didSave",
                "features.toml#lsp.did_save; lifecycle test text_document_sync_advertises_did_save_support",
            )
        },
        // -- navigation
        cap(
            "cap.hoverProvider",
            "hoverProvider",
            S_NAV,
            "textDocument/hover",
            "features.toml#lsp.hover",
        ),
        cap(
            "cap.documentHighlightProvider",
            "documentHighlightProvider",
            S_NAV,
            "textDocument/documentHighlight",
            "features.toml#lsp.document_highlight",
        ),
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: RT_INIT,
                delta: "runtime unconditionally re-sets declarationProvider=true when \
                        AdvertisedFeatures.declaration is on (same value, second writer; see \
                        mut.handle_initialize.declarationProviderRewrite)",
            }],
            ..cap(
                "cap.declarationProvider",
                "declarationProvider",
                S_NAV,
                "textDocument/declaration",
                "features.toml#lsp.declaration",
            )
        },
        cap(
            "cap.definitionProvider",
            "definitionProvider",
            S_NAV,
            "textDocument/definition",
            "features.toml#lsp.definition",
        ),
        cap(
            "cap.typeDefinitionProvider",
            "typeDefinitionProvider",
            S_NAV,
            "textDocument/typeDefinition",
            "features.toml#lsp.type_definition",
        ),
        cap(
            "cap.implementationProvider",
            "implementationProvider",
            S_NAV,
            "textDocument/implementation",
            "features.toml#lsp.implementation",
        ),
        cap(
            "cap.referencesProvider",
            "referencesProvider",
            S_NAV,
            "textDocument/references",
            "features.toml#lsp.references",
        ),
        // -- editing
        cap(
            "cap.signatureHelpProvider.triggerCharacters[]",
            "signatureHelpProvider.triggerCharacters[]",
            S_EDIT,
            "textDocument/signatureHelp",
            "features.toml#lsp.signature_help; lifecycle test signature_help_trigger_characters_are_paren_and_comma",
        ),
        cap(
            "cap.signatureHelpProvider.retriggerCharacters[]",
            "signatureHelpProvider.retriggerCharacters[]",
            S_EDIT,
            "textDocument/signatureHelp",
            "features.toml#lsp.signature_help; lifecycle test signature_help_retrigger_characters_include_required_set",
        ),
        SurfaceRow {
            client_capability_inputs: &[
                "textDocument.completion.completionItem.resolveSupport.properties",
            ],
            ..cap(
                "cap.completionProvider.resolveProvider",
                "completionProvider.resolveProvider",
                S_EDIT,
                "completionItem/resolve",
                "features.toml#lsp.completion_item_resolve",
            )
        },
        SurfaceRow {
            client_capability_inputs: &[
                "textDocument.completion.completionItem.commitCharactersSupport",
            ],
            ..cap(
                "cap.completionProvider.triggerCharacters[]",
                "completionProvider.triggerCharacters[]",
                S_EDIT,
                "textDocument/completion",
                "features.toml#lsp.completion; completion_trigger_characters() canonical list",
            )
        },
        SurfaceRow {
            client_capability_inputs: &[
                "textDocument.completion.completionItem.labelDetailsSupport",
            ],
            ..cap(
                "cap.completionProvider.completionItem.labelDetailsSupport",
                "completionProvider.completionItem.labelDetailsSupport",
                S_EDIT,
                "textDocument/completion item shape",
                "features.toml#lsp.completion; lifecycle tests label_details/commit-characters parsing",
            )
        },
        SurfaceRow {
            build_profile_config_tool_inputs: &["BuildFlags.completion"],
            client_capability_inputs: &[
                "textDocument.completion.completionItem.insertTextMode (advertised modes [1,2] = PlainText, Snippet)",
            ],
            ..cap(
                "cap.completionProvider.completionItem.insertTextModes[]",
                "completionProvider.completionItem.insertTextModes[]",
                S_JSON,
                "textDocument/completion insertTextFormat/Mode negotiation",
                "LSP 3.17 insertTextModes patched because lsp-types 0.97 lacks the field",
            )
        },
        cap(
            "cap.documentFormattingProvider",
            "documentFormattingProvider",
            S_EDIT,
            "textDocument/formatting",
            "features.toml#lsp.formatting",
        ),
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: S_JSON,
                delta: "capabilities_for() emits boolean true (OneOf::Left) but capabilities_json() \
                        replaces the whole value with {rangesSupport:true} when range_formatting is \
                        enabled (LSP 3.18 rangesSupport absent from lsp-types 0.97)",
            }],
            ..cap(
                "cap.documentRangeFormattingProvider.rangesSupport",
                "documentRangeFormattingProvider.rangesSupport",
                S_JSON,
                "textDocument/rangeFormatting multi-range variant",
                "features.toml#lsp.ranges_formatting; lifecycle tests ranges_formatting_*",
            )
        },
        SurfaceRow {
            client_capability_inputs: &[
                "textDocument.rename.prepareSupport",
                "textDocument.rename.prepareSupportDefaultBehavior",
            ],
            ..cap(
                "cap.renameProvider.prepareProvider",
                "renameProvider.prepareProvider",
                S_EDIT,
                "textDocument/prepareRename",
                "features.toml#lsp.prepare_rename; lifecycle tests prepare_support_default_behavior",
            )
        },
        cap(
            "cap.documentOnTypeFormattingProvider.firstTriggerCharacter",
            "documentOnTypeFormattingProvider.firstTriggerCharacter",
            S_EDIT,
            "textDocument/onTypeFormatting",
            "features.toml#lsp.on_type_formatting",
        ),
        cap(
            "cap.documentOnTypeFormattingProvider.moreTriggerCharacter[]",
            "documentOnTypeFormattingProvider.moreTriggerCharacter[]",
            S_EDIT,
            "textDocument/onTypeFormatting",
            "features.toml#lsp.on_type_formatting",
        ),
        cap(
            "cap.linkedEditingRangeProvider",
            "linkedEditingRangeProvider",
            S_EDIT,
            "textDocument/linkedEditingRange",
            "features.toml#lsp.linked_editing_range",
        ),
        // -- symbols / workspace
        cap(
            "cap.documentSymbolProvider",
            "documentSymbolProvider",
            S_SYM,
            "textDocument/documentSymbol",
            "features.toml#lsp.document_symbol; lifecycle tests document_symbol_provider_*",
        ),
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs apply_symbol_and_workspace_features (workspace_symbol arm)",
                delta: "workspace_symbol=true claims OneOf::Left(true); the later \
                        workspace_symbol_resolve arm overwrites the same provider with \
                        OneOf::Right({resolveProvider:true}) when both flags are set - two \
                        writers inside one function claim the same variant",
            }],
            ..cap(
                "cap.workspaceSymbolProvider.resolveProvider",
                "workspaceSymbolProvider.resolveProvider",
                S_SYM,
                "workspace/symbol + workspaceSymbol/resolve",
                "features.toml#lsp.workspace_symbol_resolve; lifecycle test workspace_symbol_provider_advertises_resolve_when_enabled",
            )
        },
        cap(
            "cap.notebookDocumentSync.notebookSelector[].notebook",
            "notebookDocumentSync.notebookSelector[].notebook",
            S_SYM,
            "notebookDocument/didOpen|didSave|didClose",
            "features.toml#lsp.notebook_document_sync (preview: all profile only)",
        ),
        SurfaceRow {
            additional_owned_pointers: &[
                "notebookDocumentSync.notebookSelector[]",
                "notebookDocumentSync.notebookSelector[].cells[]",
            ],
            ..cap(
                "cap.notebookDocumentSync.notebookSelector[].cells[].language",
                "notebookDocumentSync.notebookSelector[].cells[].language",
                S_SYM,
                "notebookDocument cell filtering (language perl)",
                "features.toml#lsp.notebook_document_sync",
            )
        },
        cap(
            "cap.notebookDocumentSync.save",
            "notebookDocumentSync.save",
            S_SYM,
            "notebookDocument/didSave",
            "features.toml#lsp.notebook_cell_execution sub-feature carrier",
        ),
        // -- analysis
        cap(
            "cap.foldingRangeProvider",
            "foldingRangeProvider",
            S_ANALYSIS,
            "textDocument/foldingRange",
            "features.toml#lsp.folding_range",
        ),
        SurfaceRow {
            client_capability_inputs: &["textDocument.inlayHint.resolveSupport.properties"],
            ..cap(
                "cap.inlayHintProvider.resolveProvider",
                "inlayHintProvider.resolveProvider",
                S_ANALYSIS,
                "inlayHint/resolve",
                "features.toml#lsp.inlay_hint_resolve; lifecycle test inlay_hint resolve-support parsing",
            )
        },
        cap(
            "cap.diagnosticProvider.interFileDependencies",
            "diagnosticProvider.interFileDependencies",
            S_ANALYSIS,
            "textDocument/diagnostic context",
            "features.toml#lsp.pull_diagnostics",
        ),
        cap(
            "cap.diagnosticProvider.workspaceDiagnostics",
            "diagnosticProvider.workspaceDiagnostics",
            S_ANALYSIS,
            "workspace/diagnostic",
            "features.toml#lsp.workspace_diagnostics",
        ),
        SurfaceRow {
            client_capability_inputs: &["textDocument.diagnostic.markupMessageSupport"],
            ..cap(
                "cap.diagnosticProvider.identifier",
                "diagnosticProvider.identifier",
                S_ANALYSIS,
                "textDocument/diagnostic identifier perl-lsp",
                "features.toml#lsp.pull_diagnostics; lifecycle test markup-message parsing",
            )
        },
        cap(
            "cap.semanticTokensProvider.legend.tokenTypes[]",
            "semanticTokensProvider.legend.tokenTypes[]",
            S_ANALYSIS,
            "textDocument/semanticTokens/full legend",
            "features.toml#lsp.semantic_tokens; lifecycle test semantic_token_types_are_exact_ordered_list",
        ),
        cap(
            "cap.semanticTokensProvider.legend.tokenModifiers[]",
            "semanticTokensProvider.legend.tokenModifiers[]",
            S_ANALYSIS,
            "textDocument/semanticTokens/full legend",
            "features.toml#lsp.semantic_tokens; lifecycle test semantic_token_modifiers_are_exact_ordered_list",
        ),
        cap(
            "cap.semanticTokensProvider.range",
            "semanticTokensProvider.range",
            S_ANALYSIS,
            "textDocument/semanticTokens/range",
            "features.toml#lsp.semantic_tokens",
        ),
        cap(
            "cap.semanticTokensProvider.full.delta",
            "semanticTokensProvider.full.delta",
            S_ANALYSIS,
            "textDocument/semanticTokens/full/delta",
            "features.toml#lsp.semantic_tokens",
        ),
        cap(
            "cap.codeActionProvider.codeActionKinds[]",
            "codeActionProvider.codeActionKinds[]",
            S_CODE_ACTION,
            "textDocument/codeAction kind filter",
            "features.toml#lsp.refactoring; lifecycle test code_action_kinds_include_exact_advertised_set; source.organizeImports withdrawn from this list (#8305), see sys.withdrawal.sourceOrganizeImports",
        ),
        SurfaceRow {
            client_capability_inputs: &[
                "textDocument.codeAction.resolveSupport",
                "textDocument.codeAction.disabledSupport (consumed downstream)",
                "textDocument.codeAction.tagSupport.valueSet (LLMGenerated=1, consumed downstream)",
            ],
            ..cap(
                "cap.codeActionProvider.resolveProvider",
                "codeActionProvider.resolveProvider",
                S_CODE_ACTION,
                "codeAction/resolve",
                "features.toml#lsp.code_action_resolve; lifecycle tests code_action documentation/disabled/llm-tag parsing",
            )
        },
        cap(
            "cap.executeCommandProvider.commands[]",
            "executeCommandProvider.commands[]",
            S_CODE_ACTION,
            "workspace/executeCommand (see cmd.* rows)",
            "features.toml#lsp.execute_command; SUPPORTED_COMMANDS canonical list",
        ),
        // -- misc providers
        cap(
            "cap.documentLinkProvider.resolveProvider",
            "documentLinkProvider.resolveProvider",
            S_MISC,
            "documentLink/resolve",
            "features.toml#lsp.document_link_resolve",
        ),
        cap(
            "cap.selectionRangeProvider",
            "selectionRangeProvider",
            S_MISC,
            "textDocument/selectionRange",
            "features.toml#lsp.selection_range",
        ),
        SurfaceRow {
            client_capability_inputs: &["textDocument.codeLens.resolveSupport.properties"],
            ..cap(
                "cap.codeLensProvider.resolveProvider",
                "codeLensProvider.resolveProvider",
                S_MISC,
                "codeLens/resolve",
                "features.toml#lsp.code_lens_resolve; lifecycle test code_lens resolve-support parsing",
            )
        },
        cap(
            "cap.inlineValueProvider",
            "inlineValueProvider",
            S_MISC,
            "textDocument/inlineValue",
            "features.toml#lsp.inline_value (ga-lock suppresses)",
        ),
        cap(
            "cap.monikerProvider",
            "monikerProvider",
            S_MISC,
            "textDocument/moniker",
            "features.toml#lsp.moniker",
        ),
        cap(
            "cap.colorProvider",
            "colorProvider",
            S_MISC,
            "textDocument/documentColor + colorPresentation",
            "features.toml#lsp.document_color",
        ),
        cap(
            "cap.callHierarchyProvider",
            "callHierarchyProvider",
            S_MISC,
            "textDocument/prepareCallHierarchy",
            "features.toml#lsp.call_hierarchy",
        ),
        // -- experimental / manual JSON patches
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: S_JSON,
                delta: "capabilities_json() additionally injects top-level typeHierarchyProvider \
                        {workDoneProgressOptions:{}} for LSP compatibility while \
                        apply_experimental_features() advertises experimental.typeHierarchyProvider \
                        boolean for feature_ids_from_caps detection - the same variant advertised \
                        twice through two paths",
            }],
            ..cap(
                "cap.experimental.typeHierarchyProvider",
                "experimental.typeHierarchyProvider",
                S_EXP,
                "typeHierarchy/subtypes|supertypes",
                "features.toml#lsp.type_hierarchy; feature_ids_from_caps detection contract",
            )
        },
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: S_EXP,
                delta: "experimental.typeHierarchyProvider boolean duplicates this top-level \
                        advertisement for typed-caps consumers that cannot see the JSON patch",
            }],
            ..cap(
                "cap.typeHierarchyProvider.workDoneProgressOptions",
                "typeHierarchyProvider.workDoneProgressOptions",
                S_JSON,
                "typeHierarchy/subtypes|supertypes work-done progress",
                "manual patch: lsp-types 0.97 lacks type_hierarchy_provider on ServerCapabilities",
            )
        },
        SurfaceRow {
            competing_paths: vec![CompetingPath {
                path: RT_INIT,
                delta: "initialize-time tri-state removes or re-inserts inlineCompletionProvider \
                        depending on (feature, client static support, client dynamicRegistration): \
                        dynamic-registration clients get the provider removed here and registered \
                        post-initialized instead (see mut.handle_initialize.inlineCompletionTriState)",
            }],
            build_profile_config_tool_inputs: &["BuildFlags.inline_completion"],
            ..cap(
                "cap.inlineCompletionProvider",
                "inlineCompletionProvider",
                S_JSON,
                "textDocument/inlineCompletion (static mode)",
                "features.toml#lsp.inline_completion; lifecycle tests inline_completion_*",
            )
        },
        // -- shared WorkDoneProgressOptions empties are not serialized by
        // lsp-types (empirically absent from every profile census), so no
        // family row exists for them.
    ]
}

// ---------------------------------------------------------------------------
// Initialize-time mutations (exact runtime paths)
// ---------------------------------------------------------------------------

fn mutation_rows() -> Vec<SurfaceRow> {
    vec![
        SurfaceRow {
            additional_owned_pointers: &[
                "textDocumentSync.willSave",
                "textDocumentSync.willSaveWaitUntil",
                "textDocumentSync.save.includeText",
            ],
            rewrites_surface_pointer: Some("textDocumentSync"),
            ..mut_row(
                "mut.handle_initialize.textDocumentSyncOverride",
                "textDocumentSync",
                NO_CLIENT,
                "#4995 typed TextDocumentSyncOptions override; lifecycle tests text_document_sync_*",
            )
        },
        SurfaceRow {
            additional_owned_pointers: &["positionEncoding"],
            client_capability_inputs: &[
                "general.positionEncodings (negotiated, stored, NOT advertised)",
            ],
            ..mut_row(
                "mut.handle_initialize.positionEncodingPin",
                "positionEncoding",
                &["general.positionEncodings"],
                "position contract pinned utf-16 until negotiated encoding threads through providers; see compat.protocol.positionEncodingUtf16Pin",
            )
        },
        SurfaceRow {
            additional_owned_pointers: &[
                "workspace.workspaceFolders.supported",
                "workspace.workspaceFolders.changeNotifications",
                "workspace.textDocumentContent.schemes[]",
            ],
            client_capability_inputs: &["workspace.workspaceFolders"],
            ..mut_row(
                "mut.handle_initialize.workspaceReplacement",
                "workspace",
                &["workspace.workspaceFolders"],
                "workspace_capabilities(); lifecycle test initialize_disables_workspace_folder_server_capability_when_client_lacks_support; matrix workspace/textDocumentContent row",
            )
        },
        SurfaceRow {
            additional_owned_pointers: &[
                "workspace.fileOperations.willCreate.filters[]",
                "workspace.fileOperations.didCreate.filters[]",
                "workspace.fileOperations.willRename.filters[]",
                "workspace.fileOperations.didRename.filters[]",
                "workspace.fileOperations.willDelete.filters[]",
                "workspace.fileOperations.didDelete.filters[]",
                "workspace.fileOperations.willCreate.filters[].pattern.glob",
                "workspace.fileOperations.didCreate.filters[].pattern.glob",
                "workspace.fileOperations.willRename.filters[].pattern.glob",
                "workspace.fileOperations.didRename.filters[].pattern.glob",
                "workspace.fileOperations.willDelete.filters[].pattern.glob",
                "workspace.fileOperations.didDelete.filters[].pattern.glob",
            ],
            client_capability_inputs: &[
                "workspace.fileOperations.willCreate",
                "workspace.fileOperations.didCreate",
                "workspace.fileOperations.willRename",
                "workspace.fileOperations.didRename",
                "workspace.fileOperations.willDelete",
                "workspace.fileOperations.didDelete",
            ],
            ..mut_row(
                "mut.handle_initialize.fileOperationsIntersection",
                "workspace.fileOperations",
                &[
                    "workspace.fileOperations.willCreate",
                    "workspace.fileOperations.didCreate",
                    "workspace.fileOperations.willRename",
                    "workspace.fileOperations.didRename",
                    "workspace.fileOperations.willDelete",
                    "workspace.fileOperations.didDelete",
                ],
                "#7682 exact-operation intersection; FileOperationSupport::from_initialize_params",
            )
        },
        SurfaceRow {
            additional_owned_pointers: &[
                "codeActionProvider.documentation[].kind",
                "codeActionProvider.documentation[].command.title",
                "codeActionProvider.documentation[].command.command",
                "codeActionProvider.documentation[].command.arguments[]",
                "codeActionProvider.documentation[].command.arguments[].provider",
                "codeActionProvider.documentation[].command.arguments[].receipt_id",
                "codeActionProvider.documentation[].command.arguments[].scenario",
            ],
            client_capability_inputs: &["textDocument.codeAction.documentationSupport"],
            ..mut_row(
                "mut.handle_initialize.codeActionDocumentationInsert",
                "codeActionProvider.documentation[]",
                &["textDocument.codeAction.documentationSupport"],
                "PLSP-SPEC-0029#code-action-documentation; features.toml#lsp.code_action_documentation; lifecycle test initialize_advertises_code_action_documentation_only_when_supported",
            )
        },
        SurfaceRow {
            additional_owned_pointers: &["experimental.perlInlineCompletionStream"],
            client_capability_inputs: &["textDocument/inlineCompletion presence"],
            ..mut_row(
                "mut.handle_initialize.experimentalPerlInlineCompletionStreamMerge",
                "experimental.perlInlineCompletionStream",
                &["textDocument/inlineCompletion presence"],
                "features.toml#experimental.perlInlineCompletionStream; merge_experimental_capability",
            )
        },
        SurfaceRow {
            rewrites_surface_pointer: Some("declarationProvider"),
            client_capability_inputs: NO_CLIENT,
            ..mut_row(
                "mut.handle_initialize.declarationProviderRewrite",
                "(rewrite) declarationProvider",
                NO_CLIENT,
                "unconditional re-set when AdvertisedFeatures.declaration; ownership stays with cap.declarationProvider",
            )
        },
        SurfaceRow {
            rewrites_surface_pointer: Some("inlineCompletionProvider"),
            client_capability_inputs: &[
                "textDocument/inlineCompletion presence",
                "textDocument/inlineCompletion/dynamicRegistration",
            ],
            ..mut_row(
                "mut.handle_initialize.inlineCompletionTriState",
                "(rewrite) inlineCompletionProvider",
                &[
                    "textDocument/inlineCompletion presence",
                    "textDocument/inlineCompletion/dynamicRegistration",
                ],
                "(feature, static-support, dynamic-support) tri-state removes/re-inserts the static provider; lsp_inline_completion_registration_tests.rs",
            )
        },
        // Initialize-result envelope assembly (outside serverCapabilities but
        // part of the final surface emitted by handle_initialize).
        SurfaceRow {
            additional_owned_pointers: &["envelope.serverInfo.name", "envelope.serverInfo.version"],
            client_capability_inputs: NO_CLIENT,
            ..mut_row(
                "mut.handle_initialize.envelopeAssembly",
                "envelope.protocolVersion=3.18",
                NO_CLIENT,
                "LSP_PROTOCOL_VERSION const + serverInfo name/version in the initialize result envelope; json!() assembly kept per in-source rationale comment",
            )
        },
    ]
}

// ---------------------------------------------------------------------------
// Dynamic registrations
// ---------------------------------------------------------------------------

fn registration_rows() -> Vec<SurfaceRow> {
    vec![
        registration(
            "reg.perl-didChangeWatchedFiles",
            "register perl-didChangeWatchedFiles@workspace/didChangeWatchedFiles",
            "crates/perl-lsp-rs/src/runtime/lifecycle/watchers.rs register_file_watchers_if_needed",
            &[
                "workspace.didChangeWatchedFiles.dynamicRegistration",
                "workspace.didChangeWatchedFiles.relativePatternSupport",
            ],
            &["AdvertisedFeatures.workspace_symbol", "config runtime_tuning.file_watchers"],
            Disposition::Dynamic,
            "features.toml#lsp.did_change_watched_files; lsp_registration_tests.rs; RelativePattern fallback string globs (**/*.pl,*.pm,*.t,*.psgi)",
        ),
        registration(
            "reg.perl-inlineCompletion",
            "register perl-inlineCompletion@textDocument/inlineCompletion",
            "crates/perl-lsp-rs/src/runtime/lifecycle/watchers.rs register_inline_completion_if_needed",
            &["textDocument/inlineCompletion.dynamicRegistration"],
            &["AdvertisedFeatures.inline_completion"],
            Disposition::Dynamic,
            "features.toml#lsp.inline_completion; lsp_inline_completion_registration_tests.rs; documentSelector perl+perl5",
        ),
        registration(
            "reg.client-unregisterCapability",
            "client/unregisterCapability",
            "none (no sender exists on current main)",
            NO_CLIENT,
            NO_INPUTS,
            Disposition::Unadvertised,
            "features.toml#lsp.client_unregister_capability is catalog-only; finding: server never unregisters today",
        ),
    ]
}

// ---------------------------------------------------------------------------
// Capability-gated refresh requests
// ---------------------------------------------------------------------------

fn refresh_rows() -> Vec<SurfaceRow> {
    vec![
        refresh_request(
            "ref.workspace/codeLens/refresh",
            "workspace/codeLens/refresh",
            &["workspace.codeLens.refreshSupport"],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_code_lens_refresh",
            "features.toml#lsp.code_lens_refresh",
        ),
        refresh_request(
            "ref.workspace/semanticTokens/refresh",
            "workspace/semanticTokens/refresh",
            &["workspace.semanticTokens.refreshSupport"],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_semantic_tokens_refresh",
            "features.toml#lsp.semantic_tokens_refresh",
        ),
        refresh_request(
            "ref.workspace/inlayHint/refresh",
            "workspace/inlayHint/refresh",
            &["workspace.inlayHint.refreshSupport"],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_inlay_hint_refresh",
            "features.toml#lsp.inlay_hint_refresh",
        ),
        refresh_request(
            "ref.workspace/inlineValue/refresh",
            "workspace/inlineValue/refresh",
            &["workspace.inlineValue.refreshSupport"],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_inline_value_refresh",
            "features.toml#lsp.inline_value_refresh",
        ),
        refresh_request(
            "ref.workspace/diagnostic/refresh",
            "workspace/diagnostic/refresh",
            &[
                "workspace.diagnostics.refreshSupport (spec plural key)",
                "workspace.diagnostic.refreshSupport (client-deviation singular)",
            ],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_diagnostic_refresh",
            "features.toml#lsp.diagnostic_refresh; #9592 dual-spelling deviation",
        ),
        refresh_request(
            "ref.workspace/foldingRange/refresh",
            "workspace/foldingRange/refresh",
            &["workspace.foldingRange.refreshSupport"],
            "crates/perl-lsp-rs/src/runtime/client_requests.rs request_folding_range_refresh",
            "features.toml#lsp.folding_range_refresh",
        ),
    ]
}

// ---------------------------------------------------------------------------
// Suppression branches
// ---------------------------------------------------------------------------

fn suppression_rows() -> Vec<SurfaceRow> {
    // One row per accepted `disabledFeatures` ID, mirroring the
    // apply_disabled_feature_id match arms exactly. The alias arm
    // lsp.ranges_formatting shares the range_formatting field with
    // lsp.range_formatting exactly as in apply_disabled_feature_id.
    let mut rows: Vec<SurfaceRow> = vec![
        suppression(
            "sup.disabledFeature.lsp.completion",
            "initializationOptions.disabledFeatures:lsp.completion",
            "completion",
        ),
        suppression(
            "sup.disabledFeature.lsp.hover",
            "initializationOptions.disabledFeatures:lsp.hover",
            "hover",
        ),
        suppression(
            "sup.disabledFeature.lsp.definition",
            "initializationOptions.disabledFeatures:lsp.definition",
            "definition",
        ),
        suppression(
            "sup.disabledFeature.lsp.declaration",
            "initializationOptions.disabledFeatures:lsp.declaration",
            "declaration",
        ),
        suppression(
            "sup.disabledFeature.lsp.references",
            "initializationOptions.disabledFeatures:lsp.references",
            "references",
        ),
        suppression(
            "sup.disabledFeature.lsp.document_symbol",
            "initializationOptions.disabledFeatures:lsp.document_symbol",
            "document_symbol",
        ),
        suppression(
            "sup.disabledFeature.lsp.workspace_symbol",
            "initializationOptions.disabledFeatures:lsp.workspace_symbol",
            "workspace_symbol",
        ),
        suppression(
            "sup.disabledFeature.lsp.code_action",
            "initializationOptions.disabledFeatures:lsp.code_action",
            "code_actions",
        ),
        suppression(
            "sup.disabledFeature.lsp.code_lens",
            "initializationOptions.disabledFeatures:lsp.code_lens",
            "code_lens",
        ),
        suppression(
            "sup.disabledFeature.lsp.rename",
            "initializationOptions.disabledFeatures:lsp.rename",
            "rename",
        ),
        suppression(
            "sup.disabledFeature.lsp.folding_range",
            "initializationOptions.disabledFeatures:lsp.folding_range",
            "folding_range",
        ),
        suppression(
            "sup.disabledFeature.lsp.selection_range",
            "initializationOptions.disabledFeatures:lsp.selection_range",
            "selection_ranges",
        ),
        suppression(
            "sup.disabledFeature.lsp.linked_editing_range",
            "initializationOptions.disabledFeatures:lsp.linked_editing_range",
            "linked_editing",
        ),
        suppression(
            "sup.disabledFeature.lsp.inlay_hint",
            "initializationOptions.disabledFeatures:lsp.inlay_hint",
            "inlay_hints",
        ),
        suppression(
            "sup.disabledFeature.lsp.semantic_tokens",
            "initializationOptions.disabledFeatures:lsp.semantic_tokens",
            "semantic_tokens",
        ),
        suppression(
            "sup.disabledFeature.lsp.call_hierarchy",
            "initializationOptions.disabledFeatures:lsp.call_hierarchy",
            "call_hierarchy",
        ),
        suppression(
            "sup.disabledFeature.lsp.type_hierarchy",
            "initializationOptions.disabledFeatures:lsp.type_hierarchy",
            "type_hierarchy",
        ),
        suppression(
            "sup.disabledFeature.lsp.pull_diagnostics",
            "initializationOptions.disabledFeatures:lsp.pull_diagnostics",
            "pull_diagnostics",
        ),
        suppression(
            "sup.disabledFeature.lsp.document_color",
            "initializationOptions.disabledFeatures:lsp.document_color",
            "document_color",
        ),
        suppression(
            "sup.disabledFeature.lsp.signature_help",
            "initializationOptions.disabledFeatures:lsp.signature_help",
            "signature_help",
        ),
        suppression(
            "sup.disabledFeature.lsp.document_highlight",
            "initializationOptions.disabledFeatures:lsp.document_highlight",
            "document_highlight",
        ),
        suppression(
            "sup.disabledFeature.lsp.formatting",
            "initializationOptions.disabledFeatures:lsp.formatting",
            "formatting",
        ),
        suppression(
            "sup.disabledFeature.lsp.range_formatting",
            "initializationOptions.disabledFeatures:lsp.range_formatting",
            "range_formatting",
        ),
        suppression(
            "sup.disabledFeature.lsp.ranges_formatting",
            "initializationOptions.disabledFeatures:lsp.ranges_formatting",
            "range_formatting",
        ),
        suppression(
            "sup.disabledFeature.lsp.on_type_formatting",
            "initializationOptions.disabledFeatures:lsp.on_type_formatting",
            "on_type_formatting",
        ),
        suppression(
            "sup.disabledFeature.lsp.document_link",
            "initializationOptions.disabledFeatures:lsp.document_link",
            "document_links",
        ),
        suppression(
            "sup.disabledFeature.lsp.inline_completion",
            "initializationOptions.disabledFeatures:lsp.inline_completion",
            "inline_completion",
        ),
        suppression(
            "sup.disabledFeature.lsp.inline_value",
            "initializationOptions.disabledFeatures:lsp.inline_value",
            "inline_values",
        ),
        suppression(
            "sup.disabledFeature.lsp.notebook_document_sync",
            "initializationOptions.disabledFeatures:lsp.notebook_document_sync",
            "notebook_document_sync",
        ),
        suppression(
            "sup.disabledFeature.lsp.notebook_cell_execution",
            "initializationOptions.disabledFeatures:lsp.notebook_cell_execution",
            "notebook_cell_execution",
        ),
        suppression(
            "sup.disabledFeature.lsp.implementation",
            "initializationOptions.disabledFeatures:lsp.implementation",
            "implementation",
        ),
        suppression(
            "sup.disabledFeature.lsp.type_definition",
            "initializationOptions.disabledFeatures:lsp.type_definition",
            "type_definition",
        ),
        suppression(
            "sup.disabledFeature.lsp.execute_command",
            "initializationOptions.disabledFeatures:lsp.execute_command",
            "execute_command",
        ),
        suppression(
            "sup.disabledFeature.lsp.moniker",
            "initializationOptions.disabledFeatures:lsp.moniker",
            "moniker",
        ),
    ];

    rows.push(systemic_suppression(
        "sys.profile.gaLock.inlineValues",
        &["profile:lsp-ga-lock:inline_values=false"],
        "crates/perl-lsp-rs-core/src/features/flags.rs BuildFlags::ga_lock",
        Some("inline_values"),
    ));
    rows.push(systemic_suppression(
        "sys.profile.preview.notebookSync",
        &["profile:not-all:notebook_document_sync=false"],
        "crates/perl-lsp-rs-core/src/features/flags.rs BuildFlags::production/all",
        Some("notebook_document_sync"),
    ));
    rows.push(systemic_suppression(
        "sys.profile.preview.notebookCellExecution",
        &["profile:not-all:notebook_cell_execution=false"],
        "crates/perl-lsp-rs-core/src/features/flags.rs BuildFlags::production/all",
        Some("notebook_cell_execution"),
    ));
    rows.push(systemic_suppression(
        "sys.tool.perltidy.runtimeFlags",
        &["tool:perltidy availability:FeatureProfile.runtime_flags"],
        "crates/perl-lsp-rs-core FeatureProfile::runtime_flags consumed in handle_initialize",
        None,
    ));
    rows.push(systemic_suppression(
        "sys.config.runtimeTuning.fileWatchers",
        &["config:runtime_tuning.file_watchers=false blocks reg.perl-didChangeWatchedFiles"],
        "crates/perl-lsp-rs/src/runtime/lifecycle/watchers.rs register_file_watchers_if_needed early return",
        None,
    ));
    rows.push(systemic_suppression(
        "sys.dispatch.advertisedFeaturesGating",
        &["config:AdvertisedFeatures false => -32601 method_not_advertised"],
        "crates/perl-lsp-rs/src/runtime/dispatch preflight/handlers gate on advertised_features",
        None,
    ));
    // Withdrawn-capability disposition, recorded rather than silently
    // deleted so the ledger explains why source.organizeImports vanished
    // from codeActionProvider.codeActionKinds[] (#8305).
    rows.push(SurfaceRow {
        surface_id: "sys.withdrawal.sourceOrganizeImports",
        kind: SurfaceKind::Suppression,
        protocol_field: "profile:source.organizeImports withdrawn from advertisement and every request path",
        builder_mutator_path:
            "crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs code_action_kinds",
        client_capability_inputs: NO_CLIENT,
        build_profile_config_tool_inputs: &["BuildFlags.source_organize_imports field removed (#8305)"],
        disposition: Disposition::Unadvertised,
        runtime_route_owner:
            "withdrawn from providers/code_actions enhanced import management; no route admits it today",
        evidence_owner:
            "#8305 withdrawal; restoration path #8319 (bounded cohort) + #10696 (proven cutover); organize_imports_containment_tests.rs",
        competing_paths: Vec::new(),
        target_issue: "#8305",
        compatibility: None,
        additional_owned_pointers: super::NO_POINTERS,
        rewrites_surface_pointer: None,
        build_flag_effect: None,
    });
    rows
}

// ---------------------------------------------------------------------------
// Compatibility exceptions (exact subject/reason/expiry required)
// ---------------------------------------------------------------------------

fn compatibility_rows() -> Vec<SurfaceRow> {
    vec![
        compat(
            "compat.client.jetbrains.watcherForceDisable",
            "clientInfo.name =~ /(jetbrains|intellij|idea)/i",
            RT_INIT,
            &["workspace.didChangeWatchedFiles.dynamicRegistration (claimed but overridden)"],
            "#4630-era workaround; lifecycle tests initialize_disables_dynamic_registration_for_jetbrains_clients",
            "JetBrains-family clients (name contains jetbrains/intellij/idea, case-insensitive)",
            "their dynamic watcher registration flow is unreliable and degrades startup; forces caps.dynamic_registration_support=false regardless of declaration and queues one-time logMessage after initialized",
            "until EffectiveLspSurface introduces typed compatibility policy (#9665)",
        ),
        compat(
            "compat.client.opencode.pushDiagnosticsRetention",
            "clientInfo.name =~ /opencode/i && textDocument.diagnostic advertised",
            RT_INIT,
            &["textDocument/diagnostic presence"],
            "lifecycle tests initialize_keeps_push_diagnostics_for_opencode / initialize_enables_pull_diagnostics_for_non_opencode_clients",
            "OpenCode clients advertising pull diagnostics",
            "OpenCode relies on push publishDiagnostics even while declaring textDocument.diagnostic; pull gating suppressed to avoid losing diagnostics",
            "revisit under #6735 negotiation matrix when OpenCode consumes pull diagnostics",
        ),
        compat(
            "compat.protocol.diagnosticRefreshSingularKey",
            "workspace.diagnostic.refreshSupport (singular) accepted beside spec workspace.diagnostics.refreshSupport",
            RT_INIT,
            &["workspace.diagnostics.refreshSupport", "workspace.diagnostic.refreshSupport"],
            "#9592; lifecycle tests initialize_reads_diagnostic_refresh_support_from_spec_plural_key/_singular_client_deviation",
            "clients built on lsp-types/helix-lsp-types emit singular diagnostic on the wire",
            "spec key is plural diagnostics; dropping singular would regress lsp-types-based clients (Helix)",
            "when observed clients stop emitting the singular spelling; dual-read guarded by tests until then",
        ),
        compat(
            "compat.protocol.markdownContentFormatFallback",
            "general.markup.contentFormat -> textDocument.hover.contentFormat legacy fallback -> default markdown=true",
            RT_INIT,
            &["general.markup.contentFormat", "textDocument.hover.contentFormat"],
            "#1724; markdown_support default-true branch in handle_initialize",
            "clients omitting general.markup.contentFormat",
            "legacy fallback reads hover.contentFormat; absent both, markdown assumed supported",
            "until #9665 canonical negotiation model owns markup selection",
        ),
        compat(
            "compat.protocol.completionItemFlattenedShape",
            "snippetSupport/commitCharactersSupport flattened onto textDocument.completion accepted beside completionItem.* shape",
            RT_INIT,
            &["textDocument.completion.completionItem.snippetSupport (flattened alternative)"],
            "dual-shape parse branch in handle_initialize; lifecycle tests initialize_parses_completion_item_capabilities_from_flattened_shape",
            "generic clients flattening completionItem booleans onto textDocument.completion",
            "both shapes parsed; flattened form wins only when completionItem lacks the key",
            "until #9665 normalizes capability parsing",
        ),
        compat(
            "compat.initialize.legacyRootPath",
            "initialize params rootPath (deprecated since LSP 3.0)",
            RT_INIT,
            NO_CLIENT,
            "legacy fallback chain in handle_initialize; lifecycle tests initialize_uses_current_directory_when_root_is_missing",
            "older JetBrains LSP clients sending rootPath",
            "rootPath converted to file URI and used after workspaceFolders/rootUri checks fail",
            "drop when minimum supported client floor excludes legacy JetBrains versions",
        ),
        compat(
            "compat.initialize.initOptionsRootFallbackChain",
            "initializationOptions.{workspaceFolders|rootUri|rootPath} (+perl-lsp/perl_lsp namespaces)",
            RT_INIT,
            NO_CLIENT,
            "initializationOptions fallback chain in handle_initialize; lifecycle tests initialize_init_options_workspace_folders_sets_root_path / initialize_reads_root_uri_from_initialization_options",
            "clients placing workspace roots inside initializationOptions instead of top-level params",
            "mirrors top-level resolution order after all standard fields are absent",
            "until #9665 owns root-resolution policy",
        ),
        compat(
            "compat.initialize.cwdFallback",
            "process current directory used when no root signal exists (e.g. Aider)",
            RT_INIT,
            NO_CLIENT,
            "cwd fallback in handle_initialize; lifecycle test initialize_uses_current_directory_when_root_is_missing",
            "lightweight clients initializing without any root signal",
            "prevents an uninitialized workspace state for minimal clients",
            "until #9665 defines explicit no-workspace behavior",
        ),
        compat(
            "compat.protocol.positionEncodingUtf16Pin",
            "positionEncoding always advertised utf-16 despite general.positionEncodings negotiation",
            RT_INIT,
            &["general.positionEncodings"],
            "phase-comment block in handle_initialize; position authority #2298",
            "every client negotiating a non-UTF-16 preferred encoding",
            "providers still compute UTF-16 offsets; advertising anything else would corrupt positions, so the negotiated value is stored but not advertised",
            "#8032 train stage threading the negotiated encoding through position/text contracts",
        ),
        compat(
            "compat.negotiated.clientInputsWithoutAdvertisementSeam",
            "parsed-but-unadvertised client negotiation inputs: workspace.workspaceEdit.{documentChanges|snippetEditSupport|metadataSupport}; completion.completionList.{itemDefaults(data)|applyKindSupport}; textDocument.codeAction.{disabledSupport|tagSupport.valueSet(1=LLMGenerated)|dataSupport}; declaration/definition/typeDefinition/implementation.linkSupport; window.showDocument.support; window.workDoneProgress; diagnostic.markupMessageSupport; textDocument.diagnostic.relatedDocuments (not parsed on current main)",
            RT_INIT,
            &[
                "workspace.workspaceEdit.documentChanges",
                "workspace.workspaceEdit.snippetEditSupport",
                "workspace.workspaceEdit.metadataSupport",
                "textDocument.completion.completionList.itemDefaults",
                "textDocument.completion.completionList.applyKindSupport",
                "textDocument.codeAction.disabledSupport",
                "textDocument.codeAction.tagSupport.valueSet",
                "textDocument.declaration.linkSupport (and definition/typeDefinition/implementation)",
                "window.showDocument.support",
                "window.workDoneProgress",
                "textDocument.diagnostic.markupMessageSupport",
            ],
            "#6735 negotiation matrix; lifecycle tests initialize_parses_* / initialize_leaves_*_disabled_when_absent",
            "client-negotiated behavior variants with no dedicated serverCapabilities advertisement field",
            "each input is parsed in handle_initialize into ClientCapabilities and consumed downstream by providers/edits/outbound requests (#8068/#8285); recorded once so the denominator stays closed instead of implying an advertisement seam that does not exist",
            "S02 EffectiveLspSurface models each as a typed negotiation outcome (#9665)",
        ),
    ]
}

// ---------------------------------------------------------------------------
// Execute-command identities (#8285)
// ---------------------------------------------------------------------------

fn command_rows() -> Vec<SurfaceRow> {
    vec![
        command("cmd.perl.runTests"),
        command("cmd.perl.runFile"),
        command("cmd.perl.runScript"),
        command("cmd.perl.runTestSub"),
        command("cmd.perl.runCritic"),
        command("cmd.perl.runTest"),
        command("cmd.perl.runTestFile"),
        command("cmd.perl.runSubtest"),
        command("cmd.perl.debugFile"),
        command("cmd.perl.debugTest"),
        command("cmd.perl.debugTests"),
        command("cmd.perl.debugTestFile"),
        command("cmd.perl.goToTest"),
        command("cmd.perl.goToImplementation"),
        command("cmd.perl.explainProviderDecision"),
        command("cmd.perl.workspaceTrustReport"),
        command("cmd.perl.agentContext"),
        command("cmd.perl.previewSafeDelete"),
        command("cmd.perl.safeDeleteSymbol"),
        command("cmd.perl.previewPackageRename"),
        command("cmd.perl.explainMissingModuleLookup"),
    ]
}
