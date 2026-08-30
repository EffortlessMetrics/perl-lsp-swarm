//! LSP capabilities handling
//!
//! Handles client capability parsing and server capabilities construction.

use super::super::{JsonRpcError, LspServer, Ordering};
use perl_workspace::folder::{extract_workspace_folder_uris, root_path_to_file_uri};
use serde_json::{Value, json};

/// Typed TextDocumentSyncOptions for ServerCapabilities construction (#4995).
///
/// Replaces inline json!() for the textDocumentSync field of
/// ServerCapabilities with a typed struct that can be serialized
/// directly, preventing field name drift.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TextDocumentSyncOptions {
    open_close: bool,
    change: i32,
    will_save: bool,
    will_save_wait_until: bool,
    save: SaveOptions,
}

/// Save options for TextDocumentSyncOptions.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveOptions {
    include_text: bool,
}

impl TextDocumentSyncOptions {
    fn new(change: i32) -> Self {
        Self {
            open_close: true,
            change,
            will_save: true,
            // Formatter-owned willSaveWaitUntil is withdrawn (#11955): the
            // save-owner decision is #8092's and a second unproven edit
            // producer must not be advertised. Direct requests receive the
            // truthful method-not-advertised refusal.
            will_save_wait_until: false,
            save: SaveOptions { include_text: true },
        }
    }
}

/// File-operation requests the client explicitly declared it can send.
///
/// LSP file-operation support is negotiated per operation. A server that
/// advertises an operation the client omitted creates a false capability
/// surface even when the client never happens to invoke it.
#[derive(Debug, Clone, Copy, Default)]
struct FileOperationSupport {
    will_create: bool,
    did_create: bool,
    will_rename: bool,
    did_rename: bool,
    will_delete: bool,
    did_delete: bool,
}

impl FileOperationSupport {
    fn from_initialize_params(params: Option<&Value>) -> Self {
        let supported = |path: &str| {
            params.and_then(|params| params.pointer(path)).and_then(Value::as_bool).unwrap_or(false)
        };

        Self {
            will_create: supported("/capabilities/workspace/fileOperations/willCreate"),
            did_create: supported("/capabilities/workspace/fileOperations/didCreate"),
            will_rename: supported("/capabilities/workspace/fileOperations/willRename"),
            did_rename: supported("/capabilities/workspace/fileOperations/didRename"),
            will_delete: supported("/capabilities/workspace/fileOperations/willDelete"),
            did_delete: supported("/capabilities/workspace/fileOperations/didDelete"),
        }
    }

    fn insert_capabilities(self, target: &mut serde_json::Map<String, Value>, filters: &[Value]) {
        for (name, supported) in [
            ("willCreate", self.will_create),
            ("didCreate", self.did_create),
            ("willRename", self.will_rename),
            ("didRename", self.did_rename),
            ("willDelete", self.will_delete),
            ("didDelete", self.did_delete),
        ] {
            if supported {
                target.insert(name.to_string(), json!({ "filters": filters }));
            }
        }
    }
}

/// Build workspace capabilities, intersecting file operations with the exact
/// operations the client declared during initialize.
fn workspace_capabilities(
    workspace_folders_support: bool,
    file_operations: FileOperationSupport,
) -> Value {
    let perl_globs = ["**/*.pl", "**/*.pm", "**/*.t", "**/*.psgi"];
    let filters: Vec<Value> =
        perl_globs.iter().map(|glob| json!({ "pattern": { "glob": glob } })).collect();
    let mut file_operation_capabilities = serde_json::Map::new();
    file_operations.insert_capabilities(&mut file_operation_capabilities, &filters);

    let mut workspace = json!({
        "workspaceFolders": {
            "supported": workspace_folders_support,
            "changeNotifications": true
        },
        "textDocumentContent": {
            "schemes": ["perldoc"]
        }
    });
    if !file_operation_capabilities.is_empty() {
        workspace["fileOperations"] = Value::Object(file_operation_capabilities);
    }
    workspace
}

/// The LSP protocol version this server implements.
///
/// Advertised in the `initialize` result's `protocolVersion` field (LSP 3.17+).
/// The server uses 3.18 extensions (e.g. `inlineCompletionProvider`), so the
/// advertised version reflects the highest spec whose features are surfaced.
const LSP_PROTOCOL_VERSION: &str = "3.18";

fn is_opencode_client(params: &Value) -> bool {
    params
        .get("clientInfo")
        .and_then(|info| info.get("name"))
        .and_then(|name| name.as_str())
        .map(|name| name.to_ascii_lowercase().contains("opencode"))
        .unwrap_or(false)
}

fn is_jetbrains_client(params: &Value) -> bool {
    params
        .get("clientInfo")
        .and_then(|info| info.get("name"))
        .and_then(|name| name.as_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower.contains("jetbrains") || lower.contains("intellij") || lower.contains("idea")
        })
        .unwrap_or(false)
}

fn merge_experimental_capability(capabilities: &mut Value, key: &str, value: Value) {
    if !capabilities.get("experimental").is_some_and(Value::is_object) {
        let Some(capabilities_object) = capabilities.as_object_mut() else {
            tracing::warn!("Failed to merge experimental capability into non-object capabilities");
            return;
        };
        capabilities_object
            .insert("experimental".to_string(), Value::Object(serde_json::Map::new()));
    }

    let Some(experimental) = capabilities.get_mut("experimental").and_then(Value::as_object_mut)
    else {
        tracing::warn!("Failed to merge experimental capability into non-object value");
        return;
    };

    experimental.insert(key.to_string(), value);
}

fn code_action_documentation_entries() -> Value {
    json!([
        {
            "kind": "quickfix",
            "command": {
                "title": "Explain Perl quick fixes",
                "command": "perl.explainProviderDecision",
                "arguments": [{
                    "provider": "diagnostics",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_quickfix"
                }]
            }
        },
        {
            "kind": "refactor",
            "command": {
                "title": "Explain Perl refactors",
                "command": "perl.explainProviderDecision",
                "arguments": [{
                    "provider": "rename",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_refactor"
                }]
            }
        },
        {
            "kind": "source.fixAll",
            "command": {
                "title": "Explain Perl fix-all actions",
                "command": "perl.explainProviderDecision",
                "arguments": [{
                    "provider": "diagnostics",
                    "receipt_id": "docs/specs/PLSP-SPEC-0029-lsp-318-conformance-boundary.md#code-action-documentation",
                    "scenario": "lsp_318_code_action_documentation_fix_all"
                }]
            }
        }
    ])
}

impl LspServer {
    /// Handle initialize request
    pub(crate) fn handle_initialize(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Atomically check and set initialize_requested
        if self
            .initialize_requested
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(JsonRpcError {
                code: -32600, // InvalidRequest per LSP spec 3.17
                message: "initialize may only be sent once".to_string(),
                data: None,
            });
        }

        // Classify the client's position-encoding offer and construct the
        // complete text-sync session contract candidate BEFORE any capability,
        // workspace, or configuration mutation (#9378). A rejection here
        // fails initialize with a typed error and publishes no state: no
        // accepted contract, no partial client capabilities, no workspace
        // side effects. The one-shot guard above records the attempted
        // request; it never makes a rejected initialize look accepted.
        let session_contract = super::session_contract::TextSyncSessionContract::accept(
            params.as_ref(),
            super::session_contract::next_session_id(),
        )
        .map_err(|rejection| rejection.to_jsonrpc_error())?;

