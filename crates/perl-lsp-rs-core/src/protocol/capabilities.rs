//! LSP Server Capabilities Configuration for Perl Tooling
//!
//! This module provides centralized configuration for LSP server capabilities
//! advertised to clients during Perl script development within the LSP workflow.
//! Serves as the single source of truth for feature availability and build-time
//! capability gating for optimal Perl parsing workflows.
//!
//! # LSP Workflow Integration
//!
//! - **Parse**: Provides capabilities for parsing and syntax analysis
//! - **Index**: Powers workspace symbols and cross-file navigation
//! - **Navigate**: Supports definition, reference, and hierarchy lookups
//! - **Complete**: Enables completion, signature help, and inline hints
//! - **Analyze**: Drives diagnostics, code actions, and refactoring support

use serde_json::Value;

mod experimental;
mod sections;

pub use crate::features::flags::{AdvertisedFeatures, BuildFlags};
/// Re-export `ServerCapabilities` from `lsp_types` for public access.
pub use lsp_types::ServerCapabilities;

/// Canonical completion trigger characters advertised to LSP clients.
///
/// LSP requires each trigger to be a single character. Multi-character Perl
/// operators (`->`, `::`) are supported by advertising their component chars.
#[must_use]
pub fn completion_trigger_characters() -> Vec<String> {
    vec![
        "$".to_string(),
        "@".to_string(),
        "%".to_string(),
        // Method and package separators.
        "-".to_string(),
        ">".to_string(),
        ":".to_string(),
        // String concat operator — triggers completion for chained access. (UX_GAP_03)
        ".".to_string(),
        // File path completion inside string literals.
        "/".to_string(),
        "\\".to_string(),
        "\"".to_string(),
        "'".to_string(),
    ]
}
/// Generate server capabilities from build flags
pub fn capabilities_for(build: BuildFlags) -> ServerCapabilities {
    let mut caps = ServerCapabilities::default();

    sections::apply_document_sync(&mut caps);
    sections::apply_navigation_features(&mut caps, &build);
    sections::apply_editing_features(&mut caps, &build);
    sections::apply_symbol_and_workspace_features(&mut caps, &build);
    sections::apply_analysis_features(&mut caps, &build);
    sections::apply_code_action_features(&mut caps, &build);
    sections::apply_misc_features(&mut caps, &build);
    experimental::apply_experimental_features(&mut caps, &build);

    caps
}

/// Generate capabilities as JSON Value for testing
pub fn capabilities_json(build: BuildFlags) -> Value {
    let caps = capabilities_for(build.clone());
    let mut json = serde_json::to_value(caps).unwrap_or_else(|e| {
        tracing::error!(error = %e, "Failed to serialize capabilities to JSON");
        serde_json::json!({})
    });

    // Manually add typeHierarchyProvider for LSP compatibility
    if build.type_hierarchy {
        json["typeHierarchyProvider"] = serde_json::json!({
            "workDoneProgressOptions": {}
        });
    }

    // Manually add rangesSupport (LSP 3.18) because lsp-types 0.97
    // lacks this field on DocumentRangeFormattingOptions. Multi-range formatting
    // is advertised through the existing documentRangeFormattingProvider key.
    if build.range_formatting {
        json["documentRangeFormattingProvider"] = serde_json::json!({
            "rangesSupport": true
        });
    }
    // Manually add inlineCompletionProvider (LSP 3.18) because lsp-types 0.97
    // predates this field. This JSON surface has no client context and
    // represents the static/default advertisement; runtime initialize removes
    // it when a client opts into dynamic inline-completion registration.
    if build.inline_completion {
        json["inlineCompletionProvider"] = serde_json::json!({});
    }

    // Manually add insertTextModes (LSP 3.17) because lsp-types 0.97 lacks this field.
    // We advertise PlainText (1) and Snippet (2) modes, which we already support.
    // Clients can use this to determine if they should rely on server-provided
    // insertReplaceEdit and insertTextFormat/insertTextMode negotiation.
    if build.completion
        && let Some(comp_provider) = json["completionProvider"].as_object_mut()
        && let Some(comp_item) =
            comp_provider.get_mut("completionItem").and_then(Value::as_object_mut)
    {
        comp_item.insert("insertTextModes".to_string(), serde_json::json!([1, 2]));
    }

    json
}

