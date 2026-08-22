//! Workspace-level operations
//!
//! Handles workspace symbols, configuration, file watching, and edits.
//!
//! # Lifecycle-Aware Behavior
//!
//! Uses the routing module for state-aware dispatch:
//! - **Ready state**: Full workspace index search with cooperative yielding
//! - **Building/Degraded state**: Open document search only (partial results)

use super::{
    AtomicBool, AtomicI32, GLOBAL_CANCELLATION_REGISTRY, IndexCoordinator, JsonRpcError, JsonRpcId,
    LspServer, LspWorkspaceSymbol, Mutex, Ordering, PendingWorkspaceConfigurationRequest,
    PerlLspCancellationToken, ServerRequestId, Value, WorkspaceFolderState,
    best_workspace_folder_for_doc, json, outbound, uri_to_fs_path,
};
#[cfg(feature = "workspace")]
use crate::runtime::readiness::{
    IndexReadinessOutcome, IndexReadinessPolicy, ReadinessMilestone, check_readiness,
};
#[cfg(feature = "workspace")]
use crate::runtime::routing::{IndexAccessMode, route_index_access};
use crate::runtime::window::RequestProgressGuard;
use crate::runtime::workspace_progress::{
    WORKSPACE_INDEX_PROGRESS_TOKEN, send_index_ready_notification, send_progress_begin,
    send_progress_create, send_progress_end, send_progress_report,
};
use crate::state::workspace_symbol_cap;
use perl_lsp_rs_core::config::{
    ExternalIncludePathAuthority, UnauthorizedExternalIncludePathSource,
    WorkspaceConfigUpdateContext,
};
use perl_module::path::file_path_to_module_name;
use perl_module::rename::plan_module_rename_edits;
#[cfg(feature = "workspace")]
use perl_parser::workspace_index::{
    DegradationReason, EarlyExitReason, IndexState, ResourceKind, SymbolKind,
};
#[cfg(feature = "workspace")]
use perl_parser_core::source_file::{is_perl_source_path, is_perl_source_uri};
#[cfg(any(test, feature = "expose_lsp_test_api"))]
use perl_semantic_facts::{
    Confidence, Provenance, ProviderFactFreshness, ProviderFactSourceKind, ProviderFallbackState,
};
use perl_workspace::folder::extract_workspace_folder_change;
#[cfg(feature = "workspace")]
use perl_workspace::ignore::is_skipped_dir_name;
use std::collections::{HashMap, HashSet};

/// Serialize a slice of typed values to a JSON array (#4995).
fn to_json_array<T: serde::Serialize>(values: &[T]) -> Value {
    serde_json::to_value(values).unwrap_or(Value::Array(Vec::new()))
}
#[cfg(feature = "workspace")]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
#[cfg(feature = "workspace")]
use std::time::Instant;
#[cfg(feature = "workspace")]
use url::Url;

#[cfg(feature = "workspace")]
mod configuration_response;

const WORKSPACE_CONFIGURATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// Note: WalkDir logic has been extracted to super::file_discovery.
// These helper functions are retained for potential future use by
// other workspace operations (e.g., file watcher filtering).
#[cfg(feature = "workspace")]
#[allow(dead_code)]
fn is_perl_source_file(path: &Path) -> bool {
    is_perl_source_path(path)
}

#[cfg(feature = "workspace")]
#[allow(dead_code)]
fn should_skip_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    is_skipped_dir_name(&entry.file_name().to_string_lossy())
}

/// Returns `true` when an I/O error represents a permission-denied condition.
///
/// Covers both the portable `ErrorKind::PermissionDenied` and the Windows
/// `ERROR_ACCESS_DENIED` code (os error 5), which may surface as
/// `ErrorKind::Uncategorized` on older Rust toolchains.
#[cfg(feature = "workspace")]
fn is_permission_denied_error(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return true;
    }
    // Windows ERROR_ACCESS_DENIED = os error 5
    #[cfg(windows)]
    if e.raw_os_error() == Some(5) {
        return true;
    }
    false
}

#[cfg(feature = "workspace")]
use crate::util::read_text_file_with_encoding;
#[cfg(feature = "workspace")]
use perl_workspace::monitoring::{IndexingPhase, WorkspaceIndexingReceipt};

#[cfg(feature = "workspace")]
fn read_watched_file_content(uri: &str, purpose: &str) -> Option<String> {
    uri_to_fs_path(uri).and_then(|path| match read_text_file_with_encoding(&path) {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::debug!("Failed to read file for {} ({}): {}", purpose, path.display(), e);
            None
        }
    })
}

/// RAII guard that clears the `indexing_in_progress` flag on drop.
///
/// Ensures the flag is always cleared, even if the indexing thread panics.
#[cfg(feature = "workspace")]
struct IndexingGuard {
    indexing_in_progress: Arc<AtomicBool>,
    indexing_rescan_pending: Arc<AtomicBool>,
    indexing_transition_lock: Arc<Mutex<()>>,
    restart: Option<Box<dyn FnOnce() + Send>>,
}

#[cfg(feature = "workspace")]
impl Drop for IndexingGuard {
    fn drop(&mut self) {
        let should_restart = release_indexing_slot(
            &self.indexing_in_progress,
            &self.indexing_rescan_pending,
            &self.indexing_transition_lock,
        );
        if should_restart && let Some(restart) = self.restart.take() {
            restart();
        }
    }
}

#[cfg(feature = "workspace")]
#[derive(Clone)]
struct IndexingResources {
    coordinator: Arc<IndexCoordinator>,
    workspace_folders: Arc<Mutex<Vec<WorkspaceFolderState>>>,
    indexing_in_progress: Arc<AtomicBool>,
    indexing_rescan_pending: Arc<AtomicBool>,
    indexing_transition_lock: Arc<Mutex<()>>,
    invocation_count: Arc<std::sync::atomic::AtomicUsize>,
    outbound: outbound::OutboundSender,
    work_done_progress: bool,
    progress_tokens: Arc<Mutex<HashSet<String>>>,
    progress_token_to_request: Arc<Mutex<HashMap<String, JsonRpcId>>>,
    next_request_id: Arc<AtomicI32>,
    permission_denied_shown: Arc<AtomicBool>,
    readiness_receipt: Arc<Mutex<crate::runtime::readiness::WorkspaceReadinessReceipt>>,
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    readiness_start_gate:
        Arc<std::sync::Mutex<Option<crate::runtime::readiness::WorkspaceIndexingStartGate>>>,
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    readiness_observer_id: u64,
}

#[cfg(feature = "workspace")]
struct WorkspaceIndexCancellationGuard {
    progress_tokens: Arc<Mutex<HashSet<String>>>,
    progress_token_to_request: Arc<Mutex<HashMap<String, JsonRpcId>>>,
    request_id: JsonRpcId,
}

#[cfg(feature = "workspace")]
impl Drop for WorkspaceIndexCancellationGuard {
    fn drop(&mut self) {
        self.progress_tokens.lock().remove(WORKSPACE_INDEX_PROGRESS_TOKEN);
        self.progress_token_to_request.lock().remove(WORKSPACE_INDEX_PROGRESS_TOKEN);
        GLOBAL_CANCELLATION_REGISTRY.remove_request(&self.request_id);
    }
}

#[cfg(feature = "workspace")]
fn next_indexing_progress_request_id(next_request_id: &AtomicI32) -> ServerRequestId {
    loop {
        let current = next_request_id.load(Ordering::Relaxed);
        let next = if current == i32::MAX { 1 } else { current + 1 };
        if next_request_id
            .compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
            && let Some(id) = ServerRequestId::new(current.max(1))
        {
            return id;
        }
    }
}

#[cfg(feature = "workspace")]
fn indexing_cancellation_request_id(progress_create_id: ServerRequestId) -> JsonRpcId {
    JsonRpcId::String(format!("workspace-indexing:{}", progress_create_id.as_i32()))
}

#[cfg(feature = "workspace")]
fn claim_indexing_slot(
    indexing_in_progress: &AtomicBool,
    indexing_rescan_pending: &AtomicBool,
    indexing_transition_lock: &Mutex<()>,
) -> bool {
    let _transition = indexing_transition_lock.lock();
    if indexing_in_progress
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        indexing_rescan_pending.store(true, Ordering::Release);
        false
    } else {
        true
    }
}

#[cfg(feature = "workspace")]
fn release_indexing_slot(
    indexing_in_progress: &AtomicBool,
    indexing_rescan_pending: &AtomicBool,
    indexing_transition_lock: &Mutex<()>,
) -> bool {
    let _transition = indexing_transition_lock.lock();
    indexing_in_progress.store(false, Ordering::Release);
    indexing_rescan_pending.swap(false, Ordering::AcqRel)
}

#[cfg(feature = "workspace")]
fn path_is_in_current_workspace(path: &Path, workspace_folders: &[WorkspaceFolderState]) -> bool {
    workspace_folders.iter().any(|folder| {
        let Some(root) = folder.path.clone().or_else(|| uri_to_fs_path(&folder.uri)) else {
            return false;
        };
        if path.starts_with(&root) {
            return true;
        }

        folder.effective_workspace_config.include_paths.iter().any(|include_path| {
            let include_root = Path::new(include_path);
            let resolved = if include_root.is_absolute() {
                include_root.to_path_buf()
            } else {
                root.join(include_root)
            };
            path.starts_with(resolved)
        })
    })
}

fn parse_configuration_response_id(value: &Value) -> Option<ServerRequestId> {
    if let Some(id) = value.as_i64() {
        return i32::try_from(id).ok().and_then(ServerRequestId::new);
    }

    value.as_str().and_then(|raw| raw.parse::<i32>().ok()).and_then(ServerRequestId::new)
}

impl LspServer {
    /// Request `workspace/configuration` for each workspace folder (if supported).
    pub(crate) fn request_workspace_configuration_for_folders(&self) {
        if !self.client_capabilities.lock().workspace_configuration_support {
            tracing::debug!("Client does not support workspace/configuration; using local config");
            return;
        }

        let now = std::time::Instant::now();

        let folder_uris: Vec<String> =
            self.workspace_folders.lock().iter().map(|folder| folder.uri.clone()).collect();
        if folder_uris.is_empty() {
            return;
        }

        let mut items: Vec<Value> = vec![json!({ "section": "perl" })];
        items.extend(folder_uris.iter().map(|uri| json!({ "scopeUri": uri, "section": "perl" })));
        let request_id =
            match self.send_request("workspace/configuration", json!({ "items": items })) {
                Ok(request_id) => request_id,
                Err(error) => {
                    tracing::warn!(%error, "Failed to send workspace/configuration request");
                    return;
                }
            };

        let mut pending = self.pending_workspace_configuration_requests.lock();

        // Count cap backstop: keep at most 10 pending requests to prevent unbounded growth
        // even if client responses are slow or missing.
        if pending.len() >= 10 {
            let to_remove = pending.len() - 9;
            let mut entries: Vec<_> =
                pending.iter().map(|(id, req)| (*id, req.created_at)).collect();
            entries.sort_by_key(|(_, created_at)| *created_at);
            for (id, _) in entries.iter().take(to_remove) {
                tracing::debug!(
                    request_id = id.as_i32(),
                    "Dropping excess workspace/configuration request (count cap)"
                );
                pending.remove(id);
            }
        }

        if !pending.is_empty() {
            tracing::debug!(
                superseded_requests = pending.len(),
                "Dropping older workspace/configuration requests in favor of latest snapshot"
            );
            pending.clear();
        }
        pending.insert(
            request_id,
            PendingWorkspaceConfigurationRequest {
                folder_uris,
                includes_global_item: true,
                created_at: now,
            },
        );
    }

    /// Apply a `workspace/configuration` response for a previously sent request.
    pub(crate) fn handle_client_response(&self, params: Option<Value>) {
        let Some(params) = params else {
            return;
        };
        let Some(id) = params.get("id").and_then(parse_configuration_response_id) else {
            return;
        };

        let maybe_pending = self.pending_workspace_configuration_requests.lock().remove(&id);
        let Some(pending) = maybe_pending else {
            return;
        };
        let response_age = std::time::Instant::now().saturating_duration_since(pending.created_at);
        if response_age > WORKSPACE_CONFIGURATION_REQUEST_TIMEOUT {
            tracing::warn!(
                request_id = id.as_i32(),
                age_ms = response_age.as_millis(),
                "Ignoring stale workspace/configuration response"
            );
            return;
        }

        if params.get("error").is_some() {
            tracing::debug!(
                request_id = id.as_i32(),
                "workspace/configuration request failed; keeping TOML/default config"
            );
            return;
        }

        let Some(results) = params.get("result").and_then(Value::as_array) else {
            tracing::warn!(
                request_id = id.as_i32(),
                "workspace/configuration response was not an array; keeping TOML/default config"
            );
            return;
        };
        let mut folders = self.workspace_folders.lock();
        let init_options_perl = self.initialization_options_perl_settings.lock();
        configuration_response::apply_workspace_configuration_results(
            &mut folders,
            &pending.folder_uris,
            pending.includes_global_item,
            results,
            i64::from(id.as_i32()),
            init_options_perl.as_ref(),
        );
    }

    /// Handle workspace/symbol request (v2 implementation with lifecycle-aware dispatch)
    ///
    /// Uses routing helper for state-aware behavior:
    /// - **Ready state**: Full workspace index search with cooperative yielding
    /// - **Building state**: Wait briefly for index readiness, then serve from
    ///   the ready index when it completes (fix for issue #1514 race condition)
    /// - **Degraded state**: Query partial index first; fall through to open-doc
    ///   search only when the partial index is also empty (Gap 2 fix, issue #4152)
    pub(super) fn handle_workspace_symbols_v2(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().workspace_symbol {
            return Err(crate::protocol::method_not_advertised());
        }

        let _progress =
            RequestProgressGuard::new(self, "workspace-symbol", "Searching workspace symbols");

        let query = params
            .as_ref()
            .and_then(|p| p.get("query"))
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .trim();
        let cap = workspace_symbol_cap();

        tracing::debug!(query, cap, "Workspace symbol search v2");

        // Use routing helper for lifecycle-aware dispatch
        #[cfg(feature = "workspace")]
        {
            // If the workspace is currently being indexed (Building state), wait
            // briefly for readiness before serving, bounded by INDEX_READY_WAIT_MS.
            // This eliminates the ~60% intermittent-empty race
            // that occurs when workspace/symbol arrives right after `initialized`.
            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);

            if self.workspace_index_stale_for_any_open_document() {
                tracing::debug!(
                    "Workspace symbol: skipping stale workspace index tier, using open-doc fallback"
                );
                return self.search_open_documents_for_symbols(query, cap);
            }

            let access_mode = route_index_access(self.coordinator());

            match access_mode {
                IndexAccessMode::Full(coordinator) => {
                    // Full query path: use workspace index.
                    // Pass the cap into the search so results are bounded before
                    // allocation — early exit at the search boundary, not after collecting.
                    let mut symbols = coordinator.index().search_source_symbols(query, Some(cap));
                    symbols.extend(
                        coordinator.index().search_generated_workspace_symbols(query, Some(cap)),
                    );

                    // Convert to LSP format with cooperative yielding.
                    // No .take(cap) needed — the search functions already apply the cap.
                    let lsp_symbols: Vec<LspWorkspaceSymbol> = symbols
                        .iter()
                        .enumerate()
                        .map(|(i, sym)| {
                            // Cooperative yield every 64 symbols
                            if i & 0x3f == 0 {
                                std::thread::yield_now();
                            }
                            sym.into()
                        })
                        .collect();
                    let generated_pilot_count = lsp_symbols
                        .iter()
                        .filter(|symbol| {
                            workspace_symbol_has_generated_label(
                                &symbol.name,
                                symbol.container_name.as_deref(),
                            )
                        })
                        .count();

                    if !lsp_symbols.is_empty() {
                        tracing::debug!(
                            count = lsp_symbols.len(),
                            "Workspace symbol: returned results from index (Ready state)"
                        );
                        self.record_workspace_symbols_provider_decision_trace(
                            query,
                            lsp_symbols.len(),
                            WorkspaceSymbolsTraceKind::SourceBackedReadyIndex,
                            generated_pilot_count,
                        );
                        return Ok(Some(to_json_array(&lsp_symbols)));
                    }
                    // If index is empty, fall through to open-doc search
                }
                IndexAccessMode::Partial(reason) => {
                    // Building/Degraded: still query the partial index so users get
                    // results from files already scanned.  Fall through to the
                    // open-doc path only when the partial index is also empty.
                    tracing::debug!(reason, "Workspace symbol: querying partial index");
                    if let Some(coordinator) = self.coordinator() {
                        let symbols = coordinator.index().search_source_symbols(query, Some(cap));
                        let lsp_symbols: Vec<LspWorkspaceSymbol> = symbols
                            .iter()
                            .enumerate()
                            .map(|(i, sym)| {
                                if i & 0x3f == 0 {
                                    std::thread::yield_now();
                                }
                                sym.into()
                            })
                            .collect();
                        if !lsp_symbols.is_empty() {
                            tracing::debug!(
                                count = lsp_symbols.len(),
                                "Workspace symbol: returned results from partial index"
                            );
                            self.record_workspace_symbols_provider_decision_trace(
                                query,
                                lsp_symbols.len(),
                                WorkspaceSymbolsTraceKind::PartialIndexFallback,
                                0,
                            );
                            return Ok(Some(to_json_array(&lsp_symbols)));
                        }
                    }
                    tracing::debug!(
                        reason,
                        "Workspace symbol: partial index empty, falling back to open-docs"
                    );
                }
                IndexAccessMode::None => {
                    tracing::debug!(
                        "Workspace symbol: no workspace feature, using open-doc fallback"
                    );
                }
            }
        }

