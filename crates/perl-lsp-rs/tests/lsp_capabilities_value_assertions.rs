//! Independent unit assertions for LSP ServerCapabilities values.
//!
//! These assertions pin the *exact* advertised values for capability fields
//! that were previously covered only by regenerable insta snapshots. Because
//! snapshot files can be silently updated with `cargo insta review`, they
//! cannot stop a value from being quietly removed or changed — only an
//! `assert_eq!` that duplicates the expected list can do that.
//!
//! Design note: the expected values below are deliberately *duplicated* from
//! the source (they do NOT import the originating constant or call the
//! originating function). This is defence-in-depth: if a value is removed
//! from the source list, both the snapshot *and* this assertion must be
//! updated consciously. Sharing a constant would recreate the single-source
//! gap that snapshots already suffer from.
//!
//! Run: `cargo test -p perl-lsp-rs --test lsp_capabilities_value_assertions`

use perl_lsp::protocol::capabilities::{BuildFlags, capabilities_json};
use serde_json::json;

// ---------------------------------------------------------------------------
// Text document synchronisation
// ---------------------------------------------------------------------------

/// openClose MUST be true — the server tracks open/close for diagnostics refresh.
#[test]
fn text_document_sync_open_close_is_true() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["textDocumentSync"]["openClose"],
        json!(true),
        "textDocumentSync.openClose must be true"
    );
}

/// change MUST be 1 (TextDocumentSyncKind::FULL).
/// The server reparses the full document on every didChange; INCREMENTAL (2)
/// would be incorrect because no incremental AST state is maintained.
#[test]
fn text_document_sync_change_is_full() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["textDocumentSync"]["change"],
        json!(1),
        "textDocumentSync.change must be 1 (FULL); INCREMENTAL would misrepresent the server"
    );
}

/// save MUST be true — the server handles didSave for diagnostics refresh and
/// post-save hooks.
#[test]
fn text_document_sync_save_is_true() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["textDocumentSync"]["save"],
        json!(true),
        "textDocumentSync.save must be true"
    );
}

// ---------------------------------------------------------------------------
// Signature help — trigger and re-trigger characters
// ---------------------------------------------------------------------------

/// Exact trigger character list for signatureHelpProvider.
/// `(` opens an argument list; `,` advances to the next parameter position.
#[test]
fn signature_help_trigger_characters_exact() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["signatureHelpProvider"]["triggerCharacters"],
        json!(["(", ","]),
        "signatureHelpProvider.triggerCharacters must be exactly [\"(\", \",\"]"
    );
}

/// Exact re-trigger character list for signatureHelpProvider.
/// `,` re-opens after the user moves between arguments; `@`, `%`, `{`, `[`
/// cover Perl-specific argument contexts.
#[test]
fn signature_help_retrigger_characters_exact() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["signatureHelpProvider"]["retriggerCharacters"],
        json!([",", "@", "%", "{", "["]),
        "signatureHelpProvider.retriggerCharacters must be exactly [\",\", \"@\", \"%\", \"{{\", \"[\"]"
    );
}

// ---------------------------------------------------------------------------
// Completion trigger characters
// ---------------------------------------------------------------------------

/// Exact completion trigger character list.
/// Covers sigils ($, @, %), method/package separators (->, ::), string
/// concat (.), path completion (/, \), and string delimiters (", ').
#[test]
fn completion_trigger_characters_exact() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["completionProvider"]["triggerCharacters"],
        json!(["$", "@", "%", "-", ">", ":", ".", "/", "\\", "\"", "'"]),
        "completionProvider.triggerCharacters must match the canonical Perl trigger list"
    );
}

// ---------------------------------------------------------------------------
// Code action kinds
// ---------------------------------------------------------------------------

/// Exact ordered code action kind list for the production build.
/// The order follows LSP convention: quickfix first, then source.* and
/// refactor.* sub-kinds. Any omission would silently prevent clients from
/// sending `context.only` filters for the missing kind.
#[test]
fn code_action_kinds_exact_ordered_list() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["codeActionProvider"]["codeActionKinds"],
        json!([
            "quickfix",
            "source.organizeImports",
            "refactor",
            "refactor.extract",
            "refactor.rewrite",
            "source.fixAll",
            "source.modernize"
        ]),
        "codeActionProvider.codeActionKinds must be the exact ordered list"
    );
}