/// Command identifiers advertised in the `executeCommand` server capability.
///
/// This is the canonical advertised set. Request validation must accept every
/// entry here — rejecting an advertised command before dispatch would make the
/// server refuse work it just told the client it could do.
pub const SUPPORTED_COMMANDS: &[&str] = &[
    "perl.runTests",
    "perl.runFile",
    "perl.runScript",
    "perl.runTestSub",
    "perl.runCritic",
    "perl.runTest",
    "perl.runTestFile",
    "perl.runSubtest",
    "perl.debugFile",
    "perl.debugTest",
    "perl.debugTests",
    "perl.debugTestFile",
    "perl.goToTest",
    "perl.goToImplementation",
    "perl.explainProviderDecision",
    "perl.workspaceTrustReport",
    "perl.agentContext",
    "perl.previewSafeDelete",
    "perl.safeDeleteSymbol",
    "perl.previewPackageRename",
    "perl.explainMissingModuleLookup",
];

/// Get the list of supported commands for the LSP executeCommand capability.
///
/// Returns all command identifiers that can be executed via the LSP executeCommand
/// method. This list is used for capability registration and command validation.
pub fn get_supported_commands() -> Vec<String> {
    SUPPORTED_COMMANDS.iter().map(|command| (*command).to_string()).collect()
}

/// Check if a capability is a boolean or object (for flexible assertions)
pub fn cap_bool_or_object(caps: &Value, key: &str) -> bool {
    caps.get(key).is_some_and(|v| v.is_boolean() || v.is_object())
}

