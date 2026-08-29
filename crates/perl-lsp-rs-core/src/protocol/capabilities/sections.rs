use super::{BuildFlags, completion_trigger_characters, get_supported_commands};
#[allow(clippy::wildcard_imports)]
use gen_lsp_types::*;

pub(super) fn apply_document_sync(caps: &mut ServerCapabilities) {
    // Use Options instead of Kind to comply with LSP 3.18 shape requirements.
    // TextDocumentSyncKind::Full (1): the server always reparses the full document
    // on every didChange notification. INCREMENTAL (2) would be inaccurate — no
    // incremental AST state is maintained between edits.
    caps.text_document_sync = Some(TextDocumentSync::Options(TextDocumentSyncOptions {
        open_close: Some(true),
        change: Some(TextDocumentSyncKind::Full),
        will_save: None,
        will_save_wait_until: None,
        // The server handles didSave for diagnostics refresh and post-save hooks.
        save: Some(Save::Bool(true)),
    }));
}

pub(super) fn apply_navigation_features(caps: &mut ServerCapabilities, build: &BuildFlags) {
    if build.hover {
        caps.hover_provider = Some(HoverProvider::Bool(true));
    }

    if build.document_highlight {
        caps.document_highlight_provider = Some(DocumentHighlightProvider::Bool(true));
    }

    if build.declaration {
        caps.declaration_provider = Some(DeclarationProvider::Bool(true));
    }

    if build.definition {
        caps.definition_provider = Some(DefinitionProvider::Bool(true));
    }

    if build.type_definition {
        caps.type_definition_provider = Some(TypeDefinitionProvider::Bool(true));
    }

    if build.implementation {
        caps.implementation_provider = Some(ImplementationProvider::Bool(true));
    }

    if build.references {
        caps.references_provider = Some(ReferencesProvider::Bool(true));
    }

    if build.type_hierarchy {
        // PATCH-TYPEHIERARCHY typed once (#11802 matrix): the selected substrate
        // carries `type_hierarchy_provider` natively, replacing both the post-hoc
        // JSON injection and the experimental workaround (removed together).
        caps.type_hierarchy_provider =
            Some(TypeHierarchyProvider::TypeHierarchyOptions(TypeHierarchyOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }));
    }
}

pub(super) fn apply_editing_features(caps: &mut ServerCapabilities, build: &BuildFlags) {
    if build.signature_help {
        caps.signature_help_provider = Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![
                ",".to_string(),
                "@".to_string(),
                "%".to_string(),
                "{".to_string(),
                "[".to_string(),
            ]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        });
    }

    if build.completion {
        caps.completion_provider = Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(completion_trigger_characters()),
            all_commit_characters: None,
            work_done_progress_options: WorkDoneProgressOptions::default(),
            completion_item: Some(ServerCompletionItemOptions {
                label_details_support: Some(true),
            }),
        });
    }

    if build.formatting {
        caps.document_formatting_provider = Some(DocumentFormattingProvider::Bool(true));
    }

    if build.range_formatting {
        // PATCH-RANGESSUPPORT typed once (#11802 matrix): the selected substrate
        // carries `ranges_support` natively, so multi-range formatting is advertised
        // through the options form instead of a post-hoc JSON overwrite.
        caps.document_range_formatting_provider =
            Some(DocumentRangeFormattingProvider::DocumentRangeFormattingOptions(
                DocumentRangeFormattingOptions {
                    ranges_support: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                },
            ));
    }

    if build.rename {
        caps.rename_provider = Some(RenameProvider::RenameOptions(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }));
    }

    if build.on_type_formatting {
        caps.document_on_type_formatting_provider = Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "}".to_string(),
            more_trigger_character: Some(vec![";".to_string(), "\n".to_string()]),
        });
    }

    if build.linked_editing {
        caps.linked_editing_range_provider = Some(LinkedEditingRangeProvider::Bool(true));
    }

    if build.inline_completion {
        // PATCH-INLINECOMPLETION typed once (#11802 matrix): the selected substrate
        // carries `inline_completion_provider` by default (no proposed gating). This
        // is the static/default advertisement; runtime initialize still removes it
        // when a client opts into dynamic inline-completion registration.
        caps.inline_completion_provider =
            Some(InlineCompletionProvider::InlineCompletionOptions(InlineCompletionOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }));
    }
}