        // Parse client capabilities
        if let Some(params) = &params {
            // Take lock once to write all capabilities
            {
                let mut caps = self.client_capabilities.lock();

                caps.declaration_link_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("declaration"))
                    .and_then(|d| d.get("linkSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                caps.definition_link_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("definition"))
                    .and_then(|d| d.get("linkSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                caps.type_definition_link_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("typeDefinition"))
                    .and_then(|d| d.get("linkSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                caps.implementation_link_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("implementation"))
                    .and_then(|d| d.get("linkSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                // Check if client supports dynamic registration for file watching
                caps.dynamic_registration_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("workspace"))
                    .and_then(|w| w.get("didChangeWatchedFiles"))
                    .and_then(|d| d.get("dynamicRegistration"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                caps.file_watcher_relative_pattern_support = params
                    .pointer("/capabilities/workspace/didChangeWatchedFiles/relativePatternSupport")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                caps.inline_completion_support =
                    params.pointer("/capabilities/textDocument/inlineCompletion").is_some();
                caps.inline_completion_dynamic_registration_support = params
                    .pointer("/capabilities/textDocument/inlineCompletion/dynamicRegistration")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                // JetBrains-family IDEs (IntelliJ IDEA, etc.) advertise dynamic watcher
                // registration but their registration flow is unreliable and can degrade LSP
                // startup behavior. Force-disable for these clients regardless of what the
                // capabilities object claims.
                if is_jetbrains_client(params) {
                    caps.dynamic_registration_support = false;
                    // Queue a one-time logMessage so the user can see the override
                    // happened. The message is emitted after the `initialized`
                    // notification arrives (see handle_initialized_dispatch)
                    // because the LSP spec discourages sending notifications
                    // before the initialize response is delivered (#4630).
                    let client_name = params
                        .get("clientInfo")
                        .and_then(|info| info.get("name"))
                        .and_then(|name| name.as_str())
                        .unwrap_or("JetBrains");
                    *self.pending_startup_log.lock() = Some(format!(
                        "Perl LSP: Dynamic file-watcher registration has been disabled for \
                         JetBrains-family client \"{client_name}\" because its registration \
                         flow is unreliable. Workspace/didChangeWatchedFiles dynamic \
                         registration requests from this client will be ignored."
                    ));
                }

                caps.workspace_configuration_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("workspace"))
                    .and_then(|w| w.get("configuration"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                caps.workspace_apply_edit_support = params
                    .pointer("/capabilities/workspace/applyEdit")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.workspace_folders_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("workspace"))
                    .and_then(|w| w.get("workspaceFolders"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                // Check if client supports snippet syntax in completion items.
                //
                // Spec-compliant clients send this under
                // textDocument.completion.completionItem.*, but some generic
                // clients flatten these booleans directly onto
                // textDocument.completion. Support both shapes.
                caps.snippet_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("completion"))
                    .and_then(|comp| {
                        comp.get("completionItem")
                            .and_then(|ci| ci.get("snippetSupport"))
                            .or_else(|| comp.get("snippetSupport"))
                    })
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                caps.completion_commit_characters_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("completion"))
                    .and_then(|comp| {
                        comp.get("completionItem")
                            .and_then(|ci| ci.get("commitCharactersSupport"))
                            .or_else(|| comp.get("commitCharactersSupport"))
                    })
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                caps.label_details_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("completion"))
                    .and_then(|comp| comp.get("completionItem"))
                    .and_then(|ci| ci.get("labelDetailsSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                caps.completion_list_item_defaults_data_support = params
                    .pointer("/capabilities/textDocument/completion/completionList/itemDefaults")
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("data")));
                caps.completion_list_apply_kind_support = params
                    .pointer(
                        "/capabilities/textDocument/completion/completionList/applyKindSupport",
                    )
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.workspace_edit_document_changes_support = params
                    .pointer("/capabilities/workspace/workspaceEdit/documentChanges")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.workspace_edit_snippet_edit_support = params
                    .pointer("/capabilities/workspace/workspaceEdit/snippetEditSupport")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.workspace_edit_metadata_support = params
                    .pointer("/capabilities/workspace/workspaceEdit/metadataSupport")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.code_action_documentation_support = params
                    .pointer("/capabilities/textDocument/codeAction/documentationSupport")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.code_action_disabled_support = params
                    .pointer("/capabilities/textDocument/codeAction/disabledSupport")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                caps.code_action_llm_generated_tag_support = params
                    .pointer("/capabilities/textDocument/codeAction/tagSupport/valueSet")
                    .and_then(Value::as_array)
                    .is_some_and(|tags| tags.iter().any(|tag| tag.as_i64() == Some(1)));
                caps.prepare_support_default_behavior = params
                    .pointer("/capabilities/textDocument/rename/prepareSupportDefaultBehavior")
                    .and_then(Value::as_u64)
                    .map(|v| u8::from(v == 1))
                    .unwrap_or(0);

                // Check if client supports markdown message content in diagnostics (LSP 3.18)
                caps.markup_message_support = params
                    .get("capabilities")
                    .and_then(|c| c.get("textDocument"))
                    .and_then(|td| td.get("diagnostic"))
                    .and_then(|d| d.get("markupMessageSupport"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);

                // Check if client supports markdown in MarkupContent (hover,
                // completion, signature help). LSP 3.17 general.markup.contentFormat
                // is the canonical source; textDocument.hover.contentFormat is the
                // legacy fallback. Default to true since most clients support
                // markdown (#1724).
                caps.markdown_support = {
                    let general_format = params
                        .pointer("/capabilities/general/markup/contentFormat")
                        .and_then(|v| v.as_array());
                    let hover_format = params
                        .pointer("/capabilities/textDocument/hover/contentFormat")
                        .and_then(|v| v.as_array());
                    let format = general_format.or(hover_format);
                    match format {
                        Some(arr) => {
                            arr.iter().any(|v| v.as_str().is_some_and(|s| s == "markdown"))
                        }
                        None => true, // Default: assume markdown support
                    }
                };

                // Check if client supports refresh requests for various features
                if let Some(cap_val) = params.get("capabilities") {
                    // workspace/codeLens/refresh
                    caps.code_lens_refresh_support = cap_val
                        .pointer("/workspace/codeLens/refreshSupport")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // textDocument/codeLens resolveSupport.properties
                    if let Some(properties) =
                        cap_val.pointer("/textDocument/codeLens/resolveSupport/properties")
                    {
                        let props: std::collections::HashSet<String> = properties
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        caps.code_lens_resolve_support = Some(props);
                    }

                    // workspace/semanticTokens/refresh
                    caps.semantic_tokens_refresh_support = cap_val
                        .pointer("/workspace/semanticTokens/refreshSupport")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // workspace/inlayHint/refresh
                    caps.inlay_hint_refresh_support = cap_val
                        .pointer("/workspace/inlayHint/refreshSupport")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // textDocument/inlayHint
                    caps.inlay_hint_support = cap_val
                        .pointer("/textDocument/inlayHint/staticRegistration")
                        .or_else(|| cap_val.pointer("/textDocument/inlayHint"))
                        .is_some();

                    // workspace/inlineValue/refresh
                    caps.inline_value_refresh_support = cap_val
                        .pointer("/workspace/inlineValue/refreshSupport")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    caps.inline_completion_support =
                        cap_val.pointer("/textDocument/inlineCompletion").is_some();

                    caps.inline_completion_dynamic_registration_support = cap_val
                        .pointer("/textDocument/inlineCompletion/dynamicRegistration")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);

                    // workspace/diagnostic/refresh
                    //
                    // Two spellings are accepted on purpose — do not "clean this up":
                    // - `workspace.diagnostics` (plural) is the spec key. LSP 3.17
                    //   names it `ClientCapabilities.workspace.diagnostics?:
                    //   DiagnosticWorkspaceClientCapabilities` (confirmed against the
                    //   published metaModel, version 3.17.0). It is preferred here.
                    // - `workspace.diagnostic` (singular) is a known client-side
                    //   deviation, not a spec reading. `lsp-types` (and its
                    //   `helix-lsp-types` fork) declare the field as
                    //   `pub diagnostic: Option<DiagnosticWorkspaceClientCapabilities>`
                    //   under `#[serde(rename_all = "camelCase")]` with no per-field
                    //   rename, so real Helix and other `lsp-types`-based clients put
                    //   `diagnostic` on the wire. Dropping it would regress them.
                    //
                    // The sibling `textDocument.diagnostic` *is* singular per spec,
                    // which is the source of the confusion. See issue #9592.
                    caps.diagnostic_refresh_support = cap_val
                        .pointer("/workspace/diagnostics/refreshSupport")
                        .or_else(|| cap_val.pointer("/workspace/diagnostic/refreshSupport"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // workspace/foldingRange/refresh
                    caps.folding_range_refresh_support = cap_val
                        .pointer("/workspace/foldingRange/refreshSupport")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // window/showDocument
                    caps.show_document_support = cap_val
                        .pointer("/window/showDocument/support")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // window/workDoneProgress
                    caps.work_done_progress_support = cap_val
                        .pointer("/window/workDoneProgress")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // textDocument/inlayHint resolveSupport.properties
                    // Collect the property names the client can resolve (e.g. "label.location")
                    if let Some(properties) =
                        cap_val.pointer("/textDocument/inlayHint/resolveSupport/properties")
                    {
                        let props: std::collections::HashSet<String> = properties
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        caps.inlay_hint_resolve_support = Some(props);
                    }
                }
                // Position encoding is NOT stored on `client_capabilities`:
                // the accepted text-sync session contract constructed before
                // this block is the single authority for the wire encoding
                // and sync kind (#9378). Keeping a separately negotiated
                // value here would let later code infer one encoding while
                // the response advertises another.
            } // caps lock released here

            // Check if client supports pull diagnostics.
            //
            // OpenCode currently relies on push diagnostics (publishDiagnostics)
            // even when it advertises textDocument.diagnostic capability.
            // Treat it as push-only to avoid suppressing diagnostics.
            let is_opencode = is_opencode_client(params);
            let supports_pull = params
                .get("capabilities")
                .and_then(|c| c.get("textDocument"))
                .and_then(|td| td.get("diagnostic"))
                .is_some();

            if supports_pull && !is_opencode {
                self.client_supports_pull_diags.store(true, Ordering::Relaxed);
                tracing::debug!(
                    "Client supports pull diagnostics - suppressing automatic publishing"
                );
            } else if supports_pull && is_opencode {
                tracing::debug!(
                    "OpenCode client detected - keeping push diagnostics enabled despite \
                     textDocument.diagnostic capability"
                );
            }

            // Initialize workspace folders
            if let Some(workspace_folders) =
                params.get("workspaceFolders").and_then(|f| f.as_array())
            {
                let uris = extract_workspace_folder_uris(workspace_folders);
                if let Some(first_uri) = uris.first() {
                    self.set_root_uri(first_uri);
                }

                let mut folders = self.workspace_folders.lock();
                for uri in uris {
                    tracing::debug!(uri, "Initialized with workspace folder");
                    let mut folder =
                        super::super::workspace_folder::WorkspaceFolderState::new(uri.clone());
                    if let Some(path) = super::super::source_path_from_uri(&uri) {
                        folder = folder.with_path(path);
                    }
                    folders.push(folder);
                }
            } else if let Some(root_uri) = params.get("rootUri").and_then(|u| u.as_str()) {
                // Fallback to rootUri if workspaceFolders is not provided
                let mut folders = self.workspace_folders.lock();
                tracing::debug!(root_uri, "Initialized with root URI");
                let mut folder =
                    super::super::workspace_folder::WorkspaceFolderState::new(root_uri.to_string());
                if let Some(path) = super::super::source_path_from_uri(root_uri) {
                    folder = folder.with_path(path);
                }
                folders.push(folder);
                // Also set the root path for module resolution
                self.set_root_uri(root_uri);
            } else if let Some(root_path) = params.get("rootPath").and_then(|p| p.as_str()) {
                // Legacy fallback: rootPath is deprecated since LSP 3.0 but still sent by some clients
                // (including older JetBrains LSP clients).
                tracing::debug!(root_path, "Initialized with legacy rootPath");
                let root_uri = root_path_to_file_uri(root_path);
                let mut folder =
                    super::super::workspace_folder::WorkspaceFolderState::new(root_uri.clone());
                // Preserve filesystem path metadata so project-config loading and other
                // path-based workflows behave the same as rootUri/workspaceFolders initialization.
                folder = folder.with_path(std::path::PathBuf::from(root_path));
                let mut folders = self.workspace_folders.lock();
                folders.push(folder);
                self.set_root_uri(&root_uri);
            } else if let Some(init_options) = params.get("initializationOptions") {
                // Compatibility fallback for clients that place workspace roots in
                // initializationOptions instead of top-level initialize params.
                if let Some(workspace_folders) =
                    init_options.get("workspaceFolders").and_then(|f| f.as_array())
                {
                    let uris = extract_workspace_folder_uris(workspace_folders);
                    // Mirror top-level workspaceFolders: set root URI from first folder.
                    if let Some(first_uri) = uris.first() {
                        self.set_root_uri(first_uri);
                    }
                    let mut folders = self.workspace_folders.lock();
                    for uri in uris {
                        tracing::debug!(
                            uri,
                            "Initialized with workspace folder from initializationOptions"
                        );
                        let mut folder =
                            super::super::workspace_folder::WorkspaceFolderState::new(uri.clone());
                        if let Some(path) = super::super::source_path_from_uri(&uri) {
                            folder = folder.with_path(path);
                        }
                        folders.push(folder);
                    }
                } else if let Some(root_uri) = init_options.get("rootUri").and_then(|u| u.as_str())
                {
                    let mut folders = self.workspace_folders.lock();
                    tracing::debug!(
                        root_uri,
                        "Initialized with root URI from initializationOptions"
                    );
                    let mut folder = super::super::workspace_folder::WorkspaceFolderState::new(
                        root_uri.to_string(),
                    );
                    if let Some(path) = super::super::source_path_from_uri(root_uri) {
                        folder = folder.with_path(path);
                    }
                    folders.push(folder);
                    self.set_root_uri(root_uri);
                } else if let Some(root_path) =
                    init_options.get("rootPath").and_then(|p| p.as_str())
                {
                    tracing::debug!(
                        root_path,
                        "Initialized with legacy rootPath from initializationOptions"
                    );
                    let root_uri = root_path_to_file_uri(root_path);
                    let mut folders = self.workspace_folders.lock();
                    folders.push(super::super::workspace_folder::WorkspaceFolderState::new(
                        root_uri.clone(),
                    ));
                    self.set_root_uri(&root_uri);
                }
            } else if let Ok(cwd) = std::env::current_dir() {
                // Compatibility fallback for lightweight clients (for example Aider)
                // that initialize without workspaceFolders/rootUri/rootPath.
                let cwd_uri = root_path_to_file_uri(&cwd.to_string_lossy());
                let mut folders = self.workspace_folders.lock();
                folders.push(super::super::workspace_folder::WorkspaceFolderState::new(
                    cwd_uri.clone(),
                ));
                self.set_root_uri(&cwd_uri);
                tracing::debug!(cwd_uri, "Initialized with process current directory fallback");
            }
        }

        // Apply initializationOptions.perl.* as the base config layer.
        // This is parsed before .perl-lsp.toml so project config overrides it,
        // and subsequent workspace/configuration responses override both.
        if let Some(params) = params.as_ref()
            && let Some(init_options) = params.get("initializationOptions")
            && let Some(perl) = super::super::workspace::extract_perl_settings(init_options)
        {
            tracing::debug!("Applying initializationOptions.perl.* as base config layer");
            {
                let mut config = self.config.lock();
                config.update_from_value(perl);
            }
            {
                let mut workspace_config = self.workspace_config.lock();
                workspace_config.update_from_value(perl);
            }
            if let Ok(mut limits) = perl_lsp_rs_core::runtime::limits::LSP_LIMITS.write() {
                limits.update_from_value(perl);
            }
            *self.initialization_options_perl_settings.lock() = Some(perl.clone());
        }

        // Load .perl-lsp.toml from workspace root (init options base layer; LSP config overrides later)
        self.load_and_apply_project_config();

        // Detect Perl interpreter and surface an actionable error if not found.
        // Runs after config load, but note that no client channel can supply an
        // interpreter path: workspace-supplied `perlPath` is refused above for
        // security (#3729) and no such editor setting exists. See
        // `check_perl_interpreter` for the full reasoning (#5034).
        self.check_perl_interpreter();

        // Construct the AI inline-completion backend if enabled in config
        self.refresh_ai_backend();

        // Check for available tools quickly with a timeout
        // Use which/where command which is much faster than spawning the actual tools
        let has_perltidy = self.detect_tool("perltidy");
        let has_perlcritic = self.detect_tool("perlcritic");

        tracing::debug!(perltidy = has_perltidy, perlcritic = has_perlcritic, "Tool availability");

        // TextDocumentSyncKind::Full (1): the server always reparses the full
        // document on every didChange notification.  Advertising Incremental (2)
        // would be inaccurate — we do not maintain incremental AST state between
        // edits; we rebuild the entire AST from the complete document text each
        // time. The wire values below come from the accepted session contract
        // constructed at the top of this handler (#9378); they are never
        // authored independently here.
        let sync_kind = session_contract.sync_kind().wire_value();

        // Build capabilities using catalog-driven approach
        let profile = self.feature_profile();
        let mut build_flags = profile.runtime_flags(has_perltidy);

        // Read user-disabled features from initializationOptions.
        //
        // Supported shapes:
        //   1) { "disabledFeatures": [...] }
        //   2) { "perl-lsp": { "disabledFeatures": [...] } }
        //   3) { "perl_lsp": { "disabledFeatures": [...] } }
        //
        // Some generic LSP clients namespace server settings under the server id,
        // while others pass options at the top level.
        if let Some(init_opts) = params.as_ref().and_then(|p| p.get("initializationOptions")) {
            for id in disabled_feature_ids_from_init_options(init_opts) {
                apply_disabled_feature_id(&mut build_flags, id);
            }
        }

        // Persist advertised features for gating
        let features = build_flags.to_advertised_features();
        *self.advertised_features.lock() = features.clone();
        *self.advertised_feature_ids.lock() = build_flags.to_feature_ids();

        // Generate capabilities from build flags
        //
        // `capabilities_json()` is the static/default capability surface and
        // has no client context. Runtime initialize may remove standard LSP
        // fields such as inlineCompletionProvider when the client asks for
        // dynamic registration for the same selector.
        let mut capabilities =
            crate::protocol::capabilities::capabilities_json(build_flags.clone());
        let (inline_completion_support, inline_completion_dynamic_registration_support) = {
            let client_capabilities = self.client_capabilities.lock();
            (
                client_capabilities.inline_completion_support,
                client_capabilities.inline_completion_dynamic_registration_support,
            )
        };

        match (
            features.inline_completion,
            inline_completion_support,
            inline_completion_dynamic_registration_support,
        ) {
            // LSP 3.18 dynamic registration is an alternate registration mode,
            // not an addition to static registration for the same selector.
            (true, true, true) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
            (true, true, false) => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.insert(
                        "inlineCompletionProvider".to_string(),
                        Value::Object(serde_json::Map::new()),
                    );
                }
            }
            _ => {
                if let Some(capabilities) = capabilities.as_object_mut() {
                    capabilities.remove("inlineCompletionProvider");
                }
            }
        }

        // Advertised wire position encoding and sync kind: derived from the
        // accepted text-sync session contract (#9378), never authored here.
        //
        // Release envelope (#8129 branch `full_document_utf16`): the contract
        // only ever holds FULL + UTF-16, and a client whose offer excludes
        // UTF-16 was already rejected before any state mutation. Providers
        // still compute positions in UTF-16 code units, so response, stored
        // session state, and provider behavior all share one encoding.
        capabilities["positionEncoding"] =
            Value::String(session_contract.position_encoding().wire_name().to_string());
        if features.declaration {
            capabilities["declarationProvider"] = Value::Bool(true);
        }
        let code_action_documentation_support =
            self.client_capabilities.lock().code_action_documentation_support;
        if features.code_action && code_action_documentation_support {
            if let Some(code_action_provider) =
                capabilities.get_mut("codeActionProvider").and_then(Value::as_object_mut)
            {
                code_action_provider
                    .insert("documentation".to_string(), code_action_documentation_entries());
            } else {
                tracing::warn!(
                    "Cannot advertise CodeAction.documentation; codeActionProvider is not an object"
                );
            }
        }
        // Override text document sync with typed struct (#4995); the change
        // kind comes from the accepted session contract (#9378).
        capabilities["textDocumentSync"] =
            serde_json::to_value(TextDocumentSyncOptions::new(sync_kind))
                .unwrap_or_else(|_| json!({"openClose": true, "change": sync_kind}));

        // Workspace capabilities: intersect client-dependent file-operation
        // participation with the exact initialize declaration (#7682).
        let workspace_folders_support = self.client_capabilities.lock().workspace_folders_support;
        let file_operations = FileOperationSupport::from_initialize_params(params.as_ref());
        capabilities["workspace"] =
            workspace_capabilities(workspace_folders_support, file_operations);

        // Advertise experimental custom requests only to clients that declared
        // the corresponding standard inline-completion capability.
        if features.inline_completion && inline_completion_support {
            merge_experimental_capability(
                &mut capabilities,
                "perlInlineCompletionStream",
                Value::Bool(true),
            );
        }

        let result = json!({
            "capabilities": capabilities,
            "protocolVersion": LSP_PROTOCOL_VERSION,
            "serverInfo": {
                "name": "perl-lsp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        // Note: the initialize result wrapper is kept as json!() because it
        // is the final envelope wrapping the dynamically-built capabilities
        // object — a typed InitializeResult struct would need to own the
        // capabilities Value, adding indirection without type safety benefit.

        // Response/contract divergence is a typed internal failure, never a
        // silent drift (#9378): the published InitializeResult must be the
        // one derived from the accepted session value.
        super::session_contract::verify_response_matches_contract(&session_contract, &result)?;

        // Atomically accept the initialized session: contract + response
        // digest are stored together, exactly once, after verification.
        let response_digest = digest_result(&result);
        self.accept_text_sync_session(session_contract, response_digest)?;

        // Bounded initialize evidence (offer, selection, sync kind, encoding,
        // and both digests) becomes observable at acceptance — the doctor/
        // receipt projection derives from the same stored session.
        if let Some(session) = self.accepted_text_sync_session() {
            tracing::info!(
                evidence = serde_json::to_string(&session.evidence())
                    .unwrap_or_else(|_| "serialization-unavailable".to_string()),
                "text-sync session contract accepted (#9378)"
            );
        }

        Ok(Some(result))
    }
}

/// Bounded digest over the exact initialize result payload, recorded with the
/// accepted session so evidence can prove response/state agreement.
fn digest_result(result: &Value) -> String {
    match serde_json::to_string(result) {
        Ok(serialized) => super::session_contract::digest_bytes(serialized.as_bytes()),
        // serde_json serialization of a JSON Value cannot fail; the fallback
        // keeps the digest total without inventing a fake payload digest.
        Err(_) => "unavailable".to_string(),
    }
}

/// Zero the `BuildFlags` field corresponding to the given feature ID.
///
/// Feature IDs use the canonical `lsp.*` format from `perl-lsp-feature-ids`
/// (e.g. `"lsp.semantic_tokens"`). Unknown IDs are logged and ignored.
pub(crate) fn apply_disabled_feature_id(
    flags: &mut crate::protocol::capabilities::BuildFlags,
    id: &str,
) {
    match id {
        "lsp.completion" => flags.completion = false,
        "lsp.hover" => flags.hover = false,
        "lsp.definition" => flags.definition = false,
        "lsp.declaration" => flags.declaration = false,
        "lsp.references" => flags.references = false,
        "lsp.document_symbol" => flags.document_symbol = false,
        "lsp.workspace_symbol" => flags.workspace_symbol = false,
        "lsp.code_action" => flags.code_actions = false,
        "lsp.code_lens" => flags.code_lens = false,
        "lsp.rename" => flags.rename = false,
        "lsp.folding_range" => flags.folding_range = false,
        "lsp.selection_range" => flags.selection_ranges = false,
        "lsp.linked_editing_range" => flags.linked_editing = false,
        "lsp.inlay_hint" => flags.inlay_hints = false,
        "lsp.semantic_tokens" => flags.semantic_tokens = false,
        "lsp.call_hierarchy" => flags.call_hierarchy = false,
        "lsp.type_hierarchy" => flags.type_hierarchy = false,
        "lsp.pull_diagnostics" => flags.pull_diagnostics = false,
        "lsp.document_color" => flags.document_color = false,
        "lsp.signature_help" => flags.signature_help = false,
        "lsp.document_highlight" => flags.document_highlight = false,
        "lsp.formatting" => flags.formatting = false,
        "lsp.range_formatting" | "lsp.ranges_formatting" => flags.range_formatting = false,
        "lsp.on_type_formatting" => flags.on_type_formatting = false,
        "lsp.document_link" => flags.document_links = false,
        "lsp.inline_completion" => flags.inline_completion = false,
        "lsp.inline_value" => flags.inline_values = false,
        "lsp.notebook_document_sync" => flags.notebook_document_sync = false,
        "lsp.notebook_cell_execution" => flags.notebook_cell_execution = false,
        "lsp.implementation" => flags.implementation = false,
        "lsp.type_definition" => flags.type_definition = false,
        "lsp.execute_command" => flags.execute_command = false,
        "lsp.moniker" => flags.moniker = false,
        unknown => tracing::warn!(id = unknown, "Unknown disabledFeatures ID ignored"),
    }
}

pub(crate) fn disabled_feature_ids_from_init_options(init_opts: &Value) -> Vec<&str> {
    let top_level = init_opts.get("disabledFeatures").and_then(Value::as_array);
    let namespaced_hyphen =
        init_opts.get("perl-lsp").and_then(|v| v.get("disabledFeatures")).and_then(Value::as_array);
    let namespaced_underscore =
        init_opts.get("perl_lsp").and_then(|v| v.get("disabledFeatures")).and_then(Value::as_array);

    top_level
        .into_iter()
        .chain(namespaced_hyphen)
        .chain(namespaced_underscore)
        .flat_map(|entries| entries.iter())
        .filter_map(Value::as_str)
        .collect()
}

#[cfg(test)]
mod init_options_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn disabled_feature_ids_reads_top_level_and_namespaced_options() {
        let init_opts = json!({
            "disabledFeatures": ["lsp.hover", true, 42],
            "perl-lsp": {
                "disabledFeatures": ["lsp.completion", null]
            },
            "perl_lsp": {
                "disabledFeatures": ["lsp.semantic_tokens"]
            }
        });

        let ids = disabled_feature_ids_from_init_options(&init_opts);
        assert_eq!(ids, vec!["lsp.hover", "lsp.completion", "lsp.semantic_tokens"]);
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::{apply_disabled_feature_id, is_jetbrains_client, is_opencode_client};
    use crate::LspServer;
    use crate::protocol::JsonRpcError;
    use crate::protocol::capabilities::BuildFlags;
    use perl_workspace::folder::root_path_to_file_uri;
    use serde_json::{Value, json};
    use std::sync::atomic::Ordering;

    #[test]
    fn apply_disabled_feature_id_zeros_correct_field() {
        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, "lsp.semantic_tokens");
        assert!(!flags.semantic_tokens);
        assert!(flags.completion, "other flags must be unchanged");
    }

    #[test]
    fn apply_disabled_feature_id_unknown_is_noop() {
        let mut flags = BuildFlags::all();
        let before = flags.clone();
        apply_disabled_feature_id(&mut flags, "lsp.does_not_exist");
        assert_eq!(flags, before, "unknown ID must not mutate flags");
    }

    #[test]
    fn handle_initialize_applies_perl_initialization_options()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("workspace");
        std::fs::create_dir_all(&folder)?;
        let uri =
            url::Url::from_directory_path(&folder).map_err(|_| "invalid folder path")?.to_string();

        let params = json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": uri, "name": "workspace" }],
            "initializationOptions": {
                "perl": {
                    "workspace": {
                        "includePaths": ["lib", "local"]
                    },
                    "inlayHints": {
                        "enabled": false
                    },
                    "testRunner": {
                        "enabled": true,
                        "command": "CANARY-EXECUTABLE",
                        "args": ["CANARY-ARG"],
                        "cwd": "CANARY-CWD",
                        "env": {"CANARY": "CANARY-VALUE"},
                        "timeout": 1
                    }
                }
            }
        });

        server.handle_initialize(Some(params))?;

        let workspace_config = server.workspace_config.lock();
        assert_eq!(workspace_config.include_paths, vec!["lib", "local"]);

        let config = server.config.lock();
        assert!(!config.inlay_hints_enabled);
        let serialized = serde_json::to_value(&*config)?;
        assert!(serialized.get("testRunner").is_none());
        assert!(serialized.to_string().find("CANARY").is_none());
        Ok(())
    }

    #[test]
    fn handle_initialize_perl_initialization_options_are_overridden_by_toml()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("workspace");
        std::fs::create_dir_all(&folder)?;
        std::fs::write(
            folder.join(".perl-lsp.toml"),
            "[perl]\ninclude_paths = [\"project_lib\"]\n",
        )?;
        let uri =
            url::Url::from_directory_path(&folder).map_err(|_| "invalid folder path")?.to_string();

        let params = json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": uri, "name": "workspace" }],
            "initializationOptions": {
                "perl": {
                    "workspace": {
                        "includePaths": ["lib", "local"]
                    }
                }
            }
        });

        server.handle_initialize(Some(params))?;

        // Per-folder effective config layers .perl-lsp.toml on top of init options.
        let folders = server.workspace_folders.lock();
        let folder_state = folders.first().ok_or("workspace folder should exist")?;
        assert_eq!(folder_state.effective_workspace_config.include_paths, vec!["project_lib"]);
        Ok(())
    }

    #[test]
    fn apply_disabled_feature_id_execute_command_zeros_field() {
        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, "lsp.execute_command");
        assert!(!flags.execute_command, "lsp.execute_command must zero execute_command field");
        assert!(flags.completion, "other flags must be unchanged");
    }

    #[test]
    fn apply_disabled_feature_id_moniker_zeros_field() {
        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, "lsp.moniker");
        assert!(!flags.moniker, "lsp.moniker must zero moniker field");
    }