// ---------------------------------------------------------------------------
// Semantic token legend
// ---------------------------------------------------------------------------

/// Exact ordered token type list (24 types).
/// Token indexes returned by the provider are positional offsets into this
/// list. Any reordering silently breaks editor highlighting.
#[test]
fn semantic_token_types_exact_ordered_list() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["semanticTokensProvider"]["legend"]["tokenTypes"],
        json!([
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
            "sql_string",
            "sql_heredoc_keyword",
            "json_heredoc_key",
            "label"
        ]),
        "semanticTokensProvider.legend.tokenTypes must be the exact ordered list"
    );
}

/// Exact ordered token modifier list (13 modifiers).
/// Modifier bits are positional (bit 0 = first modifier). Any reordering
/// silently misrepresents the semantic modifier sent to the client.
#[test]
fn semantic_token_modifiers_exact_ordered_list() {
    let caps = capabilities_json(BuildFlags::production());
    assert_eq!(
        caps["semanticTokensProvider"]["legend"]["tokenModifiers"],
        json!([
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
            "scalarVariable",
            "arrayVariable",
            "hashVariable"
        ]),
        "semanticTokensProvider.legend.tokenModifiers must be the exact ordered list"
    );
}

// ---------------------------------------------------------------------------
// Execute command — registered command IDs
// ---------------------------------------------------------------------------

/// Exact set of command IDs advertised in executeCommandProvider.
/// Every command here must be implemented; removing one silently breaks
/// editor integrations that rely on it. Adding one without implementation
/// causes LSP errors when the client invokes it.
///
/// NOTE: executeCommandProvider is only compiled on non-wasm32 targets.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn execute_command_ids_exact_set() {
    let caps = capabilities_json(BuildFlags::production());
    let actual_commands: Vec<String> = caps["executeCommandProvider"]["commands"]
        .as_array()
        .expect("executeCommandProvider.commands must be an array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();

    let expected_commands: Vec<&str> = vec![
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

    // Assert exact count before the element-by-element check so a length
    // mismatch is reported clearly.
    assert_eq!(
        actual_commands.len(),
        expected_commands.len(),
        "executeCommandProvider.commands count mismatch: got {}, expected {}",
        actual_commands.len(),
        expected_commands.len()
    );

    for cmd in &expected_commands {
        assert!(
            actual_commands.iter().any(|c| c == cmd),
            "expected command '{}' not found in executeCommandProvider.commands",
            cmd
        );
    }
}

// ---------------------------------------------------------------------------
// Legend count invariants
// ---------------------------------------------------------------------------

/// The semantic token type list MUST have exactly 24 entries.
/// This is a separate guard so a count change is flagged even if the ordered
/// list assertion happens to pass after a partial reorder.
#[test]
fn semantic_token_types_count_is_24() {
    let caps = capabilities_json(BuildFlags::production());
    let types = caps["semanticTokensProvider"]["legend"]["tokenTypes"]
        .as_array()
        .expect("tokenTypes must be an array");
    assert_eq!(
        types.len(),
        24,
        "semanticTokensProvider.legend.tokenTypes must have exactly 24 entries, got {}",
        types.len()
    );
}

/// The semantic token modifier list MUST have exactly 13 entries.
#[test]
fn semantic_token_modifiers_count_is_13() {
    let caps = capabilities_json(BuildFlags::production());
    let mods = caps["semanticTokensProvider"]["legend"]["tokenModifiers"]
        .as_array()
        .expect("tokenModifiers must be an array");
    assert_eq!(
        mods.len(),
        13,
        "semanticTokensProvider.legend.tokenModifiers must have exactly 13 entries, got {}",
        mods.len()
    );
}

/// The execute command list MUST have exactly 21 entries.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn execute_command_count_is_21() {
    let caps = capabilities_json(BuildFlags::production());
    let commands = caps["executeCommandProvider"]["commands"]
        .as_array()
        .expect("executeCommandProvider.commands must be an array");
    assert_eq!(
        commands.len(),
        21,
        "executeCommandProvider.commands must have exactly 21 entries, got {}",
        commands.len()
    );
}