pub(super) fn apply_symbol_and_workspace_features(
    caps: &mut ServerCapabilities,
    build: &BuildFlags,
) {
    if build.document_symbol {
        caps.document_symbol_provider = Some(DocumentSymbolProvider::Bool(true));
    }

    if build.workspace_symbol {
        caps.workspace_symbol_provider = Some(WorkspaceSymbolProvider::Bool(true));
    }

    if build.workspace_symbol_resolve {
        caps.workspace_symbol_provider =
            Some(WorkspaceSymbolProvider::WorkspaceSymbolOptions(WorkspaceSymbolOptions {
                resolve_provider: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }));
    }

    if build.notebook_document_sync {
        caps.notebook_document_sync =
            Some(NotebookDocumentSync::Options(NotebookDocumentSyncOptions {
                notebook_selector: vec![NotebookSelector::NotebookDocumentFilterWithNotebook(
                    NotebookDocumentFilterWithNotebook {
                        notebook: Notebook::String("jupyter-notebook".to_string()),
                        cells: Some(vec![NotebookCellLanguage { language: "perl".to_string() }]),
                    },
                )],
                save: Some(true),
            }));
    }
}

pub(super) fn apply_analysis_features(caps: &mut ServerCapabilities, build: &BuildFlags) {
    if build.folding_range {
        caps.folding_range_provider = Some(FoldingRangeProvider::Bool(true));
    }

    if build.inlay_hints {
        caps.inlay_hint_provider = Some(InlayHintProvider::InlayHintOptions(InlayHintOptions {
            resolve_provider: Some(true), // Resolver implemented in misc.rs:handle_inlay_hint_resolve
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }));
    }

    if build.pull_diagnostics {
        caps.diagnostic_provider = Some(DiagnosticProvider::DiagnosticOptions(DiagnosticOptions {
            inter_file_dependencies: false,
            workspace_diagnostics: true,
            work_done_progress_options: WorkDoneProgressOptions::default(),
            identifier: Some("perl-lsp".to_string()),
        }));
    }

    if build.semantic_tokens {
        caps.semantic_tokens_provider =
            Some(SemanticTokensProvider::SemanticTokensOptions(SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: SemanticTokensLegend {
                    token_types: semantic_token_types(),
                    token_modifiers: semantic_token_modifiers(),
                },
                range: Some(SemanticTokensOptionsRange::Bool(true)),
                // Advertise delta support so clients send
                // `textDocument/semanticTokens/full/delta` for incremental
                // token updates (LSP 3.17).
                full: Some(Full::SemanticTokensFullDelta(SemanticTokensFullDelta {
                    delta: Some(true),
                })),
            }));
    }
}