    #[test]
    fn apply_disabled_feature_id_notebook_cell_execution_zeros_field() {
        let mut flags = BuildFlags::all();
        apply_disabled_feature_id(&mut flags, "lsp.notebook_cell_execution");
        assert!(
            !flags.notebook_cell_execution,
            "lsp.notebook_cell_execution must zero notebook_cell_execution field"
        );
    }

    /// All feature IDs emitted by BuildFlags::to_feature_ids() must have a match arm.
    /// This test will fail if a new field is added to BuildFlags with a feature ID
    /// but no corresponding arm in apply_disabled_feature_id.
    #[test]
    fn all_feature_ids_have_match_arm() {
        let all_ids = BuildFlags::all().to_feature_ids();
        for id in &all_ids {
            let mut before = BuildFlags::all();
            apply_disabled_feature_id(&mut before, id);
            let still_all = before == BuildFlags::all();
            assert!(
                !still_all,
                "feature ID '{id}' emitted by to_feature_ids() has no match arm in \
                 apply_disabled_feature_id â€” add one to keep the two in sync"
            );
        }
    }

    #[test]
    fn initialize_with_workspace_folders_sets_root_path_from_first_folder() {
        use std::path::Path;

        let server = LspServer::new();

        // Create platform-appropriate URIs for workspace folders using Url::from_file_path
        #[cfg(windows)]
        let (primary_uri, secondary_uri) = {
            let primary = Path::new("C:\\tmp\\primary");
            let secondary = Path::new("C:\\tmp\\secondary");
            (
                url::Url::from_file_path(primary).unwrap().to_string(),
                url::Url::from_file_path(secondary).unwrap().to_string(),
            )
        };

        #[cfg(not(windows))]
        let (primary_uri, secondary_uri) = {
            let primary_path = Path::new("/tmp/primary");
            let secondary_path = Path::new("/tmp/secondary");
            (
                url::Url::from_file_path(primary_path).unwrap().to_string(),
                url::Url::from_file_path(secondary_path).unwrap().to_string(),
            )
        };

        let params = json!({
            "workspaceFolders": [
                { "uri": primary_uri, "name": "primary" },
                { "uri": secondary_uri, "name": "secondary" }
            ],
            "capabilities": {}
        });

        let result = server.handle_initialize(Some(params));
        assert!(result.is_ok(), "initialize should succeed");

        let root_path = server.root_path.lock();
        assert!(
            root_path.as_ref().is_some_and(|path| path.ends_with("primary")),
            "root path should come from first workspace folder. Got: {:?}",
            root_path
        );
    }