        // Fallback/degraded path: search open documents only
        self.search_open_documents_for_symbols(query, cap)
    }

    /// Apply the shared provider-readiness policy before consulting the index.
    #[cfg(feature = "workspace")]
    pub(in crate::runtime) fn check_index_readiness(
        &self,
        policy: IndexReadinessPolicy,
    ) -> IndexReadinessOutcome {
        let outcome = check_readiness(self.coordinator(), &self.indexing_in_progress, policy);
        let index_readiness = outcome.reason();
        let index_ready = outcome.is_ready();
        let fallback_safe = outcome.is_fallback_safe();
        let unsafe_rejected = outcome.is_unsafe_rejected();
        tracing::trace!(
            index_readiness,
            index_ready,
            fallback_safe,
            unsafe_rejected,
            "index readiness evaluated"
        );
        outcome
    }

    /// Resolve the best-matching workspace folder URI for a given file URI.
    ///
    /// Used by the open-document fallback path to populate `workspaceFolderUri`
    /// on symbols so that multi-root workspace disambiguation works even before
    /// the workspace index is ready (fix for issue #1514 bug 2).
    ///
    /// Returns `None` when no workspace folder matches the file URI.
    ///
    /// Delegates to the same path-aware best-folder helper used by module
    /// resolution and completion, so nested workspaces and Windows path casing
    /// follow the existing runtime ownership rule.
    #[cfg(feature = "workspace")]
    pub(crate) fn resolve_folder_uri_for_file(&self, file_uri: &str) -> Option<String> {
        let folders = self.workspace_folders.lock();
        best_workspace_folder_for_doc(&folders, file_uri).map(|folder| folder.uri.clone())
    }

    #[cfg(feature = "workspace")]
    fn populate_workspace_folder_uri_for_symbols(&self, all_symbols: &mut [Value]) {
        // Populate workspaceFolderUri on each symbol for multi-root disambiguation.
        // The open-doc fallback path does not go through WorkspaceIndex, so
        // workspace_folder_uri is never set by the provider - inject it here
        // by matching the symbol's location URI against the server's workspace folders
        // (fix for issue #1514 bug 2).
        for sym in all_symbols {
            if let Some(obj) = sym.as_object_mut() {
                // Skip symbols that already carry a workspaceFolderUri.
                if obj.contains_key("workspaceFolderUri") {
                    continue;
                }
                // Resolve from location.uri (standard LSP WorkspaceSymbol shape).
                let file_uri = obj
                    .get("location")
                    .and_then(|loc| loc.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                if !file_uri.is_empty()
                    && let Some(folder_uri) = self.resolve_folder_uri_for_file(&file_uri)
                {
                    obj.insert("workspaceFolderUri".to_string(), Value::String(folder_uri));
                }
            }
        }
    }

    #[cfg(all(feature = "workspace", any(test, feature = "expose_lsp_test_api")))]
    /// Test-only helper for exercising workspace-folder URI injection branches.
    pub fn test_populate_workspace_folder_uri_for_symbols(&self, all_symbols: &mut [Value]) {
        self.populate_workspace_folder_uri_for_symbols(all_symbols);
    }

    /// Search only open documents for symbols (degraded/fallback path)
    #[cfg(feature = "workspace")]
    fn search_open_documents_for_symbols(
        &self,
        query: &str,
        cap: usize,
    ) -> Result<Option<Value>, JsonRpcError> {
        let docs_snapshot: Vec<(String, String, Option<Arc<perl_parser::ast::Node>>)> = {
            let documents = self.documents.lock();
            documents
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.text_arc.to_string(),
                        v.current_parsed().and_then(|p| p.ast().cloned()),
                    )
                })
                .collect()
        };

        let mut provider =
            perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolsProvider::new();
        let mut source_map = std::collections::HashMap::new();
        let mut text_fallback_symbols = Vec::new();

        for (i, (uri, text, ast)) in docs_snapshot.iter().enumerate() {
            if i & 0x7 == 0 {
                std::thread::yield_now();
            }
            source_map.insert(uri.clone(), text.clone());
            if let Some(ast) = ast {
                provider.index_document(uri, ast, text);
            } else {
                text_fallback_symbols.extend(self.extract_text_based_symbols(text, uri, query));
            }
        }

        let candidates = self.name_index_measurement_candidates(query);
        let provider_results = provider.search(query, &source_map);
        let canonical_matched_count = provider_results.len();
        let canonical_names_missing_from_candidates =
            count_canonical_names_missing_from(&provider_results, &candidates);
        tracing::debug!(
            candidate_count = candidates.len(),
            canonical_matched_count,
            canonical_names_missing_from_candidates,
            candidate_acceleration = WORKSPACE_SYMBOL_CANDIDATE_UNPROVEN,
            "Workspace symbol: canonical full search over open documents"
        );
        let mut all_symbols: Vec<Value> = provider_results
            .into_iter()
            .filter_map(|symbol| serde_json::to_value(symbol).ok())
            .collect();
        all_symbols.extend(
            text_fallback_symbols
                .into_iter()
                .filter_map(|symbol| serde_json::to_value(symbol).ok()),
        );
        all_symbols.truncate(cap);

        self.populate_workspace_folder_uri_for_symbols(&mut all_symbols);

        tracing::debug!(
            count = all_symbols.len(),
            "Workspace symbol: returned results from open documents"
        );
        self.record_workspace_symbols_provider_decision_trace(
            query,
            all_symbols.len(),
            WorkspaceSymbolsTraceKind::OpenDocumentFallback {
                name_index_candidate_count: candidates.len(),
                canonical_matched_count,
                canonical_names_missing_from_candidates,
            },
            0,
        );
        Ok(Some(to_json_array(&all_symbols)))
    }

    /// Workspace symbol runtime quality receipt for staged trust proof.
    ///
    /// Calls the live `workspace/symbol` handler and wraps the result in a typed
    /// receipt for staged cutover proof. Ready-index, non-empty queries can be
    /// classified as the narrow source-backed live slice; fallback and unproven
    /// shapes remain gated.
    ///
    /// Promotes only labeled source-backed generated/framework members in the
    /// full ready index. Stale, dynamic, ambiguous, partial-index,
    /// generated/no-source, and open-document fallback candidates remain gated.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn workspace_symbols_runtime_quality_receipt(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let query = params
            .as_ref()
            .and_then(|p| p.get("query"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        let live_provider_result = self.handle_workspace_symbols_v2(params)?;
        let live_provider_count = match live_provider_result.as_ref() {
            Some(Value::Array(items)) => items.len(),
            _ => 0,
        };
        let source_backed_count = self.workspace_symbols_source_backed_count(&query);
        let generated_pilot_count =
            workspace_symbols_labeled_generated_count(live_provider_result.as_ref());
        let no_live_behavior_change = source_backed_count == 0 && generated_pilot_count == 0;
        let shadow_state = match (source_backed_count > 0, generated_pilot_count > 0) {
            (true, true) => "partial_live_source_backed_generated_pilot",
            (true, false) => "partial_live_source_backed",
            (false, true) => "partial_live_generated_labeled_pilot",
            (false, false) => "shadowed",
        };
        let compiler_receipt = if no_live_behavior_change {
            Value::Null
        } else {
            let (source, provenance, confidence) =
                match (source_backed_count > 0, generated_pilot_count > 0) {
                    (true, true) => {
                        ("CompilerFact+FrameworkAdapter", "MixedExactAstFrameworkAnchor", "Medium")
                    }
                    (true, false) => ("CompilerFact", "ExactAst", "High"),
                    (false, true) => ("FrameworkAdapter", "FrameworkAnchor", "Medium"),
                    (false, false) => ("None", "None", "None"),
                };
            json!({
                "source": source,
                "provenance": provenance,
                "confidence": confidence,
                "freshness": "Fresh",
                "fallback_state": "Primary",
                "source_backed_count": source_backed_count,
                "source_backed_provenance": if source_backed_count > 0 { "ExactAst" } else { "None" },
                "generated_pilot_count": generated_pilot_count,
                "generated_pilot_provenance": if generated_pilot_count > 0 { "FrameworkAnchor" } else { "None" },
                "generated_pilot_confidence": if generated_pilot_count > 0 { "Medium" } else { "None" },
                "generated_pilot_location_semantics": if generated_pilot_count > 0 { "source_anchor_not_exact_generated_body" } else { "None" },
                "claim_boundary": "ready workspace index source-backed symbols plus labeled source-backed generated/framework pilot symbols only; dynamic, stale, ambiguous, fallback/noise, and partial-index candidates remain gated"
            })
        };
        let (gated_expansion_receipt, gated_expansion_candidate_count) =
            workspace_symbols_generated_dynamic_noise_receipt(&query);

        Ok(Some(json!({
            "provider": "workspace_symbols",
            "query": query,
            "live_provider_result": live_provider_result,
            "live_provider_count": live_provider_count,
            "shadow_state": shadow_state,
            "compiler_receipt": compiler_receipt,
            "gated_expansion_receipt": gated_expansion_receipt,
            "no_live_behavior_change": no_live_behavior_change,
            "notes": [
                format!(
                    "workspace-symbol runtime quality receipt: query={:?}; live_provider_count={}; \
                     source_backed_compiler_symbols={}; \
                     labeled_generated_pilot_symbols={}; \
                     generated_dynamic_noise_candidates={}; \
                     fresh ready-state workspace index symbols and labeled source-backed generated/framework pilot symbols are live for non-empty queries; \
                     empty, partial-index, stale, dynamic, generated/no-source, ambiguous, and fallback/noise cases keep fallback or gated behavior",
                    query,
                    live_provider_count,
                    source_backed_count,
                    generated_pilot_count,
                    gated_expansion_candidate_count
                )
            ]
        })))
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn workspace_symbols_generated_dynamic_noise_receipt(query: &str) -> (Value, usize) {
    let candidates = vec![
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::shadow(
            "generated:source-anchor:workspace-symbol:framework_accessor:virtual",
            ProviderFactSourceKind::FrameworkAdapter,
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            ProviderFactFreshness::Fresh,
        ),
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::blocked(
            "generated:no-source:workspace-symbol:runtime_installed_method:unanchored",
            ProviderFactSourceKind::FrameworkAdapter,
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            ProviderFactFreshness::Fresh,
        ),
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::blocked(
            "generated:no-source:workspace-symbol:role_composed_method:unanchored",
            ProviderFactSourceKind::FrameworkAdapter,
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            ProviderFactFreshness::Fresh,
        ),
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::blocked(
            "blocker:workspace-symbol:dynamic_symbolic_reference",
            ProviderFactSourceKind::DynamicBoundary,
            Provenance::DynamicBoundary,
            Confidence::High,
            ProviderFactFreshness::Fresh,
        ),
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::blocked(
            "stale:workspace-symbol:removed_symbol",
            ProviderFactSourceKind::CompilerFact,
            Provenance::SemanticAnalyzer,
            Confidence::Low,
            ProviderFactFreshness::Stale,
        ),
        perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolShadowCandidate::fallback(
            "fallback:workspace-symbol:low_confidence_text_match",
            ProviderFactSourceKind::Fallback,
            Provenance::SearchFallback,
            Confidence::Low,
            ProviderFactFreshness::Unknown,
        ),
    ];
    let candidate_count = candidates.len();

    let generated_candidate_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.source == ProviderFactSourceKind::FrameworkAdapter
                && candidate.fallback_state != ProviderFallbackState::Blocked
        })
        .count();
    let generated_no_source_blocker_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.source == ProviderFactSourceKind::FrameworkAdapter
                && candidate.fallback_state == ProviderFallbackState::Blocked
                && candidate.identity.contains(":no-source:")
        })
        .count();
    let generated_no_source_candidate_identities = candidates
        .iter()
        .filter(|candidate| {
            candidate.source == ProviderFactSourceKind::FrameworkAdapter
                && candidate.fallback_state == ProviderFallbackState::Blocked
                && candidate.identity.contains(":no-source:")
        })
        .map(|candidate| candidate.identity.clone())
        .collect::<Vec<_>>();
    let dynamic_boundary_blocker_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.fallback_state == ProviderFallbackState::Blocked
                && candidate.source == ProviderFactSourceKind::DynamicBoundary
        })
        .count();
    let stale_fact_blocker_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.fallback_state == ProviderFallbackState::Blocked
                && candidate.freshness == ProviderFactFreshness::Stale
        })
        .count();
    let fallback_noise_candidate_count = candidates
        .iter()
        .filter(|candidate| {
            candidate.fallback_state == ProviderFallbackState::Fallback
                || candidate.confidence == Confidence::Low
                || candidate.freshness != ProviderFactFreshness::Fresh
        })
        .count();

    let shadow = perl_lsp_rs_core::providers::workspace_symbols::workspace_symbol_source_shadow(
        Vec::new(),
        candidates,
        query,
    );

    (
        json!({
            "schema_version": 1,
            "receipt_kind": "generated_dynamic_noise_expansion",
            "generated_candidate_count": generated_candidate_count,
            "generated_false_exact_candidate_count": generated_candidate_count,
            "generated_no_source_candidate_count": generated_no_source_blocker_count,
            "generated_no_source_blocker_count": generated_no_source_blocker_count,
            "generated_no_source_candidate_identities": generated_no_source_candidate_identities,
            "generated_location_semantics": "source_anchor_not_exact_generated_body",
            "dynamic_boundary_blocker_count": dynamic_boundary_blocker_count,
            "dynamic_false_exact_blocker_count": dynamic_boundary_blocker_count,
            "stale_fact_blocker_count": stale_fact_blocker_count,
            "fallback_noise_candidate_count": fallback_noise_candidate_count,
            "no_live_behavior_change": true,
            "edit_freshness_policy": "labeled generated workspace-symbol queries must recompute from fresh document state after didChange; stale compiler-fact shadow candidates remain blocked by the gated-expansion receipt",
            "claim_boundary": "workspace-symbol generated/dynamic/noise expansion receipt only; generated/no-source false-exact candidates, dynamic-boundary candidates, stale compiler-fact shadow candidates, and fallback/noise candidates remain gated outside the labeled source-backed generated pilot",
            "shadow_receipt": shadow.receipt,
        }),
        candidate_count,
    )
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn workspace_symbols_labeled_generated_count(result: Option<&Value>) -> usize {
    match result {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|item| {
                workspace_symbol_has_generated_label(
                    item.get("name").and_then(Value::as_str).unwrap_or_default(),
                    item.get("containerName").and_then(Value::as_str),
                )
            })
            .count(),
        _ => 0,
    }
}

fn workspace_symbol_has_generated_label(name: &str, container_name: Option<&str>) -> bool {
    name.contains("[generated/framework]")
        || container_name.is_some_and(|container| container.contains("[generated/framework]"))
}

/// The name index is a candidate accelerator only: until a candidate set proves
/// it is a superset of every canonical match tier (#8262), workspace-symbol
/// search runs the canonical full matcher and records the name-index profile as
/// measurement only.
const WORKSPACE_SYMBOL_CANDIDATE_UNPROVEN: &str = "unproven_superset_full_search";

fn count_canonical_names_missing_from(
    results: &[perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbol],
    candidates: &[String],
) -> usize {
    let candidates: HashSet<&str> = candidates.iter().map(String::as_str).collect();
    let result_names: HashSet<&str> = results.iter().map(|symbol| symbol.name.as_str()).collect();
    result_names.iter().filter(|name| !candidates.contains(**name)).count()
}