pub(super) fn apply_code_action_features(caps: &mut ServerCapabilities, build: &BuildFlags) {
    if build.code_actions {
        caps.code_action_provider =
            Some(CodeActionProvider::CodeActionOptions(CodeActionOptions {
                code_action_kinds: Some(code_action_kinds(build)),
                documentation: None,
                resolve_provider: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    if build.execute_command {
        // Only advertise commands that are actually implemented and tested.
        caps.execute_command_provider = Some(ExecuteCommandOptions {
            commands: get_supported_commands(),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        });
    }
}

pub(super) fn apply_misc_features(caps: &mut ServerCapabilities, build: &BuildFlags) {
    if build.document_links {
        caps.document_link_provider = Some(DocumentLinkOptions {
            resolve_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        });
    }

    if build.selection_ranges {
        caps.selection_range_provider = Some(SelectionRangeProvider::Bool(true));
    }

    if build.code_lens {
        caps.code_lens_provider = Some(CodeLensOptions {
            resolve_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        });
    }

    if build.inline_values {
        caps.inline_value_provider = Some(InlineValueProvider::Bool(true));
    }

    if build.moniker {
        caps.moniker_provider = Some(MonikerProvider::Bool(true));
    }

    if build.document_color {
        caps.color_provider = Some(ColorProvider::Bool(true));
    }

    if build.call_hierarchy {
        caps.call_hierarchy_provider = Some(CallHierarchyProvider::Bool(true));
    }
}

fn semantic_token_types() -> Vec<String> {
    vec![
        SemanticTokenTypes::Namespace.into(),
        SemanticTokenTypes::Type.into(),
        SemanticTokenTypes::Class.into(),
        SemanticTokenTypes::Interface.into(),
        SemanticTokenTypes::Enum.into(),
        SemanticTokenTypes::EnumMember.into(),
        SemanticTokenTypes::TypeParameter.into(),
        SemanticTokenTypes::Function.into(),
        SemanticTokenTypes::Method.into(),
        SemanticTokenTypes::Property.into(),
        SemanticTokenTypes::Macro.into(),
        SemanticTokenTypes::Variable.into(),
        SemanticTokenTypes::Parameter.into(),
        SemanticTokenTypes::Keyword.into(),
        SemanticTokenTypes::Modifier.into(),
        SemanticTokenTypes::Comment.into(),
        SemanticTokenTypes::String.into(),
        SemanticTokenTypes::Number.into(),
        SemanticTokenTypes::Regexp.into(),
        SemanticTokenTypes::Operator.into(),
        // Perl-specific extensions:
        "sql_string".to_string(), // DBI/SQL string context (Issue #2337)
        "sql_heredoc_keyword".to_string(), // SQL keyword in <<SQL heredoc (Issue #2059)
        "json_heredoc_key".to_string(), // JSON key in <<JSON heredoc (Issue #2059)
        // The selected substrate models `label` natively (LSP 3.18); the wire
        // bytes are identical to the previous custom-string advertisement.
        SemanticTokenTypes::Label.into(),
    ]
}

fn semantic_token_modifiers() -> Vec<String> {
    vec![
        SemanticTokenModifiers::Declaration.into(),
        SemanticTokenModifiers::Definition.into(),
        SemanticTokenModifiers::Readonly.into(),
        SemanticTokenModifiers::Static.into(),
        SemanticTokenModifiers::Deprecated.into(),
        SemanticTokenModifiers::Abstract.into(),
        SemanticTokenModifiers::Async.into(),
        SemanticTokenModifiers::Modification.into(),
        SemanticTokenModifiers::Documentation.into(),
        SemanticTokenModifiers::DefaultLibrary.into(),
        // Perl-specific modifiers:
        "scalarVariable".to_string(),
        "arrayVariable".to_string(),
        "hashVariable".to_string(),
    ]
}

fn code_action_kinds(_build: &BuildFlags) -> Vec<CodeActionKind> {
    // Build code action kinds based on flags.
    let mut kinds = vec![CodeActionKind::QuickFix];

    // `source.organizeImports` is intentionally NOT advertised (#8305): its
    // only implementation was a destructive line-oriented sorter that has been
    // withdrawn from every request path. Advertisement must match runtime
    // availability; restoration requires #8319 to admit a bounded
    // source-preserving cohort and #10696 to land the proven cutover, at which
    // point advertisement returns together with a working implementation.

    // Advertise generic `refactor` plus concrete sub-kinds so clients can
    // surface the full refactoring menu and send precise `context.only`
    // filters (for example `refactor.rewrite`).
    kinds.push(CodeActionKind::Refactor);

    // REFACTOR_EXTRACT is implemented in code_actions_enhanced.rs.
    // Tests verified in lsp_code_actions_tests.rs (Issue #181).
    kinds.push(CodeActionKind::RefactorExtract);
    // Note: refactor.inline is NOT advertised because no inline action
    // is currently implemented.
    kinds.push(CodeActionKind::RefactorRewrite);

    // SOURCE_FIX_ALL aggregates every safe `quickfix` action into a single invocation.
    kinds.push(CodeActionKind::SourceFixAll);

    // SOURCE_MODERNIZE actions (3-arg open, use strict, Carp::croak, etc.)
    // are produced by modernize.rs but were missing from the advertised kinds.
    kinds.push(CodeActionKind::new("source.modernize"));

    kinds
}