/// Default capabilities for the current build
pub fn default_capabilities() -> ServerCapabilities {
    #[cfg(feature = "lsp-ga-lock")]
    let flags = BuildFlags::ga_lock();

    #[cfg(not(feature = "lsp-ga-lock"))]
    let flags = BuildFlags::production();

    capabilities_for(flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::contracts::feature_ids_from_caps;
    use lsp_types::{
        CodeActionKind, CodeActionProviderCapability, OneOf, SemanticTokensServerCapabilities,
        TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncSaveOptions,
    };
    use std::collections::BTreeSet;

    /// Feature IDs that `to_feature_ids()` correctly emits but
    /// `feature_ids_from_caps()` cannot detect because lsp-types 0.97
    /// lacks the corresponding `ServerCapabilities` field.
    ///
    /// - `inline_completion`: injected in `capabilities_json()` (LSP 3.18, not in lsp-types 0.97)
    /// - `notebook_cell_execution`: sub-feature of notebook sync, no own field
    /// - `ranges_formatting`: injected in `capabilities_json()` (LSP 3.18, not in lsp-types 0.97)
    ///
    /// Note: `type_hierarchy` was previously a gap but is now advertised via
    /// `experimental` in `capabilities_for()` and detected by `feature_ids_from_caps`.
    const KNOWN_STRUCTURAL_GAPS: &[&str] =
        &["lsp.inline_completion", "lsp.notebook_cell_execution", "lsp.ranges_formatting"];

    /// Guard: feature IDs from BuildFlags must match feature IDs extracted
    /// from the ServerCapabilities that `capabilities_for()` actually builds.
    ///
    /// Any mismatch means `--features-json` under-reports or over-reports
    /// vs the actual initialize response.
    fn assert_feature_id_alignment(profile: &str, flags: BuildFlags) {
        let flag_ids: BTreeSet<&str> = flags.to_feature_ids().into_iter().collect();
        let caps = capabilities_for(flags);
        let cap_ids: BTreeSet<&str> = feature_ids_from_caps(&caps).into_iter().collect();

        let gaps: BTreeSet<&str> = KNOWN_STRUCTURAL_GAPS.iter().copied().collect();

        let in_flags_not_caps: BTreeSet<_> =
            flag_ids.difference(&cap_ids).copied().filter(|id| !gaps.contains(id)).collect();
        let in_caps_not_flags: BTreeSet<_> = cap_ids.difference(&flag_ids).collect();

        assert!(
            in_flags_not_caps.is_empty() && in_caps_not_flags.is_empty(),
            "feature ID mismatch for {profile} profile:\n  \
             in to_feature_ids() but not in capabilities: {in_flags_not_caps:?}\n  \
             in capabilities but not in to_feature_ids(): {in_caps_not_flags:?}",
        );
    }

    #[test]
    fn feature_id_alignment_ga_lock() {
        assert_feature_id_alignment("ga-lock", BuildFlags::ga_lock());
    }

    #[test]
    fn feature_id_alignment_production() {
        assert_feature_id_alignment("production", BuildFlags::production());
    }

    #[test]
    fn feature_id_alignment_all() {
        assert_feature_id_alignment("all", BuildFlags::all());
    }

    /// Verify that `documentRangeFormattingProvider.rangesSupport` is present in
    /// the JSON capabilities when `range_formatting` is enabled (LSP 3.18).
    #[test]
    fn ranges_formatting_advertised_in_json_when_enabled() {
        let flags = BuildFlags { range_formatting: true, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert_eq!(
            json.pointer("/documentRangeFormattingProvider/rangesSupport"),
            Some(&serde_json::json!(true)),
            "documentRangeFormattingProvider.rangesSupport must be present when range_formatting \
             is enabled"
        );
        assert!(
            json.get("documentRangesFormattingProvider").is_none(),
            "documentRangesFormattingProvider is not an LSP 3.18 server capability"
        );
    }

    /// Verify that multi-range formatting support is absent when disabled.
    #[test]
    fn ranges_formatting_absent_in_json_when_disabled() {
        let flags = BuildFlags { range_formatting: false, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert!(
            json.get("documentRangesFormattingProvider").is_none(),
            "documentRangesFormattingProvider must not be present when range_formatting is disabled"
        );
        assert!(
            json.pointer("/documentRangeFormattingProvider/rangesSupport").is_none(),
            "documentRangeFormattingProvider.rangesSupport must not be present when \
             range_formatting is disabled"
        );
    }

    #[test]
    fn inline_completion_advertised_as_top_level_json_when_enabled() {
        let flags = BuildFlags { inline_completion: true, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert_eq!(
            json.get("inlineCompletionProvider"),
            Some(&serde_json::json!({})),
            "inlineCompletionProvider must be advertised as an empty top-level object when enabled"
        );
    }

    #[test]
    fn inline_completion_absent_when_disabled() {
        let flags = BuildFlags { inline_completion: false, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert!(
            json.get("inlineCompletionProvider").is_none(),
            "inlineCompletionProvider must be absent when inline completion is disabled"
        );
    }

    #[test]
    fn inline_completion_not_advertised_under_experimental() {
        let flags = BuildFlags { inline_completion: true, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert!(
            json.pointer("/experimental/inlineCompletionProvider").is_none(),
            "inlineCompletionProvider must not be advertised under capabilities.experimental"
        );
    }

    /// Verify that `completionProvider.completionItem.insertTextModes` is injected
    /// as `[1, 2]` (PlainText, Snippet) in the JSON capabilities when completion is enabled.
    /// lsp-types 0.97 lacks this field, so it is manually added in `capabilities_json()`.
    #[test]
    fn insert_text_modes_advertised_in_json_when_completion_enabled() {
        let flags = BuildFlags { completion: true, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert_eq!(
            json.pointer("/completionProvider/completionItem/insertTextModes"),
            Some(&serde_json::json!([1, 2])),
            "completionProvider.completionItem.insertTextModes must be [1, 2] \
             when completion is enabled (LSP 3.17)"
        );
    }

    /// Verify that `insertTextModes` is NOT injected when completion is disabled.
    #[test]
    fn insert_text_modes_absent_when_completion_disabled() {
        let flags = BuildFlags { completion: false, ..BuildFlags::default() };
        let json = capabilities_json(flags);
        assert!(
            json.pointer("/completionProvider/completionItem/insertTextModes").is_none(),
            "completionProvider.completionItem.insertTextModes must be absent \
             when completion is disabled"
        );
    }

    /// Verify that `perl.runSubtest` is included in the supported commands list.
    #[test]
    fn test_subtest_lens_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.runSubtest"),
            "perl.runSubtest must be in get_supported_commands"
        );
    }

    /// Verify that `perl.explainProviderDecision` is included in the supported commands list.
    #[test]
    fn explain_provider_decision_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.explainProviderDecision"),
            "perl.explainProviderDecision must be in get_supported_commands"
        );
    }

    /// Verify that `perl.workspaceTrustReport` is included in the supported commands list.
    #[test]
    fn workspace_trust_report_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.workspaceTrustReport"),
            "perl.workspaceTrustReport must be in get_supported_commands"
        );
    }

    /// Verify that `perl.debugTests` (plural) is advertised alongside
    /// `perl.debugTest`. Regression guard for issue #5276 — the command was
    /// dispatched but missing from the advertised capabilities list.
    #[test]
    fn debug_tests_plural_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.debugTests"),
            "perl.debugTests must be in get_supported_commands"
        );
    }

    /// Verify that `perl.agentContext` is included in the supported commands list.
    #[test]
    fn agent_context_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.agentContext"),
            "perl.agentContext must be in get_supported_commands"
        );
    }

    /// Verify that `perl.previewSafeDelete` is included in the supported commands list.
    #[test]
    fn preview_safe_delete_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.previewSafeDelete"),
            "perl.previewSafeDelete must be in get_supported_commands"
        );
    }

    /// Verify that `perl.safeDeleteSymbol` is included in the supported commands list.
    #[test]
    fn safe_delete_symbol_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.safeDeleteSymbol"),
            "perl.safeDeleteSymbol must be in get_supported_commands"
        );
    }

    /// Verify that `perl.previewPackageRename` is included in the supported commands list.
    #[test]
    fn preview_package_rename_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.previewPackageRename"),
            "perl.previewPackageRename must be in get_supported_commands"
        );
    }

    /// Verify that `perl.explainMissingModuleLookup` is included in the supported commands list.
    #[test]
    fn explain_missing_module_lookup_command_id_is_registered() {
        let cmds = get_supported_commands();
        assert!(
            cmds.iter().any(|c| c == "perl.explainMissingModuleLookup"),
            "perl.explainMissingModuleLookup must be in get_supported_commands"
        );
    }

    /// Verify resolve providers are advertised in the full capabilities JSON.
    #[test]
    fn resolve_providers_advertised_in_full_profile() {
        let json = capabilities_json(BuildFlags::all());
        assert!(
            json["completionProvider"]["resolveProvider"].as_bool().unwrap_or(false),
            "completionProvider.resolveProvider must be true"
        );
        assert!(
            json["codeActionProvider"]["resolveProvider"].as_bool().unwrap_or(false),
            "codeActionProvider.resolveProvider must be true"
        );
        assert!(
            json["codeLensProvider"]["resolveProvider"].as_bool().unwrap_or(false),
            "codeLensProvider.resolveProvider must be true"
        );
    }

    #[test]
    fn completion_trigger_characters_include_file_path_and_perl_tokens() {
        let triggers = completion_trigger_characters();
        // `.` (string concat, UX_GAP_03) is included deliberately: it was
        // previously guarded only by the insta snapshot, so when the snapshot
        // went stale nothing asserted the trigger still existed.
        for expected in ["$", "@", "%", "-", ">", ":", ".", "/", "\\", "\"", "'"] {
            assert!(
                triggers.iter().any(|trigger| trigger == expected),
                "missing completion trigger character: {expected}"
            );
        }
    }

    #[test]
    fn text_document_sync_advertises_did_save_support() {
        let caps = capabilities_for(BuildFlags::default());
        let save = caps.text_document_sync.as_ref().and_then(|sync| match sync {
            TextDocumentSyncCapability::Options(opts) => opts.save.as_ref(),
            TextDocumentSyncCapability::Kind(_) => None,
        });

        assert_eq!(
            save,
            Some(&TextDocumentSyncSaveOptions::Supported(true)),
            "textDocumentSync.save must advertise didSave support"
        );
    }

    // --------------------------------------------------------------------------
    // Independent unit assertions for snapshot-only-guarded capability fields.
    //
    // Each of these tests pins a specific ServerCapabilities field that was
    // previously only guarded by the `lsp_cap_snap` snapshot fixtures. Snapshots
    // can be regenerated away; an independent assertion fails loudly and cannot
    // be. The expected values deliberately duplicate the source lists rather than
    // sharing a constant — sharing would recreate the single-source-of-truth gap
    // that allows silent drift (see issue #5357 and #5353/#5354 for context).
    // --------------------------------------------------------------------------

    /// Pin the `textDocumentSync.change` kind as FULL and `openClose` as true.
    ///
    /// These are the core sync options for every client — changing them silently
    /// would break incremental-edit handling for all connected editors.
    #[test]
    fn text_document_sync_advertises_full_sync_and_open_close() {
        let caps = capabilities_for(BuildFlags::default());
        match caps.text_document_sync.as_ref() {
            Some(TextDocumentSyncCapability::Options(opts)) => {
                assert_eq!(
                    opts.change,
                    Some(TextDocumentSyncKind::FULL),
                    "textDocumentSync.change must be FULL (1) — the server reparses the whole \
                     document on every didChange; INCREMENTAL would be inaccurate"
                );
                assert_eq!(
                    opts.open_close,
                    Some(true),
                    "textDocumentSync.openClose must be true — didOpen/didClose are required \
                     for workspace tracking"
                );
            }
            other => panic!("expected TextDocumentSyncCapability::Options, got {other:?}"),
        }
    }

    /// Pin the exact set of `codeActionProvider.codeActionKinds` advertised to
    /// clients.
    ///
    /// `refactor.inline` must NOT appear: no inline action is implemented and
    /// advertising it would make clients send requests the server cannot handle.
    /// `source.modernize` must appear: it was added in PR #5384 but was missing
    /// from the JSON fixtures until they were regenerated (#5357).
    /// `source.organizeImports` must NOT appear (#8305): its only implementation
    /// was a destructive line-oriented sorter, withdrawn from every request
    /// path; advertisement may return only with the proven #10696 cohort.
    ///
    /// The expected list intentionally duplicates `sections.rs::code_action_kinds`
    /// rather than sharing a constant — divergence between the two is the thing
    /// this test is meant to catch.
    #[test]
    fn code_action_kinds_include_exact_advertised_set() {
        let flags = BuildFlags { code_actions: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let kinds: Vec<String> = match caps.code_action_provider.as_ref() {
            Some(CodeActionProviderCapability::Options(opts)) => opts
                .code_action_kinds
                .as_ref()
                .expect("code_action_kinds must be Some when code_actions is enabled")
                .iter()
                .map(|k| k.as_str().to_string())
                .collect(),
            other => panic!("expected CodeActionProviderCapability::Options, got {other:?}"),
        };

        // Duplicate the full ordered list from sections.rs::code_action_kinds().
        let expected: &[&str] = &[
            "quickfix",
            "refactor",
            "refactor.extract",
            "refactor.rewrite",
            "source.fixAll",
            "source.modernize",
        ];
        assert_eq!(
            kinds.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "codeActionKinds must match the exact ordered list — extra kinds (e.g. \
             refactor.inline) or omissions (e.g. source.modernize) break client filtering"
        );
    }

    /// `source.organizeImports` is withdrawn (#8305): no build profile may
    /// advertise it while the destructive line-oriented sorter has no proven
    /// replacement. Restoration (#8319/#10696) must re-introduce advertisement
    /// together with a working implementation.
    #[test]
    fn withdrawn_organizer_kind_is_absent_from_every_profile() {
        for (profile, flags) in [
            ("default", BuildFlags::default()),
            ("production", BuildFlags::production()),
            ("ga_lock", BuildFlags::ga_lock()),
            ("all", BuildFlags::all()),
        ] {
            let caps = capabilities_for(flags);
            let kinds: Vec<String> = match caps.code_action_provider.as_ref() {
                Some(CodeActionProviderCapability::Options(opts)) => opts
                    .code_action_kinds
                    .as_ref()
                    .map(|kinds| kinds.iter().map(|k| k.as_str().to_string()).collect())
                    .unwrap_or_default(),
                // Absent provider advertises nothing — also acceptable.
                _ => Vec::new(),
            };
            assert!(
                !kinds.iter().any(|kind| kind == "source.organizeImports"),
                "{profile} profile must not advertise source.organizeImports; got {kinds:?}"
            );
        }
    }

    /// Pin the `signatureHelpProvider.triggerCharacters` as `["(", ","]`.
    ///
    /// These are the only characters that open or re-open the signature-help
    /// popup. Removing or adding characters here silently changes editor UX.
    #[test]
    fn signature_help_trigger_characters_are_paren_and_comma() {
        let flags = BuildFlags { signature_help: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let triggers = caps
            .signature_help_provider
            .as_ref()
            .expect("signatureHelpProvider must be present when signature_help is enabled")
            .trigger_characters
            .as_ref()
            .expect("signatureHelpProvider.triggerCharacters must be Some");

        // Duplicate the expected list from sections.rs — divergence is the defect.
        let expected: &[&str] = &["(", ","];
        assert_eq!(
            triggers.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "signatureHelpProvider.triggerCharacters must be exactly [\"(\", \",\"]"
        );
    }

    /// Pin the `signatureHelpProvider.retriggerCharacters`.
    ///
    /// Retrigger characters refresh the signature popup when already visible
    /// (e.g. when the user types another argument). Missing one silently removes
    /// that refresh point without any client error.
    #[test]
    fn signature_help_retrigger_characters_include_required_set() {
        let flags = BuildFlags { signature_help: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let retriggers = caps
            .signature_help_provider
            .as_ref()
            .expect("signatureHelpProvider must be present when signature_help is enabled")
            .retrigger_characters
            .as_ref()
            .expect("signatureHelpProvider.retriggerCharacters must be Some");

        // Duplicate the expected list from sections.rs.
        let expected: &[&str] = &[",", "@", "%", "{", "["];
        assert_eq!(
            retriggers.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "signatureHelpProvider.retriggerCharacters must be exactly [\",\", \"@\", \"%\", \
             \"{{\" , \"[\"]"
        );
    }

    /// Pin the complete `semanticTokensProvider.legend.tokenTypes` list.
    ///
    /// The token-type index is the wire format — client semantic-highlighting
    /// rules depend on index position, not name. Removing or reordering a type
    /// silently breaks highlighting for every client that cached the legend.
    /// The custom types (`sql_string`, `sql_heredoc_keyword`, `json_heredoc_key`,
    /// `label`) must appear at the end of the standard types.
    #[test]
    fn semantic_token_types_are_exact_ordered_list() {
        let flags = BuildFlags { semantic_tokens: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let types: Vec<String> = match caps.semantic_tokens_provider.as_ref() {
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(opts)) => {
                opts.legend.token_types.iter().map(|t| t.as_str().to_string()).collect()
            }
            other => panic!(
                "expected SemanticTokensServerCapabilities::SemanticTokensOptions, got {other:?}"
            ),
        };

        // Duplicate the full ordered list from sections.rs::semantic_token_types().
        // Index position is the wire format — order matters.
        let expected: &[&str] = &[
            "namespace",
            "type",
            "class",
            "interface",
            "enum",
            "enumMember",
            "typeParameter",
            "function",
            "method",
            "property",
            "macro",
            "variable",
            "parameter",
            "keyword",
            "modifier",
            "comment",
            "string",
            "number",
            "regexp",
            "operator",
            // Perl-specific extensions:
            "sql_string",          // DBI/SQL string context (#2337)
            "sql_heredoc_keyword", // SQL keyword in <<SQL heredoc (#2059)
            "json_heredoc_key",    // JSON key in <<JSON heredoc (#2059)
            "label", // statement labels (lsp-types 0.97 lacks SemanticTokenType::LABEL)
        ];

        assert_eq!(
            types.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "semanticTokensProvider.legend.tokenTypes must match the exact ordered list — \
             index position is the wire format, reordering or removing a type breaks clients"
        );
    }

    /// Pin the complete `semanticTokensProvider.legend.tokenModifiers` list.
    ///
    /// Token modifiers are advertised as a bitmask — each modifier occupies
    /// a bit position determined by its index in this list. Reordering or
    /// removing a modifier silently corrupts all semantic highlighting.
    #[test]
    fn semantic_token_modifiers_are_exact_ordered_list() {
        let flags = BuildFlags { semantic_tokens: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let modifiers: Vec<String> = match caps.semantic_tokens_provider.as_ref() {
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(opts)) => {
                opts.legend.token_modifiers.iter().map(|m| m.as_str().to_string()).collect()
            }
            other => panic!(
                "expected SemanticTokensServerCapabilities::SemanticTokensOptions, got {other:?}"
            ),
        };

        // Duplicate the full ordered list from sections.rs::semantic_token_modifiers().
        // Bitmask position matters — order is part of the wire contract.
        let expected: &[&str] = &[
            "declaration",
            "definition",
            "readonly",
            "static",
            "deprecated",
            "abstract",
            "async",
            "modification",
            "documentation",
            "defaultLibrary",
            // Perl-specific modifiers:
            "scalarVariable",
            "arrayVariable",
            "hashVariable",
        ];

        assert_eq!(
            modifiers.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "semanticTokensProvider.legend.tokenModifiers must match the exact ordered list — \
             bitmask position is the wire format, reordering or removing a modifier breaks clients"
        );
    }

    /// Assert `documentSymbolProvider` is advertised when the flag is enabled.
    ///
    /// This is a simple presence guard. A snapshot would catch the same thing,
    /// but an independent assertion cannot be regenerated away.
    #[test]
    fn document_symbol_provider_advertised_when_enabled() {
        let flags = BuildFlags { document_symbol: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        match caps.document_symbol_provider.as_ref() {
            Some(OneOf::Left(true)) => {}
            Some(OneOf::Left(false)) => {
                panic!(
                    "documentSymbolProvider must be true when document_symbol is enabled, \
                     not false"
                );
            }
            other => panic!(
                "documentSymbolProvider must be Some(true) when document_symbol is enabled, \
                 got {other:?}"
            ),
        }
    }

    /// Assert `documentSymbolProvider` is absent when the flag is disabled.
    #[test]
    fn document_symbol_provider_absent_when_disabled() {
        let flags = BuildFlags { document_symbol: false, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        assert!(
            caps.document_symbol_provider.is_none(),
            "documentSymbolProvider must be absent when document_symbol is disabled"
        );
    }

    /// Assert `workspaceSymbolProvider` is advertised with `resolveProvider: true`
    /// when `workspace_symbol_resolve` is enabled.
    ///
    /// The resolve variant is required for on-demand name resolution of
    /// workspace symbols without serializing the full detail up-front.
    #[test]
    fn workspace_symbol_provider_advertises_resolve_when_enabled() {
        let flags = BuildFlags { workspace_symbol_resolve: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        match caps.workspace_symbol_provider.as_ref() {
            Some(OneOf::Right(opts)) => {
                assert_eq!(
                    opts.resolve_provider,
                    Some(true),
                    "workspaceSymbolProvider.resolveProvider must be true when \
                     workspace_symbol_resolve is enabled"
                );
            }
            Some(OneOf::Left(_)) => {
                panic!(
                    "expected workspaceSymbolProvider to use the Options variant (with \
                     resolveProvider), not the simple boolean variant, when \
                     workspace_symbol_resolve is enabled"
                );
            }
            None => {
                panic!(
                    "workspaceSymbolProvider must be advertised when workspace_symbol_resolve \
                     is enabled"
                );
            }
        }
    }

    /// Assert `codeActionProvider` is absent when `code_actions` is disabled —
    /// regression guard against accidental unconditional wiring.
    #[test]
    fn code_action_provider_absent_when_disabled() {
        let flags = BuildFlags { code_actions: false, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        assert!(
            caps.code_action_provider.is_none(),
            "codeActionProvider must be absent when code_actions is disabled"
        );
    }

    /// Assert the `source.fixAll` kind is advertised alongside `quickfix`.
    ///
    /// `source.fixAll` aggregates every safe quickfix into a single invocation;
    /// clients use it for "fix all in file" commands. The kinds are independent
    /// (both must appear), so a targeted test catches if one is dropped.
    #[test]
    fn code_action_source_fix_all_and_quickfix_are_both_present() {
        let flags = BuildFlags { code_actions: true, ..BuildFlags::default() };
        let caps = capabilities_for(flags);

        let kinds: Vec<String> = match caps.code_action_provider.as_ref() {
            Some(CodeActionProviderCapability::Options(opts)) => opts
                .code_action_kinds
                .as_ref()
                .expect("code_action_kinds must be Some")
                .iter()
                .map(|k| k.as_str().to_string())
                .collect(),
            other => panic!("expected CodeActionProviderCapability::Options, got {other:?}"),
        };

        assert!(
            kinds.iter().any(|k| k == CodeActionKind::QUICKFIX.as_str()),
            "codeActionKinds must contain \"quickfix\""
        );
        assert!(
            kinds.iter().any(|k| k == CodeActionKind::SOURCE_FIX_ALL.as_str()),
            "codeActionKinds must contain \"source.fixAll\""
        );
    }
}