impl LspServer {
    /// Name-index candidate profile for measurement only.
    ///
    /// Mirrors the historical candidate composition (case-sensitive prefix,
    /// falling back to exact-token fuzzy when empty) so traces can quantify what
    /// the accelerator would have contributed. The result never restricts the
    /// canonical workspace-symbol search: a non-empty candidate set is not
    /// completeness evidence (#8262).
    ///
    /// Live measurements are best-effort across transient `didChange` windows:
    /// `symbol_index` reflects the prior generation until post-parse side
    /// effects run, so counts recorded during a pending parse may compare
    /// canonical results and candidates from different generations. Discriminating
    /// accelerator validation happens through the pinned differential tests, not
    /// through live-window receipts.
    fn name_index_measurement_candidates(&self, query: &str) -> Vec<String> {
        let mut candidates = self.symbol_index.lock().search_prefix(query);
        if candidates.is_empty() && !query.is_empty() {
            candidates = self.symbol_index.lock().search_fuzzy(query);
        }
        let mut seen = HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.clone()));
        candidates
    }

    /// Search open documents for symbols (non-workspace stub)
    #[cfg(not(feature = "workspace"))]
    fn search_open_documents_for_symbols(
        &self,
        query: &str,
        _cap: usize,
    ) -> Result<Option<Value>, JsonRpcError> {
        tracing::debug!(query, "Workspace symbol: no workspace feature, returning empty");
        Ok(Some(json!([])))
    }

    /// Handle workspace/symbol request (legacy implementation)
    #[cfg(not(feature = "workspace"))]
    pub(super) fn handle_workspace_symbols(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().workspace_symbol {
            return Err(crate::protocol::method_not_advertised());
        }

        let query = params
            .as_ref()
            .and_then(|p| p.get("query"))
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .trim();

        tracing::debug!(query, "Workspace symbol search");

        // Try the prebuilt workspace index first to avoid re-indexing every
        // open document on each workspace/symbol request (#4999 claim 1).
        // Only fall back to the expensive per-document re-index when the
        // index is genuinely unavailable or returned no results.
        #[cfg(feature = "workspace")]
        {
            let _ = self.check_index_readiness(IndexReadinessPolicy::NoWait);
            if !self.workspace_index_stale_for_any_open_document() {
                let access_mode = route_index_access(self.coordinator());
                if let IndexAccessMode::Full(coordinator) = access_mode {
                    let cap = workspace_symbol_cap();
                    let mut symbols = coordinator.index().search_source_symbols(query, Some(cap));
                    symbols.extend(
                        coordinator.index().search_generated_workspace_symbols(query, Some(cap)),
                    );
                    if !symbols.is_empty() {
                        let lsp_symbols: Vec<Value> = symbols
                            .iter()
                            .map(|sym| serde_json::to_value(sym).unwrap_or_else(|_| json!({})))
                            .collect();
                        tracing::debug!(
                            count = lsp_symbols.len(),
                            "Workspace symbol: served from prebuilt index (v1 fast path)"
                        );
                        return Ok(Some(to_json_array(&lsp_symbols)));
                    }
                    // Index returned empty — fall through to re-index fallback.
                }
            }
        }

        // Fallback: re-index open documents (expensive, O(docs × size)).
        // Lightweight snapshot: only clone fields needed for symbol extraction,
        // avoiding expensive Rope, ParentMap, LineStartsCache, and parse_errors clones.
        let docs_snapshot: Vec<(String, String, Option<Arc<perl_parser::ast::Node>>)> = {
            let documents = self.documents.lock();
            documents
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.text_arc.to_string(),
                        v.current_parsed().and_then(|p| p.ast().cloned()),
                    )
                })
                .collect()
        };

        // Build source map and index documents with WorkspaceSymbolsProvider.
        let cap = workspace_symbol_cap();
        let mut provider =
            perl_lsp_rs_core::providers::workspace_symbols::WorkspaceSymbolsProvider::new();
        let mut source_map = std::collections::HashMap::new();
        for (uri, text, ast) in docs_snapshot.iter() {
            if let Some(ast) = ast {
                provider.index_document(uri, ast, text);
            }
            source_map.insert(uri.clone(), text.clone());
        }

        let candidates = self.name_index_measurement_candidates(query);
        let mut symbols = provider.search(query, &source_map);
        tracing::debug!(
            count = symbols.len(),
            cap,
            candidate_count = candidates.len(),
            canonical_matched_count = symbols.len(),
            canonical_names_missing_from_candidates =
                count_canonical_names_missing_from(&symbols, &candidates),
            candidate_acceleration = WORKSPACE_SYMBOL_CANDIDATE_UNPROVEN,
            "Found symbols total"
        );
        symbols.truncate(cap);

        let result = serde_json::to_value(&symbols).unwrap_or_else(|_| json!([]));

        Ok(Some(result))
    }

    /// Handle workspaceSymbol/resolve request
    pub(super) fn handle_workspace_symbol_resolve(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let params = params.ok_or_else(|| {
            crate::protocol::invalid_params(
                "workspace/symbol/resolve: missing required parameter 'params'",
            )
        })?;

        // Extract the symbol to resolve
        let symbol = params.as_object().ok_or_else(|| {
            crate::protocol::invalid_params(
                "workspace/symbol/resolve: parameter 'params' must be an object",
            )
        })?;

        // Get the URI and name from the symbol
        let uri = symbol
            .get("location")
            .and_then(|l| l.get("uri"))
            .and_then(|u| u.as_str())
            .unwrap_or("");

        let name = symbol.get("name").and_then(|n| n.as_str()).unwrap_or("");

        // Normalize the URI for lookup
        let uri_key = self.normalize_uri_key(uri);

        // Look up the symbol in our index to get more details
        let documents = self.documents.lock();
        let doc_opt = documents.get(&uri_key).or_else(|| documents.get(uri)); // try raw as a fallback

        if let Some(doc) = doc_opt {
            let parsed = doc.current_parsed();
            if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                // Find the symbol in the AST to get more accurate information
                let extractor = crate::symbol::SymbolExtractor::new_with_source(&doc.text);
                let symbol_table = extractor.extract(ast);

                // Find matching symbol
                for symbols in symbol_table.symbols.values() {
                    for sym in symbols {
                        if sym.name == name {
                            // Return enhanced symbol with detail and accurate range
                            let start_pos =
                                doc.line_starts.offset_to_position(&doc.text, sym.location.start);
                            let end_pos =
                                doc.line_starts.offset_to_position(&doc.text, sym.location.end);

                            // Start with the provided symbol JSON so we can add
                            // additional details without panicking if fields are missing
                            let mut resolved = json!(symbol);

                            use crate::symbol::VarKind;
                            // Add detail based on symbol kind
                            let detail = match sym.kind {
                                crate::symbol::SymbolKind::Subroutine => {
                                    format!("sub {}", name)
                                }
                                crate::symbol::SymbolKind::Method => {
                                    format!("method {}", name)
                                }
                                crate::symbol::SymbolKind::Variable(VarKind::Scalar) => {
                                    format!("${}", name)
                                }
                                crate::symbol::SymbolKind::Variable(VarKind::Array) => {
                                    format!("@{}", name)
                                }
                                crate::symbol::SymbolKind::Variable(VarKind::Hash) => {
                                    format!("%{}", name)
                                }
                                crate::symbol::SymbolKind::Package => {
                                    format!("package {}", name)
                                }
                                crate::symbol::SymbolKind::Constant => {
                                    format!("constant {}", name)
                                }
                                _ => name.to_string(),
                            };
                            resolved["detail"] = json!(detail);
                            if let Some(doc) = &sym.documentation {
                                resolved["documentation"] = json!(doc);
                            }

                            // Update location with accurate range
                            resolved["location"]["range"] = json!({
                                "start": {
                                    "line": start_pos.0,
                                    "character": start_pos.1,
                                },
                                "end": {
                                    "line": end_pos.0,
                                    "character": end_pos.1,
                                }
                            });

                            // Add container name derived from qualified symbol name
                            if let Some(container) =
                                perl_parser_core::qualified_name::container_name(
                                    &sym.qualified_name,
                                )
                            {
                                resolved["containerName"] = json!(
                                    perl_module::path::normalize_package_separator(container)
                                );
                            }

                            return Ok(Some(json!(resolved)));
                        }
                    }
                }
            }
        }

        // Return the original symbol if we couldn't enhance it
        Ok(Some(json!(symbol)))
    }

    /// Handle workspace/configuration request
    ///
    /// Supports both direct array format and ConfigurationParams with items property
    pub(super) fn handle_configuration(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            // Support both direct array format and ConfigurationParams with items property
            let items =
                params.get("items").and_then(|i| i.as_array()).or_else(|| params.as_array());

            if let Some(items) = items {
                let mut results = Vec::new();

                for item in items {
                    if let Some(section) = item.get("section").and_then(|s| s.as_str()) {
                        tracing::debug!(section, "Configuration requested");

                        // Handle workspace configuration sections
                        let value = if section.starts_with("perl.workspace.") {
                            let workspace_config = self.workspace_config.lock();
                            match section {
                                "perl.workspace.includePaths" => {
                                    json!(workspace_config.include_paths)
                                }
                                "perl.workspace.discoveryExtensions" => {
                                    json!(workspace_config.discovery_extra_extensions)
                                }
                                "perl.workspace.discoverySkippedDirs" => {
                                    json!(workspace_config.discovery_extra_skipped_dirs)
                                }
                                "perl.workspace.useSystemInc" => {
                                    json!(workspace_config.use_system_inc)
                                }
                                "perl.workspace.usePerl5lib" => {
                                    json!(workspace_config.use_perl5lib)
                                }
                                "perl.workspace.perl5libPrecedence" => {
                                    let precedence = match workspace_config.perl5lib_precedence {
                                        perl_lsp_rs_core::config::Perl5LibPrecedence::Prepend => {
                                            "prepend"
                                        }
                                        perl_lsp_rs_core::config::Perl5LibPrecedence::Append => {
                                            "append"
                                        }
                                    };
                                    json!(precedence)
                                }
                                "perl.workspace.resolutionTimeout" => {
                                    json!(workspace_config.resolution_timeout_ms)
                                }
                                _ => json!(null),
                            }
                        } else {
                            let config = self.config.lock();
                            match section {
                                "perl.inlayHints.enabled" => json!(config.inlay_hints_enabled),
                                "perl.inlayHints.parameterHints" => {
                                    json!(config.inlay_hints_parameter_hints)
                                }
                                "perl.inlayHints.typeHints" => json!(config.inlay_hints_type_hints),
                                "perl.inlayHints.chainedHints" => {
                                    json!(config.inlay_hints_chained_hints)
                                }
                                "perl.inlayHints.maxLength" => json!(config.inlay_hints_max_length),
                                "perl.formatting.enabled" => json!(config.perltidy_enabled),
                                "perl.formatting.engine" => {
                                    let engine = match config.formatting_engine {
                                        perl_lsp_rs_core::config::FormatterMode::Native => "native",
                                        perl_lsp_rs_core::config::FormatterMode::Compat => "compat",
                                        perl_lsp_rs_core::config::FormatterMode::ExternalLegacy => {
                                            "external-perltidy"
                                        }
                                        perl_lsp_rs_core::config::FormatterMode::Off => "off",
                                    };
                                    json!(engine)
                                }
                                "perl.formatting.profile" => json!(config.perltidy_profile),
                                "perl.formatting.maximumLineLength" => {
                                    json!(config.perltidy_maximum_line_length)
                                }
                                "perl.formatting.indentColumns" => {
                                    json!(config.perltidy_indent_columns)
                                }
                                "perl.formatting.tabs" => json!(config.perltidy_tabs),
                                "perl.formatting.openingBraceOnNewLine" => {
                                    json!(config.perltidy_opening_brace_on_new_line)
                                }
                                "perl.formatting.cuddledElse" => {
                                    json!(config.perltidy_cuddled_else)
                                }
                                "perl.formatting.spaceAfterKeyword" => {
                                    json!(config.perltidy_space_after_keyword)
                                }
                                "perl.formatting.addTrailingCommas" => {
                                    json!(config.perltidy_add_trailing_commas)
                                }
                                "perl.formatting.verticalAlignment" => {
                                    json!(config.perltidy_vertical_alignment)
                                }
                                "perl.formatting.blockCommentIndentation" => {
                                    json!(config.perltidy_block_comment_indentation)
                                }
                                "perl.formatting.extraArgs" => json!(config.perltidy_extra_args),
                                "perl.formatting.timeoutSecs" => {
                                    json!(config.perltidy_timeout_secs)
                                }
                                _ => json!(null),
                            }
                        };

                        results.push(value);
                    }
                }

                return Ok(Some(json!(results)));
            }
        }

        Ok(Some(json!([])))
    }
}

/// Extract the perl-specific settings object from a configuration payload.
///
/// Accepts both the standard wrapped form `{"perl": {...}}` and the unwrapped form `{...}` used
/// by clients such as Sublime Text's LSP package that omit the outer `"perl"` key.
pub(crate) fn extract_perl_settings(settings: &Value) -> Option<&Value> {
    if let Some(perl) = settings.get("perl")
        && perl.is_object()
    {
        return Some(perl);
    }
    // Unwrapped: the settings object itself contains perl config keys directly.
    if settings.is_object() { Some(settings) } else { None }
}

impl LspServer {
    /// Surface invalid enum values from editor-provided settings without changing
    /// the fail-safe configuration update behavior.
    fn warn_invalid_client_settings(&self, settings: &Value) {
        for invalid in
            perl_lsp_rs_core::config::ServerConfig::invalid_client_setting_values(settings)
        {
            let normalized_value = if invalid.setting == "formatting.engine" {
                perl_lsp_rs_core::config::normalize_formatter_mode_value(&invalid.value)
            } else {
                invalid.value.trim().to_ascii_lowercase()
            };
            let key = format!("{}={}={normalized_value}", invalid.setting, invalid.value_type);
            if !self.client_setting_warnings_sent.lock().insert(key) {
                continue;
            }

            let message = format!(
                "Perl LSP ignored invalid `{}` value {:?}; keeping the current setting. Valid values: {}.",
                invalid.setting, invalid.value, invalid.valid_options
            );
            if let Err(error) =
                self.show_message(crate::runtime::window::MessageType::Warning, &message)
            {
                tracing::warn!(
                    setting = invalid.setting,
                    error = %error,
                    "failed to show invalid client setting warning"
                );
            }
        }
    }

    /// Handle workspace/didChangeConfiguration notification
    ///
    /// Updates both ServerConfig and WorkspaceConfig when the client
    /// notifies of configuration changes.
    pub(super) fn handle_did_change_configuration(&self, params: Option<Value>) {
        if let Some(params) = params
            && let Some(settings) = params.get("settings")
        {
            tracing::debug!("Configuration changed, updating server settings");

            // Read perl settings once and update both configs.
            // Some clients (e.g. Sublime Text's LSP package) send settings without
            // wrapping them under a top-level "perl" key. Accept both shapes:
            //   - Wrapped:   {"perl": { "workspace": { "includePaths": [...] } }}
            //   - Unwrapped: { "workspace": { "includePaths": [...] } }
            if let Some(perl) = extract_perl_settings(settings) {
                self.warn_invalid_client_settings(perl);
                // Snapshot the critic-relevant config fields before applying the
                // update so we can decide whether to reset the shared
                // CriticAnalyzer (config-bound on severity/profile/enabled). We
                // compare before/after `update_from_value` rather than re-parsing
                // the payload here so this stays in lockstep with the parser — in
                // particular it detects severity/enabled changes that arrive via
                // either the legacy `perlcritic.*` keys or the native `critic.*`
                // keys, which the parser folds into the same fields.
                #[cfg(not(target_arch = "wasm32"))]
                let critic_snapshot_before = {
                    let cfg = self.config.lock();
                    (
                        cfg.perlcritic_enabled,
                        cfg.perlcritic_severity,
                        cfg.perlcritic_profile.clone(),
                        cfg.perlcritic_theme.clone(),
                        cfg.native_critic_profile.clone(),
                        cfg.native_critic_include.clone(),
                        cfg.native_critic_exclude.clone(),
                    )
                };

                // Update server-owned LSP configuration.
                {
                    let mut config = self.config.lock();
                    config.update_from_value(perl);
                    tracing::debug!("Updated server config from perl settings");
                }

                #[cfg(not(target_arch = "wasm32"))]
                let critic_config_changed = {
                    let cfg = self.config.lock();
                    critic_snapshot_before
                        != (
                            cfg.perlcritic_enabled,
                            cfg.perlcritic_severity,
                            cfg.perlcritic_profile.clone(),
                            cfg.perlcritic_theme.clone(),
                            cfg.native_critic_profile.clone(),
                            cfg.native_critic_include.clone(),
                            cfg.native_critic_exclude.clone(),
                        )
                };

                // Reset the shared CriticAnalyzer when any critic-related setting
                // changed so the next diagnostic cycle rebuilds it with the new config.
                #[cfg(not(target_arch = "wasm32"))]
                if critic_config_changed {
                    *self.critic_analyzer.lock() = None;
                    self.critic_workspace_warnings_sent.lock().clear();
                    self.pull_diagnostics_orchestrator.reset();
                }

                // Update workspace config (include paths, @INC)
                {
                    let mut workspace_config = self.workspace_config.lock();
                    let root_path = self.root_path.lock().clone();
                    let rejected = workspace_config.update_from_value_with_context(
                        perl,
                        WorkspaceConfigUpdateContext {
                            workspace_root: root_path.as_deref(),
                            external_include_paths: ExternalIncludePathAuthority::Untrusted(
                                UnauthorizedExternalIncludePathSource::DidChangeConfiguration,
                            ),
                        },
                    );
                    for entry in rejected {
                        tracing::warn!(
                            target: "perl_lsp::config",
                            entry = %entry.entry,
                            reason = %entry.render(),
                            "rejected client includePaths entry"
                        );
                    }
                    tracing::debug!("Updated workspace config from perl settings");
                }

                // Update global limits from the same perl settings layer.
                if let Ok(mut limits) = perl_lsp_rs_core::runtime::limits::LSP_LIMITS.write() {
                    limits.update_from_value(perl);
                }

                // Apply global client settings to each folder's effective config immediately.
                // The async workspace/configuration pull that follows will refine per-folder
                // settings once the client responds, but we update now so the window between
                // didChangeConfiguration arrival and the pull response doesn't leave folders
                // with stale settings.
                {
                    let mut folders = self.workspace_folders.lock();
                    let init_options_perl = self.initialization_options_perl_settings.lock();
                    for folder in folders.iter_mut() {
                        let mut effective_config =
                            perl_lsp_rs_core::config::WorkspaceConfig::default();
                        if let Some(init_opts) = init_options_perl.as_ref() {
                            let rejected = effective_config.update_from_value_with_context(
                                init_opts,
                                WorkspaceConfigUpdateContext {
                                    workspace_root: folder.path.as_deref(),
                                    external_include_paths: ExternalIncludePathAuthority::Untrusted(
                                        UnauthorizedExternalIncludePathSource::InitializationOptions,
                                    ),
                                },
                            );
                            for entry in rejected {
                                tracing::warn!(
                                    target: "perl_lsp::config",
                                    folder_uri = %folder.uri,
                                    entry = %entry.entry,
                                    reason = %entry.render(),
                                    "rejected initializationOptions includePaths entry"
                                );
                            }
                        }
                        if let Some(project_config) = &folder.project_config {
                            // Re-applying an already-loaded, already-warned-about
                            // project_config; discard the rejection list rather than
                            // re-warning on every reconfiguration.
                            if let Some(folder_path) = folder.path.as_deref() {
                                let _ = project_config
                                    .apply_to_workspace_config(&mut effective_config, folder_path);
                            }
                        }
                        let rejected = effective_config.update_from_value_with_context(
                            perl,
                            WorkspaceConfigUpdateContext {
                                workspace_root: folder.path.as_deref(),
                                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                                    UnauthorizedExternalIncludePathSource::DidChangeConfiguration,
                                ),
                            },
                        );
                        for entry in rejected {
                            tracing::warn!(
                                target: "perl_lsp::config",
                                folder_uri = %folder.uri,
                                entry = %entry.entry,
                                reason = %entry.render(),
                                "rejected client includePaths entry"
                            );
                        }
                        folder.effective_workspace_config = effective_config;
                        folder.refresh_workspace_metadata();
                    }
                }

                // A configuration notification starts a new user-visible
                // configuration session; do not let an old auth failure
                // suppress feedback after settings are changed or removed.
                self.ai_backend_warnings_sent.lock().clear();

                // Refresh AI backend when config changes (constructs or clears provider)
                self.refresh_ai_backend();

                // Trigger client refresh for configuration-dependent features
                if let Err(e) = self.refresh_controller.refresh_all(self) {
                    tracing::warn!(error = %e, "Failed to refresh client after config change");
                }
            }
        }