    #[test]
    fn initialize_parses_workspace_configuration_capability() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(server.client_capabilities.lock().workspace_configuration_support);
        assert!(server.client_capabilities.lock().workspace_folders_support);
    }

    #[test]
    fn initialize_parses_apply_edit_metadata_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "applyEdit": true,
                    "workspaceEdit": {
                        "metadataSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        let caps = server.client_capabilities.lock();

        assert!(caps.workspace_apply_edit_support);
        assert!(caps.workspace_edit_metadata_support);
    }

    #[test]
    fn initialize_leaves_apply_edit_metadata_disabled_when_absent() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "workspaceEdit": {}
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        let caps = server.client_capabilities.lock();

        assert!(!caps.workspace_apply_edit_support);
        assert!(!caps.workspace_edit_metadata_support);
    }

    #[test]
    fn initialize_parses_file_watcher_relative_pattern_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "didChangeWatchedFiles": {
                        "dynamicRegistration": true,
                        "relativePatternSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        let caps = server.client_capabilities.lock();
        assert!(caps.dynamic_registration_support);
        assert!(caps.file_watcher_relative_pattern_support);
    }

    #[test]
    fn initialize_disables_workspace_folder_server_capability_when_client_lacks_support()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "workspaceFolders": false
                }
            }
        });

        let response =
            server.handle_initialize(Some(params))?.ok_or("initialize should return payload")?;

        let workspace_folders = response
            .pointer("/capabilities/workspace/workspaceFolders/supported")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let change_notifications = response
            .pointer("/capabilities/workspace/workspaceFolders/changeNotifications")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        assert!(!workspace_folders, "server must not advertise unsupported workspace folders");
        assert!(
            change_notifications,
            "server must always advertise workspace folder change notifications (per LSP spec)"
        );
        Ok(())
    }

    #[test]
    fn initialize_always_advertises_workspace_folder_change_notifications_per_lsp_spec()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "workspaceFolders": true
                }
            }
        });

        let response =
            server.handle_initialize(Some(params))?.ok_or("initialize should return payload")?;

        let change_notifications = response
            .pointer("/capabilities/workspace/workspaceFolders/changeNotifications")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        assert!(
            change_notifications,
            "server must always advertise workspace folder change notifications (per LSP spec)"
        );
        Ok(())
    }

    #[test]
    fn initialize_parses_completion_item_capabilities_from_spec_shape() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                            "commitCharactersSupport": true
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        let caps = server.client_capabilities.lock();
        assert!(caps.snippet_support);
        assert!(caps.completion_commit_characters_support);
    }

    #[test]
    fn initialize_parses_completion_item_capabilities_from_flattened_shape() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "snippetSupport": true,
                        "commitCharactersSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        let caps = server.client_capabilities.lock();
        assert!(caps.snippet_support);
        assert!(caps.completion_commit_characters_support);
    }

    #[test]
    fn initialize_parses_completion_list_item_defaults_data_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionList": {
                            "itemDefaults": [
                                "commitCharacters",
                                "editRange",
                                "insertTextFormat",
                                "insertTextMode",
                                "data"
                            ]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(server.client_capabilities.lock().completion_list_item_defaults_data_support);
    }

    #[test]
    fn handle_initialize_boundary_discriminator() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionList": {
                            "itemDefaults": ["commitCharacters", "data"]
                        }
                    },
                    "codeAction": {
                        "tagSupport": {
                            "valueSet": [2, 1]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        let caps = server.client_capabilities.lock();
        assert!(
            caps.completion_list_item_defaults_data_support,
            "input that hits the boundary: item.as_str() == Some(\"data\")"
        );
        assert!(
            caps.code_action_llm_generated_tag_support,
            "input that hits the boundary: tag.as_i64() == Some(1)"
        );
    }

    #[test]
    fn initialize_leaves_completion_list_item_defaults_data_disabled_when_absent() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionList": {
                            "itemDefaults": ["commitCharacters", "insertTextFormat"]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(!server.client_capabilities.lock().completion_list_item_defaults_data_support);
    }

    #[test]
    fn initialize_parses_completion_list_apply_kind_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionList": {
                            "applyKindSupport": true
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(server.client_capabilities.lock().completion_list_apply_kind_support);
    }

    #[test]
    fn initialize_leaves_completion_list_apply_kind_disabled_when_absent() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionList": {
                            "itemDefaults": ["data"]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(!server.client_capabilities.lock().completion_list_apply_kind_support);
    }

    #[test]
    fn initialize_parses_workspace_edit_snippet_text_edit_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "workspaceEdit": {
                        "documentChanges": true,
                        "snippetEditSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        let caps = server.client_capabilities.lock();

        assert!(caps.workspace_edit_document_changes_support);
        assert!(caps.workspace_edit_snippet_edit_support);
    }

    #[test]
    fn initialize_leaves_workspace_edit_snippet_text_edit_disabled_when_absent() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "workspaceEdit": {
                        "documentChanges": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        let caps = server.client_capabilities.lock();

        assert!(caps.workspace_edit_document_changes_support);
        assert!(!caps.workspace_edit_snippet_edit_support);
    }

    #[test]
    fn initialize_parses_code_action_documentation_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "codeAction": {
                        "documentationSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(server.client_capabilities.lock().code_action_documentation_support);
    }

    #[test]
    fn initialize_parses_code_action_disabled_support() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "codeAction": {
                        "disabledSupport": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        if !server.client_capabilities.lock().code_action_disabled_support {
            return Err("disabledSupport capability was not parsed".into());
        }
        Ok(())
    }

    #[test]
    fn initialize_parses_code_action_llm_generated_tag_support() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "codeAction": {
                        "tagSupport": {
                            "valueSet": [1]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(server.client_capabilities.lock().code_action_llm_generated_tag_support);
    }

    #[test]
    fn initialize_leaves_code_action_llm_generated_tag_disabled_when_value_set_omits_it() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "codeAction": {
                        "tagSupport": {
                            "valueSet": [99]
                        }
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(!server.client_capabilities.lock().code_action_llm_generated_tag_support);
    }

    #[test]
    fn handle_initialize_exact_error_variant() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let params = json!({ "capabilities": {} });

        server.handle_initialize(Some(params.clone()))?;
        let err = match server.handle_initialize(Some(params)) {
            Err(err) => err,
            Ok(_) => return Err("second initialize should fail with InvalidRequest".into()),
        };

        assert_eq!(err.code, -32600, "duplicate initialize must return InvalidRequest");
        assert_eq!(
            err.message, "initialize may only be sent once",
            "duplicate initialize must preserve the exact error message"
        );
        assert!(err.data.is_none(), "duplicate initialize error must not attach data");

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Text-sync session contract (#9378, LSP-FS16-001..010)
    // -----------------------------------------------------------------------

    use crate::runtime::lifecycle::session_contract::{
        AcceptedPositionEncoding, AcceptedSyncKind, Utf16SelectionReason,
    };
    /// Accept initialize with `positionEncodings` and return (response,
    /// error-code-on-failure). Exactly one side is populated.
    fn initialize_with_offer(offer: Value) -> (Option<Value>, Option<JsonRpcError>, LspServer) {
        let server = LspServer::new();
        let params = json!({ "capabilities": { "general": { "positionEncodings": offer } } });
        match server.handle_initialize(Some(params)) {
            Ok(response) => (response, None, server),
            Err(error) => (None, Some(error), server),
        }
    }

    #[test]
    fn initialize_offer_containing_utf16_accepts_and_stores_contract() {
        for offer in [
            json!(["utf-16"]),
            json!(["utf-8", "utf-16"]),
            json!(["utf-32", "utf-16"]),
            json!(["utf-16", "utf-16"]),
            json!(["utf-7", "utf-16"]),
            json!(["utf-32", "utf-8", "utf-16"]),
        ] {
            let (response, error, server) = initialize_with_offer(offer.clone());
            assert!(error.is_none(), "offer {offer} must be accepted: {error:?}");
            let response = response.unwrap();

            let session = server.accepted_text_sync_session().unwrap();
            let contract = session.contract();
            assert_eq!(contract.sync_kind(), AcceptedSyncKind::Full);
            assert_eq!(contract.position_encoding(), AcceptedPositionEncoding::Utf16);
            assert_eq!(
                contract.selection_reason(),
                Utf16SelectionReason::ClientOfferedUtf16,
                "offer {offer} must record the client selection reason"
            );

            // Response, stored state, and evidence agree (LSP-FS16-006/010).
            assert_eq!(
                response.pointer("/capabilities/positionEncoding").and_then(Value::as_str),
                Some(contract.position_encoding().wire_name())
            );
            assert_eq!(
                response.pointer("/capabilities/textDocumentSync/change").and_then(Value::as_i64),
                Some(i64::from(contract.sync_kind().wire_value()))
            );
            let evidence = session.evidence();
            assert_eq!(evidence.contract_digest, contract.digest());
            let expected_response_digest = evidence.response_digest.clone();
            assert!(!expected_response_digest.is_empty(), "response digest must be recorded");
        }
    }

    #[test]
    fn initialize_absent_null_and_empty_offers_default_to_utf16_with_distinct_reasons() {
        // Absent (LSP-FS16-002).
        let server = LspServer::new();
        let response =
            server.handle_initialize(Some(json!({ "capabilities": {} }))).unwrap().unwrap();
        let session = server.accepted_text_sync_session().unwrap();
        assert_eq!(session.contract().selection_reason(), Utf16SelectionReason::OfferAbsent);
        assert_eq!(
            response.pointer("/capabilities/positionEncoding").and_then(Value::as_str),
            Some("utf-16")
        );

        // JSON null is the absent spelling for an optional array.
        let (response, error, server) = initialize_with_offer(Value::Null);
        assert!(error.is_none(), "null offer must be accepted: {error:?}");
        let session = server.accepted_text_sync_session().unwrap();
        assert_eq!(session.contract().selection_reason(), Utf16SelectionReason::OfferAbsent);
        assert_eq!(
            response.unwrap().pointer("/capabilities/positionEncoding").and_then(Value::as_str),
            Some("utf-16")
        );

        // Present but empty — reviewed disposition: no constraint expressed.
        let (response, error, server) = initialize_with_offer(json!([]));
        assert!(error.is_none(), "empty offer must be accepted: {error:?}");
        let session = server.accepted_text_sync_session().unwrap();
        assert_eq!(session.contract().selection_reason(), Utf16SelectionReason::OfferEmpty);
        assert_eq!(
            response.unwrap().pointer("/capabilities/positionEncoding").and_then(Value::as_str),
            Some("utf-16")
        );
    }

    #[test]
    fn initialize_no_common_offer_fails_before_any_state_mutation() {
        // LSP-FS16-004/007: a client that explicitly excludes UTF-16 is
        // rejected with a typed initialize failure and NO partial state.
        // Negative control: restoring the old no-common fallback makes this
        // focused gate red.
        for offer in [json!(["utf-32", "utf-7"]), json!(["utf-8"]), json!(["utf-32"])] {
            let (response, error, server) = initialize_with_offer(offer.clone());
            assert!(response.is_none(), "offer {offer} must not produce a response");
            let error = error.unwrap();
            assert_eq!(error.code, -32602, "offer {offer} must be typed InvalidParams");
            assert_eq!(
                error
                    .data
                    .as_ref()
                    .and_then(|data| data.pointer("/rejection/reason"))
                    .and_then(Value::as_str),
                Some("no-common-encoding"),
                "offer {offer} must carry the typed rejection reason"
            );

            // No accepted session, no capabilities, no workspace mutation.
            assert!(server.accepted_text_sync_session().is_none());
            assert!(!server.is_initialized());
            assert!(server.workspace_folders.lock().is_empty());
            let caps = server.client_capabilities.lock();
            assert!(!caps.workspace_configuration_support);
            assert!(!caps.workspace_folders_support);
            assert!(!caps.snippet_support);
        }
    }

    #[test]
    fn initialize_malformed_offers_fail_typed() {
        for offer in [json!("utf-16"), json!(42), json!({}), json!(["utf-16", 42])] {
            let (response, error, server) = initialize_with_offer(offer.clone());
            assert!(response.is_none(), "malformed offer {offer} must fail");
            let error = error.unwrap();
            assert_eq!(error.code, -32602);
            assert_eq!(
                error
                    .data
                    .as_ref()
                    .and_then(|data| data.pointer("/rejection/reason"))
                    .and_then(Value::as_str),
                Some("malformed-offer"),
                "malformed input must never collapse into absence: {offer}"
            );
            assert!(server.accepted_text_sync_session().is_none());
        }
    }

    #[test]
    fn second_initialize_cannot_replace_accepted_contract() {
        // LSP-FS16-008: repeated initialize cannot replace or partially alter
        // the accepted contract.
        let (response, error, server) = initialize_with_offer(json!(["utf-16"]));
        assert!(error.is_none(), "first initialize must succeed: {error:?}");
        assert!(response.is_some());
        let accepted = server.accepted_text_sync_session().unwrap();
        let original_digest = accepted.contract().digest();

        let second = server.handle_initialize(Some(json!({
            "capabilities": { "general": { "positionEncodings": ["utf-8", "utf-16"] } }
        })));
        assert!(second.is_err(), "second initialize must fail");
        assert_eq!(second.unwrap_err().code, -32600);

        let after = server.accepted_text_sync_session().unwrap();
        assert_eq!(after.contract().digest(), original_digest);
        assert_eq!(after.contract().selection_reason(), Utf16SelectionReason::ClientOfferedUtf16);
    }

    #[test]
    fn client_name_cannot_change_selection() {
        let (_, _, plain) = initialize_with_offer(json!(["utf-32", "utf-16"]));
        let server_named = LspServer::new();
        let response = server_named
            .handle_initialize(Some(json!({
                "clientInfo": { "name": "fancy-editor" },
                "capabilities": { "general": { "positionEncodings": ["utf-32", "utf-16"] } }
            })))
            .unwrap()
            .unwrap();
        assert_eq!(
            response.pointer("/capabilities/positionEncoding").and_then(Value::as_str),
            Some("utf-16")
        );
        assert_eq!(
            server_named.accepted_text_sync_session().unwrap().contract().selection_reason(),
            plain.accepted_text_sync_session().unwrap().contract().selection_reason()
        );
    }

    #[test]

    fn initialize_advertises_code_action_documentation_only_when_supported()
    -> Result<(), Box<dyn std::error::Error>> {
        let unsupported = LspServer::new()
            .handle_initialize(Some(json!({ "capabilities": {} })))?
            .ok_or("initialize should return unsupported-client payload")?;
        assert!(
            unsupported.pointer("/capabilities/codeActionProvider/documentation").is_none(),
            "default clients must not receive CodeAction.documentation: {unsupported}"
        );

        let supported = LspServer::new()
            .handle_initialize(Some(json!({
                "capabilities": {
                    "textDocument": {
                        "codeAction": {
                            "documentationSupport": true
                        }
                    }
                }
            })))?
            .ok_or("initialize should return supported-client payload")?;
        let docs = supported
            .pointer("/capabilities/codeActionProvider/documentation")
            .and_then(Value::as_array)
            .ok_or("supported clients should receive CodeActionOptions.documentation")?;
        assert_eq!(docs.len(), 3, "expected quickfix, refactor, and source.fixAll docs");
        Ok(())
    }

    #[test]
    fn lsp4ij_inline_completion_dynamic_registration_shape_is_parsed() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "inlineCompletion": {
                        "dynamicRegistration": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        let caps = server.client_capabilities.lock();
        assert!(caps.inline_completion_support);
        assert!(caps.inline_completion_dynamic_registration_support);
    }

    #[test]
    fn initialize_uses_current_directory_when_root_is_missing() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {}
        });

        let _ = server.handle_initialize(Some(params));

        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 1, "missing roots should fall back to current directory");

        let expected_uri =
            std::env::current_dir().ok().map(|cwd| root_path_to_file_uri(&cwd.to_string_lossy()));
        assert_eq!(
            folders[0].uri,
            expected_uri.unwrap_or_default(),
            "workspace folder should match current directory fallback URI"
        );
    }

    /// Guard: cwd fallback must NOT fire when a top-level rootUri is present.
    #[test]
    fn initialize_cwd_fallback_not_used_when_root_uri_present() {
        let server = LspServer::new();
        let params = json!({
            "rootUri": "file:///explicit-workspace"
        });

        let _ = server.handle_initialize(Some(params));

        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 1, "must create exactly one workspace folder from rootUri");
        assert_eq!(
            folders[0].uri, "file:///explicit-workspace",
            "cwd fallback must not override an explicitly provided rootUri"
        );
    }

    #[test]
    fn opencode_client_detection_is_case_insensitive() {
        let params = json!({
            "clientInfo": {
                "name": "OpenCode"
            }
        });
        assert!(is_opencode_client(&params));
    }

    #[test]
    fn jetbrains_client_detection_matches_jetbrains_intellij_idea_names() {
        for name in &["JetBrains", "IntelliJ IDEA", "idea", "JetBrains Client"] {
            let params = json!({ "clientInfo": { "name": name } });
            assert!(is_jetbrains_client(&params), "should detect JetBrains client: {name}");
        }
        let non_jetbrains = json!({ "clientInfo": { "name": "vscode" } });
        assert!(!is_jetbrains_client(&non_jetbrains));
        assert!(!is_jetbrains_client(&Value::Object(serde_json::Map::new())));
    }

    #[test]
    fn initialize_disables_dynamic_registration_for_jetbrains_clients() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "JetBrains"
            },
            "capabilities": {
                "workspace": {
                    "didChangeWatchedFiles": {
                        "dynamicRegistration": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(
            !server.client_capabilities.lock().dynamic_registration_support,
            "JetBrains clients must have dynamic_registration_support forced to false \
             even when the capabilities object claims support"
        );
    }

    #[test]
    fn initialize_preserves_dynamic_registration_for_non_jetbrains_clients() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "vscode"
            },
            "capabilities": {
                "workspace": {
                    "didChangeWatchedFiles": {
                        "dynamicRegistration": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(
            server.client_capabilities.lock().dynamic_registration_support,
            "non-JetBrains clients that advertise dynamic registration must have it enabled"
        );
    }

    #[test]
    fn initialize_queues_startup_log_for_jetbrains_dynamic_registration_override() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "IntelliJ IDEA"
            },
            "capabilities": {
                "workspace": {
                    "didChangeWatchedFiles": {
                        "dynamicRegistration": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        let pending = server.pending_startup_log.lock();
        assert!(pending.is_some(), "JetBrains override should queue a pending startup logMessage");
        if let Some(ref msg) = *pending {
            assert!(msg.contains("IntelliJ IDEA"), "pending log should name the client: got {msg}");
            assert!(
                msg.contains("dynamic"),
                "pending log should mention the disabled capability: got {msg}"
            );
        }
    }

    #[test]
    fn initialize_does_not_queue_startup_log_for_non_jetbrains_clients() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "vscode"
            },
            "capabilities": {
                "workspace": {
                    "didChangeWatchedFiles": {
                        "dynamicRegistration": true
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(
            server.pending_startup_log.lock().is_none(),
            "non-JetBrains clients should not have a pending startup logMessage"
        );
    }

    #[test]
    fn initialize_keeps_push_diagnostics_for_opencode() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "opencode"
            },
            "capabilities": {
                "textDocument": {
                    "diagnostic": {}
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(
            !server.client_supports_pull_diags.load(Ordering::Relaxed),
            "opencode should keep push diagnostics enabled"
        );
    }

    #[test]
    fn initialize_enables_pull_diagnostics_for_non_opencode_clients() {
        let server = LspServer::new();
        let params = json!({
            "clientInfo": {
                "name": "vscode"
            },
            "capabilities": {
                "textDocument": {
                    "diagnostic": {}
                }
            }
        });

        let _ = server.handle_initialize(Some(params));

        assert!(
            server.client_supports_pull_diags.load(Ordering::Relaxed),
            "non-opencode clients with textDocument.diagnostic should enable pull diagnostics"
        );
    }

    #[test]
    fn initialize_reads_root_uri_from_initialization_options() {
        let server = LspServer::new();
        let params = json!({
            "initializationOptions": {
                "rootUri": "file:///tmp/claude-workspace"
            }
        });

        let _ = server.handle_initialize(Some(params));

        let folders = server.workspace_folders.lock();
        assert_eq!(
            folders.len(),
            1,
            "must create one workspace folder from initializationOptions.rootUri"
        );
        assert_eq!(folders[0].uri, "file:///tmp/claude-workspace");
    }

    /// Precedence guard: top-level `rootUri` must take priority over
    /// `initializationOptions.rootUri` when both are present.
    #[test]
    fn initialize_top_level_root_uri_takes_precedence_over_initialization_options() {
        let server = LspServer::new();
        let params = json!({
            "rootUri": "file:///top-level-workspace",
            "initializationOptions": {
                "rootUri": "file:///init-options-workspace"
            }
        });

        let _ = server.handle_initialize(Some(params));

        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 1, "must create exactly one workspace folder");
        assert_eq!(
            folders[0].uri, "file:///top-level-workspace",
            "top-level rootUri must take precedence over initializationOptions.rootUri"
        );
    }

    /// Parity guard: initializationOptions.workspaceFolders must also call set_root_uri
    /// for the first folder, matching the behavior of the top-level workspaceFolders branch.
    #[test]
    fn initialize_init_options_workspace_folders_sets_root_path() {
        use std::path::Path;

        let server = LspServer::new();

        // Use platform-appropriate real file URIs so source_path_from_uri can convert them.
        #[cfg(windows)]
        let (primary_uri, secondary_uri) = {
            let primary = Path::new("C:\\tmp\\init-opts-primary");
            let secondary = Path::new("C:\\tmp\\init-opts-secondary");
            (
                url::Url::from_file_path(primary).unwrap().to_string(),
                url::Url::from_file_path(secondary).unwrap().to_string(),
            )
        };
        #[cfg(not(windows))]
        let (primary_uri, secondary_uri) = {
            let primary = Path::new("/tmp/init-opts-primary");
            let secondary = Path::new("/tmp/init-opts-secondary");
            (
                url::Url::from_file_path(primary).unwrap().to_string(),
                url::Url::from_file_path(secondary).unwrap().to_string(),
            )
        };

        let params = json!({
            "initializationOptions": {
                "workspaceFolders": [
                    { "uri": primary_uri, "name": "primary" },
                    { "uri": secondary_uri, "name": "secondary" }
                ]
            }
        });

        let _ = server.handle_initialize(Some(params));

        // Workspace folders must be populated
        let folders = server.workspace_folders.lock();
        assert_eq!(
            folders.len(),
            2,
            "both workspace folders from initializationOptions must be registered"
        );
        drop(folders);

        // root_path must be set from the first folder (module resolution depends on this).
        // This is the parity check — the top-level workspaceFolders branch calls
        // set_root_uri; the initializationOptions branch must do the same.
        let root_path = server.root_path.lock();
        assert!(
            root_path.as_ref().is_some_and(|p| p.ends_with("init-opts-primary")),
            "root_path must be set from first initializationOptions.workspaceFolders entry. Got: {:?}",
            root_path
        );
    }

    #[test]
    fn initialize_parses_prepare_support_default_behavior() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "rename": {
                        "prepareSupport": true,
                        "prepareSupportDefaultBehavior": 1
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert_eq!(server.client_capabilities.lock().prepare_support_default_behavior, 1);
    }

    #[test]
    fn initialize_leaves_prepare_support_default_behavior_zero_when_absent() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "rename": { "prepareSupport": true }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert_eq!(server.client_capabilities.lock().prepare_support_default_behavior, 0);
    }

    #[test]
    fn initialize_ignores_out_of_range_prepare_support_default_behavior() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "textDocument": {
                    "rename": {
                        "prepareSupport": true,
                        "prepareSupportDefaultBehavior": 257
                    }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert_eq!(server.client_capabilities.lock().prepare_support_default_behavior, 0);
    }

    /// LSP 3.17 spec key: `ClientCapabilities.workspace.diagnostics` (plural).
    #[test]
    fn initialize_reads_diagnostic_refresh_support_from_spec_plural_key() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "diagnostics": { "refreshSupport": true }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert!(
            server.client_capabilities.lock().diagnostic_refresh_support,
            "spec-conformant `workspace.diagnostics.refreshSupport` must enable refresh"
        );
    }

    /// `lsp-types`/`helix-lsp-types` client deviation: `workspace.diagnostic`
    /// (singular). Accepted for compatibility, not because the spec says so.
    #[test]
    fn initialize_reads_diagnostic_refresh_support_from_singular_client_deviation() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": {
                    "diagnostic": { "refreshSupport": true }
                }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert!(
            server.client_capabilities.lock().diagnostic_refresh_support,
            "`lsp-types`-style `workspace.diagnostic.refreshSupport` must still enable refresh"
        );
    }

    #[test]
    fn initialize_leaves_diagnostic_refresh_support_false_when_neither_key_present() {
        let server = LspServer::new();
        let params = json!({
            "capabilities": {
                "workspace": { "codeLens": { "refreshSupport": true } },
                "textDocument": { "diagnostic": { "dynamicRegistration": true } }
            }
        });

        let _ = server.handle_initialize(Some(params));
        assert!(
            !server.client_capabilities.lock().diagnostic_refresh_support,
            "neither workspace diagnostic-refresh spelling advertised: must stay false"
        );
    }
}