        // Invalidate client-provided workspace/configuration values and re-fetch.
        self.pending_workspace_configuration_requests.lock().clear();
        self.request_workspace_configuration_for_folders();
    }

    /// Handle workspace/didChangeWatchedFiles notification
    ///
    /// Deterministic state transitions:
    /// - DELETED events are processed immediately (low frequency, state cleanup)
    /// - CREATED/CHANGED events are debounced to avoid blocking I/O storms during
    ///   bulk operations (e.g., `git checkout`, formatter rewrites)
    /// - State recovery is handled by coordinator's internal logic
    pub(super) fn handle_did_change_watched_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        use lsp_types::{DidChangeWatchedFilesParams, FileChangeType};

        let Some(params) = params else {
            return Ok(None);
        };

        let Ok(params) = serde_json::from_value::<DidChangeWatchedFilesParams>(params) else {
            tracing::warn!("Failed to parse didChangeWatchedFiles params");
            return Ok(None);
        };

        for change in params.changes {
            let uri = change.uri.to_string();
            let change_type = change.typ;

            tracing::debug!(uri, change_type = ?change_type, "File change detected");

            match change_type {
                FileChangeType::DELETED => {
                    // DELETED must be processed immediately — the file is gone and
                    // stale index data should not linger.
                    #[cfg(feature = "workspace")]
                    if let Some(coordinator) = self.coordinator() {
                        coordinator.notify_change(&uri);
                    }

                    self.evict_deleted_file_state(&uri);

                    tracing::debug!(uri, "Removed deleted file from index");

                    #[cfg(feature = "workspace")]
                    if let Some(coordinator) = self.coordinator() {
                        coordinator.notify_parse_complete(&uri);
                    }
                }
                FileChangeType::CREATED | FileChangeType::CHANGED => {
                    // CREATED and CHANGED are debounced so that bulk operations
                    // (git checkout, formatter rewrites, etc.) coalesce into a
                    // single batch rather than triggering many sequential file reads.
                    if !self.schedule_file_watcher_uri(&uri) {
                        // `false` means the event was NOT queued: no debouncer
                        // is installed (unit-test path), or the coalescer
                        // reported Overflowed/Unavailable/ShuttingDown (#8064).
                        // Either way, fall through to immediate synchronous
                        // processing so degraded modes never lose events.
                        self.process_file_watcher_uri_immediate(&uri);
                    }
                }
                _ => {}
            }
        }

        // This is a notification, no response needed
        Ok(None)
    }

    /// Process a debounced batch of file URIs.
    ///
    /// Called by the [`FileWatcherDebouncer`] background thread after the quiet
    /// period expires.  Re-reads and re-indexes each URI.  Files that no longer
    /// exist on disk are silently skipped — they should have arrived as DELETED
    /// events and been handled immediately.
    pub(crate) fn handle_watched_file_batch(&self, uris: Vec<String>) {
        tracing::debug!("Processing debounced file watcher batch: {} URIs", uris.len());
        for uri in &uris {
            self.process_file_watcher_uri_immediate(uri);
        }
    }

    /// Re-index a single URI from the file system.
    ///
    /// Shared implementation used by both the debounced batch path and the
    /// immediate fall-through path when no debouncer is installed.
    fn process_file_watcher_uri_immediate(&self, uri: &str) {
        // Notify coordinator of pending change
        #[cfg(feature = "workspace")]
        if let Some(coordinator) = self.coordinator() {
            coordinator.notify_change(uri);
        }

        let mut loaded_content: Option<String> = None;

        // Re-index the file if it is a Perl source file.
        #[cfg(feature = "workspace")]
        if let Some(coordinator) = self.coordinator()
            && is_perl_source_uri(uri)
        {
            if loaded_content.is_none() {
                loaded_content = read_watched_file_content(uri, "re-indexing");
            }

            let workspace_index = coordinator.index();
            if let Ok(url) = url::Url::parse(uri)
                && let Some(content) = loaded_content.as_ref()
            {
                // Clear old index data before re-indexing
                workspace_index.clear_file(uri);
                match workspace_index.index_file(url, content.clone()) {
                    Ok(()) => tracing::debug!("Re-indexed file: {}", uri),
                    Err(e) => {
                        tracing::warn!("Failed to re-index file {}: {}", uri, e);
                    }
                }
            }
        }

        // For open documents, do NOT overwrite doc.text or bump doc.version.
        // The document map is authoritative for open files — the editor's
        // didChange notifications drive the content. Overwriting from disk
        // clobbers unsaved user edits and the blind version+1 can cause
        // subsequent didChange to be silently dropped as stale. (#5112, #5040)
        //
        // The workspace index above was already re-indexed from disk, which
        // is sufficient for cross-file features. The document map stays as-is.
        #[cfg(feature = "workspace")]
        {
            let document_is_open = {
                let documents = self.documents.lock();
                self.get_document(&documents, uri).is_some()
            };

            if document_is_open {
                tracing::debug!(
                    "File watcher change for open document {} — skipping in-memory overwrite (document map is authoritative)",
                    uri
                );
            }
        }

        // Notify coordinator that file processing is complete
        #[cfg(feature = "workspace")]
        if let Some(coordinator) = self.coordinator() {
            coordinator.notify_parse_complete(uri);
        }

        tracing::debug!("Processed file watcher change: {}", uri);
    }

    /// Handle workspace/didDeleteFiles notification
    pub(super) fn handle_did_delete_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params
            && let Some(files) = params["files"].as_array()
        {
            for file in files {
                let Some(uri) = file["uri"].as_str() else {
                    continue;
                };

                tracing::debug!(uri, "File deleted");

                #[cfg(feature = "workspace")]
                if let Some(coordinator) = self.coordinator() {
                    coordinator.notify_change(uri);
                }
                self.evict_deleted_file_state(uri);
                #[cfg(feature = "workspace")]
                if let Some(coordinator) = self.coordinator() {
                    coordinator.notify_parse_complete(uri);
                }
            }

            // Trigger client refresh after file deletions
            if let Err(e) = self.refresh_controller.refresh_all(self) {
                tracing::warn!(error = %e, "Failed to refresh client after file deletions");
            }
        }

        // This is a notification, no response needed
        Ok(None)
    }

    /// Handle workspace/willDeleteFiles request
    pub(super) fn handle_will_delete_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params
            && let Some(files) = params["files"].as_array()
        {
            #[cfg(feature = "workspace")]
            if let Some(coordinator) = self.coordinator() {
                let idx = coordinator.index();
                let open_documents: Vec<(String, String)> = {
                    let documents = self.documents.lock();
                    documents
                        .iter()
                        .map(|(uri, doc)| (uri.clone(), doc.text_arc.to_string()))
                        .collect()
                };
                let deleting_uris: std::collections::HashSet<String> = files
                    .iter()
                    .filter_map(|file| file["uri"].as_str().map(|uri| self.normalize_uri_key(uri)))
                    .collect();
                let mut unsafe_deletes: Vec<(String, usize, Vec<String>)> = Vec::new();

                for file in files {
                    let Some(uri) = file["uri"].as_str() else {
                        continue;
                    };

                    tracing::debug!(uri, "File will be deleted");
                    let mut dependents: std::collections::BTreeSet<String> =
                        collect_cross_file_delete_dependents(idx, uri, &deleting_uris);
                    dependents.extend(collect_open_document_delete_dependents(
                        idx,
                        uri,
                        &deleting_uris,
                        &open_documents,
                    ));
                    dependents.extend(collect_symbol_reference_delete_dependents(
                        idx,
                        uri,
                        &deleting_uris,
                    ));
                    let dependents: Vec<String> = dependents.into_iter().collect();

                    if !dependents.is_empty() {
                        let examples: Vec<String> =
                            dependents.iter().take(3).map(|uri| short_uri(uri)).collect();
                        tracing::warn!(
                            uri,
                            dependent_file_count = dependents.len(),
                            "Safe delete detected dependent workspace files"
                        );
                        unsafe_deletes.push((short_uri(uri), dependents.len(), examples));
                    }
                }

                if !unsafe_deletes.is_empty() {
                    let msg = if unsafe_deletes.len() == 1 {
                        let (uri, dependent_count, examples) = &unsafe_deletes[0];
                        let example_suffix = if examples.is_empty() {
                            String::new()
                        } else {
                            format!(" Example dependents: {}.", examples.join(", "))
                        };
                        format!(
                            "Safe delete warning: '{}' has {} dependent workspace file(s). \
                                 Delete may break callers.{}",
                            uri, dependent_count, example_suffix
                        )
                    } else {
                        format!(
                            "Safe delete warning: {} files have dependent workspace files. \
                                 Delete may break callers.",
                            unsafe_deletes.len()
                        )
                    };
                    if let Err(e) =
                        self.show_message(crate::runtime::window::MessageType::Warning, &msg)
                    {
                        tracing::debug!("Failed to send safe-delete warning: {}", e);
                    }
                }
            }
        }

        // Return empty edit - no cleanup edits needed for now
        Ok(Some(json!({"changes": {}})))
    }

    /// Handle workspace/willCreateFiles request
    pub(super) fn handle_will_create_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params
            && let Some(files) = params["files"].as_array()
        {
            for file in files {
                let Some(uri) = file["uri"].as_str() else {
                    continue;
                };

                tracing::debug!("File will be created: {}", uri);
            }
        }

        // Return empty edit - no setup edits needed for now
        Ok(Some(json!({"changes": {}})))
    }

    /// Handle workspace/didCreateFiles notification
    pub(super) fn handle_did_create_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params
            && let Some(files) = params["files"].as_array()
        {
            for file in files {
                let Some(uri) = file["uri"].as_str() else {
                    continue;
                };

                tracing::debug!("File created: {}", uri);

                // Index the new file if it's a Perl file
                // Note: Mutation operation - use coordinator with lifecycle tracking
                #[cfg(feature = "workspace")]
                if let Some(coordinator) = self.coordinator()
                    && is_perl_source_uri(uri)
                    && let Some(path) = uri_to_fs_path(uri)
                {
                    match read_text_file_with_encoding(&path) {
                        Ok(content) => {
                            coordinator.notify_change(uri);
                            if let Ok(url) = url::Url::parse(uri) {
                                match coordinator.index().index_file(url, content) {
                                    Ok(()) => {
                                        tracing::debug!("Indexed new file: {}", uri)
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to index new file {}: {}", uri, e)
                                    }
                                }
                            }
                            coordinator.notify_parse_complete(uri);
                        }
                        Err(e) => {
                            tracing::debug!(
                                "Failed to read new file for indexing ({}): {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
            }

            // Trigger client refresh after file creations
            if let Err(e) = self.refresh_controller.refresh_all(self) {
                tracing::warn!("Failed to refresh client after file creations: {}", e);
            }
        }

        // This is a notification, no response needed
        Ok(None)
    }

    /// Handle workspace/didRenameFiles notification
    pub(super) fn handle_did_rename_files(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params
            && let Some(files) = params["files"].as_array()
        {
            for file in files {
                let Some(old_uri) = file["oldUri"].as_str() else {
                    continue;
                };
                let Some(new_uri) = file["newUri"].as_str() else {
                    continue;
                };

                // Normalize URIs so the index and pinned_doc_map_for
                // lookups (which use normalize_uri_key / uri_key) match
                // regardless of percent-encoding or case differences in
                // the client-supplied URIs (#3665).
                let old_uri = self.normalize_uri_key(old_uri);
                let new_uri = self.normalize_uri_key(new_uri);

                tracing::debug!("File renamed: {} -> {}", old_uri, new_uri);

                // Update the index for the renamed file
                // Note: Mutation operation - use coordinator with lifecycle tracking
                #[cfg(feature = "workspace")]
                if let Some(coordinator) = self.coordinator() {
                    coordinator.notify_change(&old_uri);
                    coordinator.notify_change(&new_uri);

                    // Remove old file from index
                    coordinator.index().remove_file(&old_uri);

                    // Index new file if it's a Perl file
                    if is_perl_source_uri(&new_uri)
                        && let Some(path) = uri_to_fs_path(&new_uri)
                    {
                        match read_text_file_with_encoding(&path) {
                            Ok(content) => {
                                if let Ok(url) = url::Url::parse(&new_uri) {
                                    match coordinator.index().index_file(url, content) {
                                        Ok(()) => {
                                            tracing::debug!("Indexed renamed file: {}", new_uri)
                                        }
                                        Err(e) => tracing::warn!(
                                            "Failed to index renamed file {}: {}",
                                            new_uri,
                                            e
                                        ),
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Failed to read renamed file for indexing ({}): {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }

                    coordinator.notify_parse_complete(&old_uri);
                    coordinator.notify_parse_complete(&new_uri);
                }

                // Update document store
                {
                    let mut documents = self.documents.lock();
                    if let Some(doc) = documents.remove(&old_uri) {
                        documents.insert(new_uri.clone(), doc);
                    }
                }
            }

            // Trigger client refresh after file renames
            if let Err(e) = self.refresh_controller.refresh_all(self) {
                tracing::warn!(error = %e, "Failed to refresh client after file renames");
            }
        }

        // This is a notification, no response needed
        Ok(None)
    }

    /// Handle workspace/didChangeWorkspaceFolders notification
    pub(super) fn handle_did_change_workspace_folders(
        &self,
        params: Option<Value>,
    ) -> Result<(), JsonRpcError> {
        if let Some(params) = params
            && let Some(event) = params.get("event")
        {
            let change = extract_workspace_folder_change(event);
            if change.added.is_empty() && change.removed.is_empty() {
                tracing::debug!("Ignoring empty workspace folder change notification");
                return Ok(());
            }

            #[cfg(feature = "workspace")]
            let _indexing_transition = self.indexing_transition_lock.lock();

            if !change.added.is_empty() {
                let mut workspace_folders = self.workspace_folders.lock();
                for uri in &change.added {
                    tracing::debug!(uri, "Added workspace folder");
                    let mut folder_state =
                        super::workspace_folder::WorkspaceFolderState::new(uri.clone());

                    // Resolve the folder path
                    if let Some(path) = super::source_path_from_uri(uri) {
                        folder_state = folder_state.with_path(path);
                    }

                    workspace_folders.push(folder_state);
                }
            }

            if !change.removed.is_empty() {
                let mut workspace_folders = self.workspace_folders.lock();
                let removed_uris: std::collections::HashSet<String> =
                    change.removed.iter().cloned().collect();

                for uri in &change.removed {
                    tracing::debug!(uri, "Removed workspace folder");
                    self.evict_workspace_folder_state(uri);
                }

                // Retain only folders that are not in the removed list
                workspace_folders.retain(|f| !removed_uris.contains(&f.uri));
            }

            // Workspace folder membership changed, so any in-flight reverse
            // request now has stale per-folder scoping. Drop pending entries
            // before issuing a fresh `workspace/configuration` pull.
            self.pending_workspace_configuration_requests.lock().clear();

            // Load config for all folders after changes
            self.load_and_apply_project_config();

            // Update workspace index with new folder list
            #[cfg(feature = "workspace")]
            {
                if let Some(coordinator) = self.coordinator() {
                    coordinator.index().set_workspace_folders(self.workspace_folder_uris());

                    // Removed folders were evicted above before folder
                    // membership was updated.
                }
            }

            #[cfg(feature = "workspace")]
            drop(_indexing_transition);

            // Trigger client refresh after workspace folder changes
            if let Err(e) = self.refresh_controller.refresh_all(self) {
                tracing::warn!(error = %e, "Failed to refresh client after workspace folder changes");
            }

            // Rebuild workspace index after folder changes
            #[cfg(feature = "workspace")]
            self.start_workspace_indexing();
        }

        Ok(())
    }

    /// Start a background workspace indexing scan
    ///
    /// Uses a compare-exchange guard on `indexing_in_progress` to ensure only
    /// one scan runs at a time.  If a scan is already running the call is
    /// coalesced into one follow-up scan.
    #[cfg(feature = "workspace")]
    pub(super) fn start_workspace_indexing(&self) {
        let Some(coordinator) = self.coordinator().map(Arc::clone) else {
            self.workspace_indexing_invocation_count.fetch_add(1, Ordering::SeqCst);
            return;
        };

        Self::start_workspace_indexing_with_resources(IndexingResources {
            coordinator,
            workspace_folders: Arc::clone(&self.workspace_folders),
            indexing_in_progress: Arc::clone(&self.indexing_in_progress),
            indexing_rescan_pending: Arc::clone(&self.indexing_rescan_pending),
            indexing_transition_lock: Arc::clone(&self.indexing_transition_lock),
            invocation_count: Arc::clone(&self.workspace_indexing_invocation_count),
            outbound: self.outbound.clone(),
            work_done_progress: self.client_capabilities.lock().work_done_progress_support,
            progress_tokens: Arc::clone(&self.progress_tokens),
            progress_token_to_request: Arc::clone(&self.progress_token_to_request),
            next_request_id: Arc::clone(&self.next_request_id),
            permission_denied_shown: Arc::clone(&self.permission_denied_shown),
            readiness_receipt: Arc::clone(&self.workspace_readiness_receipt),
            #[cfg(any(test, feature = "expose_lsp_test_api"))]
            readiness_start_gate: Arc::clone(&self.workspace_indexing_start_gate),
            #[cfg(any(test, feature = "expose_lsp_test_api"))]
            readiness_observer_id: self
                .readiness_receipt_observer_id
                .load(std::sync::atomic::Ordering::Relaxed),
        });
    }

    #[cfg(feature = "workspace")]
    fn start_workspace_indexing_with_resources(resources: IndexingResources) {
        resources.invocation_count.fetch_add(1, Ordering::SeqCst);

        if !claim_indexing_slot(
            &resources.indexing_in_progress,
            &resources.indexing_rescan_pending,
            &resources.indexing_transition_lock,
        ) {
            tracing::debug!("Workspace indexing already in progress, queued a follow-up scan");
            return;
        }

        let restart_resources = resources.clone();
        let indexing_guard = IndexingGuard {
            indexing_in_progress: Arc::clone(&resources.indexing_in_progress),
            indexing_rescan_pending: Arc::clone(&resources.indexing_rescan_pending),
            indexing_transition_lock: Arc::clone(&resources.indexing_transition_lock),
            restart: Some(Box::new(move || {
                Self::start_workspace_indexing_with_resources(restart_resources);
            })),
        };

        let current_workspace_folders = Arc::clone(&resources.workspace_folders);
        let indexing_transition_lock = Arc::clone(&resources.indexing_transition_lock);
        let coordinator = resources.coordinator;

        // Ensure workspace folders are set in the index before indexing starts
        let workspace_folders = {
            let _transition = indexing_transition_lock.lock();
            let workspace_folders = resources.workspace_folders.lock().clone();
            let workspace_folder_uris =
                workspace_folders.iter().map(|folder| folder.uri.clone()).collect();
            coordinator.index().set_workspace_folders(workspace_folder_uris);
            workspace_folders
        };

        if workspace_folders.is_empty() {
            return;
        }

        let limits = coordinator.limits().clone();
        let caps = coordinator.performance_caps().clone();
        // Generate a request ID for the workDoneProgress/create call. Atomically
        // increment so it doesn't collide with IDs from other server-to-client requests.
        let progress_create_id = next_indexing_progress_request_id(&resources.next_request_id);
        let outbound = resources.outbound;
        let work_done_progress = resources.work_done_progress;
        // Keep the cancellation registry identity in a string namespace. The
        // progress-create request ID is server-generated, while the registry
        // also contains client request IDs; sharing numeric IDs would allow a
        // progress registration to overwrite an unrelated client request.
        let progress_request_id = indexing_cancellation_request_id(progress_create_id);
        let progress_tokens = resources.progress_tokens;
        let progress_token_to_request = resources.progress_token_to_request;
        if work_done_progress {
            let cancellation_token = PerlLspCancellationToken::new(
                progress_request_id.clone(),
                "workspace-indexing".to_string(),
            );
            if let Err(error) = GLOBAL_CANCELLATION_REGISTRY.register_token(cancellation_token) {
                tracing::warn!(%error, "Failed to register workspace indexing cancellation token");
            } else {
                progress_tokens.lock().insert(WORKSPACE_INDEX_PROGRESS_TOKEN.to_string());
                progress_token_to_request.lock().insert(
                    WORKSPACE_INDEX_PROGRESS_TOKEN.to_string(),
                    progress_request_id.clone(),
                );
            }
        }
        let permission_denied_shown = resources.permission_denied_shown;
        let readiness_receipt = resources.readiness_receipt;
        #[cfg(any(test, feature = "expose_lsp_test_api"))]
        let readiness_start_gate = resources.readiness_start_gate;
        #[cfg(any(test, feature = "expose_lsp_test_api"))]
        let readiness_observer_id = resources.readiness_observer_id;

        std::thread::spawn(move || {
            let _guard = indexing_guard; // moved into closure, drops when closure exits
            let _cancellation_guard = work_done_progress.then(|| WorkspaceIndexCancellationGuard {
                progress_tokens,
                progress_token_to_request,
                request_id: progress_request_id.clone(),
            });
            let budget_start = Instant::now();
            {
                let mut receipt = readiness_receipt.lock();
                receipt.begin_workspace(budget_start);
                #[cfg(any(test, feature = "expose_lsp_test_api"))]
                receipt.set_test_observer_id(readiness_observer_id);
            }
            coordinator.transition_to_scanning();
            #[cfg(any(test, feature = "expose_lsp_test_api"))]
            crate::runtime::readiness::notify_workspace_indexing_started(&readiness_start_gate);

            // Send progress begin if client supports work done progress.
            if work_done_progress {
                send_progress_create(&outbound, progress_create_id);
                send_progress_begin(&outbound);
            }

            let mut files: Vec<std::path::PathBuf> = Vec::new();
            let mut early_exit: Option<(EarlyExitReason, u64, usize, usize)> = None;
            let mut indexing_receipt = WorkspaceIndexingReceipt::default();
            let discovery_started = Instant::now();

            'scan: for folder_state in workspace_folders {
                if GLOBAL_CANCELLATION_REGISTRY.is_cancelled(&progress_request_id) {
                    let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                    early_exit = Some((EarlyExitReason::Cancelled, elapsed_ms, 0, files.len()));
                    break 'scan;
                }
                let Some(root) =
                    folder_state.path.clone().or_else(|| uri_to_fs_path(&folder_state.uri))
                else {
                    tracing::debug!(
                        uri = %folder_state.uri,
                        "Skipping non-filesystem workspace folder during indexing scan"
                    );
                    continue;
                };

                let workspace_config = &folder_state.effective_workspace_config;
                let discovery_config = super::file_discovery::DiscoveryConfig::new(
                    workspace_config.discovery_extra_extensions.clone(),
                    workspace_config.discovery_extra_skipped_dirs.clone(),
                );
                let discovery = super::file_discovery::discover_perl_files_with_config_and_cancel(
                    &root,
                    &workspace_config.include_paths,
                    &discovery_config,
                    || {
                        work_done_progress
                            && GLOBAL_CANCELLATION_REGISTRY.is_cancelled(&progress_request_id)
                    },
                );

                if discovery.cancelled {
                    let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                    early_exit = Some((EarlyExitReason::Cancelled, elapsed_ms, 0, files.len()));
                    break 'scan;
                }

                for path in discovery.files {
                    if GLOBAL_CANCELLATION_REGISTRY.is_cancelled(&progress_request_id) {
                        let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                        early_exit = Some((EarlyExitReason::Cancelled, elapsed_ms, 0, files.len()));
                        break 'scan;
                    }
                    files.push(path);
                    let total_files = files.len();

                    if total_files.is_multiple_of(64) {
                        coordinator.update_scan_progress(total_files);
                    }

                    let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                    if total_files >= limits.max_files {
                        early_exit = Some((EarlyExitReason::FileLimit, elapsed_ms, 0, total_files));
                        break 'scan;
                    }

                    if elapsed_ms > caps.initial_scan_budget_ms {
                        early_exit =
                            Some((EarlyExitReason::InitialTimeBudget, elapsed_ms, 0, total_files));
                        break 'scan;
                    }
                }
            }

            coordinator.update_scan_progress(files.len());
            readiness_receipt.lock().record_peak_queued_work(files.len());
            indexing_receipt.record_phase(IndexingPhase::Discovery, discovery_started.elapsed());
            indexing_receipt.record_discovery(files.len(), budget_start.elapsed());
            coordinator.transition_to_indexing(files.len());

            let mut indexed_files = 0usize;
            let total_files = files.len();
            // Track the last file count at which a progress report was sent so we
            // can batch updates every 50 files (avoid flooding small workspaces).
            let mut last_reported = 0usize;

            for path in files {
                if GLOBAL_CANCELLATION_REGISTRY.is_cancelled(&progress_request_id) {
                    let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                    early_exit =
                        Some((EarlyExitReason::Cancelled, elapsed_ms, indexed_files, total_files));
                    break;
                }
                let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                if elapsed_ms > caps.initial_scan_budget_ms {
                    early_exit = Some((
                        EarlyExitReason::InitialTimeBudget,
                        elapsed_ms,
                        indexed_files,
                        total_files,
                    ));
                    break;
                }

                let read_started = Instant::now();
                let content = match read_text_file_with_encoding(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        indexing_receipt.record_phase(IndexingPhase::Read, read_started.elapsed());
                        indexing_receipt.record_read_error();
                        if is_permission_denied_error(&e) {
                            // ONE-TIME window/showMessage (AtomicBool guard)
                            if permission_denied_shown
                                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                            {
                                let msg = "Perl LSP: some workspace files could not be read \
                                           due to permission denied. Features for those files \
                                           will be unavailable. Check file permissions.";
                                if let Err(send_err) = outbound.send_notification(
                                    "window/showMessage",
                                    json!({ "type": 2, "message": msg }),
                                ) {
                                    tracing::warn!(
                                        error = %send_err,
                                        "Failed to send permission-denied showMessage"
                                    );
                                }
                            }
                            // Per-file diagnostic (always fires for each affected file)
                            if let Ok(url) = Url::from_file_path(&path) {
                                let uri_str = url.as_str();
                                if let Err(send_err) = outbound.send_notification(
                                    "textDocument/publishDiagnostics",
                                    json!({
                                        "uri": uri_str,
                                        "diagnostics": [{
                                            "range": {
                                                "start": { "line": 0, "character": 0 },
                                                "end":   { "line": 0, "character": 0 }
                                            },
                                            "severity": 1,
                                            "source": "perl-lsp",
                                            "message": format!(
                                                "File cannot be read: permission denied ({})",
                                                path.display()
                                            )
                                        }]
                                    }),
                                ) {
                                    tracing::warn!(
                                        error = %send_err,
                                        "Failed to send permission-denied diagnostic"
                                    );
                                }
                            }
                        } else {
                            tracing::debug!(
                                "Skipping unreadable file during indexing ({}): {}",
                                path.display(),
                                e
                            );
                        }
                        continue;
                    }
                };
                indexing_receipt.record_phase(IndexingPhase::Read, read_started.elapsed());
                let Ok(url) = Url::from_file_path(&path) else {
                    indexing_receipt.record_index_error();
                    continue;
                };
                let read_elapsed = read_started.elapsed();
                let index_started = Instant::now();
                let indexed_uri = url.to_string();
                let index_result = {
                    let _transition = indexing_transition_lock.lock();
                    let current_folders = current_workspace_folders.lock();
                    if path_is_in_current_workspace(&path, &current_folders) {
                        Some(coordinator.index().index_file(url, content))
                    } else {
                        tracing::debug!(
                            path = %path.display(),
                            "Skipping file from workspace folder removed during indexing"
                        );
                        None
                    }
                };
                let Some(index_result) = index_result else {
                    continue;
                };
                let index_elapsed = index_started.elapsed();
                indexing_receipt.record_phase(IndexingPhase::IndexFileOperation, index_elapsed);
                if index_result.is_ok() {
                    indexing_receipt.record_indexed_file(
                        &path,
                        read_elapsed,
                        index_started.elapsed(),
                    );
                    readiness_receipt.lock().record_indexed_uri(&indexed_uri, Instant::now());
                    indexed_files += 1;
                    coordinator.update_building_progress(indexed_files);

                    // Send a progress report every 50 files.
                    if work_done_progress && indexed_files - last_reported >= 50 {
                        send_progress_report(&outbound, indexed_files, total_files);
                        last_reported = indexed_files;
                    }
                } else {
                    indexing_receipt.record_index_error();
                }
            }

            if let Some((reason, elapsed_ms, indexed_files, total_files)) = early_exit {
                indexing_receipt.log(budget_start.elapsed(), Some(reason));
                coordinator.record_early_exit(reason, elapsed_ms, indexed_files, total_files);
                match reason {
                    EarlyExitReason::Cancelled => {
                        coordinator.transition_to_degraded(DegradationReason::Cancelled);
                    }
                    EarlyExitReason::FileLimit => {
                        coordinator.transition_to_degraded(DegradationReason::ResourceLimit {
                            kind: ResourceKind::MaxFiles,
                        });
                    }
                    EarlyExitReason::InitialTimeBudget | EarlyExitReason::IncrementalTimeBudget => {
                        coordinator
                            .transition_to_degraded(DegradationReason::ScanTimeout { elapsed_ms });
                    }
                }
                if work_done_progress {
                    let message = if reason == EarlyExitReason::Cancelled {
                        "Indexing cancelled"
                    } else {
                        "Indexing stopped early"
                    };
                    send_progress_end(&outbound, message);
                }
                readiness_receipt.lock().log();
                send_index_ready_notification(&outbound, &coordinator.state());
            } else if work_done_progress
                && GLOBAL_CANCELLATION_REGISTRY.is_cancelled(&progress_request_id)
            {
                let elapsed_ms = budget_start.elapsed().as_millis() as u64;
                coordinator.transition_to_degraded(DegradationReason::Cancelled);
                coordinator.record_early_exit(
                    EarlyExitReason::Cancelled,
                    elapsed_ms,
                    indexed_files,
                    total_files,
                );
                send_progress_end(&outbound, "Indexing cancelled");
                readiness_receipt.lock().log();
                send_index_ready_notification(&outbound, &coordinator.state());
            } else {
                indexing_receipt.log(budget_start.elapsed(), None);
                let resource_limited = matches!(
                    coordinator.state(),
                    IndexState::Degraded { reason: DegradationReason::ResourceLimit { .. }, .. }
                );
                if resource_limited {
                    readiness_receipt.lock().log();
                    if work_done_progress {
                        send_progress_end(&outbound, "Indexing stopped at resource limit");
                    }
                    send_index_ready_notification(&outbound, &coordinator.state());
                } else {
                    let file_count = coordinator.index().file_count();
                    let symbol_count = coordinator.index().symbol_count();
                    coordinator.transition_to_ready(file_count, symbol_count);
                    let mut receipt = readiness_receipt.lock();
                    receipt
                        .record_milestone(ReadinessMilestone::WholeWorkspaceReady, Instant::now());
                    receipt.log();
                    if work_done_progress {
                        send_progress_end(&outbound, "Indexing complete");
                    }
                    send_index_ready_notification(&outbound, &coordinator.state());
                }
            }
        });
    }

    /// Handle workspace/applyEdit request
    pub(super) fn handle_apply_edit(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let Some(edit) = params.get("edit") else {
                return Ok(Some(
                    json!({"applied": false, "failureReason": "Missing 'edit' field"}),
                ));
            };

            tracing::debug!("Applying workspace edit");

            // Apply changes to each document
            if let Some(changes) = edit["changes"].as_object() {
                for (uri, edits) in changes {
                    if let Some(edits) = edits.as_array() {
                        let mut documents = self.documents.lock();
                        if let Some(doc) = self.get_document_mut(&mut documents, uri) {
                            // Apply edits in reverse order to maintain positions
                            let mut sorted_edits = edits.clone();
                            sorted_edits.sort_by(|a, b| {
                                let a_line = a["range"]["start"]["line"].as_u64().unwrap_or(0);
                                let b_line = b["range"]["start"]["line"].as_u64().unwrap_or(0);
                                b_line.cmp(&a_line)
                            });

                            for edit in sorted_edits {
                                if let Some(new_text) = edit["newText"].as_str() {
                                    let start_line =
                                        edit["range"]["start"]["line"].as_u64().unwrap_or(0)
                                            as usize;
                                    let start_char =
                                        edit["range"]["start"]["character"].as_u64().unwrap_or(0)
                                            as usize;
                                    let end_line =
                                        edit["range"]["end"]["line"].as_u64().unwrap_or(0) as usize;
                                    let end_char =
                                        edit["range"]["end"]["character"].as_u64().unwrap_or(0)
                                            as usize;

                                    // Apply the edit to the document content
                                    let lines: Vec<String> =
                                        doc.text.lines().map(String::from).collect();
                                    let mut new_lines = Vec::new();

                                    // Copy lines before the edit
                                    for i in 0..start_line {
                                        new_lines.push(lines[i].clone());
                                    }

                                    // Apply the edit
                                    if start_line == end_line {
                                        let line = &lines[start_line];
                                        let new_line = format!(
                                            "{}{}{}",
                                            &line[..start_char.min(line.len())],
                                            new_text,
                                            &line[end_char.min(line.len())..]
                                        );
                                        new_lines.push(new_line);
                                    } else {
                                        // Multi-line edit
                                        let first_line = &lines[start_line];
                                        let last_line = &lines[end_line];
                                        let new_line = format!(
                                            "{}{}{}",
                                            &first_line[..start_char.min(first_line.len())],
                                            new_text,
                                            &last_line[end_char.min(last_line.len())..]
                                        );
                                        new_lines.push(new_line);
                                    }

                                    // Copy lines after the edit
                                    for i in (end_line + 1)..lines.len() {
                                        new_lines.push(lines[i].clone());
                                    }

                                    doc.text = new_lines.join("\n");
                                    doc.version += 1;
                                }
                            }

                            // Re-index the file after changes
                            // Note: Mutation operation - use coordinator with lifecycle tracking
                            #[cfg(feature = "workspace")]
                            if let Some(coordinator) = self.coordinator() {
                                coordinator.notify_change(uri);
                                if let Ok(url) = url::Url::parse(uri)
                                    && let Err(e) = coordinator
                                        .index()
                                        .index_file(url, doc.text_arc.to_string())
                                {
                                    tracing::warn!("Failed to re-index file {}: {}", uri, e);
                                }
                                coordinator.notify_parse_complete(uri);
                            }

                            // Invalidate the cached parse (see the
                            // generation-bump comment above; `parsed` is
                            // private -- state::ParsedSnapshot).
                            doc.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
            }

            // Return success
            return Ok(Some(json!({"applied": true})));
        }

        Ok(Some(json!({"applied": false, "failureReason": "Invalid parameters"})))
    }
}

#[cfg(feature = "workspace")]
#[derive(Clone, Copy)]
enum WorkspaceSymbolsTraceKind {
    SourceBackedReadyIndex,
    PartialIndexFallback,
    OpenDocumentFallback {
        name_index_candidate_count: usize,
        canonical_matched_count: usize,
        canonical_names_missing_from_candidates: usize,
    },
}

#[cfg(feature = "workspace")]
impl LspServer {
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    fn workspace_symbols_source_backed_count(&self, query: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let IndexAccessMode::Full(coordinator) = route_index_access(self.coordinator()) else {
            return 0;
        };
        coordinator.index().search_source_symbols(query, Some(workspace_symbol_cap())).len()
    }

    fn record_workspace_symbols_provider_decision_trace(
        &self,
        query: &str,
        result_count: usize,
        kind: WorkspaceSymbolsTraceKind,
        generated_pilot_count: usize,
    ) {
        let (
            decision,
            reason,
            fact_source,
            confidence,
            source_backed,
            source_backed_state,
            fallback_state,
            claim_boundary,
        ) = match kind {
            WorkspaceSymbolsTraceKind::SourceBackedReadyIndex
                if !query.is_empty() && generated_pilot_count > 0 =>
            {
                (
                    "acted",
                    "source_backed_generated_label_pilot",
                    "framework_adapter",
                    "medium",
                    true,
                    "ready_workspace_index_generated_label_pilot",
                    "none",
                    "returns ready-state source-backed workspace index symbols plus labeled generated/framework pilot symbols; generated symbols point to source framework declarations, not exact generated method bodies; dynamic, stale, and ambiguous compiler candidates remain gated",
                )
            }
            WorkspaceSymbolsTraceKind::SourceBackedReadyIndex if !query.is_empty() => (
                "acted",
                "source_backed_high_confidence",
                "compiler_fact",
                "high",
                true,
                "ready_workspace_index",
                "none",
                "returns ready-state source-backed workspace index symbols only; generated, dynamic, stale, and ambiguous compiler candidates remain gated",
            ),
            WorkspaceSymbolsTraceKind::SourceBackedReadyIndex => (
                "fallback",
                "empty_query",
                "legacy_workspace",
                "low",
                false,
                "empty_query_not_promoted",
                "legacy_provider",
                "empty workspace-symbol queries keep existing broad provider behavior; no compiler-symbol promotion",
            ),
            WorkspaceSymbolsTraceKind::PartialIndexFallback => (
                "fallback",
                "partial_index",
                "legacy_workspace",
                "medium",
                false,
                "partial_index_not_full_workspace",
                "legacy_provider",
                "partial-index workspace symbols remain fallback behavior until the index is fresh and ready",
            ),
            WorkspaceSymbolsTraceKind::OpenDocumentFallback { .. } => (
                "fallback",
                "open_document_fallback",
                "fallback",
                "low",
                false,
                "not_proven_by_workspace_index",
                "legacy_provider",
                "open-document workspace-symbol fallback does not promote compiler-backed workspace symbols",
            ),
        };

        let mut receipt = json!({
            "provider": "workspace_symbols",
            "provider_action": "workspace/symbol",
            "decision": decision,
            "reason": reason,
            "fact_source": fact_source,
            "confidence": confidence,
            "freshness": "fresh",
            "source_backed": source_backed,
            "source_backed_state": source_backed_state,
            "dynamic_boundary": false,
            "fallback_state": fallback_state,
            "live_provider_result_kind": "array",
            "live_provider_result_count": result_count,
            "generated_pilot_count": generated_pilot_count,
            "query_empty": query.is_empty(),
            "live_cutover": if source_backed {
                if generated_pilot_count > 0 {
                    "partial_live_source_backed_generated_pilot"
                } else {
                    "partial_live_source_backed"
                }
            } else {
                "fallback_only"
            },
            "claim_boundary": claim_boundary,
        });
        if let WorkspaceSymbolsTraceKind::OpenDocumentFallback {
            name_index_candidate_count,
            canonical_matched_count,
            canonical_names_missing_from_candidates,
        } = kind
        {
            receipt["name_index_candidate_count"] = json!(name_index_candidate_count);
            receipt["canonical_matched_count"] = json!(canonical_matched_count);
            receipt["canonical_names_missing_from_candidates"] =
                json!(canonical_names_missing_from_candidates);
            receipt["candidate_acceleration"] = json!(WORKSPACE_SYMBOL_CANDIDATE_UNPROVEN);
        }

        self.record_provider_decision_trace("workspace_symbols", &receipt);
    }
}

#[cfg(feature = "workspace")]
fn collect_delete_target_module_names(
    index: &perl_parser::workspace_index::WorkspaceIndex,
    uri: &str,
) -> std::collections::BTreeSet<String> {
    let mut module_names = std::collections::BTreeSet::new();
    let path_module_name = path_to_module_name(uri);
    if !path_module_name.is_empty() {
        module_names.insert(path_module_name);
    }

    for symbol in index.file_symbols(uri) {
        if matches!(symbol.kind, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role)
            && let Some(module_name) = symbol
                .qualified_name
                .clone()
                .or_else(|| (!symbol.name.is_empty()).then_some(symbol.name.clone()))
        {
            module_names.insert(module_name);
        }
    }

    module_names
}

#[cfg(feature = "workspace")]
fn collect_cross_file_delete_dependents(
    index: &perl_parser::workspace_index::WorkspaceIndex,
    uri: &str,
    deleting_uris: &std::collections::HashSet<String>,
) -> std::collections::BTreeSet<String> {
    let normalized_uri = perl_parser::workspace_index::uri_key(uri);
    let module_names = collect_delete_target_module_names(index, uri);
    let mut dependents = std::collections::BTreeSet::new();
    for module_name in module_names {
        for dependent_uri in index.find_dependents(&module_name) {
            if dependent_uri != normalized_uri && !deleting_uris.contains(&dependent_uri) {
                dependents.insert(dependent_uri);
            }
        }
    }

    dependents
}

#[cfg(feature = "workspace")]
fn collect_open_document_delete_dependents(
    index: &perl_parser::workspace_index::WorkspaceIndex,
    uri: &str,
    deleting_uris: &std::collections::HashSet<String>,
    open_documents: &[(String, String)],
) -> std::collections::BTreeSet<String> {
    let normalized_uri = perl_parser::workspace_index::uri_key(uri);
    let module_names = collect_delete_target_module_names(index, uri);
    let mut dependents = std::collections::BTreeSet::new();

    for (doc_uri, text) in open_documents {
        let normalized_doc_uri = perl_parser::workspace_index::uri_key(doc_uri);
        if normalized_doc_uri == normalized_uri || deleting_uris.contains(&normalized_doc_uri) {
            continue;
        }

        if module_names.iter().any(|module_name| {
            !plan_module_rename_edits(text, module_name, "__PerlLspDeleteProbe__").is_empty()
        }) {
            dependents.insert(normalized_doc_uri);
        }
    }

    dependents
}

#[cfg(feature = "workspace")]
fn collect_symbol_reference_delete_dependents(
    index: &perl_parser::workspace_index::WorkspaceIndex,
    uri: &str,
    deleting_uris: &std::collections::HashSet<String>,
) -> std::collections::BTreeSet<String> {
    let normalized_uri = perl_parser::workspace_index::uri_key(uri);
    let mut dependents = std::collections::BTreeSet::new();

    for symbol in index.file_symbols(uri) {
        let mut names = std::collections::BTreeSet::new();
        if !symbol.name.is_empty() {
            names.insert(symbol.name.clone());
        }
        if let Some(qualified_name) = symbol.qualified_name
            && !qualified_name.is_empty()
        {
            names.insert(qualified_name);
        }

        for symbol_name in names {
            for reference in index.find_references(&symbol_name) {
                let reference_uri = perl_parser::workspace_index::uri_key(&reference.uri);
                if reference_uri != normalized_uri && !deleting_uris.contains(&reference_uri) {
                    dependents.insert(reference_uri);
                }
            }
        }
    }

    dependents
}

fn short_uri(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|parsed| {
            parsed.path_segments().and_then(|mut s| s.next_back().map(str::to_owned))
        })
        .filter(|tail| !tail.is_empty())
        .unwrap_or_else(|| uri.to_string())
}

/// Return `true` if `module_name` appears in `text` as a whole-identifier token.
///
/// This prevents false-positive rename warnings when a short module name (e.g. `"Base"`)
/// appears as a suffix of an unrelated longer identifier (e.g. `"Database"`).  The check
/// rejects any match that is immediately preceded or followed by a word character (`\w`)
/// or a colon (`:`), both of which extend a Perl identifier.
pub(super) fn module_name_appears_in_text(text: &str, module_name: &str) -> bool {
    if module_name.is_empty() {
        return false;
    }
    let name_len = module_name.len();
    let text_len = text.len();

    let mut start = 0usize;
    while start + name_len <= text_len {
        if let Some(pos) = text[start..].find(module_name) {
            let abs = start + pos;
            // Check character before the match
            let before_ok = abs == 0 || {
                let c = text[..abs].chars().next_back();
                c.is_none_or(|c| !is_perl_identifier_continue(c))
            };
            // Check character after the match
            let after_ok = abs + name_len >= text_len || {
                let c = text[abs + name_len..].chars().next();
                c.is_none_or(|c| !is_perl_identifier_continue(c))
            };
            if before_ok && after_ok {
                return true;
            }
            start = abs + 1;
        } else {
            break;
        }
    }
    false
}

fn is_perl_identifier_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

/// Convert a file path to a Perl module name
pub(super) fn path_to_module_name(uri: &str) -> String {
    #[cfg(feature = "workspace")]
    let path =
        uri_to_fs_path(uri).and_then(|p| p.to_str().map(|s| s.to_string())).unwrap_or_else(|| {
            // Fallback to trim_start_matches for backward compatibility
            uri.trim_start_matches("file://").to_string()
        });
    #[cfg(not(feature = "workspace"))]
    let path = uri.trim_start_matches("file://").to_string();

    file_path_to_module_name(&path)
}

#[cfg(test)]
mod tests {
    // Test assertions favor `expect_err()` with a descriptive message over
    // silent unwraps; the workspace-wide deny is a production-code rule.
    #![allow(clippy::expect_used)]
    #[cfg(feature = "workspace")]
    use super::WORKSPACE_INDEX_PROGRESS_TOKEN;
    use super::{LspServer, module_name_appears_in_text};
    #[cfg(feature = "workspace")]
    use crate::cancellation::{GLOBAL_CANCELLATION_REGISTRY, PerlLspCancellationToken};
    #[cfg(feature = "workspace")]
    use crate::protocol::JsonRpcId;
    #[cfg(feature = "workspace")]
    use crate::util::read_text_file_with_encoding;
    use parking_lot::Mutex;
    use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
    #[cfg(feature = "workspace")]
    use perl_parser::workspace_index::{
        DegradationReason, IndexCoordinator, IndexPerformanceCaps, IndexResourceLimits, IndexState,
    };
    use serde_json::{Value, json};
    use std::io::{self, Write};
    use std::sync::Arc;

    #[test]
    fn workspace_symbol_resolve_missing_params_name_method_and_field() {
        let err = LspServer::new()
            .handle_workspace_symbol_resolve(None)
            .expect_err("missing workspace symbol params must be rejected");

        assert_eq!(err.code, crate::protocol::INVALID_PARAMS);
        assert_eq!(err.message, "workspace/symbol/resolve: missing required parameter 'params'");
    }

    /// #8262 counterexample: the case-sensitive name-index prefix search returns
    /// only `foobar2` for query "foo", but a non-empty candidate set must never
    /// suppress the canonical case-insensitive matcher that also admits `FooBar`.
    #[cfg(feature = "workspace")]
    #[test]
    fn open_document_symbol_query_returns_case_insensitive_matches_despite_partial_candidates()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = LspServer::new();
        server.index_coordinator = None;
        server.test_apply_did_open(
            "file:///wssym8262/upper.pm",
            "package Upper;\nsub FooBar { 1 }\n1;\n",
            1,
        )?;
        server.test_apply_did_open(
            "file:///wssym8262/lower.pm",
            "package Lower;\nsub foobar2 { 1 }\n1;\n",
            1,
        )?;

        assert_eq!(
            server.symbol_index.lock().search_prefix("foo"),
            vec!["foobar2".to_string()],
            "precondition: the case-sensitive name index must be partial for this fixture"
        );

        let result = server
            .test_handle_workspace_symbols(Some(json!({"query": "foo"})))
            .map_err(|e| format!("workspace/symbol failed: {e:?}"))?;
        let symbols = result.ok_or("workspace/symbol returned no payload")?;
        let names: Vec<&str> = symbols
            .as_array()
            .ok_or("workspace/symbol must return an array")?
            .iter()
            .filter_map(|symbol| symbol.get("name").and_then(Value::as_str))
            .collect();

        assert_eq!(
            names,
            vec!["FooBar", "foobar2"],
            "canonical case-insensitive matching must surface both symbols in stable rank order"
        );
        Ok(())
    }

    /// #8262 matrix through the production open-document path: mixed case,
    /// camelCase/snake_case/acronyms, package-qualified names, one-char vs
    /// loose-query threshold, empty/whitespace queries, duplicate names across
    /// documents, and multiple occurrences of one name. Every row must match
    /// what the unrestricted canonical matcher produces.
    #[cfg(feature = "workspace")]
    #[test]
    fn open_document_symbol_query_matrix_is_completeness_neutral()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = LspServer::new();
        server.index_coordinator = None;
        server.test_apply_did_open(
            "file:///wssym8262/matrix_a.pm",
            "package MatrixA;\nsub FooBar { 1 }\nsub run { 2 }\nsub getLogger { 3 }\n1;\n",
            1,
        )?;
        server.test_apply_did_open(
            "file:///wssym8262/matrix_b.pm",
            "package MatrixB;\nsub foobar2 { 1 }\nsub run { 2 }\nsub parseHTML { 3 }\n1;\n",
            1,
        )?;
        server.test_apply_did_open(
            "file:///wssym8262/matrix_c.pm",
            "package MatrixC::Inner;\nsub run { 1 }\npackage MatrixC::Other;\nsub run { 2 }\n1;\n",
            1,
        )?;

        let names_for = |query: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
            let result = server
                .test_handle_workspace_symbols(Some(json!({ "query": query })))
                .map_err(|e| format!("workspace/symbol failed: {e:?}"))?;
            let payload = result.ok_or("workspace/symbol returned no payload")?;
            let array = payload.as_array().ok_or("workspace/symbol must return an array")?;
            Ok(array
                .iter()
                .filter_map(|symbol| symbol.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect())
        };

        assert_eq!(names_for("foo")?, vec!["FooBar".to_string(), "foobar2".to_string()]);
        assert_eq!(names_for("FooBar")?.first(), Some(&"FooBar".to_string()));
        assert!(names_for("gL")?.contains(&"getLogger".to_string()));
        assert!(names_for("html")?.contains(&"parseHTML".to_string()));
        assert_eq!(names_for("MatrixC")?.len(), 2, "package-qualified names match");

        assert_eq!(
            names_for("f")?,
            vec!["FooBar".to_string(), "foobar2".to_string()],
            "one-char queries stay on exact/prefix tier"
        );

        let all = names_for("")?;
        let whitespace = names_for("   ")?;
        assert_eq!(all.len(), 12, "empty query returns every open-document symbol");
        assert_eq!(whitespace.len(), all.len(), "whitespace queries trim to empty");

        assert_eq!(
            names_for("run")?.iter().filter(|name| name.as_str() == "run").count(),
            4,
            "duplicate names keep every occurrence across and within documents"
        );
        Ok(())
    }

    /// #8262: caps apply after the canonical full search, so truncation never
    /// depends on candidate acceleration.
    #[cfg(feature = "workspace")]
    #[test]
    fn open_document_symbol_cap_truncates_after_canonical_search()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server = LspServer::new();
        server.index_coordinator = None;
        let mut source = String::from("package CapDemo;\n");
        for i in 0..250 {
            source.push_str(&format!("sub cap_symbol_{i:03} {{ {i} }}\n"));
        }
        source.push_str("1;\n");
        server.test_apply_did_open("file:///wssym8262/cap.pm", &source, 1)?;

        let result = server
            .test_handle_workspace_symbols(Some(json!({ "query": "cap_symbol" })))
            .map_err(|e| format!("workspace/symbol failed: {e:?}"))?;
        let payload = result.ok_or("workspace/symbol returned no payload")?;
        let array = payload.as_array().ok_or("workspace/symbol must return an array")?;

        assert_eq!(array.len(), crate::state::workspace_symbol_cap().min(250));
        Ok(())
    }

    #[derive(Clone, Default)]
    struct OutputCapture {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl OutputCapture {
        fn messages(&self) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
            let bytes = self.buffer.lock().clone();
            let mut framer = ContentLengthFramer::new();
            framer.push(&bytes);
            let mut messages = Vec::new();
            while let Some(body) = framer.try_next()? {
                messages.push(serde_json::from_slice::<Value>(&body)?);
            }
            Ok(messages)
        }
    }

    impl Write for OutputCapture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.lock().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn server_with_output_capture() -> (LspServer, OutputCapture) {
        let output = OutputCapture::default();
        let server = LspServer::with_output(Arc::new(Mutex::new(
            Box::new(output.clone()) as Box<dyn Write + Send>
        )));
        (server, output)
    }

    #[test]
    fn test_module_name_appears_exact_match() {
        assert!(module_name_appears_in_text("use MyBase;", "MyBase"));
    }

    #[test]
    fn invalid_client_enum_setting_is_shown_once_and_keeps_current_value()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, output) = server_with_output_capture();

        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "critic": { "engine": "nativ" }
                }
            }
        })));
        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "critic": { "engine": "nativ" }
                }
            }
        })));
        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "critic": { "engine": " Nativ " },
                    "formatting": { "engine": "bad_mode" }
                }
            }
        })));
        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "formatting": { "engine": "bad-mode" }
                }
            }
        })));

        let current_engine = server.config.lock().critic_engine;
        drop(server);

        let messages = output.messages()?;
        let warnings: Vec<&Value> = messages
            .iter()
            .filter(|message| {
                message.get("method").and_then(Value::as_str) == Some("window/showMessage")
            })
            .collect();
        let warning = warnings.first().ok_or("expected an invalid-setting warning")?;
        assert_eq!(
            warnings.len(),
            2,
            "semantically repeated values must be deduplicated: {warnings:?}"
        );
        assert_eq!(warning.pointer("/params/type").and_then(Value::as_i64), Some(2));
        let text = warning
            .pointer("/params/message")
            .and_then(Value::as_str)
            .ok_or("expected warning message text")?;
        assert!(text.contains("critic.engine"), "critic warning must name its setting: {text}");
        assert!(text.contains("nativ"), "critic warning must preserve the supplied value: {text}");
        assert!(text.contains("native"), "critic warning must list the accepted value: {text}");
        let formatter_warning = warnings
            .iter()
            .find(|message| {
                message
                    .pointer("/params/message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.contains("formatting.engine"))
            })
            .ok_or("expected a formatter warning")?;
        let formatter_text = formatter_warning
            .pointer("/params/message")
            .and_then(Value::as_str)
            .ok_or("expected formatter warning message text")?;
        assert!(
            formatter_text.contains("bad_mode"),
            "formatter warning must preserve the supplied value: {formatter_text}"
        );
        assert!(
            formatter_text.contains("Valid values"),
            "formatter warning must list the accepted values: {formatter_text}"
        );
        assert_eq!(current_engine, perl_lsp_rs_core::config::CriticEngine::Native);
        Ok(())
    }

    #[test]
    fn did_change_configuration_ignores_removed_test_runner_authority() {
        let server = LspServer::new();

        server.test_handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "testRunner": {
                        "enabled": false,
                        "command": "CANARY-EXECUTABLE",
                        "args": ["CANARY-ARG"],
                        "cwd": "CANARY-CWD",
                        "env": {"CANARY": "CANARY-VALUE"},
                        "timeout": 0
                    },
                    "telemetry": {"enabled": true}
                }
            }
        })));

        assert!(server.config.lock().telemetry_enabled);
        let serialized = serde_json::to_value(&*server.config.lock()).expect("serialize config");
        assert!(serialized.get("testRunner").is_none());
        assert!(serialized.to_string().find("CANARY").is_none());
    }

    #[test]
    fn invalid_client_enum_warning_keeps_json_types_distinct()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, output) = server_with_output_capture();

        for engine in [json!("false"), json!(false)] {
            server.test_handle_did_change_configuration(Some(json!({
                "settings": { "perl": { "critic": { "engine": engine } } }
            })));
        }

        drop(server);
        let warnings: Vec<Value> = output
            .messages()?
            .into_iter()
            .filter(|message| {
                message.get("method").and_then(Value::as_str) == Some("window/showMessage")
                    && message
                        .pointer("/params/message")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.contains("critic.engine"))
            })
            .collect();

        assert_eq!(warnings.len(), 2, "string and boolean values need distinct warning keys");
        Ok(())
    }

    #[test]
    fn test_module_name_appears_as_suffix_rejected() {
        // "Base" must NOT match inside "Database" (false-positive guard)
        assert!(!module_name_appears_in_text("use Database;", "Base"));
    }

    #[test]
    fn test_module_name_appears_as_prefix_rejected() {
        // "Foo" must NOT match inside "FooBar"
        assert!(!module_name_appears_in_text("FooBar->method()", "Foo"));
    }

    #[test]
    fn test_module_name_appears_with_colon_boundary() {
        // "Bar" must NOT match when followed by "::" (it is a namespace prefix, not a standalone name)
        assert!(!module_name_appears_in_text("Foo::Bar::Baz", "Bar"));
    }

    #[test]
    fn test_module_name_appears_qualified_name() {
        // "Foo::Bar" should match as a whole module path
        assert!(module_name_appears_in_text("use Foo::Bar;", "Foo::Bar"));
    }

    #[test]
    fn test_module_name_appears_in_string_literal() {
        // Module name inside a single-quoted string counts as a reference
        assert!(module_name_appears_in_text("use parent 'MyBase';", "MyBase"));
    }

    #[test]
    fn test_module_name_empty_returns_false() {
        assert!(!module_name_appears_in_text("anything", ""));
    }

    #[test]
    fn test_module_name_unicode_letter_before_rejected() {
        // Unicode letters still extend identifiers; do not match inside "ÅBase".
        assert!(!module_name_appears_in_text("use ÅBase;", "Base"));
    }

    #[test]
    fn test_module_name_unicode_letter_after_rejected() {
        // Unicode letters still extend identifiers; do not match inside "BaseΔ".
        assert!(!module_name_appears_in_text("use BaseΔ;", "Base"));
    }

    #[test]
    fn did_change_workspace_folders_clears_pending_workspace_configuration_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let request_id =
            crate::runtime::types::ServerRequestId::new(7).ok_or("valid request id")?;
        server.pending_workspace_configuration_requests.lock().insert(
            request_id,
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec!["file:///tmp/folder-a".to_string()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        let result = server.handle_did_change_workspace_folders(Some(json!({
            "event": {
                "added": [
                    { "uri": "file:///tmp/folder-b", "name": "folder-b" }
                ],
                "removed": []
            }
        })));

        assert!(result.is_ok());
        assert!(server.pending_workspace_configuration_requests.lock().is_empty());
        Ok(())
    }

    #[test]
    fn did_change_workspace_folders_empty_event_is_noop() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = LspServer::new();
        let request_id =
            crate::runtime::types::ServerRequestId::new(8).ok_or("valid request id")?;
        server.pending_workspace_configuration_requests.lock().insert(
            request_id,
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec!["file:///tmp/folder-a".to_string()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );
        let before_invocations = server.workspace_indexing_invocation_count();

        let result = server.handle_did_change_workspace_folders(Some(json!({
            "event": {
                "added": [],
                "removed": []
            }
        })));

        assert!(result.is_ok());
        assert_eq!(server.pending_workspace_configuration_requests.lock().len(), 1);
        assert_eq!(server.workspace_indexing_invocation_count(), before_invocations);
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn watched_file_deleted_clears_raw_and_normalized_uri_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("delete_variant.pm");
        let source = "package Delete::Variant;\nsub gone { 1 }\n1;\n";
        std::fs::write(&path, source)?;
        let normalized_uri = url::Url::from_file_path(&path).map_err(|_| "invalid file path")?;
        let normalized_uri = normalized_uri.to_string();
        let file_path_part = normalized_uri.strip_prefix("file:///").ok_or("expected file URI")?;
        let raw_uri = format!("file://localhost/{file_path_part}");

        assert_ne!(raw_uri, normalized_uri);
        assert_eq!(server.normalize_uri_key(&raw_uri), server.normalize_uri_key(&normalized_uri));

        server.did_open(json!({
            "textDocument": {
                "uri": normalized_uri,
                "languageId": "perl",
                "version": 1,
                "text": source
            }
        }))?;
        let in_flight_token = server.new_parse_token(&normalized_uri);
        server.stream_sessions().start_session(crate::runtime::stream_session::SessionKey {
            uri: normalized_uri.clone(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        if let Some(coordinator) = server.coordinator() {
            coordinator
                .index()
                .index_file(url::Url::parse(&normalized_uri)?, source.to_string())?;
            assert!(!coordinator.index().file_symbols(&normalized_uri).is_empty());
            assert!(coordinator.index().document_store().get(&normalized_uri).is_some());
        }

        server.handle_did_change_watched_files(Some(json!({
            "changes": [
                { "uri": raw_uri, "type": 3 }
            ]
        })))?;

        assert!(in_flight_token.load(std::sync::atomic::Ordering::Relaxed));
        let after = server.memory_state_snapshot();
        assert_eq!(after.documents, 0);
        assert_eq!(after.open_text_bytes, 0);
        assert_eq!(after.parse_cancel_flags, 0);
        assert_eq!(after.stream_sessions, 0);
        if let Some(coordinator) = server.coordinator() {
            assert!(coordinator.index().file_symbols(&normalized_uri).is_empty());
            assert!(coordinator.index().file_symbols(&raw_uri).is_empty());
            assert!(coordinator.index().document_store().get(&normalized_uri).is_none());
            assert!(coordinator.index().document_store().get(&raw_uri).is_none());
        }

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn bulk_file_watcher_churn_drains_pressure_and_delete_state()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::runtime::file_watcher_debounce::FileWatcherDebouncer;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let server = LspServer::with_io(Box::new(std::io::empty()), Box::new(Vec::<u8>::new()));
        let delivered = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let delivered_for_worker = Arc::clone(&delivered);
        server.install_file_watcher_debouncer(FileWatcherDebouncer::with_interval(
            Duration::from_millis(150),
            move |uris| {
                delivered_for_worker.lock().extend(uris);
            },
        ));

        let dir = tempfile::tempdir()?;
        let mut uris = Vec::new();
        for i in 0..20 {
            let path = dir.path().join(format!("BulkWatcher{i}.pm"));
            let source = format!("package BulkWatcher{i};\nsub value {{ {i} }}\n1;\n");
            std::fs::write(&path, &source)?;
            let uri = url::Url::from_file_path(&path).map_err(|_| "invalid file path")?;
            let uri = uri.to_string();

            server.did_open(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": source
                }
            }))?;
            server.new_parse_token(&uri);
            server.stream_sessions().start_session(crate::runtime::stream_session::SessionKey {
                uri: uri.clone(),
                document_version: 1,
                line: i,
                character: 0,
            });
            if let Some(coordinator) = server.coordinator() {
                coordinator.index().index_file(url::Url::parse(&uri)?, source)?;
                assert!(
                    !coordinator.index().file_symbols(&uri).is_empty(),
                    "workspace index should contain {uri} before delete"
                );
            }
            uris.push(uri);
        }

        let changes = uris.iter().map(|uri| json!({ "uri": uri, "type": 2 })).collect::<Vec<_>>();
        server.handle_did_change_watched_files(Some(json!({ "changes": changes })))?;

        let mut max_pending = 0usize;
        let pressure_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < pressure_deadline {
            let pending = server.runtime_pressure_snapshot().file_watcher_pending_uris;
            max_pending = max_pending.max(pending);
            if pending == uris.len() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            max_pending > 1,
            "bulk watched-file changes should raise file watcher pressure, max pending {max_pending}"
        );

        let drain_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < drain_deadline {
            if server.runtime_pressure_snapshot().file_watcher_pending_uris == 0
                && delivered.lock().len() == uris.len()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(server.runtime_pressure_snapshot().file_watcher_pending_uris, 0);

        let delivered_uris = delivered.lock().clone();
        assert_eq!(delivered_uris.len(), uris.len());
        server.handle_watched_file_batch(delivered_uris);

        for uri in &uris {
            if let Some(path) = perl_uri::uri_to_fs_path(uri) {
                std::fs::remove_file(path)?;
            }
        }
        let deletes = uris.iter().map(|uri| json!({ "uri": uri, "type": 3 })).collect::<Vec<_>>();
        server.handle_did_change_watched_files(Some(json!({ "changes": deletes })))?;

        for _ in 0..100 {
            let pressure = server.runtime_pressure_snapshot();
            if pressure.pending_index_tasks == 0 && pressure.file_watcher_pending_uris == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let memory = server.memory_state_snapshot();
        assert_eq!(memory.documents, 0);
        assert_eq!(memory.open_text_bytes, 0);
        assert_eq!(memory.parse_cancel_flags, 0);
        assert_eq!(memory.stream_sessions, 0);
        assert_eq!(memory.pending_index_tasks, 0);
        assert_eq!(server.runtime_pressure_snapshot().file_watcher_pending_uris, 0);

        if let Some(coordinator) = server.coordinator() {
            for uri in &uris {
                assert!(coordinator.index().file_symbols(uri).is_empty());
                assert!(coordinator.index().document_store().get(uri).is_none());
            }
        }

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_folder_removal_evicts_open_docs_under_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let dir = tempfile::tempdir()?;
        let removed_root = dir.path().join("removed");
        let kept_root = dir.path().join("kept");
        std::fs::create_dir_all(&removed_root)?;
        std::fs::create_dir_all(&kept_root)?;

        let removed_path = removed_root.join("inside.pm");
        let kept_path = kept_root.join("outside.pm");
        let removed_source = "package Removed::Inside;\nsub inside { 1 }\n1;\n";
        let kept_source = "package Kept::Outside;\nsub outside { 1 }\n1;\n";
        std::fs::write(&removed_path, removed_source)?;
        std::fs::write(&kept_path, kept_source)?;

        let removed_folder_uri =
            url::Url::from_directory_path(&removed_root).map_err(|_| "invalid removed root")?;
        let kept_folder_uri =
            url::Url::from_directory_path(&kept_root).map_err(|_| "invalid kept root")?;
        let removed_uri = url::Url::from_file_path(&removed_path)
            .map_err(|_| "invalid removed path")?
            .to_string();
        let kept_uri =
            url::Url::from_file_path(&kept_path).map_err(|_| "invalid kept path")?.to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(
                removed_folder_uri.to_string(),
            ),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(
                kept_folder_uri.to_string(),
            ),
        );

        server.did_open(json!({
            "textDocument": {
                "uri": removed_uri,
                "languageId": "perl",
                "version": 1,
                "text": removed_source
            }
        }))?;
        server.did_open(json!({
            "textDocument": {
                "uri": kept_uri,
                "languageId": "perl",
                "version": 1,
                "text": kept_source
            }
        }))?;
        let removed_token = server.new_parse_token(&removed_uri);
        let kept_token = server.new_parse_token(&kept_uri);
        server.stream_sessions().start_session(crate::runtime::stream_session::SessionKey {
            uri: removed_uri.clone(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        server.stream_sessions().start_session(crate::runtime::stream_session::SessionKey {
            uri: kept_uri.clone(),
            document_version: 1,
            line: 0,
            character: 0,
        });

        server.handle_did_change_workspace_folders(Some(json!({
            "event": {
                "added": [],
                "removed": [
                    { "uri": removed_folder_uri.to_string(), "name": "removed" }
                ]
            }
        })))?;

        assert!(removed_token.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!kept_token.load(std::sync::atomic::Ordering::Relaxed));
        {
            let documents = server.documents.lock();
            assert!(!documents.contains_key(&server.normalize_uri_key(&removed_uri)));
            assert!(documents.contains_key(&server.normalize_uri_key(&kept_uri)));
        }
        assert!(!server.parse_cancel_flags.lock().contains_key(&removed_uri));
        assert!(server.parse_cancel_flags.lock().contains_key(&kept_uri));
        assert_eq!(server.stream_sessions().len(), 1);
        assert!(
            server
                .workspace_folders
                .lock()
                .iter()
                .all(|folder| { folder.uri != removed_folder_uri.to_string() })
        );

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_decodes_utf16le_bom() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("utf16le.pm");
        let text = "my $x = \"π\";";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::File::create(&path)?.write_all(&bytes)?;

        let read = read_text_file_with_encoding(&path)?;
        assert_eq!(read, text);
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn real_indexing_thread_emits_populated_readiness_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let source_path = dir.path().join("readiness.pm");
        std::fs::write(&source_path, "package Readiness;\nsub ready { 1 }\n1;\n")?;
        let folder_uri = url::Url::from_directory_path(dir.path())
            .map_err(|_| "invalid workspace folder path")?
            .to_string();

        let mut server = LspServer::new();
        server.index_coordinator =
            Some(std::sync::Arc::new(IndexCoordinator::with_limits_and_caps(
                IndexResourceLimits::default(),
                IndexPerformanceCaps { initial_scan_budget_ms: 30_000, ..Default::default() },
            )));
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(dir.path().to_path_buf()),
        );
        let (receipt_tx, receipt_rx) = std::sync::mpsc::channel();
        let _receipt_observer_guard =
            crate::runtime::readiness::set_workspace_readiness_receipt_observer(receipt_tx);
        server
            .readiness_receipt_observer_id
            .store(_receipt_observer_guard.id(), std::sync::atomic::Ordering::Relaxed);

        server.start_workspace_indexing();
        // The channel is the observable completion barrier; the timeout only
        // prevents a broken indexing thread from hanging the test forever.
        let receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;

        assert_eq!(receipt["workspace_start_us"], 0);
        let _whole_workspace_ready_us =
            receipt["whole_workspace_ready_us"].as_u64().ok_or("missing ready milestone")?;
        let peak_queued_work =
            receipt["peak_queued_work"].as_u64().ok_or("missing queued-work receipt")?;
        assert_eq!(peak_queued_work, 1);
        let coordinator = server.coordinator().ok_or("missing workspace coordinator")?;
        assert_eq!(coordinator.index().file_count(), 1);
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn cancelled_indexing_is_degraded_and_cleans_up_progress_registration()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        for index in 0..128 {
            std::fs::write(
                dir.path().join(format!("cancel-{index}.pm")),
                format!("package Cancel{index};\nsub symbol_{index} {{ 1 }}\n1;\n"),
            )?;
        }
        let folder_uri = url::Url::from_directory_path(dir.path())
            .map_err(|_| "invalid workspace folder path")?
            .to_string();

        let (mut server, output) = server_with_output_capture();
        server.client_capabilities.lock().work_done_progress_support = true;
        server.index_coordinator =
            Some(std::sync::Arc::new(IndexCoordinator::with_limits_and_caps(
                IndexResourceLimits::default(),
                IndexPerformanceCaps { initial_scan_budget_ms: 30_000, ..Default::default() },
            )));
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(dir.path().to_path_buf()),
        );

        let (receipt_tx, receipt_rx) = std::sync::mpsc::channel();
        let _receipt_observer_guard =
            crate::runtime::readiness::set_workspace_readiness_receipt_observer(receipt_tx);
        server
            .readiness_receipt_observer_id
            .store(_receipt_observer_guard.id(), std::sync::atomic::Ordering::Relaxed);
        let client_request_id = JsonRpcId::Integer(1);
        GLOBAL_CANCELLATION_REGISTRY.remove_request(&client_request_id);
        GLOBAL_CANCELLATION_REGISTRY.register_token(PerlLspCancellationToken::new(
            client_request_id.clone(),
            "client-request".to_string(),
        ))?;
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        server.test_gate_workspace_indexing_start(started_tx, release_rx);

        server.start_workspace_indexing();
        started_rx.recv_timeout(std::time::Duration::from_secs(5))?;
        server.handle_progress_cancel(Some(json!({
            "token": "workspace-index"
        })));
        release_tx.send(())?;

        let receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        if receipt["whole_workspace_ready_us"].is_number() {
            return Err("cancelled indexing reported whole-workspace readiness".into());
        }
        let coordinator = server.coordinator().ok_or("missing workspace coordinator")?;
        if !matches!(
            coordinator.state(),
            IndexState::Degraded { reason: DegradationReason::Cancelled, .. }
        ) {
            return Err("cancelled indexing did not leave the coordinator degraded".into());
        }
        if server.progress_tokens.lock().contains(WORKSPACE_INDEX_PROGRESS_TOKEN)
            || server.progress_token_to_request.lock().contains_key(WORKSPACE_INDEX_PROGRESS_TOKEN)
        {
            return Err("cancelled indexing left progress registration behind".into());
        }
        let client_request_preserved =
            GLOBAL_CANCELLATION_REGISTRY.get_token(&client_request_id).is_some();
        GLOBAL_CANCELLATION_REGISTRY.remove_request(&client_request_id);
        if !client_request_preserved {
            return Err("workspace indexing overwrote a client cancellation registration".into());
        }
        drop(server);
        let messages = output.messages()?;
        if !messages.iter().any(|message| {
            message.get("method").and_then(Value::as_str) == Some("$/progress")
                && message.pointer("/params/value/kind").and_then(Value::as_str) == Some("begin")
                && message.pointer("/params/value/cancellable").and_then(Value::as_bool)
                    == Some(true)
        }) {
            return Err("workspace indexing did not advertise cancellable progress".into());
        }
        if !messages.iter().any(|message| {
            message.get("method").and_then(Value::as_str) == Some("$/progress")
                && message.pointer("/params/value/kind").and_then(Value::as_str) == Some("end")
                && message.pointer("/params/value/message").and_then(Value::as_str)
                    == Some("Indexing cancelled")
        }) {
            return Err(
                "cancelled indexing did not end progress with a cancellation message".into()
            );
        }
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_folder_change_during_indexing_triggers_rescan()
    -> Result<(), Box<dyn std::error::Error>> {
        let old_dir = tempfile::tempdir()?;
        let new_dir = tempfile::tempdir()?;
        let old_path = old_dir.path().join("Old.pm");
        let new_path = new_dir.path().join("New.pm");
        std::fs::write(&old_path, "package OldFolder;\nsub old_symbol { 1 }\n1;\n")?;
        std::fs::write(&new_path, "package NewFolder;\nsub new_symbol { 1 }\n1;\n")?;
        let old_uri = url::Url::from_directory_path(old_dir.path())
            .map_err(|_| "invalid old workspace folder path")?
            .to_string();
        let new_uri = url::Url::from_directory_path(new_dir.path())
            .map_err(|_| "invalid new workspace folder path")?
            .to_string();

        let mut server = LspServer::new();
        server.index_coordinator =
            Some(std::sync::Arc::new(IndexCoordinator::with_limits_and_caps(
                IndexResourceLimits::default(),
                IndexPerformanceCaps { initial_scan_budget_ms: 30_000, ..Default::default() },
            )));
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(old_uri.clone())
                .with_path(old_dir.path().to_path_buf()),
        );

        let (receipt_tx, receipt_rx) = std::sync::mpsc::channel();
        let receipt_observer_guard =
            crate::runtime::readiness::set_workspace_readiness_receipt_observer(receipt_tx);
        server
            .readiness_receipt_observer_id
            .store(receipt_observer_guard.id(), std::sync::atomic::Ordering::Relaxed);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        server.test_gate_workspace_indexing_start(started_tx, release_rx);

        server.start_workspace_indexing();
        started_rx.recv_timeout(std::time::Duration::from_secs(5))?;

        server.handle_did_change_workspace_folders(Some(json!({
            "event": {
                "added": [{ "uri": new_uri, "name": "new" }],
                "removed": [{ "uri": old_uri, "name": "old" }]
            }
        })))?;
        release_tx.send(())?;

        let _first_receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        let _second_receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        let coordinator = server.coordinator().ok_or("missing workspace coordinator")?;
        if coordinator.index().find_definition("NewFolder::new_symbol").is_none() {
            return Err("pending folder change did not index the new workspace".into());
        }
        if coordinator.index().find_definition("OldFolder::old_symbol").is_some() {
            return Err("superseded scan reintroduced the removed workspace".into());
        }
        if server.workspace_indexing_invocation_count() < 3 {
            return Err("expected initial scan, pending request, and follow-up scan".into());
        }
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn indexing_slot_handoff_keeps_pending_request_visible_to_completion()
    -> Result<(), Box<dyn std::error::Error>> {
        let indexing_in_progress = std::sync::atomic::AtomicBool::new(true);
        let indexing_rescan_pending = std::sync::atomic::AtomicBool::new(false);
        let indexing_transition_lock = parking_lot::Mutex::new(());

        if super::claim_indexing_slot(
            &indexing_in_progress,
            &indexing_rescan_pending,
            &indexing_transition_lock,
        ) {
            return Err("an active scan unexpectedly accepted a second slot".into());
        }
        if !indexing_rescan_pending.load(std::sync::atomic::Ordering::Acquire) {
            return Err("the concurrent request did not remain pending".into());
        }
        if !super::release_indexing_slot(
            &indexing_in_progress,
            &indexing_rescan_pending,
            &indexing_transition_lock,
        ) {
            return Err("scan completion did not observe the pending request".into());
        }
        if indexing_rescan_pending.load(std::sync::atomic::Ordering::Acquire) {
            return Err("scan completion left the pending request set".into());
        }
        if !super::claim_indexing_slot(
            &indexing_in_progress,
            &indexing_rescan_pending,
            &indexing_transition_lock,
        ) {
            return Err("follow-up scan could not claim the released slot".into());
        }
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn real_indexing_thread_resets_readiness_between_runs() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let active_path = dir.path().join("main.pl");
        let dependency_path = dir.path().join("Dep.pm");
        let replacement_path = dir.path().join("Replacement.pm");
        let active_uri = url::Url::from_file_path(&active_path)
            .map_err(|_| "invalid active document path")?
            .to_string();
        let dependency_uri = url::Url::from_file_path(&dependency_path)
            .map_err(|_| "invalid dependency path")?
            .to_string();
        let replacement_uri = url::Url::from_file_path(&replacement_path)
            .map_err(|_| "invalid replacement path")?
            .to_string();
        let active_text = "package Main;\nuse Dep;\nmy $value = 1;\n$value;\n";
        std::fs::write(&active_path, active_text)?;
        std::fs::write(&dependency_path, "package Dep;\nsub value { 1 }\n1;\n")?;
        let folder_uri = url::Url::from_directory_path(dir.path())
            .map_err(|_| "invalid workspace folder path")?
            .to_string();

        let mut server = LspServer::new();
        server.index_coordinator =
            Some(std::sync::Arc::new(IndexCoordinator::with_limits_and_caps(
                IndexResourceLimits::default(),
                IndexPerformanceCaps { initial_scan_budget_ms: 30_000, ..Default::default() },
            )));
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(dir.path().to_path_buf()),
        );
        server.test_set_readiness_target(Some(&active_uri), &[&dependency_uri]);

        let (receipt_tx, receipt_rx) = std::sync::mpsc::channel();
        let _receipt_observer_guard =
            crate::runtime::readiness::set_workspace_readiness_receipt_observer(receipt_tx);
        server
            .readiness_receipt_observer_id
            .store(_receipt_observer_guard.id(), std::sync::atomic::Ordering::Relaxed);

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        server.test_gate_workspace_indexing_start(started_tx, release_rx);
        server.start_workspace_indexing();
        started_rx.recv_timeout(std::time::Duration::from_secs(5))?;
        server.test_apply_did_open(&active_uri, active_text, 1)?;

        let provider_result = server.test_handle_completion(Some(json!({
            "textDocument": {"uri": active_uri},
            "position": {"line": 3, "character": 1},
            "context": {"triggerKind": 1}
        })));
        let observation_result = server.test_record_readiness_provider_observation(
            "completion",
            &provider_result,
            "explicit_partial_or_fallback",
        );
        release_tx.send(())?;
        provider_result?;
        observation_result.map_err(std::io::Error::other)?;

        let first_receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        if first_receipt["first_correct_answers"]["completion"].is_null() {
            return Err("first indexing run did not record its provider observation".into());
        }
        if !first_receipt["active_document_ready_us"].is_u64()
            || !first_receipt["direct_dependency_set_ready_us"].is_u64()
        {
            return Err("first indexing run did not record target milestones".into());
        }

        let wait_for_indexing = || -> Result<(), Box<dyn std::error::Error>> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while server.indexing_in_progress.load(std::sync::atomic::Ordering::Acquire) {
                if std::time::Instant::now() >= deadline {
                    return Err("indexing thread did not finish before timeout".into());
                }
                std::thread::yield_now();
            }
            Ok(())
        };
        wait_for_indexing()?;

        std::fs::remove_file(&active_path)?;
        std::fs::remove_file(&dependency_path)?;
        std::fs::write(&replacement_path, "package Replacement;\n1;\n")?;

        server.start_workspace_indexing();
        let second_receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        wait_for_indexing()?;

        if !second_receipt["workspace_start_us"].is_u64()
            || !second_receipt["whole_workspace_ready_us"].is_u64()
        {
            return Err("second indexing run did not emit fresh workspace milestones".into());
        }
        if second_receipt["first_correct_answers"]["completion"].is_object()
            || second_receipt["active_document_ready_us"].is_number()
            || second_receipt["direct_dependency_set_ready_us"].is_number()
        {
            return Err("second indexing run retained stale readiness state".into());
        }

        let receipt = server.workspace_readiness_receipt.lock();
        let (configured_active_uri, configured_dependencies, indexed_uris) =
            receipt.test_target_state();
        if configured_active_uri.as_deref() != Some(active_uri.as_str())
            || !configured_dependencies.contains(&dependency_uri)
            || indexed_uris.contains(&active_uri)
            || indexed_uris.contains(&dependency_uri)
            || !indexed_uris.contains(&replacement_uri)
            || indexed_uris.len() != 1
        {
            return Err(
                "second indexing run did not reset indexed state or preserve targets".into()
            );
        }

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn readiness_probe_records_pre_index_answer_and_index_milestones()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let active_path = dir.path().join("main.pl");
        let dependency_path = dir.path().join("Dep.pm");
        let active_uri = url::Url::from_file_path(&active_path)
            .map_err(|_| "invalid active document path")?
            .to_string();
        let dependency_uri = url::Url::from_file_path(&dependency_path)
            .map_err(|_| "invalid dependency path")?
            .to_string();
        let active_text = "package Main;\nuse Dep;\nmy $value = 1;\n$value;\n";
        std::fs::write(&active_path, active_text)?;
        std::fs::write(&dependency_path, "package Dep;\nsub value { 1 }\n1;\n")?;
        let folder_uri = url::Url::from_directory_path(dir.path())
            .map_err(|_| "invalid workspace folder path")?
            .to_string();

        let mut server = LspServer::new();
        server.index_coordinator =
            Some(std::sync::Arc::new(IndexCoordinator::with_limits_and_caps(
                IndexResourceLimits::default(),
                IndexPerformanceCaps { initial_scan_budget_ms: 30_000, ..Default::default() },
            )));
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(dir.path().to_path_buf()),
        );
        server.test_set_readiness_target(Some(&active_uri), &[&dependency_uri]);

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        server.test_gate_workspace_indexing_start(started_tx, release_rx);
        let (receipt_tx, receipt_rx) = std::sync::mpsc::channel();
        let receipt_observer_guard =
            crate::runtime::readiness::set_workspace_readiness_receipt_observer(receipt_tx);
        server
            .readiness_receipt_observer_id
            .store(receipt_observer_guard.id(), std::sync::atomic::Ordering::Relaxed);

        server.start_workspace_indexing();
        started_rx.recv_timeout(std::time::Duration::from_secs(5))?;
        server.test_apply_did_open(&active_uri, active_text, 1)?;

        let provider_result = server.test_handle_completion(Some(json!({
            "textDocument": {"uri": active_uri},
            "position": {"line": 3, "character": 1},
            "context": {"triggerKind": 1}
        })));
        let observation_result = server.test_record_readiness_provider_observation(
            "completion",
            &provider_result,
            "explicit_partial_or_fallback",
        );
        let pre_index_receipt = server.test_readiness_receipt_snapshot();
        release_tx.send(())?;

        provider_result?;
        observation_result.map_err(std::io::Error::other)?;
        if pre_index_receipt["first_correct_answers"]["completion"].is_null() {
            return Err("pre-index completion observation was not recorded".into());
        }
        if pre_index_receipt["whole_workspace_ready_us"].is_number() {
            return Err("pre-index probe observed whole-workspace readiness too early".into());
        }

        let receipt = receipt_rx.recv_timeout(std::time::Duration::from_secs(30))?;
        for field in [
            "active_document_ready_us",
            "direct_dependency_set_ready_us",
            "whole_workspace_ready_us",
        ] {
            if !receipt[field].is_u64() {
                return Err(format!("readiness receipt missing {field}: {receipt}").into());
            }
        }
        let answer_elapsed = receipt["first_correct_answers"]["completion"]["elapsed_us"]
            .as_u64()
            .ok_or("missing completion answer elapsed time")?;
        let whole_ready = receipt["whole_workspace_ready_us"]
            .as_u64()
            .ok_or("missing whole-workspace elapsed time")?;
        if answer_elapsed > whole_ready {
            return Err(format!(
                "pre-index completion elapsed after whole-workspace readiness: {answer_elapsed} > {whole_ready}"
            )
            .into());
        }
        if receipt["first_correct_answers"]["completion"]["readiness_outcome"] != "partial" {
            return Err("completion readiness outcome was not preserved".into());
        }

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_strips_utf8_bom() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("utf8_bom.pm");
        std::fs::write(&path, [0xEF, 0xBB, 0xBF, b'p', b'a', b'c', b'k', b'a', b'g', b'e'])?;

        let read = read_text_file_with_encoding(&path)?;
        assert_eq!(read, "package");
        Ok(())
    }

    /// Regression: a UTF-16 LE BOM followed by an odd number of payload
    /// bytes must not panic or silently truncate. We fall back to lossy
    /// UTF-8 of the original bytes so the caller still gets something
    /// reasonable to index.
    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_handles_odd_length_utf16le()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("odd_utf16le.pm");
        // BOM (2 bytes) + 3 payload bytes = odd-length UTF-16 payload.
        std::fs::write(&path, [0xFF, 0xFE, 0x6D, 0x00, 0x79])?;

        let read = read_text_file_with_encoding(&path)?;
        // Must return something (not panic) — the replacement string is
        // lossy but deterministic.
        assert!(!read.is_empty());
        Ok(())
    }

    /// Regression: a UTF-16 BE BOM followed by an odd number of payload
    /// bytes must not panic or silently truncate.
    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_handles_odd_length_utf16be()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("odd_utf16be.pm");
        // BOM (2 bytes) + 3 payload bytes = odd-length UTF-16 payload.
        std::fs::write(&path, [0xFE, 0xFF, 0x00, 0x6D, 0x00])?;

        let read = read_text_file_with_encoding(&path)?;
        assert!(!read.is_empty());
        Ok(())
    }

    /// Edge case: empty file should decode to an empty string without panic.
    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_handles_empty_file() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("empty.pm");
        std::fs::write(&path, [])?;

        let read = read_text_file_with_encoding(&path)?;
        assert_eq!(read, "", "Empty file should decode to empty string");
        Ok(())
    }

    /// Edge case: file with only a UTF-8 BOM and no content should decode
    /// to an empty string (BOM is stripped, nothing remains).
    #[cfg(feature = "workspace")]
    #[test]
    fn read_text_file_with_encoding_handles_bom_only_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("bom_only.pm");
        std::fs::write(&path, [0xEF, 0xBB, 0xBF])?;

        let read = read_text_file_with_encoding(&path)?;
        assert_eq!(read, "", "BOM-only file should decode to empty string after BOM strip");
        Ok(())
    }

    #[test]
    fn did_change_configuration_preserves_initialization_options_base_layer()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("workspace");
        std::fs::create_dir_all(&folder)?;
        let uri =
            url::Url::from_directory_path(&folder).map_err(|_| "invalid folder path")?.to_string();

        let init_params = json!({
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
        server.handle_initialize(Some(init_params))?;

        // Change a different workspace setting; includePaths from init options should remain.
        server.handle_did_change_configuration(Some(json!({
            "settings": {
                "perl": {
                    "workspace": {
                        "resolutionTimeout": 123
                    }
                }
            }
        })));

        let folders = server.workspace_folders.lock();
        let folder_state = folders.first().ok_or("workspace folder should exist")?;
        assert_eq!(folder_state.effective_workspace_config.include_paths, vec!["lib", "local"]);
        assert_eq!(folder_state.effective_workspace_config.resolution_timeout_ms, 123);
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_symbol_skips_stale_workspace_index_tier() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = LspServer::default();
        let source_uri = "file:///workspace/stale_ws_symbol_source.pl";
        let source_v1 = "package StaleWs::Source;\nsub stale_symbol { return 1; }\n1;\n";
        let source_v2 = "package StaleWs::Source;\nsub fresh_only { return 2; }\n1;\n";

        server.test_apply_did_open(source_uri, source_v1, 1)?;
        server
            .test_index_file_in_building_state(source_uri, source_v1)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        let fresh = server.handle_workspace_symbols_v2(Some(json!({"query": "stale_symbol"})))?;
        let fresh_symbols = fresh.and_then(|value| value.as_array().cloned()).unwrap_or_default();
        assert!(
            fresh_symbols
                .iter()
                .any(|symbol| symbol.get("name").and_then(|name| name.as_str())
                    == Some("stale_symbol")),
            "fresh workspace index should return stale_symbol: {fresh_symbols:?}"
        );

        server
            .test_replace_document_without_index(source_uri, source_v2, 2)
            .map_err(std::io::Error::other)?;
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "test setup must leave the workspace index stale relative to open documents"
        );

        let stale = server.handle_workspace_symbols_v2(Some(json!({"query": "stale_symbol"})))?;
        let stale_symbols = stale.and_then(|value| value.as_array().cloned()).unwrap_or_default();
        assert!(
            !stale_symbols
                .iter()
                .any(|symbol| symbol.get("name").and_then(|name| name.as_str())
                    == Some("stale_symbol")),
            "stale workspace index must not return removed symbol: {stale_symbols:?}"
        );

        Ok(())
    }
}
