//! Diagnostic publishing and handling
//!
//! Handles both push and pull diagnostics for the LSP server.
//! - Push diagnostics: Server-initiated via `textDocument/publishDiagnostics`
//! - Pull diagnostics: Client-initiated via `textDocument/diagnostic` and `workspace/diagnostic`

#[cfg(test)]
use super::*;
use super::{
    Arc, BuiltInAnalyzer, DiagnosticsProvider, DocumentState, InternalDiagnosticSeverity,
    JsonRpcError, LspServer, Mutex, Ordering, Value, json, md5, source_path_from_uri,
};
use crate::features::diagnostics::{
    Diagnostic as InternalDiagnostic, DiagnosticTag as InternalDiagnosticTag,
    PullDiagnosticsContext,
};
use crate::runtime::window::RequestProgressGuard;
use perl_diagnostics::codes::DiagnosticCode;

/// Serialize a slice of typed values to a JSON array (#4995).
fn to_json_array<T: serde::Serialize>(values: &[T]) -> Value {
    serde_json::to_value(values).unwrap_or(Value::Array(Vec::new()))
}

/// Build a typed LSP Diagnostic JSON value (#4995).
///
/// Replaces repeated inline `json!({...})` constructions with a single
/// typed constructor so the Diagnostic shape is defined in one place.
fn diagnostic_json(
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    severity: u32,
    code: &str,
    source: &str,
    message: String,
) -> Value {
    json!({
        "range": {
            "start": {"line": start_line, "character": start_char},
            "end": {"line": end_line, "character": end_char},
        },
        "severity": severity,
        "code": code,
        "source": source,
        "message": message,
    })
}

/// Build the typed `data` field for a diagnostic with explanation metadata (#4995).
fn diagnostic_data(code: &str, category: &str, fixable: bool, tags: &[String]) -> Value {
    json!({
        "code": code,
        "category": category,
        "fixable": fixable,
        "tags": tags,
    })
}

/// Build a publishDiagnostics notification params value (#4995).
fn publish_diagnostics_params(uri: &str, version: Option<i32>, diagnostics: &[Value]) -> Value {
    let mut params = json!({
        "uri": uri,
        "diagnostics": diagnostics,
    });
    if let Some(v) = version {
        params["version"] = json!(v);
    }
    params
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_configured_profile_path(
    configured_profile: &str,
    workspace_root: Option<&std::path::Path>,
    file_path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let profile_path = std::path::Path::new(configured_profile);
    if profile_path.is_absolute() {
        return profile_path.exists().then(|| profile_path.to_path_buf());
    }

    let file_dir = file_path.parent();
    [
        Some(profile_path.to_path_buf()),
        workspace_root.map(|root| root.join(profile_path)),
        file_dir.map(|dir| dir.join(profile_path)),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| candidate.exists())
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_root_for_doc(server: &LspServer, uri: &str) -> Option<std::path::PathBuf> {
    server
        .folder_for_doc_uri(uri)
        .and_then(|folder| folder.path.or_else(|| source_path_from_uri(&folder.uri)))
        .or_else(|| server.root_path.lock().clone())
}

#[cfg(not(target_arch = "wasm32"))]
fn find_workspace_perlcritic_profile(
    workspace_root: Option<&std::path::Path>,
    file_path: &std::path::Path,
) -> Option<String> {
    let mut dir = file_path.parent().map(|p| p.to_path_buf());
    while let Some(current) = dir {
        for profile_name in [".perlcriticrc", "perlcriticrc"] {
            let candidate = current.join(profile_name);
            if candidate.exists() {
                return candidate.to_str().map(|s| s.to_string());
            }
        }

        if workspace_root == Some(current.as_path()) || current.parent().is_none() {
            break;
        }
        dir = current.parent().map(|p| p.to_path_buf());
    }
    None
}

fn critic_range_to_byte_range(
    content: &str,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
) -> Option<(usize, usize)> {
    let start = crate::util::position_to_offset(content, start_line, start_column)?;
    let end = crate::util::position_to_offset(content, end_line, end_column)?;
    (start <= end).then_some((start, end))
}

/// Orchestrator for pull diagnostics operations.
///
/// Coordinates between LspServer state and the pure-logic PullDiagnosticsProvider.
/// Handles:
/// - Building context from server state (config, workspace index, capabilities)
/// - Managing the cached CriticAnalyzer for external perlcritic integration
/// - Emitting workspace-scoped warnings (with deduplication)
/// - @INC path resolution for module diagnostics
pub struct PullDiagnosticsOrchestrator {
    /// Cached CriticAnalyzer for external perlcritic
    #[cfg(not(target_arch = "wasm32"))]
    critic_analyzer: Mutex<Option<perl_lsp_rs_core::tooling::perl_critic::CriticAnalyzer>>,
    /// Track warnings already emitted (deduplication)
    #[cfg(not(target_arch = "wasm32"))]
    warnings_sent: Mutex<std::collections::HashSet<String>>,
}

impl PullDiagnosticsOrchestrator {
    /// Create a new orchestrator.
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            critic_analyzer: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            warnings_sent: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Build context from LspServer state.
    pub fn build_context(&self, server: &LspServer, uri: &str) -> PullDiagnosticsContext {
        // Get config values
        let (
            perlcritic_enabled,
            perlcritic_severity,
            perlcritic_profile,
            critic_engine,
            native_critic_profile,
            native_critic_include,
            native_critic_exclude,
        ) = {
            let cfg = server.config.lock();
            (
                cfg.perlcritic_enabled,
                cfg.perlcritic_severity,
                cfg.perlcritic_profile.clone(),
                cfg.critic_engine,
                cfg.native_critic_profile.clone(),
                cfg.native_critic_include.clone(),
                cfg.native_critic_exclude.clone(),
            )
        };

        let profile =
            perlcritic_profile.and_then(|p| if p.trim().is_empty() { None } else { Some(p) });

        // Get workspace root for this document's containing folder (multi-root aware).
        // Falls back to the global root_path when no specific folder matches.
        //
        // Note: we inline the resolution here rather than calling `workspace_root_for_doc`
        // because `build_context` runs on all targets (including wasm32), while
        // `workspace_root_for_doc` is `#[cfg(not(target_arch = "wasm32"))]` since it is
        // only needed from the native perlcritic diagnostic paths.
        let workspace_root = server
            .folder_for_doc_uri(uri)
            .and_then(|folder| folder.path.or_else(|| source_path_from_uri(&folder.uri)))
            .or_else(|| server.root_path.lock().clone());

        // Get include paths for the document
        let include_paths: Vec<String> = server
            .include_paths_for_doc(uri)
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        // Get client capabilities
        let markup_message_support = server.client_capabilities.lock().markup_message_support;

        // Wait for index build, then sample per-document staleness before wiring
        // workspace semantic queries or dead-code analysis into pull diagnostics
        // (#5016 item 2).
        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        let workspace_index = {
            let _ = server.check_index_readiness(
                crate::runtime::readiness::IndexReadinessPolicy::WaitBriefly,
            );
            if server.workspace_index_stale_for_document(uri) {
                None
            } else {
                server.workspace_index()
            }
        };

        // Build context
        PullDiagnosticsContext {
            perlcritic_enabled,
            perlcritic_severity: perlcritic_severity.into(),
            perlcritic_profile: profile,
            critic_engine,
            native_critic_profile,
            native_critic_include,
            native_critic_exclude,
            workspace_root,
            include_paths,
            markup_message_support,
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            workspace_index,
        }
    }

    /// Collect external perlcritic diagnostics.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn collect_perlcritic_diagnostics(
        &self,
        server: &LspServer,
        uri: &str,
        doc_text: &str,
        diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
        use perl_lsp_rs_core::tooling::perl_critic::{CriticAnalyzer, CriticConfig};

        // Check config
        let (enabled, severity, profile, theme, critic_engine) = {
            let cfg = server.config.lock();
            (
                cfg.perlcritic_enabled,
                cfg.perlcritic_severity,
                cfg.perlcritic_profile.clone(),
                cfg.perlcritic_theme.clone(),
                cfg.critic_engine,
            )
        };

        if !enabled || critic_engine == perl_lsp_rs_core::config::CriticEngine::Native {
            return;
        }

        let profile = profile.and_then(|p| if p.trim().is_empty() { None } else { Some(p) });

        // Convert URI to file path
        let file_path = match url::Url::parse(uri) {
            Ok(u) => match u.to_file_path() {
                Ok(p) => p,
                Err(()) => {
                    tracing::warn!(uri, "perlcritic: URI is not a file path");
                    return;
                }
            },
            Err(e) => {
                tracing::warn!(uri, error = %e, "perlcritic: failed to parse URI");
                return;
            }
        };

        // Check if perlcritic is available (unless bypassed for tests)
        let skip_check = server.skip_perlcritic_command_check.load(Ordering::Relaxed);
        let force_unavailable =
            server.force_perlcritic_command_unavailable.load(std::sync::atomic::Ordering::Relaxed);
        if force_unavailable
            || (!skip_check && !crate::execute_command::command_exists("perlcritic"))
        {
            self.emit_warning(
                server,
                "missing-binary".to_string(),
                "Perl::Critic is enabled but `perlcritic` was not found on PATH. Install Perl::Critic (for example: `cpanm Perl::Critic`) or disable perl.perlcritic.enabled.",
            );
            return;
        }

        let workspace_root = workspace_root_for_doc(server, uri);

        // Validate configured profile if present.
        let resolved_configured_profile = if let Some(ref configured_profile) = profile {
            let resolved = resolve_configured_profile_path(
                configured_profile,
                workspace_root.as_deref(),
                &file_path,
            );
            if resolved.is_none() {
                self.emit_warning(
                    server,
                    format!("missing-profile:{configured_profile}"),
                    &format!(
                        "Perl::Critic profile path does not exist: {configured_profile}. Update perl.perlcritic.profile or create the profile file."
                    ),
                );
                return;
            }
            resolved
        } else {
            None
        };

        // Lazy-init the CriticAnalyzer
        {
            let mut guard = self.critic_analyzer.lock();
            if guard.is_none() {
                // Walk up directory tree looking for .perlcriticrc / perlcriticrc.
                let resolved_profile = resolved_configured_profile
                    .as_ref()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .or_else(|| {
                        find_workspace_perlcritic_profile(workspace_root.as_deref(), &file_path)
                    });

                let critic_config = CriticConfig {
                    severity,
                    profile: resolved_profile,
                    theme: theme.clone(),
                    ..Default::default()
                };

                // Use injected test runtime if present, otherwise OS runtime
                let analyzer = {
                    let rt_guard = server.critic_runtime_override.lock();
                    if let Some(ref rt) = *rt_guard {
                        CriticAnalyzer::new(critic_config, std::sync::Arc::clone(rt))
                    } else {
                        CriticAnalyzer::with_os_runtime(critic_config)
                    }
                };

                *guard = Some(analyzer);
            }
        }

        // Compute content hash for cache validation (detects stale entries from
        // external file changes that bypass the LSP didChange notification).
        let content_hash = perl_lsp_rs_core::tooling::perl_critic::hash_content(doc_text);

        // Run analysis
        let result = {
            let mut guard = self.critic_analyzer.lock();
            guard
                .as_mut()
                .map(|a| a.analyze_file_with_hash(&file_path, content_hash, Some(doc_text)))
        };

        match result {
            Some(Ok(violations)) => {
                for v in violations {
                    let internal_severity = critic_severity_to_internal(v.severity);

                    let Some((start_byte, end_byte)) = critic_range_to_byte_range(
                        doc_text,
                        v.range.start.line,
                        v.range.start.column,
                        v.range.end.line,
                        v.range.end.column,
                    ) else {
                        tracing::trace!(
                            uri,
                            policy = %v.policy,
                            start_line = v.range.start.line,
                            start_column = v.range.start.column,
                            end_line = v.range.end.line,
                            end_column = v.range.end.column,
                            "dropping malformed perlcritic diagnostic range"
                        );
                        continue;
                    };

                    diagnostics.push(InternalDiagnostic {
                        range: (start_byte, end_byte),
                        severity: internal_severity,
                        code: Some(v.policy),
                        message: v.description,
                        related_information: Vec::new(),
                        tags: Vec::new(),
                        suggestion: None,
                    });
                }
            }
            Some(Err(e)) => {
                self.emit_warning(
                    server,
                    format!("execution-failed:{e}"),
                    &format!("Perl::Critic execution failed: {e}"),
                );
                tracing::warn!(uri, error = %e, "perlcritic failed");
            }
            None => {}
        }
    }

    /// No-op stub for WASM targets.
    #[cfg(target_arch = "wasm32")]
    pub fn collect_perlcritic_diagnostics(
        &self,
        _server: &LspServer,
        _uri: &str,
        _doc_text: &str,
        _diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
    }

    /// Emit a workspace-scoped warning (with deduplication).
    #[cfg(not(target_arch = "wasm32"))]
    fn emit_warning(&self, server: &LspServer, key: String, message: &str) {
        let mut sent = self.warnings_sent.lock();
        if sent.insert(key) {
            server.show_message_or_log(super::window::MessageType::Warning, message);
        }
    }

    /// No-op stub for WASM targets.
    #[cfg(target_arch = "wasm32")]
    fn emit_warning(&self, _server: &LspServer, _key: String, _message: &str) {}

    /// Reset the orchestrator state (e.g., on configuration change).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reset(&self) {
        *self.critic_analyzer.lock() = None;
        self.warnings_sent.lock().clear();
    }

    /// No-op stub for WASM targets.
    #[cfg(target_arch = "wasm32")]
    pub fn reset(&self) {}

    /// Invalidate cached perlcritic violations for a single file path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn invalidate_file_cache(&self, file_path: &std::path::Path) {
        let path_str = file_path.to_string_lossy().to_string();
        if let Some(ref mut analyzer) = *self.critic_analyzer.lock() {
            analyzer.invalidate_cache(&path_str);
        }
    }

    /// No-op stub for WASM targets.
    #[cfg(target_arch = "wasm32")]
    pub fn invalidate_file_cache(&self, _file_path: &std::path::Path) {}
}

impl Default for PullDiagnosticsOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl LspServer {
    /// Convert internal diagnostic tags to LSP tag values
    ///
    /// Maps internal `DiagnosticTag` variants to their LSP numeric equivalents:
    /// - Unnecessary â†’ 1
    /// - Deprecated â†’ 2
    fn diagnostic_tags_to_lsp(tags: &[InternalDiagnosticTag]) -> Vec<i32> {
        tags.iter()
            .map(|t| match t {
                InternalDiagnosticTag::Unnecessary => 1,
                InternalDiagnosticTag::Deprecated => 2,
                // Forward-compatible fallback for future variants (#2898)
                _ => 1,
            })
            .collect()
    }

    /// Publish diagnostics for a document (push diagnostics)
    ///
    /// Computes and publishes diagnostics for a Perl document including syntax
    /// errors, semantic issues, and Perl::Critic-style code quality checks.
    /// Uses push-based notification model for backward compatibility with LSP 3.16 clients.
    ///
    /// # LSP Protocol
    ///
    /// Notification: `textDocument/publishDiagnostics`
    /// Capability: `textDocument.publishDiagnostics`
    ///
    /// # Arguments
    ///
    /// * `uri` - Document URI to compute diagnostics for
    ///
    /// # Diagnostics Sources
    ///
    /// - Parse errors from Perl parser
    /// - Unused variable warnings from scope analysis
    /// - Perl::Critic built-in policy violations
    /// - External perlcritic violations (opt-in via config)
    /// - Semantic errors from type inference
    ///
    /// # Performance
    ///
    /// Only publishes if client doesn't support pull diagnostics to avoid
    /// double-flow for modern LSP 3.17+ clients.
    pub(crate) fn publish_diagnostics(&self, uri: &str) {
        if self.client_supports_pull_diags.load(Ordering::Relaxed) {
            return;
        }

        // Syntax-only mode: report parse errors only and skip the full
        // semantic / critic / module-resolution / dead-code stack. Latency
        // harnesses use this so "first useful answer" measurements don't
        // include the background analysis cost on every keystroke.
        if self.runtime_tuning.diagnostic_mode
            == perl_lsp_rs_core::runtime::tuning::DiagnosticMode::SyntaxOnly
        {
            self.publish_syntax_only_diagnostics(uri);
            return;
        }

        let normalized_uri = self.normalize_uri_key(uri);

        // Snapshot all fields needed from DocumentState while holding the lock briefly,
        // then drop the lock before calling resolve_module_to_path which also acquires
        // documents.lock().  Holding the lock across that call causes a reentrant deadlock
        // because parking_lot::Mutex is not reentrant.
        let snapshot = {
            let documents = self.documents.lock();
            documents.get(&normalized_uri).or_else(|| documents.get(uri)).and_then(|doc| {
                // `current_parsed()` is `None` when the document's text
                // generation is ahead of the last published parse snapshot
                // (#3396 PR4 -- the pending-parse gap a future async parse
                // worker can open). Skip the push entirely in that case
                // rather than falling through to the `ast: None` branch
                // below: that would publish an empty (or parse-error-only)
                // diagnostics set computed from no current-generation AST at
                // all, silently overwriting whatever the client is currently
                // displaying with a false "nothing wrong" claim. Preserve the
                // client's last-known-good display instead -- the debounced
                // publish (or the next didChange's publish) fires again once
                // a fresh snapshot lands for this generation.
                let parsed = doc.current_parsed()?;
                Some((
                    parsed.ast().cloned(),
                    std::sync::Arc::clone(&doc.text_arc),
                    parsed.parse_errors_arc(),
                    doc.version,
                    parsed.degradation_tier(),
                    doc.line_starts.clone(),
                    Arc::clone(&doc.generation),
                    doc.generation.load(Ordering::SeqCst),
                ))
            })
            // lock is released here
        };

        let Some((
            ast_opt,
            text,
            parse_errors,
            version,
            degradation_tier,
            line_starts,
            generation,
            gen_at_snapshot,
        )) = snapshot
        else {
            return;
        };

        #[cfg(test)]
        if let Some(hook) = self.diagnostic_after_snapshot_hook.lock().as_ref() {
            hook();
        }

        // Position helper on the snapshotted line_starts + text (no rope clone).
        let pos16 = |offset: usize| line_starts.offset_to_position(&text, offset);

        let lsp_diagnostics: Vec<Value> = if let Some(ast) = &ast_opt {
            // Get diagnostics (already includes unused variable detection).
            // resolver is called with the documents lock *released* — no reentrant deadlock.
            //
            // The resolver is position-aware: it receives the byte offset of each `use`
            // statement so that `no lib` cancellations that precede the statement are
            // respected.  Passing `Some(use_site_offset)` to
            // `resolve_module_to_path_with_doc_at_offset` causes
            // `effective_inc_context_for_doc` to call
            // `resolve_use_lib_paths_from_source_at_offset` instead of the whole-file
            // scan, ensuring `no lib 'lib'` strips the path before `use GoneModule` is
            // checked.
            let provider = DiagnosticsProvider::new();
            let resolver = |module: &str, use_site_offset: usize| {
                self.resolve_module_to_path_with_doc_at_offset(
                    module,
                    Some(&text),
                    Some(uri),
                    Some(use_site_offset),
                )
                .is_some()
            };
            let search_context = self
                .effective_inc_context_for_doc(Some(uri), Some(&text), None)
                .map(|context| context.search_display_paths())
                .unwrap_or_default();
            let source_path = source_path_from_uri(uri);

            // Wait for index build, then sample staleness before touching the
            // workspace index tier (#5016 item 2).  Sample after readiness and
            // before any documents_guard re-entry (#6199 deadlock lesson).
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            let workspace_index_tier_enabled = {
                let _ = self.check_index_readiness(
                    crate::runtime::readiness::IndexReadinessPolicy::WaitBriefly,
                );
                !self.workspace_index_stale_for_any_open_document()
            };

            // Wire semantic queries when workspace data is available for this URI.
            // Falls back to NullSemanticQueries (legacy behavior) when the URI is
            // not yet indexed, the workspace feature is disabled, or the index is
            // stale relative to this open document (#5016 item 2).
            //
            // When the file consumes roles via `with 'Role'`, build a bounded
            // per-request PackageGraphIndex that includes ComposesRole edges for
            // those roles (the persistent index only carries Inherits edges from
            // HIR). This enables PL303 cross-file detection without a whole-workspace
            // parse; files with no `with` clauses skip the build as a fast path.
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            let mut diagnostics = {
                let semantic_diags = workspace_index_tier_enabled
                    .then(|| self.workspace_index())
                    .flatten()
                    .and_then(|workspace_index| {
                        use perl_lsp_rs_core::providers::diagnostics::role_graph_scope::{
                            build_role_scoped_package_graph, consumed_role_names,
                        };
                        let role_names = consumed_role_names(ast);
                        if role_names.is_empty() {
                            workspace_index.with_semantic_queries_for_uri(
                                uri,
                                |file_id, queries| {
                                    provider.get_diagnostics_with_search_context_and_semantics(
                                        ast,
                                        &parse_errors,
                                        &text,
                                        Some(&resolver),
                                        &search_context,
                                        source_path.as_deref(),
                                        file_id,
                                        &queries,
                                    )
                                },
                            )
                        } else {
                            let scoped_graph =
                                build_role_scoped_package_graph(&workspace_index, &role_names);
                            workspace_index.with_semantic_queries_for_uri_and_graph(
                                uri,
                                &scoped_graph,
                                |file_id, queries| {
                                    provider.get_diagnostics_with_search_context_and_semantics(
                                        ast,
                                        &parse_errors,
                                        &text,
                                        Some(&resolver),
                                        &search_context,
                                        source_path.as_deref(),
                                        file_id,
                                        &queries,
                                    )
                                },
                            )
                        }
                    });
                semantic_diags.unwrap_or_else(|| {
                    provider.get_diagnostics_with_search_context(
                        ast,
                        &parse_errors,
                        &text,
                        Some(&resolver),
                        &search_context,
                        source_path.as_deref(),
                    )
                })
            };
            #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
            let mut diagnostics = provider.get_diagnostics_with_search_context(
                ast,
                &parse_errors,
                &text,
                Some(&resolver),
                &search_context,
                source_path.as_deref(),
            );

            // Add configured policy critic diagnostics.
            self.collect_policy_critic_diagnostics(ast, &text, &mut diagnostics);

            // Add external perlcritic diagnostics (opt-in)
            self.collect_external_perlcritic_diagnostics(uri, &text, &mut diagnostics);

            // Add dead code diagnostics from workspace-wide symbol analysis.
            // Re-check freshness immediately before reading the index: readiness
            // work and semantic queries above may have crossed an index-generation
            // boundary since the first tier decision. Never let a stale snapshot
            // reach publish even if the earlier readiness sample was current.
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            if workspace_index_tier_enabled
                && !self.workspace_index_stale_for_any_open_document()
                && let Some(workspace_index) = self.workspace_index()
            {
                let dead_code_diags = perl_lsp_rs_core::providers::diagnostics::detect_dead_code(
                    &workspace_index,
                    uri,
                    &text,
                    &line_starts,
                );
                // The workspace snapshot can become stale while the
                // workspace-wide query runs. Recheck after the query and
                // discard its result unless the complete computation is fresh.
                if !self.workspace_index_stale_for_any_open_document() {
                    diagnostics.extend(dead_code_diags);
                }
            }

            // Deduplicate diagnostics appended after the provider's own dedup pass
            // (critic, dead-code).  The native critic's `recommended` profile
            // overlaps with built-in lints (RequireUseStrict↔PL100, etc.) — both
            // fire on the same range with the same severity but different codes.
            // Collapse them, preferring built-in PL* codes over native-critic codes.
            // (#5088)
            dedup_overlapping_diagnostics(&mut diagnostics);

            // Convert to LSP diagnostics
            diagnostics
                .into_iter()
                .map(|d| {
                    let (start_line, start_char) = pos16(d.range.0);
                    let (end_line, end_char) = pos16(d.range.1);

                    let severity = match d.severity {
                        InternalDiagnosticSeverity::Error => 1,
                        InternalDiagnosticSeverity::Warning => 2,
                        InternalDiagnosticSeverity::Information => 3,
                        InternalDiagnosticSeverity::Hint => 4,
                        // Forward-compatible fallback for future variants (#2898)
                        _ => 1,
                    };
                    let code_str = d.code.as_deref().unwrap_or("");
                    let mut diag = diagnostic_json(
                        start_line,
                        start_char,
                        end_line,
                        end_char,
                        severity,
                        code_str,
                        push_diagnostic_source(d.code.as_deref()),
                        d.message.clone(),
                    );
                    if !d.tags.is_empty() {
                        diag["tags"] = to_json_array(&Self::diagnostic_tags_to_lsp(&d.tags));
                    }

                    // Enrichment fields for push/pull parity (#1773):
                    // codeDescription, relatedInformation, and data.
                    if let Some(ref code_str) = d.code {
                        // codeDescription: link to documentation URL
                        if let Some(url) = DiagnosticCode::parse_code(code_str)
                            .and_then(|dc| dc.documentation_url())
                        {
                            diag["codeDescription"] = json!({ "href": url });
                        }
                    }

                    // relatedInformation: additional context locations
                    if !d.related_information.is_empty() {
                        diag["relatedInformation"] = json!(
                            d.related_information
                                .iter()
                                .map(|ri| {
                                    let (ri_sl, ri_sc) = pos16(ri.location.0);
                                    let (ri_el, ri_ec) = pos16(ri.location.1);
                                    json!({
                                        "location": {
                                            "uri": uri,
                                            "range": {
                                                "start": {"line": ri_sl, "character": ri_sc},
                                                "end":   {"line": ri_el, "character": ri_ec},
                                            }
                                        },
                                        "message": ri.message
                                    })
                                })
                                .collect::<Vec<_>>()
                        );
                    }

                    // data: structured metadata (category, fixability, tags)
                    if let Some(ref code_str) = d.code {
                        let category = DiagnosticCode::parse_code(code_str)
                            .map(|dc| format!("{:?}", dc.category()))
                            .unwrap_or_else(|| "Other".to_string());
                        let fixable = is_fixable_diagnostic(code_str);
                        let tag_strings: Vec<String> = d
                            .tags
                            .iter()
                            .map(|t| match t {
                                InternalDiagnosticTag::Unnecessary => "Unnecessary".to_string(),
                                InternalDiagnosticTag::Deprecated => "Deprecated".to_string(),
                                // Forward-compatible fallback for future variants (#2898)
                                _ => "Unnecessary".to_string(),
                            })
                            .collect();
                        diag["data"] = diagnostic_data(code_str, &category, fixable, &tag_strings);
                    }

                    diag
                })
                .collect()
        } else {
            // No AST available (parse failed completely), just report parse errors
            parse_errors
                .iter()
                .map(|e| {
                    // Extract location and base message from error enum
                    let (location, base_message) = match e {
                        crate::error::ParseError::UnexpectedToken { location, expected, found } => {
                            (*location, format!("Expected {}, found {}", expected, found))
                        }
                        crate::error::ParseError::SyntaxError { location, message } => {
                            (*location, message.clone())
                        }
                        crate::error::ParseError::Advisory { location, message } => {
                            (*location, message.clone())
                        }
                        crate::error::ParseError::UnexpectedEof => {
                            (text.len(), "Unexpected end of input".to_string())
                        }
                        crate::error::ParseError::LexerError { message } => (0, message.clone()),
                        _ => (0, e.to_string()),
                    };

                    // Append hint so users see actionable guidance in push fallback path too
                    let message =
                        match perl_lsp_rs_core::providers::diagnostics::build_parse_error_hint(
                            e,
                            &base_message,
                        ) {
                            Some(hint) => format!("{base_message}\nSuggestion: {hint}"),
                            None => base_message,
                        };

                    // Convert byte offset to line/column
                    let (line, character) = pos16(location);

                    diagnostic_json(
                        line,
                        character,
                        line,
                        character + 1,
                        if e.blocks_clean_parse() { 1 } else { 2 },
                        DiagnosticCode::ParseError.as_str(),
                        "perl-lsp",
                        message,
                    )
                })
                .collect()
        };

        // Generation-aware staleness guard: if a newer didChange arrived while
        // diagnostics were being computed, discard this result â€” the debouncer
        // will fire again for the latest version.
        if generation.load(Ordering::SeqCst) != gen_at_snapshot {
            tracing::debug!(
                uri = %normalized_uri,
                gen_at_snapshot,
                current_gen = generation.load(Ordering::SeqCst),
                "Skipping stale diagnostic publish (generation advanced during computation)"
            );
            return;
        }

        tracing::debug!(
            count = lsp_diagnostics.len(),
            uri = %normalized_uri,
            version,
            tier = %degradation_tier,
            "Publishing diagnostics"
        );

        // Send diagnostics notification with version.
        // This ensures diagnostics are cleared when all errors are fixed.
        if let Err(e) = self.notify(
            "textDocument/publishDiagnostics",
            publish_diagnostics_params(uri, Some(version), &lsp_diagnostics),
        ) {
            tracing::error!(uri, error = %e, "Failed to publish diagnostics");
        }
    }

    /// Build LSP diagnostics from a document's `parse_errors`, without the
    /// AST-based semantic / critic / dead-code / module-resolution passes.
    ///
    /// Returns an empty `Vec` when the document has no parse errors —
    /// publishing this still produces an empty `publishDiagnostics` payload,
    /// which is how LSP signals "the parse cleared". This is what makes the
    /// `syntax_only_clears_when_parse_errors_clear` acceptance case work.
    fn syntax_only_lsp_diagnostics(
        parse_errors: &[perl_parser::error::ParseError],
        text: &str,
        line_starts: &perl_parser::position::LineStartsCache,
        markup_message_support: bool,
    ) -> Vec<Value> {
        let pos16 = |offset: usize| line_starts.offset_to_position(text, offset);
        parse_errors
            .iter()
            .map(|e| {
                let (location, base_message) = match e {
                    crate::error::ParseError::UnexpectedToken { location, expected, found } => {
                        (*location, format!("Expected {}, found {}", expected, found))
                    }
                    crate::error::ParseError::SyntaxError { location, message } => {
                        (*location, message.clone())
                    }
                    crate::error::ParseError::Advisory { location, message } => {
                        (*location, message.clone())
                    }
                    crate::error::ParseError::UnexpectedEof => {
                        (text.len(), "Unexpected end of input".to_string())
                    }
                    crate::error::ParseError::LexerError { message } => (0, message.clone()),
                    crate::error::ParseError::RecursionLimit => (0, e.to_string()),
                    crate::error::ParseError::InvalidNumber { .. } => (0, e.to_string()),
                    crate::error::ParseError::InvalidString => (0, e.to_string()),
                    crate::error::ParseError::UnclosedDelimiter { .. } => (0, e.to_string()),
                    crate::error::ParseError::InvalidRegex { .. } => (0, e.to_string()),
                    crate::error::ParseError::NestingTooDeep { .. } => (0, e.to_string()),
                    crate::error::ParseError::Cancelled => (0, e.to_string()),
                    crate::error::ParseError::Recovered { location, .. } => {
                        (*location, e.to_string())
                    }
                };
                let message =
                    match perl_lsp_rs_core::providers::diagnostics::build_parse_error_hint(
                        e,
                        &base_message,
                    ) {
                        Some(hint) => format!("{base_message}\nSuggestion: {hint}"),
                        None => base_message,
                    };
                let (line, character) = pos16(location);
                let msg_val = Self::diagnostic_message_value(
                    &message,
                    None,
                    markup_message_support,
                );
                diagnostic_json(
                    line, character, line, character + 1,
                    if e.blocks_clean_parse() { 1 } else { 2 },
                    DiagnosticCode::ParseError.as_str(),
                    "perl-lsp",
                    msg_val.as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }

    /// Push-path publication restricted to parse errors. See
    /// [`Self::publish_diagnostics`] for the full pipeline.
    fn publish_syntax_only_diagnostics(&self, uri: &str) {
        let normalized_uri = self.normalize_uri_key(uri);

        let snapshot = {
            let documents = self.documents.lock();
            documents.get(&normalized_uri).or_else(|| documents.get(uri)).and_then(|doc| {
                // Pending-parse gap guard (#3396 PR4) -- mirrors `publish_diagnostics`.
                // Skip the push entirely rather than publishing an empty
                // parse-errors set computed from no current-generation
                // snapshot; that would overwrite whatever the client is
                // currently displaying with a false "no errors" claim.
                let parsed = doc.current_parsed()?;
                Some((
                    parsed.parse_errors_arc(),
                    std::sync::Arc::clone(&doc.text_arc),
                    doc.version,
                    doc.line_starts.clone(),
                    Arc::clone(&doc.generation),
                    doc.generation.load(Ordering::SeqCst),
                ))
            })
        };

        let Some((parse_errors, text, version, line_starts, generation, gen_at_snapshot)) =
            snapshot
        else {
            return;
        };

        let lsp_diagnostics =
            Self::syntax_only_lsp_diagnostics(&parse_errors, &text, &line_starts, false);

        // Generation-aware staleness guard mirrors the full path.
        if generation.load(Ordering::SeqCst) != gen_at_snapshot {
            tracing::debug!(
                uri = %normalized_uri,
                gen_at_snapshot,
                current_gen = generation.load(Ordering::SeqCst),
                "Skipping stale syntax-only diagnostic publish (generation advanced)"
            );
            return;
        }

        tracing::debug!(
            count = lsp_diagnostics.len(),
            uri = %normalized_uri,
            version,
            "Publishing syntax-only diagnostics"
        );

        if let Err(e) = self.notify(
            "textDocument/publishDiagnostics",
            publish_diagnostics_params(uri, Some(version), &lsp_diagnostics),
        ) {
            tracing::error!(uri, error = %e, "Failed to publish syntax-only diagnostics");
        }
    }

    /// Publish parse-error diagnostics immediately (fast path, ~10ms).
    ///
    /// Called on `didChange` before the full debounced diagnostic cycle so that
    /// syntax errors are visible to the user as soon as parsing completes, without
    /// waiting for the 250ms debounce that gates the slower scope-analysis and
    /// perlcritic passes.
    ///
    /// Only emits a notification when:
    /// - The client uses push diagnostics (no pull-diagnostic capability), AND
    /// - The document has at least one parse error to report.
    ///
    /// The slow path (`publish_diagnostics`) will follow and replace this
    /// notification with the full diagnostic set â€” LSP publishDiagnostics is
    /// replace-mode, so the client never sees a partial accumulation.
    pub(crate) fn publish_parse_errors_fast(&self, uri: &str) {
        // Fast path is only meaningful for push-diagnostic clients.
        // Pull-diagnostic clients request diagnostics on-demand.
        if self.client_supports_pull_diags.load(Ordering::Relaxed) {
            return;
        }

        let normalized_uri = self.normalize_uri_key(uri);
        let snapshot = {
            let documents = self.documents.lock();
            documents.get(&normalized_uri).or_else(|| documents.get(uri)).map(|doc| {
                // Pending-parse gap (#3396 PR4): `current_parsed()` is `None`
                // when the text generation is ahead of the last published
                // snapshot. `parse_errors` deliberately collapses to empty in
                // that case rather than falling back to a stale generation's
                // errors -- the empty-check right below then skips the fast
                // publish entirely, which is exactly the desired "don't
                // publish a claim for a generation we haven't parsed yet"
                // behavior (same policy as `publish_diagnostics`).
                let parse_errors = doc
                    .current_parsed()
                    .map_or_else(|| Arc::from([]) as Arc<[_]>, |p| p.parse_errors_arc());
                (
                    parse_errors,
                    doc.version,
                    doc.line_starts.clone(),
                    std::sync::Arc::clone(&doc.text_arc),
                )
            })
            // lock is released here
        };
        let Some((parse_errors, version, line_starts, text)) = snapshot else { return };

        // Nothing to fast-publish when there are no parse errors (this also
        // covers the pending-parse gap -- see comment above).
        if parse_errors.is_empty() {
            return;
        }

        let pos16 = |offset: usize| line_starts.offset_to_position(&text, offset);

        let lsp_diagnostics: Vec<Value> =
            parse_errors
                .iter()
                .map(|e| {
                    let (location, base_message) = match e {
                        crate::error::ParseError::UnexpectedToken { location, expected, found } => {
                            (*location, format!("Expected {}, found {}", expected, found))
                        }
                        crate::error::ParseError::SyntaxError { location, message } => {
                            (*location, message.clone())
                        }
                        crate::error::ParseError::Advisory { location, message } => {
                            (*location, message.clone())
                        }
                        crate::error::ParseError::UnexpectedEof => {
                            (text.len(), "Unexpected end of input".to_string())
                        }
                        crate::error::ParseError::LexerError { message } => (0, message.clone()),
                        _ => (0, e.to_string()),
                    };
                    let message =
                        match perl_lsp_rs_core::providers::diagnostics::build_parse_error_hint(
                            e,
                            &base_message,
                        ) {
                            Some(hint) => format!("{base_message}\nSuggestion: {hint}"),
                            None => base_message,
                        };
                    let (line, character) = pos16(location);
                    diagnostic_json(
                        line,
                        character,
                        line,
                        character + 1,
                        if e.blocks_clean_parse() { 1 } else { 2 },
                        DiagnosticCode::ParseError.as_str(),
                        "perl-lsp",
                        message,
                    )
                })
                .collect();

        tracing::debug!(
            count = lsp_diagnostics.len(),
            uri = %normalized_uri,
            version,
            "Publishing fast parse-error diagnostics"
        );

        if let Err(e) = self.notify(
            "textDocument/publishDiagnostics",
            json!({
                "uri": uri,
                "version": version,
                "diagnostics": lsp_diagnostics
            }),
        ) {
            tracing::error!(uri, error = %e, "Failed to publish fast parse-error diagnostics");
        }
    }

    /// Handle textDocument/diagnostic request (pull diagnostics - LSP 3.17)
    ///
    /// Computes diagnostics for a single document using the pull-based model
    /// introduced in LSP 3.17. Uses PullDiagnosticsProvider with context
    /// from the orchestrator for clean separation of concerns.
    ///
    /// # LSP Protocol
    ///
    /// Request: `textDocument/diagnostic`
    /// Response: `DocumentDiagnosticReport`
    /// Capability: `textDocument.diagnostic`
    ///
    /// # Arguments
    ///
    /// * `params` - JSON-RPC parameters containing document URI and optional previousResultId
    ///
    /// # Returns
    ///
    /// DocumentDiagnosticReport with kind "unchanged" or "full" depending on content changes
    ///
    /// # Caching Strategy
    ///
    /// Uses MD5 hash of document content as result ID for efficient change detection.
    /// Returns "unchanged" response when content hash matches previousResultId.
    pub(super) fn handle_document_diagnostic(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().diagnostic_provider {
            return Err(crate::protocol::method_not_advertised());
        }

        use crate::features::diagnostics::PullDiagnosticsProvider;
        use crate::protocol::invalid_params;
        use lsp_types::Uri;

        // LSP 3.17: missing or malformed params/URI is a client protocol error, not a
        // silent empty response.  Return InvalidParams so the client can distinguish
        // "no diagnostics for this file" from "I didn't understand your request".
        let params =
            params.ok_or_else(|| invalid_params("textDocument/diagnostic requires params"))?;
        let uri_str = params["textDocument"]["uri"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid_params("Missing required parameter: textDocument.uri"))?;
        let previous_result_id = params["previousResultId"].as_str().map(|s| s.to_string());

        // Parse URI — an unparseable URI is a client-side protocol error (LSP 3.17)
        let uri: Uri =
            uri_str.parse().map_err(|_| invalid_params("Invalid URI in textDocument.uri"))?;

        // Syntax-only short-circuit for pull diagnostics. Mirrors the
        // push-path gate in `publish_diagnostics`.
        if self.runtime_tuning.diagnostic_mode
            == perl_lsp_rs_core::runtime::tuning::DiagnosticMode::SyntaxOnly
        {
            // Capture the generation Arc alongside the document clone so we can
            // detect a concurrent didChange that arrives during syntax analysis.
            let doc_snapshot = {
                let documents = self.documents.lock();
                self.get_document(&documents, uri_str).map(|doc| {
                    (
                        doc.clone(),
                        std::sync::Arc::clone(&doc.generation),
                        doc.generation.load(std::sync::atomic::Ordering::SeqCst),
                    )
                })
            };
            if let Some((doc, generation, gen_at_snapshot)) = doc_snapshot {
                let markup_message_support = self.client_capabilities.lock().markup_message_support;
                let parse_errors = doc
                    .current_parsed()
                    .map_or_else(|| Arc::from([]) as Arc<[_]>, |p| p.parse_errors_arc());
                let items = Self::syntax_only_lsp_diagnostics(
                    &parse_errors,
                    &doc.text,
                    &doc.line_starts,
                    markup_message_support,
                );
                // Generation-aware staleness guard: discard if a didChange arrived
                // while we were analysing the parse errors.
                let current_gen = generation.load(std::sync::atomic::Ordering::SeqCst);
                if current_gen != gen_at_snapshot {
                    tracing::debug!(
                        uri = uri_str,
                        gen_at_snapshot,
                        current_gen,
                        "Skipping stale syntax-only diagnostic (generation advanced during computation)"
                    );
                    return Ok(Some(Self::empty_full_diagnostic_report()));
                }
                return Ok(Some(Self::full_diagnostic_report(items)));
            }
            let _ = previous_result_id;
            return Ok(Some(Self::empty_full_diagnostic_report()));
        }

        // Snapshot the document, capturing a clone of the generation Arc so
        // we can re-check after computation (mirrors the push-path guard).
        let doc_snapshot = {
            let documents = self.documents.lock();
            self.get_document(&documents, uri_str).map(|doc| {
                (
                    doc.clone(),
                    std::sync::Arc::clone(&doc.generation),
                    doc.generation.load(std::sync::atomic::Ordering::SeqCst),
                )
            })
        };

        if let Some((doc, generation, gen_at_snapshot)) = doc_snapshot {
            // Coarse workDoneProgress for the full pull-diagnostics path, which
            // may spawn the perlcritic subprocess over large trees. Initialized
            // here (after the document-existence check) so that immediately-
            // failing or empty requests don't trigger an unnecessary
            // workDoneProgress/create round-trip (#4626, gemini review).
            let _progress = RequestProgressGuard::new(self, "diagnostics", "Running diagnostics");

            // Build context from server state
            let context = self.pull_diagnostics_orchestrator.build_context(self, uri_str);

            // Use PullDiagnosticsProvider for clean, testable logic
            let provider = PullDiagnosticsProvider::new();
            let report = provider.get_document_diagnostics_with_context(
                &uri,
                &doc.text,
                previous_result_id,
                &context,
                Some(&doc),
            );

            // Collect external perlcritic diagnostics via orchestrator
            let mut perlcritic_diags = Vec::new();
            self.pull_diagnostics_orchestrator.collect_perlcritic_diagnostics(
                self,
                uri_str,
                &doc.text,
                &mut perlcritic_diags,
            );

            // Generation-aware staleness guard: if a newer didChange arrived while
            // diagnostics were being computed, discard this result — the next
            // diagnostic request will compute from the latest version.  Mirrors the
            // guard already present in the push path.
            let current_gen = generation.load(std::sync::atomic::Ordering::SeqCst);
            if current_gen != gen_at_snapshot {
                tracing::debug!(
                    uri = uri_str,
                    gen_at_snapshot,
                    current_gen,
                    "Skipping stale document diagnostic (generation advanced during computation)"
                );
                // Return an empty full report with no resultId so the client
                // does not cache this stale result and retries on the next request.
                return Ok(Some(Self::empty_full_diagnostic_report()));
            }

            // Convert report to JSON
            return Ok(Some(self.document_report_to_json(
                &report,
                &doc,
                uri_str,
                &perlcritic_diags,
            )));
        }

        // Return empty diagnostics if document not found or document not yet open
        Ok(Some(Self::empty_full_diagnostic_report()))
    }

    fn empty_full_diagnostic_report() -> Value {
        Self::full_diagnostic_report(Vec::new())
    }

    fn full_diagnostic_report(items: Vec<Value>) -> Value {
        json!({
            "kind": "full",
            "items": items,
        })
    }

    fn diagnostic_message_value(
        message: &str,
        message_data: Option<&Value>,
        markup_message_support: bool,
    ) -> Value {
        if !markup_message_support {
            return json!(message);
        }

        if let Some(markup) = message_data.and_then(|data| data.get("messageMarkup"))
            && Self::is_markup_content_value(markup)
        {
            return markup.clone();
        }

        json!({
            "kind": "markdown",
            "value": message,
        })
    }

    fn is_markup_content_value(value: &Value) -> bool {
        matches!(value.get("kind").and_then(Value::as_str), Some("markdown" | "plaintext"))
            && value.get("value").and_then(Value::as_str).is_some()
    }

    /// Convert DocumentDiagnosticReport to JSON, merging perlcritic diagnostics.
    fn document_report_to_json(
        &self,
        report: &lsp_types::DocumentDiagnosticReport,
        doc: &crate::state::DocumentState,
        uri: &str,
        perlcritic_diags: &[InternalDiagnostic],
    ) -> Value {
        use lsp_types::DocumentDiagnosticReport;

        match report {
            DocumentDiagnosticReport::Full(full) => {
                let markup_message_support = self.client_capabilities.lock().markup_message_support;
                let mut items: Vec<Value> = full
                    .full_document_diagnostic_report
                    .items
                    .iter()
                    .map(|d| self.lsp_diagnostic_to_json(d, doc, uri, markup_message_support))
                    .collect();

                // Add perlcritic diagnostics
                for d in perlcritic_diags {
                    items.push(self.internal_diagnostic_to_json(
                        d,
                        doc,
                        uri,
                        markup_message_support,
                    ));
                }

                json!({
                    "kind": "full",
                    "resultId": full.full_document_diagnostic_report.result_id,
                    "items": items
                })
            }
            DocumentDiagnosticReport::Unchanged(unchanged) => {
                json!({
                    "kind": "unchanged",
                    "resultId": unchanged.unchanged_document_diagnostic_report.result_id
                })
            }
        }
    }

    /// Convert LSP diagnostic to JSON value.
    ///
    /// Uses `serde_json::to_value` for the standard fields (aligning push and
    /// pull on the same wire shape — the pull path already uses
    /// `lsp_types::Diagnostic` directly), then overrides `message` with the
    /// markup-aware version when the client supports markdown messages (#5017).
    fn lsp_diagnostic_to_json(
        &self,
        d: &lsp_types::Diagnostic,
        _doc: &crate::state::DocumentState,
        _uri: &str,
        markup_message_support: bool,
    ) -> Value {
        // `to_value` is infallible for `Diagnostic` (every field is serializable),
        // but fall back to the hand-built shape if a future field breaks
        // serialization rather than dropping the diagnostic entirely.
        let Ok(mut diag) = serde_json::to_value(d) else {
            return json!({
                "range": d.range,
                "severity": d.severity,
                "source": d.source,
                "message": d.message,
            });
        };

        // Override the message field with the markup-aware version. When the
        // client supports markdown, render structured markup from `data`; when
        // it does not, the plain string is correct (and `to_value` already
        // produced it, so this is a no-op in the common case).
        let message_value =
            Self::diagnostic_message_value(&d.message, d.data.as_ref(), markup_message_support);
        if diag.get("message") != Some(&message_value) {
            diag["message"] = message_value;
        }

        diag
    }

    /// Convert internal diagnostic to JSON value.
    fn internal_diagnostic_to_json(
        &self,
        d: &InternalDiagnostic,
        doc: &crate::state::DocumentState,
        uri: &str,
        markup_message_support: bool,
    ) -> Value {
        let start_pos = doc.line_starts.offset_to_position_rope(&doc.rope, d.range.0);
        let end_pos = doc.line_starts.offset_to_position_rope(&doc.rope, d.range.1);

        let severity = match d.severity {
            InternalDiagnosticSeverity::Error => 1,
            InternalDiagnosticSeverity::Warning => 2,
            InternalDiagnosticSeverity::Information => 3,
            InternalDiagnosticSeverity::Hint => 4,
            // Forward-compatible fallback for future variants (#2898)
            _ => 1,
        };
        let code_str = d.code.as_deref().unwrap_or("");
        let message_val = Self::diagnostic_message_value(&d.message, None, markup_message_support);

        let mut diag = diagnostic_json(
            start_pos.0,
            start_pos.1,
            end_pos.0,
            end_pos.1,
            severity,
            code_str,
            diagnostic_source(d.code.as_deref()),
            message_val.as_str().unwrap_or("").to_string(),
        );

        if !d.tags.is_empty() {
            diag["tags"] = to_json_array(&Self::diagnostic_tags_to_lsp(&d.tags));
        }

        if !d.related_information.is_empty() {
            diag["relatedInformation"] = json!(
                d.related_information
                    .iter()
                    .map(|ri| {
                        let ri_start =
                            doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.0);
                        let ri_end =
                            doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.1);
                        json!({
                            "location": {
                                "uri": uri,
                                "range": {
                                    "start": {"line": ri_start.0, "character": ri_start.1},
                                    "end":   {"line": ri_end.0,   "character": ri_end.1},
                                }
                            },
                            "message": ri.message
                        })
                    })
                    .collect::<Vec<_>>()
            );
        }

        if let Some(ref code_str) = d.code {
            let category = DiagnosticCode::parse_code(code_str)
                .map(|dc| format!("{:?}", dc.category()))
                .unwrap_or_else(|| "Other".to_string());
            let fixable = is_fixable_diagnostic(code_str);
            let tag_strings: Vec<String> = d
                .tags
                .iter()
                .map(|t| match t {
                    InternalDiagnosticTag::Unnecessary => "Unnecessary".to_string(),
                    InternalDiagnosticTag::Deprecated => "Deprecated".to_string(),
                    // Forward-compatible fallback for future variants (#2898)
                    _ => "Unnecessary".to_string(),
                })
                .collect();
            diag["data"] = diagnostic_data(code_str, &category, fixable, &tag_strings);
        }

        diag
    }

    /// Handle workspace/diagnostic request (LSP 3.17 pull diagnostics)
    ///
    /// Computes diagnostics for all open documents in the workspace using the
    /// pull-based model. Provides efficient batch processing with incremental
    /// updates via content-based result IDs.
    ///
    /// # LSP Protocol
    ///
    /// Request: `workspace/diagnostic`
    /// Response: `WorkspaceDiagnosticReport`
    /// Capability: `workspace.diagnostics`
    ///
    /// # Arguments
    ///
    /// * `params` - JSON-RPC parameters with optional previousResultIds map
    ///
    /// # Returns
    ///
    /// WorkspaceDiagnosticReport containing document diagnostic reports with
    /// "unchanged" or "full" kind per document based on content changes
    ///
    /// # Performance
    ///
    /// - Cooperative yielding every 8 documents for responsiveness
    /// - MD5-based content hashing for efficient change detection
    /// - Lock-free document snapshot to avoid blocking other requests
    pub(super) fn handle_workspace_diagnostic(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().diagnostic_provider {
            return Err(crate::protocol::method_not_advertised());
        }

        let previous_result_ids = if let Some(params) = &params {
            if let Some(ids) = params["previousResultIds"].as_array() {
                ids.iter()
                    .filter_map(|item| {
                        let uri = item["uri"].as_str()?;
                        let id = item["value"].as_str()?;
                        Some((uri.to_string(), id.to_string()))
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut items = Vec::new();
        let markup_message_support = self.client_capabilities.lock().markup_message_support;

        // Collect document snapshots without holding lock.
        // Also capture each document's generation Arc and the generation value
        // observed at snapshot time so we can guard against stale results below
        // (mirrors the guard already present in handle_document_diagnostic and
        // the push path).
        let docs_snapshot: Vec<(
            String,
            DocumentState,
            std::sync::Arc<std::sync::atomic::AtomicU32>,
            u32,
        )> = {
            let documents = self.documents.lock();
            documents
                .iter()
                .map(|(k, v)| {
                    let generation_arc = std::sync::Arc::clone(&v.generation);
                    let gen_val = v.generation.load(std::sync::atomic::Ordering::SeqCst);
                    (k.clone(), v.clone(), generation_arc, gen_val)
                })
                .collect()
        };

        // Wait for index build before sampling per-document staleness for the
        // workspace semantic tier (#5016 item 2).
        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        let _ = self
            .check_index_readiness(crate::runtime::readiness::IndexReadinessPolicy::WaitBriefly);

        // Coarse workDoneProgress for the workspace diagnostic path, which
        // iterates over every open document and invokes perlcritic per
        // document — the path most consistent with #4626's "may spawn the
        // perlcritic subprocess over large trees" rationale (#4626, factory-droid review).
        let _workspace_progress = RequestProgressGuard::new(
            self,
            "workspace-diagnostics",
            "Scanning workspace diagnostics",
        );

        for (i, (uri_str, doc, generation, gen_at_snapshot)) in docs_snapshot.iter().enumerate() {
            // Cooperative yield every 8 documents
            if i & 0x7 == 0 {
                std::thread::yield_now();
            }

            // Check if we have a previous result ID for this document
            let prev_id =
                previous_result_ids.iter().find(|(u, _)| u == uri_str).map(|(_, id)| id.clone());

            let Some(parsed) = doc.current_parsed() else { continue };
            if let Some(ast) = parsed.ast() {
                let parse_errors = parsed.parse_errors();
                let provider = DiagnosticsProvider::new();
                // Position-aware resolver: each `use` statement is checked against only
                // the @INC roots that are lexically active at its offset, so `no lib`
                // cancellations that precede the statement are respected.
                let resolver = |module: &str, use_site_offset: usize| {
                    self.resolve_module_to_path_with_doc_at_offset(
                        module,
                        Some(&doc.text),
                        Some(uri_str),
                        Some(use_site_offset),
                    )
                    .is_some()
                };
                let search_context = self
                    .effective_inc_context_for_doc(Some(uri_str), Some(&doc.text), None)
                    .map(|context| context.search_display_paths())
                    .unwrap_or_default();
                let source_path = source_path_from_uri(uri_str);

                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                let workspace_index_tier_enabled =
                    !self.workspace_index_stale_for_document(uri_str);

                // Wire semantic queries when workspace data is available for this URI.
                // When the file consumes roles via `with 'Role'`, build a bounded
                // per-request PackageGraphIndex with ComposesRole edges so PL303
                // cross-file detection is reachable (the persistent index only holds
                // Inherits edges). Files without `with` clauses skip the build.
                // Skipped when the workspace index is stale for this document (#5016 item 2).
                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                let mut diagnostics = {
                    let semantic_diags = workspace_index_tier_enabled
                        .then(|| self.workspace_index())
                        .flatten()
                        .and_then(|workspace_index| {
                            use perl_lsp_rs_core::providers::diagnostics::role_graph_scope::{
                                build_role_scoped_package_graph, consumed_role_names,
                            };
                            let role_names = consumed_role_names(ast);
                            if role_names.is_empty() {
                                workspace_index.with_semantic_queries_for_uri(
                                    uri_str,
                                    |file_id, queries| {
                                        provider.get_diagnostics_with_search_context_and_semantics(
                                            ast,
                                            parse_errors,
                                            &doc.text,
                                            Some(&resolver),
                                            &search_context,
                                            source_path.as_deref(),
                                            file_id,
                                            &queries,
                                        )
                                    },
                                )
                            } else {
                                let scoped_graph =
                                    build_role_scoped_package_graph(&workspace_index, &role_names);
                                workspace_index.with_semantic_queries_for_uri_and_graph(
                                    uri_str,
                                    &scoped_graph,
                                    |file_id, queries| {
                                        provider.get_diagnostics_with_search_context_and_semantics(
                                            ast,
                                            parse_errors,
                                            &doc.text,
                                            Some(&resolver),
                                            &search_context,
                                            source_path.as_deref(),
                                            file_id,
                                            &queries,
                                        )
                                    },
                                )
                            }
                        });
                    semantic_diags.unwrap_or_else(|| {
                        provider.get_diagnostics_with_search_context(
                            ast,
                            parse_errors,
                            &doc.text,
                            Some(&resolver),
                            &search_context,
                            source_path.as_deref(),
                        )
                    })
                };
                #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
                let mut diagnostics = provider.get_diagnostics_with_search_context(
                    ast,
                    parse_errors,
                    &doc.text,
                    Some(&resolver),
                    &search_context,
                    source_path.as_deref(),
                );

                // Add native critic diagnostics when explicitly selected.
                self.collect_native_critic_diagnostics(ast, &doc.text, &mut diagnostics);

                // Add external perlcritic diagnostics (opt-in)
                self.collect_external_perlcritic_diagnostics(uri_str, &doc.text, &mut diagnostics);

                // Add dead code diagnostics from workspace-wide symbol analysis
                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                if workspace_index_tier_enabled
                    && let Some(workspace_index) = self.workspace_index()
                {
                    let dead_code_diags =
                        perl_lsp_rs_core::providers::diagnostics::detect_dead_code(
                            &workspace_index,
                            uri_str,
                            &doc.text,
                            &doc.line_starts,
                        );
                    diagnostics.extend(dead_code_diags);
                }

                // Generation-aware staleness guard: if a newer didChange arrived
                // while diagnostics were being computed, skip this document's
                // result — the next workspace/diagnostic request will compute
                // from the latest version.  Mirrors the guard in the push path
                // and handle_document_diagnostic.
                if generation.load(std::sync::atomic::Ordering::SeqCst) != *gen_at_snapshot {
                    tracing::debug!(
                        uri = uri_str,
                        gen_at_snapshot,
                        current_gen = generation.load(std::sync::atomic::Ordering::SeqCst),
                        "Skipping stale workspace diagnostic (generation advanced during computation)"
                    );
                    continue;
                }

                // Generate result ID
                let result_id = format!("{:x}", md5::compute(&doc.text));

                // Check if unchanged
                let report = if let Some(prev) = prev_id {
                    if prev == result_id {
                        json!({
                            "uri": uri_str,
                            "version": doc.version,
                            "kind": "unchanged",
                            "resultId": prev
                        })
                    } else {
                        // Convert diagnostics (prev_id exists but content changed)
                        let lsp_diagnostics: Vec<Value> = diagnostics
                            .into_iter()
                            .enumerate()
                            .map(|(j, d)| {
                                // Cooperative yield every 32 items
                                if j & 0x1f == 0 {
                                    std::thread::yield_now();
                                }
                                let start_pos =
                                    doc.line_starts.offset_to_position_rope(&doc.rope, d.range.0);
                                let end_pos =
                                    doc.line_starts.offset_to_position_rope(&doc.rope, d.range.1);

                                let message = match d.suggestion {
                                    Some(ref s) => format!("{}\nSuggestion: {}", d.message, s),
                                    None => d.message.clone(),
                                };

                                let mut diag = json!({
                                    "range": {
                                        "start": {
                                            "line": start_pos.0,
                                            "character": start_pos.1,
                                        },
                                        "end": {
                                            "line": end_pos.0,
                                            "character": end_pos.1,
                                        },
                                    },
                                    "severity": match d.severity {
                                        InternalDiagnosticSeverity::Error => 1,
                                        InternalDiagnosticSeverity::Warning => 2,
                                        InternalDiagnosticSeverity::Information => 3,
                                        InternalDiagnosticSeverity::Hint => 4,
                                        // Forward-compatible fallback for future variants (#2898)
                                        _ => 1,
                                    },
                                    "code": d.code.clone(),
                                    "source": diagnostic_source(d.code.as_deref()),
                                    "message": Self::diagnostic_message_value(
                                        &message,
                                        None,
                                        markup_message_support,
                                    ),
                                });
                                if !d.tags.is_empty() {
                                    diag["tags"] = to_json_array(&Self::diagnostic_tags_to_lsp(&d.tags));
                                }
                                if !d.related_information.is_empty() {
                                    diag["relatedInformation"] = json!(
                                        d.related_information.iter().map(|ri| {
                                            let ri_start = doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.0);
                                            let ri_end = doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.1);
                                            json!({
                                                "location": {
                                                    "uri": uri_str,
                                                    "range": {
                                                        "start": {"line": ri_start.0, "character": ri_start.1},
                                                        "end":   {"line": ri_end.0,   "character": ri_end.1},
                                                    }
                                                },
                                                "message": ri.message
                                            })
                                        }).collect::<Vec<_>>()
                                    );
                                }
                                if let Some(ref code_str) = d.code {
                                    let category = DiagnosticCode::parse_code(code_str)
                                        .map(|dc| format!("{:?}", dc.category()))
                                        .unwrap_or_else(|| "Other".to_string());
                                    let fixable = is_fixable_diagnostic(code_str);
                                    let tag_strings: Vec<String> =
                                        d.tags.iter().map(|t| match t {
                                            InternalDiagnosticTag::Unnecessary => "Unnecessary".to_string(),
                                            InternalDiagnosticTag::Deprecated => "Deprecated".to_string(),
                                            // Forward-compatible fallback for future variants (#2898)
                                            _ => "Unnecessary".to_string(),
                                        }).collect();
                                    diag["data"] = diagnostic_data(
                                        code_str,
                                        &category,
                                        fixable,
                                        &tag_strings,
                                    );
                                }
                                diag
                            })
                            .collect();

                        json!({
                            "uri": uri_str,
                            "version": doc.version,
                            "kind": "full",
                            "resultId": result_id,
                            "items": lsp_diagnostics
                        })
                    }
                } else {
                    // No previous result, return full
                    let lsp_diagnostics: Vec<Value> = diagnostics
                        .into_iter()
                        .enumerate()
                        .map(|(j, d)| {
                            // Cooperative yield every 32 items
                            if j & 0x1f == 0 {
                                std::thread::yield_now();
                            }
                            let start_pos =
                                doc.line_starts.offset_to_position_rope(&doc.rope, d.range.0);
                            let end_pos =
                                doc.line_starts.offset_to_position_rope(&doc.rope, d.range.1);

                            let message = match d.suggestion {
                                Some(ref s) => format!("{}\nSuggestion: {}", d.message, s),
                                None => d.message.clone(),
                            };

                            let mut diag = json!({
                                "range": {
                                    "start": {
                                        "line": start_pos.0,
                                        "character": start_pos.1,
                                    },
                                    "end": {
                                        "line": end_pos.0,
                                        "character": end_pos.1,
                                    },
                                },
                                "severity": match d.severity {
                                    InternalDiagnosticSeverity::Error => 1,
                                    InternalDiagnosticSeverity::Warning => 2,
                                    InternalDiagnosticSeverity::Information => 3,
                                    InternalDiagnosticSeverity::Hint => 4,
                                    // Forward-compatible fallback for future variants (#2898)
                                    _ => 1,
                                },
                                "code": d.code.clone(),
                                "source": diagnostic_source(d.code.as_deref()),
                                "message": Self::diagnostic_message_value(
                                    &message,
                                    None,
                                    markup_message_support,
                                ),
                            });
                            if !d.tags.is_empty() {
                                diag["tags"] = to_json_array(&Self::diagnostic_tags_to_lsp(&d.tags));
                            }
                            if !d.related_information.is_empty() {
                                diag["relatedInformation"] = json!(
                                    d.related_information.iter().map(|ri| {
                                        let ri_start = doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.0);
                                        let ri_end = doc.line_starts.offset_to_position_rope(&doc.rope, ri.location.1);
                                        json!({
                                            "location": {
                                                "uri": uri_str,
                                                "range": {
                                                    "start": {"line": ri_start.0, "character": ri_start.1},
                                                    "end":   {"line": ri_end.0,   "character": ri_end.1},
                                                }
                                            },
                                            "message": ri.message
                                        })
                                    }).collect::<Vec<_>>()
                                );
                            }
                            if let Some(ref code_str) = d.code {
                                let category = DiagnosticCode::parse_code(code_str)
                                    .map(|dc| format!("{:?}", dc.category()))
                                    .unwrap_or_else(|| "Other".to_string());
                                let fixable = is_fixable_diagnostic(code_str);
                                let tag_strings: Vec<String> =
                                    d.tags.iter().map(|t| match t {
                                        InternalDiagnosticTag::Unnecessary => "Unnecessary".to_string(),
                                        InternalDiagnosticTag::Deprecated => "Deprecated".to_string(),
                                        // Forward-compatible fallback for future variants (#2898)
                                        _ => "Unnecessary".to_string(),
                                    }).collect();
                                diag["data"] = diagnostic_data(
                                    code_str,
                                    &category,
                                    fixable,
                                    &tag_strings,
                                );
                            }
                            diag
                        })
                        .collect();

                    json!({
                        "uri": uri_str,
                        "version": doc.version,
                        "kind": "full",
                        "resultId": result_id,
                        "items": lsp_diagnostics
                    })
                };

                items.push(report);
            }
        }

        Ok(Some(json!({ "items": items })))
    }

    fn collect_policy_critic_diagnostics(
        &self,
        ast: &std::sync::Arc<perl_parser::ast::Node>,
        doc_text: &str,
        diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
        let critic_engine = { self.config.lock().critic_engine };
        match critic_engine {
            perl_lsp_rs_core::config::CriticEngine::Legacy => {
                let built_in_analyzer = BuiltInAnalyzer::new();
                let violations = built_in_analyzer.analyze(ast, doc_text);
                diagnostics.extend(violations.iter().map(builtin_violation_to_diagnostic));
            }
            perl_lsp_rs_core::config::CriticEngine::Native => {
                self.collect_native_critic_diagnostics(ast, doc_text, diagnostics);
            }
        }
    }

    fn collect_native_critic_diagnostics(
        &self,
        ast: &std::sync::Arc<perl_parser::ast::Node>,
        doc_text: &str,
        diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
        let (critic_engine, severity, profile, native_profile, native_include, native_exclude) = {
            let cfg = self.config.lock();
            (
                cfg.critic_engine,
                cfg.perlcritic_severity,
                cfg.perlcritic_profile.clone(),
                cfg.native_critic_profile.clone(),
                cfg.native_critic_include.clone(),
                cfg.native_critic_exclude.clone(),
            )
        };
        if critic_engine != perl_lsp_rs_core::config::CriticEngine::Native {
            return;
        }

        let critic_config = crate::perl_critic::CriticConfig {
            severity: severity.clamp(1, 5),
            profile,
            include: native_include,
            exclude: native_exclude,
            ..crate::perl_critic::CriticConfig::default()
        };
        let critic_context =
            crate::perl_critic::CriticContext::new(doc_text, ast.as_ref(), &critic_config);
        let profile = crate::perl_critic::NativeCriticProfile::parse(&native_profile)
            .unwrap_or(crate::perl_critic::NativeCriticProfile::Strict);
        let registry = crate::perl_critic::NativeCriticRegistry::for_profile_with_config(
            profile,
            &critic_config,
        );

        diagnostics
            .extend(registry.check(&critic_context).into_iter().map(native_finding_to_diagnostic));
    }

    /// Collect external perlcritic diagnostics if the feature is enabled.
    ///
    /// Checks the `perlcritic_enabled` config flag and whether `perlcritic` is
    /// installed on the system. If both conditions are met, runs perlcritic on
    /// the file and appends violations with severity mapped from Perl::Critic's
    /// 1-5 scale to LSP severity levels (5 -> Error, 4/3 -> Warning,
    /// 2 -> Information, 1 -> Hint).
    ///
    /// The `CriticAnalyzer` is reused across calls via `self.critic_analyzer`
    /// so that the per-file violation cache survives between `didChange` events.
    /// `invalidate_cache` is called from the `didChange` handler, and the
    /// analyzer is reset to `None` from `didChangeConfiguration` whenever any
    /// critic-related setting changes.
    ///
    /// Emits a workspace-scoped warning when perlcritic is unavailable,
    /// configured profile is missing, or execution fails.
    /// Skips file-local diagnostics for those tooling-state errors.
    /// The `doc_text` parameter is used to convert perlcritic's line/column
    /// positions into byte offsets for the internal diagnostic range.
    #[cfg(not(target_arch = "wasm32"))]
    fn collect_external_perlcritic_diagnostics(
        &self,
        uri: &str,
        doc_text: &str,
        diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
        // Check config: perlcritic must be explicitly enabled (opt-in)
        let (enabled, severity, profile, theme, critic_engine) = {
            let cfg = self.config.lock();
            (
                cfg.perlcritic_enabled,
                cfg.perlcritic_severity,
                cfg.perlcritic_profile.clone(),
                cfg.perlcritic_theme.clone(),
                cfg.critic_engine,
            )
        };
        if !enabled || critic_engine == perl_lsp_rs_core::config::CriticEngine::Native {
            return;
        }
        let profile = profile.and_then(|profile| (!profile.trim().is_empty()).then_some(profile));

        // Convert URI to file system path; skip non-file URIs
        let file_path = match url::Url::parse(uri) {
            Ok(u) => match u.to_file_path() {
                Ok(p) => p,
                Err(()) => {
                    tracing::warn!(uri, "perlcritic: URI is not a file path");
                    return;
                }
            },
            Err(e) => {
                tracing::warn!(uri, error = %e, "perlcritic: failed to parse URI");
                return;
            }
        };

        // Warn the user once if perlcritic is not installed.
        // The `skip_perlcritic_command_check` flag is always `false` in production
        // and is only set to `true` through the test helper
        // `LspServer::test_bypass_perlcritic_command_check`, enabling mock-runtime
        // injection without a real `perlcritic` binary.
        let skip_check =
            self.skip_perlcritic_command_check.load(std::sync::atomic::Ordering::Relaxed);
        let force_unavailable =
            self.force_perlcritic_command_unavailable.load(std::sync::atomic::Ordering::Relaxed);
        if force_unavailable
            || (!skip_check && !crate::execute_command::command_exists("perlcritic"))
        {
            self.emit_perlcritic_workspace_warning(
                "missing-binary".to_string(),
                "Perl::Critic is enabled but `perlcritic` was not found on PATH. Install Perl::Critic (for example: `cpanm Perl::Critic`) or disable perl.perlcritic.enabled.",
            );
            return;
        }

        let workspace_root = workspace_root_for_doc(self, uri);
        let resolved_configured_profile = if let Some(ref configured_profile) = profile {
            let resolved = resolve_configured_profile_path(
                configured_profile,
                workspace_root.as_deref(),
                &file_path,
            );
            if resolved.is_none() {
                self.emit_perlcritic_workspace_warning(
                    format!("missing-profile:{configured_profile}"),
                    &format!(
                        "Perl::Critic profile path does not exist: {configured_profile}. Update perl.perlcritic.profile or create the profile file."
                    ),
                );
                return;
            }
            resolved
        } else {
            None
        };

        // Lazy-init the shared CriticAnalyzer.  If the profile or severity
        // changed, `didChangeConfiguration` has already reset the field to
        // `None`, so we rebuild here with the current config.
        //
        // The `.perlcriticrc` walk-up is intentionally placed inside the
        // `is_none()` branch so that filesystem stat calls are skipped on
        // every subsequent diagnostic cycle once the analyzer is warm.
        {
            let mut guard = self.critic_analyzer.lock();
            if guard.is_none() {
                // Walk up the directory tree from the file's parent to the
                // workspace root looking for `.perlcriticrc`.  Ensures that a
                // repo-root config is found even when the file lives in a
                // sub-directory.  Only runs when the analyzer needs (re-)init.
                let resolved_profile = resolved_configured_profile
                    .as_ref()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .or_else(|| {
                        find_workspace_perlcritic_profile(workspace_root.as_deref(), &file_path)
                    });
                let critic_config = crate::perl_critic::CriticConfig {
                    severity,
                    profile: resolved_profile,
                    theme: theme.clone(),
                    ..crate::perl_critic::CriticConfig::default()
                };
                // Use the injected test runtime when present; otherwise fall back
                // to the OS subprocess runtime.
                let analyzer = {
                    let rt_guard = self.critic_runtime_override.lock();
                    if let Some(ref rt) = *rt_guard {
                        crate::perl_critic::CriticAnalyzer::new(
                            critic_config,
                            std::sync::Arc::clone(rt),
                        )
                    } else {
                        crate::perl_critic::CriticAnalyzer::with_os_runtime(critic_config)
                    }
                };
                *guard = Some(analyzer);
            }
        }

        // Compute a content hash so the cache can detect stale entries when the
        // file changes without triggering a `didChange` LSP event (e.g. external
        // editor or `git checkout` while the server is running).
        let content_hash = crate::perl_critic::hash_content(doc_text);

        // Borrow the shared analyzer to run the analysis.  The lock is held
        // only for the duration of the `analyze_file_with_hash` call.
        let result = {
            let mut guard = self.critic_analyzer.lock();
            guard
                .as_mut()
                .map(|a| a.analyze_file_with_hash(&file_path, content_hash, Some(doc_text)))
        };

        match result {
            Some(Ok(violations)) => {
                for v in violations {
                    let internal_severity = critic_severity_to_internal(v.severity);

                    let Some((start_byte, end_byte)) = critic_range_to_byte_range(
                        doc_text,
                        v.range.start.line,
                        v.range.start.column,
                        v.range.end.line,
                        v.range.end.column,
                    ) else {
                        tracing::trace!(
                            uri,
                            policy = %v.policy,
                            start_line = v.range.start.line,
                            start_column = v.range.start.column,
                            end_line = v.range.end.line,
                            end_column = v.range.end.column,
                            "dropping malformed perlcritic diagnostic range"
                        );
                        continue;
                    };

                    diagnostics.push(InternalDiagnostic {
                        range: (start_byte, end_byte),
                        severity: internal_severity,
                        code: Some(v.policy),
                        message: v.description,
                        related_information: Vec::new(),
                        tags: Vec::new(),
                        suggestion: None,
                    });
                }
            }
            Some(Err(e)) => {
                self.emit_perlcritic_workspace_warning(
                    format!("execution-failed:{e}"),
                    &format!("Perl::Critic execution failed: {e}"),
                );
                tracing::warn!(uri, error = %e, "perlcritic failed");
            }
            None => {}
        }
    }

    /// No-op stub for WASM targets where subprocess execution is unavailable.
    #[cfg(target_arch = "wasm32")]
    fn collect_external_perlcritic_diagnostics(
        &self,
        _uri: &str,
        _doc_text: &str,
        _diagnostics: &mut Vec<InternalDiagnostic>,
    ) {
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn emit_perlcritic_workspace_warning(&self, key: String, message: &str) {
        let mut sent = self.critic_workspace_warnings_sent.lock();
        if sent.insert(key) {
            self.show_message_or_log(super::window::MessageType::Warning, message);
        }
    }
}

/// Deduplicate diagnostics that share the same `(range, severity)`, which occurs
/// when the native perlcritic engine and built-in lints report the same finding
/// (e.g. `RequireUseStrictRule` ↔ PL100).  When collapsing, prefer built-in PL*
/// codes over native-critic codes.  (#5088)
fn dedup_overlapping_diagnostics(diagnostics: &mut Vec<perl_lsp_rs_core::providers::Diagnostic>) {
    // Sort so that PL* codes come before native.* codes at the same (range, severity).
    diagnostics.sort_by(|a, b| {
        (a.range, a.severity, is_native_critic_code(a.code.as_deref())).cmp(&(
            b.range,
            b.severity,
            is_native_critic_code(b.code.as_deref()),
        ))
    });
    // Only collapse pairs where exactly one is a native-critic code — this
    // eliminates the native-critic↔built-in-lint overlap (e.g.
    // native.testing.require_use_strict vs PL100) without collapsing two
    // distinct PL* codes that happen to share range+severity (e.g. PL100
    // MissingStrict vs PL101 MissingWarnings, both at (0,0) Warning).
    diagnostics.dedup_by(|a, b| {
        a.range == b.range
            && a.severity == b.severity
            && (is_native_critic_code(a.code.as_deref()) ^ is_native_critic_code(b.code.as_deref()))
    });
}

/// Returns `true` if the code string looks like a native-critic code (not a PL* code).
fn is_native_critic_code(code: Option<&str>) -> bool {
    !code.is_some_and(|c| c.starts_with("PL"))
}

/// Returns `true` when a quick-fix code action exists for the given diagnostic code.
///
/// Mirrors the list in `crates/perl-lsp-rs/src/features/diagnostics/pull.rs`.
/// The authoritative source is `crates/perl-lsp-code-actions/src/code_actions.rs`.
fn is_fixable_diagnostic(code: &str) -> bool {
    is_fixable_perlcritic_policy(code)
        || matches!(
            DiagnosticCode::parse_code(code),
            Some(
                DiagnosticCode::ParseError
                    | DiagnosticCode::MissingStrict
                    | DiagnosticCode::MissingWarnings
                    | DiagnosticCode::PhaseScopedStrictPragma
                    | DiagnosticCode::PhaseScopedWarningsPragma
                    | DiagnosticCode::UnusedVariable
                    | DiagnosticCode::UndefinedVariable
                    | DiagnosticCode::VariableShadowing
                    | DiagnosticCode::UnusedParameter
                    | DiagnosticCode::UnquotedBareword
                    | DiagnosticCode::BarewordFilehandle
                    | DiagnosticCode::TwoArgOpen
                    | DiagnosticCode::AssignmentInCondition
                    | DiagnosticCode::NumericComparisonWithUndef
                    | DiagnosticCode::DeprecatedDefined
                    | DiagnosticCode::MissingPackageDeclaration
                    | DiagnosticCode::VariableRedeclaration
                    | DiagnosticCode::MisspelledPragma
                    | DiagnosticCode::UnreachableCode
                    | DiagnosticCode::DuplicateSubroutine
                    | DiagnosticCode::MissingReturn
            )
        )
}

fn is_fixable_perlcritic_policy(code: &str) -> bool {
    matches!(
        code,
        "InputOutput::ProhibitBarewordFileHandles"
            | "InputOutput::RequireBriefOpen"
            | "InputOutput::RequireThreeArgOpen"
            | "TestingAndDebugging::RequireUseStrict"
            | "TestingAndDebugging::RequireUseWarnings"
            | "native.testing.require_use_strict"
            | "native.testing.require_use_warnings"
            | "native.common.undef_comparison"
            | "native.io.bareword_filehandle"
            | "native.io.two_arg_open"
            | "Variables::ProhibitUnusedVariables"
    )
}

/// Determine the diagnostic source based on the code.
///
/// Source taxonomy (see issue #4627):
/// - `perl-lsp` — all built-in diagnostics: parse errors, built-in lints, and
///   native critic findings (`native.*` codes).
/// - `perl-lsp-critic` — findings from the external `perlcritic` binary, whose
///   codes are fully-qualified Perl::Critic policy names (`Policy::Name`).
fn diagnostic_source(code: Option<&str>) -> &'static str {
    match code {
        Some(code) if code.contains("::") && DiagnosticCode::parse_code(code).is_none() => {
            "perl-lsp-critic"
        }
        _ => "perl-lsp",
    }
}

/// Determine the push-path diagnostic source based on the code.
///
/// Mirrors [`diagnostic_source`] so the same logical finding carries the same
/// source regardless of whether it traveled the push or pull transport. Parse
/// errors previously used the divergent `perl-parser` string here; they now use
/// `perl-lsp` to match the pull path (see issue #4627).
fn push_diagnostic_source(code: Option<&str>) -> &'static str {
    match code {
        Some(code) if code.contains("::") && DiagnosticCode::parse_code(code).is_none() => {
            "perl-lsp-critic"
        }
        _ => "perl-lsp",
    }
}

fn native_finding_to_diagnostic(finding: crate::perl_critic::CriticFinding) -> InternalDiagnostic {
    InternalDiagnostic {
        range: (finding.range.start.byte, finding.range.end.byte),
        severity: critic_severity_to_internal(finding.severity),
        code: Some(finding.rule_id),
        message: finding.message,
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    }
}

/// Map a Perl::Critic severity onto an internal diagnostic severity.
///
/// Perl::Critic scores violations 1 (least severe) to 5 (most severe). The
/// variant names run the other way -- they are `perlcritic` threshold names --
/// so `Gentle` is numeric 5 and becomes `Error`, and `Brutal` is numeric 1 and
/// becomes `Hint`. See `perl_lsp_rs_core::tooling::perl_critic::Severity` for
/// the full explanation.
///
/// This is the only place the perlcritic-to-internal mapping is written for
/// the runtime diagnostics path; every caller routes through it.
pub(crate) fn critic_severity_to_internal(
    severity: crate::perl_critic::Severity,
) -> InternalDiagnosticSeverity {
    match severity {
        crate::perl_critic::Severity::Gentle => InternalDiagnosticSeverity::Error,
        crate::perl_critic::Severity::Stern | crate::perl_critic::Severity::Harsh => {
            InternalDiagnosticSeverity::Warning
        }
        crate::perl_critic::Severity::Cruel => InternalDiagnosticSeverity::Information,
        crate::perl_critic::Severity::Brutal => InternalDiagnosticSeverity::Hint,
    }
}

/// Convert a built-in analyzer violation to an internal diagnostic.
fn builtin_violation_to_diagnostic(
    violation: &crate::perl_critic::Violation,
) -> InternalDiagnostic {
    InternalDiagnostic {
        range: (violation.range.start.byte, violation.range.end.byte),
        severity: critic_severity_to_internal(violation.severity),
        code: Some(violation.policy.clone()),
        message: violation.description.clone(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    // Tests are permitted to use `.expect()` on Result/Option per the repo's
    // coding standards (unlike production code, where it is banned).
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::sync::Arc as StdArc;
    use std::time::{Duration, Instant};

    /// Shared-buffer writer for capturing outbound LSP notifications in tests.
    struct SharedVecWriter {
        inner: StdArc<parking_lot::Mutex<Vec<u8>>>,
    }
    impl Write for SharedVecWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn make_server_with_capture() -> (LspServer, StdArc<parking_lot::Mutex<Vec<u8>>>) {
        let buf = StdArc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
        let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
        let server =
            LspServer::with_io(Box::new(std::io::Cursor::new(Vec::<u8>::new())), Box::new(writer));
        (server, buf)
    }

    fn drain_pending_index_tasks(server: &LspServer) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while server.pending_index_tasks() > 0 {
            assert!(
                Instant::now() < deadline,
                "background index tasks did not drain before diagnostic assertion"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn make_server_with_capture_and_tuning(
        runtime_tuning: perl_lsp_rs_core::runtime::tuning::RuntimeTuning,
    ) -> (LspServer, StdArc<parking_lot::Mutex<Vec<u8>>>) {
        let buf = StdArc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
        let writer = SharedVecWriter { inner: StdArc::clone(&buf) };
        let server = LspServer::with_io_feature_profile_and_tuning(
            Box::new(std::io::Cursor::new(Vec::<u8>::new())),
            Box::new(writer),
            FeatureProfile::current(),
            runtime_tuning,
        );
        (server, buf)
    }

    fn capture_until(
        buffer: &StdArc<parking_lot::Mutex<Vec<u8>>>,
        predicate: impl Fn(&str) -> bool,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let output = String::from_utf8_lossy(&buffer.lock()).into_owned();
            if predicate(&output) || Instant::now() >= deadline {
                return output;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn critic_range_mapping_rejects_malformed_positions() {
        assert_eq!(critic_range_to_byte_range("my $x = 1;\n", 0, 0, 0, 2), Some((0, 2)));
        assert_eq!(critic_range_to_byte_range("my $x = 1;\n", 0, 2, 0, 2), Some((2, 2)));
        assert_eq!(critic_range_to_byte_range("my $x = 1;\n", 3, 0, 3, 1), None);
        assert_eq!(critic_range_to_byte_range("my $x = 1;\n", 0, 4, 0, 2), None);
    }

    #[test]
    fn push_perlcritic_drops_malformed_ranges() {
        use perl_lsp_rs_core::config::CriticEngine;
        use perl_subprocess_runtime::mock::{MockResponse, MockSubprocessRuntime};

        let (server, buffer) = make_server_with_capture_and_tuning(
            perl_lsp_rs_core::runtime::tuning::RuntimeTuning::normal_defaults(),
        );
        let uri = if cfg!(windows) { "file:///C:/tmp/test.pl" } else { "file:///tmp/test.pl" };
        server.test_configure_perlcritic(true, 3, None);
        server.test_configure_critic_engine(CriticEngine::Legacy);

        let runtime = StdArc::new(MockSubprocessRuntime::new());
        let mock_response = MockResponse::success(
            b"test.pl:1:1:3:TestingAndDebugging::RequireUseStrict:valid range\n\
              test.pl:99:1:3:TestingAndDebugging::RequireUseStrict:bad line range\n\
              test.pl:1:99:3:TestingAndDebugging::RequireUseStrict:bad column range\n"
                .to_vec(),
        );
        runtime.add_response(mock_response);
        let runtime_for_server: StdArc<dyn perl_subprocess_runtime::SubprocessRuntime> =
            runtime.clone();
        server.test_install_mock_critic_runtime(runtime_for_server);
        server.test_bypass_perlcritic_command_check();

        server
            .test_handle_did_open(Some(json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "perl",
                        "version": 1,
                        "text": "print 'hello';\n"
                    }
            })))
            .expect("didOpen should succeed");
        let _initial_output =
            capture_until(&buffer, |output| output.contains("publishDiagnostics"));
        server
            .test_publish_parse_for_current_generation(uri)
            .expect("test parse should publish the current snapshot");
        buffer.lock().clear();
        server.publish_diagnostics(uri);
        capture_until(&buffer, |output| output.contains("valid range"));
        drop(server);
        let output = String::from_utf8_lossy(&buffer.lock()).into_owned();

        assert!(
            output.contains("valid range"),
            "valid external critic range must publish a diagnostic: {output:?}"
        );
        assert!(
            !output.contains("bad line range") && !output.contains("bad column range"),
            "malformed external critic ranges must not publish diagnostics: {output:?}"
        );
    }

    /// Positive case: when no concurrent change arrives during diagnostic computation,
    /// `publish_diagnostics` MUST send a `textDocument/publishDiagnostics` notification.
    #[test]
    fn stable_generation_publishes_diagnostics() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///stable_gen_test.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": "my $x = 1;\n"}
            })))
            .unwrap();

        // No concurrent change: generation is stable throughout, publish must fire.
        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50)); // flush outbound writer

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        assert!(
            text.contains("publishDiagnostics"),
            "stable generation must produce a publishDiagnostics notification; got: {text:?}"
        );
    }

    /// #1773: push diagnostics must include enrichment fields (codeDescription,
    /// data) for parity with the pull-based path. A code like PL103 (undefined
    /// variable) should produce both a `codeDescription.href` link and a `data`
    /// object with category/fixable metadata.
    #[test]
    fn push_diagnostics_include_enrichment_fields() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///push_enrichment_test.pl";
        // Code that produces an UndefinedVariable (PL103) diagnostic under strict
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "use strict;\nprint $undeclared_var;\n"
                }
            })))
            .unwrap();

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();

        // The push path must include codeDescription with an href link
        assert!(
            text.contains("codeDescription"),
            "push diagnostics must include codeDescription (#1773); got: {text:?}"
        );
        assert!(
            text.contains("href"),
            "codeDescription must include href URL (#1773); got: {text:?}"
        );

        // The push path must include data with structured metadata
        assert!(
            text.contains("\"data\""),
            "push diagnostics must include data field (#1773); got: {text:?}"
        );
        assert!(text.contains("category"), "data must include category (#1773); got: {text:?}");
        assert!(text.contains("fixable"), "data must include fixable flag (#1773); got: {text:?}");
    }

    /// Pending-parse gap (#3396 PR4): bumping the generation counter WITHOUT
    /// publishing a new `ParsedSnapshot` for it forces `current_parsed()` to
    /// return `None` -- exactly the state a future async parse worker can
    /// leave the document in between a fast text update and a slower parse
    /// completion. `publish_diagnostics` must skip the push entirely in that
    /// state rather than publishing an empty/parse-error-only diagnostics set
    /// computed from no current-generation AST: that would silently overwrite
    /// whatever the client is currently displaying with a false "nothing
    /// wrong" claim.
    ///
    /// Before #3396 PR4 this scenario (deliberately) published anyway, because
    /// the only guard was "did the generation change during computation" --
    /// it never checked whether the snapshot was already stale *before*
    /// computation started. This test replaces the old
    /// `pre_advanced_generation_does_not_suppress_publish` assertion, which
    /// encoded the pre-ParsedSnapshot-seam behavior that this PR corrects.
    #[test]
    fn pending_parse_gap_suppresses_push_publish() -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///pre_advanced_gen_test.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": "my $y = 2;\n"}
        })))?;
        // `didOpen` publishes once via the outbound notification channel,
        // which flushes to `buf` on a background writer thread -- wait for
        // it to land before clearing, otherwise the clear can race ahead of
        // the write and this test would flakily "see" the didOpen publish
        // instead of the (correctly suppressed) publish under test.
        std::thread::sleep(Duration::from_millis(50));
        buf.lock().clear();

        // Advance generation BEFORE calling publish_diagnostics, WITHOUT
        // republishing a snapshot for it -- this opens the pending-parse gap.
        server
            .test_apply_text_change_without_reparse(uri, "my $y = 2;\n", 2)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(
            !text.contains("publishDiagnostics"),
            "pending-parse gap (current_parsed() == None) must suppress the push publish \
             instead of overwriting the client's display with an empty/parse-error-only \
             diagnostics set; got: {text:?}"
        );
        Ok(())
    }

    /// Companion to `pending_parse_gap_suppresses_push_publish`: once a
    /// snapshot is published for the current generation, `publish_diagnostics`
    /// resumes normally -- the gap is transient, not a permanent suppression.
    #[test]
    fn publish_resumes_once_generation_gap_closes() -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///gap_closes_publish_test.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": "my $y = 2;\n"}
        })))?;
        // Wait for didOpen's own publish to flush through the outbound
        // channel before clearing, so the clear can't race ahead of it (see
        // the identical comment in `pending_parse_gap_suppresses_push_publish`).
        std::thread::sleep(Duration::from_millis(50));

        server
            .test_apply_text_change_without_reparse(uri, "my $y = 3;\n", 2)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        buf.lock().clear();

        // While the gap is open, nothing is published.
        server.publish_diagnostics(uri);
        {
            let bytes = buf.lock().clone();
            let text = String::from_utf8(bytes)?;
            assert!(
                !text.contains("publishDiagnostics"),
                "gap must still suppress publish before republication; got: {text:?}"
            );
        }

        // Close the gap by publishing a snapshot for the current generation.
        server
            .test_publish_parse_for_current_generation(uri)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(
            text.contains("publishDiagnostics"),
            "publish must resume once a fresh snapshot closes the pending-parse gap; got: {text:?}"
        );
        Ok(())
    }

    #[test]
    fn publish_diagnostics_boundary_discriminator_syntax_only_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture_and_tuning(
            perl_lsp_rs_core::runtime::tuning::RuntimeTuning::e2e_defaults(),
        );
        let uri = "file:///syntax_only_publish_boundary.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub broken {\n"
            }
        })))?;

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(
            text.contains("publishDiagnostics") && text.contains("\"source\":\"perl-lsp\""),
            "input that hits the boundary: self.runtime_tuning.diagnostic_mode\n            == perl_lsp_rs_core::runtime::tuning::DiagnosticMode::SyntaxOnly; got: {text:?}"
        );
        Ok(())
    }

    #[test]
    fn publish_diagnostics_boundary_discriminator_generation_changed_after_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///stale_publish_boundary.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "my $stable = 1;\n"
            }
        })))?;
        std::thread::sleep(Duration::from_millis(50));
        buf.lock().clear();

        let generation = {
            let documents = server.documents.lock();
            let document = documents.get(uri).ok_or("missing open document")?;
            StdArc::clone(&document.generation)
        };
        let generation_after_publish = StdArc::clone(&generation);
        *server.diagnostic_after_snapshot_hook.lock() = Some(Box::new(move || {
            generation.fetch_add(1, Ordering::SeqCst);
        }));

        server.publish_diagnostics(uri);
        assert_eq!(
            generation_after_publish.load(Ordering::SeqCst),
            1,
            "test hook must advance generation after the diagnostics snapshot"
        );
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(
            !text.contains("publishDiagnostics"),
            "input that hits the boundary: generation.load(Ordering::SeqCst) != gen_at_snapshot; got: {text:?}"
        );
        Ok(())
    }

    #[test]
    fn native_critic_engine_publishes_native_policy_diagnostics() {
        let (server, buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("strict");
        let uri = "file:///native_critic_push_test.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $x = 1;\nmy $x = 2;\nmy $unused = 3;\nmy $shadow = 4;\nmy $outer_param = 0;\nmy $cond = 0;\nmy $path = 'file.txt';\nmy $eval_code = 'print 1';\nmy $cmd_out = `ls`;\nmy $qx_out = qx(date);\nmy $readpipe_out = readpipe($path);\nif ($cond = 1) { print $cond; }\nif ($path == undef) { print $path; }\neval { die $path; };\nif ($@) { warn $@; }\nopen(FH, '<', 'file.txt');\nopen(my $log_fh, $path);\nopen(my $pipe_fh, '-|', 'ls');\neval $eval_code;\nsystem($path);\nexec('ls', '-la');\nprint $log_fh;\nprint $pipe_fh;\n{ my $shadow = 5; print $shadow; }\nsub helper($used_param, $unused_param) { return $used_param; }\nsub duplicate_param($dup_param, $dup_param) { return $dup_param; }\nsub shadow_param($outer_param) { return $outer_param; }\nsub unreachable_helper { return 1; my $dead_after_return = 2; }\nprint $x + $shadow + $outer_param + $cond + $cmd_out + $qx_out + $readpipe_out;\n"
                }
            })))
            .unwrap();

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        // After dedup (#5088), native-critic diagnostics that overlap with built-in
        // PL* lints are collapsed — the PL* code wins.  Verify the strict/warnings
        // findings are present via their PL* codes, and that native-only findings
        // (no PL* equivalent) still appear.
        assert!(
            text.contains("PL100"),
            "strict finding should be present (PL100 after dedup); got: {text:?}"
        );
        // PL101 (MissingWarnings) must survive — the XOR dedup only collapses
        // native-critic↔PL* pairs, not PL*↔PL* pairs. Both PL100 and PL101
        // are distinct built-in lints at (0,0) Warning and must both appear.
        assert!(
            text.contains("PL101"),
            "warnings finding should be present (PL101 NOT collapsed with PL100); got: {text:?}"
        );
        // Verify deduplication: the native strict/warnings diagnostics should NOT
        // duplicate the built-in PL100/PL101 findings.
        assert!(
            !text.contains("native.testing.require_use_strict"),
            "native strict should be deduped to PL100; got: {text:?}"
        );
        // Native-only findings (no PL* equivalent) should still appear:
        assert!(
            text.contains("native.common.stale_dollar_at"),
            "native stale-$@ finding (no PL* equivalent) should still be present; got: {text:?}"
        );
        // PL601 (SecurityBacktickExec) and native.security.backtick_exec both
        // emit at Warning severity after the #5285 fix. Since they share the same
        // range+severity, the dedup collapses the native-critic one — PL601 wins.
        assert!(
            text.contains("PL601"),
            "backtick-exec should be present as PL601 (deduped from native.security.backtick_exec); got: {text:?}"
        );
        assert!(
            !text.contains("native.security.backtick_exec"),
            "native backtick-exec should be deduped to PL601 (same severity after #5285); got: {text:?}"
        );
        assert!(
            text.contains("PL404"),
            "undef-comparison finding should be present (PL404 after dedup); got: {text:?}"
        );
        // Findings that overlap with PL* codes at the same severity are deduped.
        // Verify the PL* equivalents are present:
        assert!(
            text.contains("PL403") || text.contains("native.common.assignment_in_condition"),
            "assignment-in-condition finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("PL400") || text.contains("native.io.bareword_filehandle"),
            "bareword-filehandle finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("PL401") || text.contains("native.io.two_arg_open"),
            "two-arg-open finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("PL605") || text.contains("native.io.pipe_open"),
            "pipe-open finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("PL600") || text.contains("native.security.string_eval"),
            "string-eval finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("PL603") || text.contains("native.security.system_exec"),
            "system-exec finding should be present; got: {text:?}"
        );
        assert!(
            text.contains("\"source\":\"perl-lsp\""),
            "native critic diagnostics should use perl-lsp source; got: {text:?}"
        );
        assert!(
            !text.contains("TestingAndDebugging::RequireUseStrict"),
            "native critic engine should not publish legacy built-in critic policy IDs; got: {text:?}"
        );
    }

    #[test]
    fn native_critic_recommended_profile_publishes_lower_noise_policy_diagnostics() {
        let (server, buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("recommended");
        let uri = "file:///native_critic_recommended_push_test.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $unused = 1;\nmy $cond = 0;\nif ($cond = 1) { print $cond; }\n"
                }
            })))
            .unwrap();

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        // After dedup (#5088), strict/warnings appear as PL100/PL101 instead of
        // native.testing.*.  The recommended profile's assignment-in-condition
        // rule maps to PL403.
        assert!(
            text.contains("PL100"),
            "recommended profile should publish strict finding (PL100 after dedup); got: {text:?}"
        );
        assert!(
            text.contains("PL403"),
            "recommended profile should publish assignment-in-condition (PL403 after dedup); got: {text:?}"
        );
        assert!(
            !text.contains("native.variables.unused_lexical"),
            "recommended native critic profile should omit broader variable-noise findings; got: {text:?}"
        );
    }

    #[test]
    fn native_critic_push_diagnostics_honor_include_and_exclude_filters() {
        let (server, buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("recommended");
        server.test_configure_native_critic_filters(
            vec!["native.testing.require_use_strict".to_string()],
            vec!["native.common.assignment_in_condition".to_string()],
        );
        let uri = "file:///native_critic_filtered_push_test.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $cond = 0;\nif ($cond = 1) { print $cond; }\n"
                }
            })))
            .unwrap();

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        // After dedup (#5088), native.testing.require_use_strict collapses to PL100.
        // The include filter still works (the strict finding is present via PL100).
        // The exclude filter removes the native critic's assignment rule, but PL403
        // (the built-in lint equivalent) still fires independently — the exclude
        // filter only affects the critic engine, not the built-in lints.
        assert!(
            text.contains("PL100"),
            "native include should keep selected strict rule (PL100 after dedup); got: {text:?}"
        );
        assert!(
            !text.contains("native.common.assignment_in_condition"),
            "native exclude should suppress native assignment rule; got: {text:?}"
        );
        // PL101 (MissingWarnings) is a built-in lint, not a native-critic rule.
        // The include/exclude filter only affects the native-critic engine.
        // PL101 fires independently and is NOT collapsed by the XOR dedup
        // (it's PL* vs PL*, not native vs PL*).
        assert!(
            text.contains("PL101"),
            "PL101 (built-in MissingWarnings) should be present — not affected by critic filters; got: {text:?}"
        );
    }

    #[test]
    fn legacy_critic_engine_keeps_legacy_policy_diagnostics_for_push() {
        let (server, buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Legacy);
        let uri = "file:///legacy_critic_push_test.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $x = 1;\n"
                }
            })))
            .unwrap();

        server.publish_diagnostics(uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        // After dedup (#5088), the legacy critic's RequireUseStrict collapses to
        // PL100 (same range + severity).  Verify the strict finding is present via
        // PL100 and that native policy IDs are absent.
        assert!(
            text.contains("PL100"),
            "legacy critic strict finding should be present (PL100 after dedup); got: {text:?}"
        );
        assert!(
            !text.contains("native.testing.require_use_strict"),
            "explicit legacy critic engine should not publish native policy IDs; got: {text:?}"
        );
    }

    #[test]
    fn push_pl701_uses_effective_inc_context_labels() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("lib"))?;
        let script = workspace.join("script.pl");
        let uri = url::Url::from_file_path(&script)
            .map_err(|()| "failed to build script URI")?
            .to_string();
        let folder_uri = url::Url::from_directory_path(&workspace)
            .map_err(|()| "failed to build workspace URI")?
            .to_string();

        let (server, buf) = make_server_with_capture();
        *server.root_path.lock() = Some(workspace.clone());
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_system_inc = false;
        config.use_perl5lib = false;
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "use Missing::From::Lib;\n"
            }
        })))?;

        server.publish_diagnostics(&uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("PL701"), "missing module should publish PL701; got: {text:?}");
        assert!(
            text.contains("Searched @INC"),
            "PL701 should include searched @INC context; got: {text:?}"
        );
        assert!(
            text.contains("workspace includePaths"),
            "PL701 should label the include root source; got: {text:?}"
        );
        Ok(())
    }

    #[test]
    fn native_critic_engine_adds_native_workspace_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, _buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("strict");
        let uri = "file:///native_critic_workspace_test.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "my $x = 1;\nmy $x = 2;\nmy $unused = 3;\nmy $shadow = 4;\nmy $outer_param = 0;\nmy $cond = 0;\nmy $path = 'file.txt';\nmy $eval_code = 'print 1';\nmy $cmd_out = `ls`;\nmy $qx_out = qx(date);\nmy $readpipe_out = readpipe($path);\nif ($cond = 1) { print $cond; }\nif ($path == undef) { print $path; }\neval { die $path; };\nif ($@) { warn $@; }\nopen(FH, '<', 'file.txt');\nopen(my $log_fh, $path);\nopen(my $pipe_fh, '-|', 'ls');\neval $eval_code;\nsystem($path);\nexec('ls', '-la');\nprint $log_fh;\nprint $pipe_fh;\n{ my $shadow = 5; print $shadow; }\nsub helper($used_param, $unused_param) { return $used_param; }\nsub duplicate_param($dup_param, $dup_param) { return $dup_param; }\nsub shadow_param($outer_param) { return $outer_param; }\nsub unreachable_helper { return 1; my $dead_after_return = 2; }\nprint $x + $shadow + $outer_param + $cond + $cmd_out + $qx_out + $readpipe_out;\n"
            }
        })))?;

        let report = server
            .handle_workspace_diagnostic(Some(json!({
                "identifier": "perl-lsp",
                "previousResultIds": []
            })))?
            .ok_or("workspace diagnostic response missing")?;
        let items = report["items"].as_array().ok_or("workspace diagnostic items missing")?;
        let file_report = items
            .iter()
            .find(|item| item["uri"].as_str() == Some(uri))
            .ok_or("workspace diagnostic report missing opened document")?;
        let diagnostics =
            file_report["items"].as_array().ok_or("workspace diagnostic report missing items")?;

        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.testing.require_use_strict")
                    && diag["source"].as_str() == Some("perl-lsp")
            }),
            "native critic engine should add native strict finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.testing.require_use_warnings")
                    && diag["source"].as_str() == Some("perl-lsp")
            }),
            "native critic engine should add native warnings finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.common.assignment_in_condition")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Assignment in condition - did you mean '=='?")
            }),
            "native critic engine should add native assignment-in-condition finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.common.undef_comparison")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Using '==' with undef -- use defined() to check first")
            }),
            "native critic engine should add native undef-comparison finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.common.stale_dollar_at")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Checking $@ after eval can observe a stale error")
            }),
            "native critic engine should add native stale-dollar-at finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.common.unreachable_code")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Unreachable code: this statement cannot be executed")
            }),
            "native critic engine should add native unreachable-code finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.io.bareword_filehandle")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Bareword filehandle 'FH' should be lexical")
            }),
            "native critic engine should add native bareword filehandle finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.io.two_arg_open")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Two-argument open should use an explicit mode")
            }),
            "native critic engine should add native two-arg open finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.io.pipe_open")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("Pipe-open executes a shell command")
            }),
            "native critic engine should add native pipe-open finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.io.unchecked_open_close")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("open() return value should be checked")
            }),
            "native critic engine should add native unchecked open/close finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.security.backtick_exec")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("Command execution detected")
            }),
            "native critic engine should add native backtick execution finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.security.qx_readpipe")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("qx/readpipe command execution detected")
            }),
            "native critic engine should add native qx/readpipe finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.security.string_eval")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("String eval is a security risk")
            }),
            "native critic engine should add native string eval finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.security.system_exec")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("system() executes a shell command")
            }),
            "native critic engine should add native system/exec finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.unused_lexical")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Lexical variable '$unused' is declared but never used")
            }),
            "native critic engine should add native unused lexical finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.unused_parameter")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str() == Some("Parameter '$unused_param' is never used")
            }),
            "native critic engine should add native unused parameter finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.duplicate_parameter")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Parameter '$dup_param' appears more than once in this signature")
            }),
            "native critic engine should add native duplicate parameter finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.parameter_shadows_global")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Parameter '$outer_param' shadows an outer declaration")
            }),
            "native critic engine should add native parameter shadowing finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.duplicate_lexical")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some(
                            "Lexical variable '$x' is declared more than once in the same scope",
                        )
            }),
            "native critic engine should add native duplicate lexical finding to workspace diagnostics: {report}"
        );
        assert!(
            diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("native.variables.shadowed_lexical")
                    && diag["source"].as_str() == Some("perl-lsp")
                    && diag["message"].as_str()
                        == Some("Lexical variable '$shadow' shadows an outer declaration")
            }),
            "native critic engine should add native shadowed lexical finding to workspace diagnostics: {report}"
        );
        assert!(
            !diagnostics.iter().any(|diag| {
                diag["code"].as_str() == Some("TestingAndDebugging::RequireUseStrict")
            }),
            "native critic workspace diagnostics should not publish legacy built-in policy IDs: {report}"
        );

        Ok(())
    }

    /// Positive case: `publish_parse_errors_fast` must immediately emit a
    /// `textDocument/publishDiagnostics` notification when the document has
    /// parse errors and the client uses push diagnostics.
    #[test]
    fn fast_path_emits_notification_for_documents_with_parse_errors() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///fast_path_parse_err_test.pl";
        // Open a document with a deliberate syntax error.
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "sub { SYNTAX ERROR HERE }\n"
                }
            })))
            .unwrap();

        server.publish_parse_errors_fast(uri);
        // Drop server to flush the writer thread, then inspect the buffer.
        drop(server);

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes).unwrap_or_default();
        assert!(
            text.contains("publishDiagnostics"),
            "fast path must emit publishDiagnostics when parse errors exist; got: {text:?}"
        );
    }

    /// Guard: `publish_parse_errors_fast` with a pull-diagnostic client must NOT
    /// emit any notification (pull clients handle diagnostics on demand).
    #[test]
    fn fast_path_silent_for_pull_diagnostic_clients() {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///fast_path_pull_diags_test.pl";
        // Simulate a client that supports pull diagnostics by setting the flag.
        server.client_supports_pull_diags.store(true, Ordering::Relaxed);
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    // Intentional syntax error â€” fast path would fire if not guarded.
                    "text": "sub { SYNTAX ERROR }\n"
                }
            })))
            .unwrap();

        // didOpen may enqueue active-document readiness asynchronously; drain it
        // and let the outbound writer thread flush before isolating fast-path behavior.
        drain_pending_index_tasks(&server);
        std::thread::sleep(Duration::from_millis(50));

        // Record buffer length before the fast-path call.
        let len_before = buf.lock().len();
        server.publish_parse_errors_fast(uri);
        let len_after = buf.lock().len();
        drop(server);

        assert_eq!(
            len_before,
            len_after,
            "fast path must not emit any bytes for pull-diagnostic clients; \
             buffer grew by {} bytes",
            len_after.saturating_sub(len_before)
        );
    }

    /// Guard: the full diagnostic path must also stay silent for pull clients.
    /// This prevents didOpen from doing slow push-diagnostic work for clients
    /// that will request diagnostics on demand.
    #[test]
    fn slow_path_silent_for_pull_diagnostic_clients() -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///slow_path_pull_diags_test.pl";
        server.client_supports_pull_diags.store(true, Ordering::Relaxed);
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub { SYNTAX ERROR }\n"
            }
        })))?;

        assert_eq!(
            0,
            buf.lock().len(),
            "didOpen must not emit push diagnostics for pull-diagnostic clients"
        );

        server.publish_diagnostics(uri);
        drop(server);
        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;
        assert!(
            !text.contains("publishDiagnostics"),
            "slow path must not emit push diagnostics for pull-diagnostic clients; got: {text:?}"
        );

        Ok(())
    }

    #[test]
    fn builtin_violation_maps_gentle_to_error() {
        let violation = crate::perl_critic::Violation {
            policy: "GentlePolicy".to_string(),
            description: "gentle".to_string(),
            explanation: String::new(),
            severity: crate::perl_critic::Severity::Gentle,
            range: perl_parser::position::Range {
                start: perl_parser::position::Position { byte: 0, line: 0, column: 0 },
                end: perl_parser::position::Position { byte: 0, line: 0, column: 1 },
            },
            file: "test.pl".to_string(),
        };

        let diagnostic = builtin_violation_to_diagnostic(&violation);
        assert_eq!(diagnostic.severity, InternalDiagnosticSeverity::Error);
        assert_eq!(diagnostic.code.as_deref(), Some("GentlePolicy"));
    }

    // --- build_context workspace root tests ---

    /// build_context must use the workspace root of the folder that owns the document,
    /// not the global root_path (which points to the first workspace folder).
    #[test]
    fn build_context_uses_doc_scoped_workspace_root() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let folder_a = temp.path().join("folder-a");
        let folder_b = temp.path().join("folder-b");
        let script_b = folder_b.join("script.pl");
        std::fs::create_dir_all(&folder_a)?;
        std::fs::create_dir_all(&folder_b)?;
        std::fs::write(&script_b, "use strict;\n")?;

        let doc_uri = url::Url::from_file_path(&script_b).map_err(|_| "bad uri")?.to_string();

        let (server, _buf) = make_server_with_capture();
        // root_path points to folder_a (the "primary" folder)
        *server.root_path.lock() = Some(folder_a.clone());
        {
            let mut folders = server.workspace_folders.lock();
            folders.push(
                crate::runtime::workspace_folder::WorkspaceFolderState::new(
                    url::Url::from_directory_path(&folder_a).map_err(|_| "bad uri_a")?.to_string(),
                )
                .with_path(folder_a.clone()),
            );
            folders.push(
                crate::runtime::workspace_folder::WorkspaceFolderState::new(
                    url::Url::from_directory_path(&folder_b).map_err(|_| "bad uri_b")?.to_string(),
                )
                .with_path(folder_b.clone()),
            );
        }

        let orchestrator = PullDiagnosticsOrchestrator::new();
        let context = orchestrator.build_context(&server, &doc_uri);

        assert_eq!(
            context.workspace_root.as_deref(),
            Some(folder_b.as_path()),
            "workspace_root must be the folder containing the document, not root_path"
        );
        Ok(())
    }

    /// When no workspace folder contains the document, build_context must fall back
    /// to the global root_path.
    #[test]
    fn build_context_falls_back_to_root_path_when_no_folder_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        let script = outside.join("script.pl");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&outside)?;
        std::fs::write(&script, "use strict;\n")?;

        let doc_uri = url::Url::from_file_path(&script).map_err(|_| "bad uri")?.to_string();

        let (server, _buf) = make_server_with_capture();
        *server.root_path.lock() = Some(workspace.clone());
        {
            let mut folders = server.workspace_folders.lock();
            folders.push(
                crate::runtime::workspace_folder::WorkspaceFolderState::new(
                    url::Url::from_directory_path(&workspace)
                        .map_err(|_| "bad folder uri")?
                        .to_string(),
                )
                .with_path(workspace.clone()),
            );
        }

        let orchestrator = PullDiagnosticsOrchestrator::new();
        let context = orchestrator.build_context(&server, &doc_uri);

        assert_eq!(
            context.workspace_root.as_deref(),
            Some(workspace.as_path()),
            "workspace_root must fall back to root_path when no folder contains the document"
        );
        Ok(())
    }

    /// Regression guard for scenario_14_no_lib_cancellation:
    ///
    /// When `use lib 'lib'` is followed by `no lib 'lib'` and then `use GoneModule`,
    /// push diagnostics must emit PL701 (missing module) for GoneModule, NOT PL700
    /// (unused import). PL700 would mean the resolver found the module, which would
    /// be wrong — the `no lib` should have cancelled the path.
    ///
    /// This test uses the DEFAULT WorkspaceConfig (include_paths = ["lib", ".", ...])
    /// to match the UX harness scenario where no explicit includePaths are configured.
    #[test]
    fn push_pl701_fires_after_no_lib_cancels_default_include_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("lib"))?;
        // Create GoneModule.pm in lib/ so that without no-lib it WOULD be found.
        std::fs::write(
            workspace.join("lib").join("GoneModule.pm"),
            "package GoneModule;\nsub gone { 1 }\n1;\n",
        )?;
        let script = workspace.join("fixture.pl");
        let uri = url::Url::from_file_path(&script)
            .map_err(|()| "failed to build script URI")?
            .to_string();
        let folder_uri = url::Url::from_directory_path(&workspace)
            .map_err(|()| "failed to build workspace URI")?
            .to_string();

        let (server, buf) = make_server_with_capture();
        *server.root_path.lock() = Some(workspace.clone());
        // Use default config — include_paths = ["lib", ".", "local/lib/perl5"], use_perl5lib = true.
        // This matches the UX harness startup state (no workspace/didChangeConfiguration sent).
        let config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(folder_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        let source = "use strict;\n\
use warnings;\n\
use lib 'lib';\n\
no lib 'lib';\n\
use GoneModule;\n\
\n\
print \"unreachable\\n\";\n";

        server.test_handle_did_open(Some(serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": source
            }
        })))?;

        server.publish_diagnostics(&uri);
        drop(server);
        std::thread::sleep(Duration::from_millis(50));

        let bytes = buf.lock().clone();
        let text = String::from_utf8(bytes)?;

        assert!(
            text.contains("PL701"),
            "PL701 MUST fire: 'no lib' cancelled the path, GoneModule must not be found.\n\
             Published: {text:?}"
        );
        assert!(
            !text.contains("PL700"),
            "PL700 must not fire after 'no lib' cancels the path; that would mean \
             the missing module was still treated as resolved.\n\
             Published: {text:?}"
        );
        Ok(())
    }

    // ── pull-diagnostic params-validation tests (#2292) ──────────────────────

    /// `textDocument/diagnostic` with `None` params must return `INVALID_PARAMS`
    /// (LSP 3.17 — missing params is a client protocol error, not silent empty).
    #[test]
    fn pull_diagnostic_none_params_returns_invalid_params() {
        let server = LspServer::new();
        let result = server.test_handle_document_diagnostic(None);
        assert!(result.is_err(), "None params must produce an error, not Ok(empty report)");
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            crate::protocol::INVALID_PARAMS,
            "error code must be INVALID_PARAMS (-32602); got {}",
            err.code
        );
        assert!(
            err.message.contains("requires params"),
            "error message must explain that params are required; got: {}",
            err.message
        );
    }

    /// `textDocument/diagnostic` where `textDocument.uri` is absent must return
    /// `INVALID_PARAMS`.
    #[test]
    fn pull_diagnostic_missing_uri_returns_invalid_params() {
        let server = LspServer::new();
        let result = server.test_handle_document_diagnostic(Some(json!({ "textDocument": {} })));
        assert!(result.is_err(), "missing textDocument.uri must produce an error");
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            crate::protocol::INVALID_PARAMS,
            "error code must be INVALID_PARAMS (-32602); got {}",
            err.code
        );
        assert!(
            err.message.contains("textDocument.uri"),
            "error message must name the missing field; got: {}",
            err.message
        );
    }

    /// `textDocument/diagnostic` where `textDocument.uri` is an empty string
    /// must return `INVALID_PARAMS`.
    #[test]
    fn pull_diagnostic_empty_uri_returns_invalid_params() {
        let server = LspServer::new();
        let result =
            server.test_handle_document_diagnostic(Some(json!({ "textDocument": { "uri": "" } })));
        assert!(result.is_err(), "empty textDocument.uri must produce an error");
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            crate::protocol::INVALID_PARAMS,
            "error code must be INVALID_PARAMS (-32602); got {}",
            err.code
        );
    }

    /// `textDocument/diagnostic` where `textDocument.uri` cannot be parsed as
    /// a valid URI must return `INVALID_PARAMS`.
    #[test]
    fn pull_diagnostic_unparseable_uri_returns_invalid_params() {
        let server = LspServer::new();
        let result = server.test_handle_document_diagnostic(Some(
            json!({ "textDocument": { "uri": ":::not a uri:::" } }),
        ));
        assert!(result.is_err(), "unparseable URI must produce an error");
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            crate::protocol::INVALID_PARAMS,
            "error code must be INVALID_PARAMS (-32602); got {}",
            err.code
        );
    }

    #[test]
    fn pull_diagnostic_boundary_discriminator_syntax_only_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new_with_tuning(
            perl_lsp_rs_core::runtime::tuning::RuntimeTuning::e2e_defaults(),
        );
        let uri = "file:///syntax_only_pull_boundary.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub broken {\n"
            }
        })))?;

        let report = server
            .test_handle_document_diagnostic(Some(json!({
                "textDocument": { "uri": uri },
                "identifier": "perl-lsp",
                "previousResultId": null
            })))?
            .ok_or("syntax-only pull diagnostic must return a report")?;
        assert_eq!(
            report.get("kind").and_then(serde_json::Value::as_str),
            Some("full"),
            "input that hits the boundary: self.runtime_tuning.diagnostic_mode\n            == perl_lsp_rs_core::runtime::tuning::DiagnosticMode::SyntaxOnly"
        );
        let items = report
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or("syntax-only pull diagnostic report must include items")?;

        assert!(
            !items.is_empty(),
            "input that hits the boundary: self.runtime_tuning.diagnostic_mode\n            == perl_lsp_rs_core::runtime::tuning::DiagnosticMode::SyntaxOnly"
        );
        Ok(())
    }

    #[test]
    fn pull_syntax_only_diagnostic_boundary_discriminator_current_gen_ne_gen_at_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = StdArc::new(LspServer::new_with_tuning(
            perl_lsp_rs_core::runtime::tuning::RuntimeTuning::e2e_defaults(),
        ));
        let uri = "file:///stale_syntax_only_pull_boundary.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub broken {\n"
            }
        })))?;

        let capabilities_guard = server.client_capabilities.lock();
        let worker_server = StdArc::clone(&server);
        let handle = std::thread::spawn(move || {
            worker_server.test_handle_document_diagnostic(Some(json!({
                "textDocument": { "uri": uri },
                "identifier": "perl-lsp",
                "previousResultId": null
            })))
        });

        std::thread::sleep(Duration::from_millis(50));
        {
            let documents = server.documents.lock();
            let document = documents.get(uri).ok_or("missing open document")?;
            document.generation.fetch_add(1, Ordering::SeqCst);
        }
        drop(capabilities_guard);

        let report = handle
            .join()
            .map_err(|_| std::io::Error::other("syntax-only diagnostic worker panicked"))??
            .ok_or("stale syntax-only pull diagnostic must return an empty full report")?;
        assert_eq!(
            report,
            json!({"kind": "full", "items": []}),
            "input that hits the boundary: current_gen != gen_at_snapshot"
        );
        let items = report
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or("stale syntax-only pull diagnostic report must include items")?;
        assert!(items.is_empty(), "input that hits the boundary: current_gen != gen_at_snapshot");
        Ok(())
    }

    #[test]
    fn pull_diagnostic_boundary_discriminator_current_gen_ne_gen_at_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = StdArc::new(LspServer::new());
        let uri = "file:///stale_pull_boundary.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "my $unused = 1;\n"
            }
        })))?;

        let workspace_guard = server.workspace_folders.lock();
        let worker_server = StdArc::clone(&server);
        let handle = std::thread::spawn(move || {
            worker_server.test_handle_document_diagnostic(Some(json!({
                "textDocument": { "uri": uri },
                "identifier": "perl-lsp",
                "previousResultId": null
            })))
        });

        std::thread::sleep(Duration::from_millis(50));
        {
            let documents = server.documents.lock();
            let document = documents.get(uri).ok_or("missing open document")?;
            document.generation.fetch_add(1, Ordering::SeqCst);
        }
        drop(workspace_guard);

        let report = handle
            .join()
            .map_err(|_| std::io::Error::other("diagnostic worker panicked"))??
            .ok_or("stale pull diagnostic must return an empty full report")?;
        assert_eq!(
            report,
            json!({"kind": "full", "items": []}),
            "input that hits the boundary: current_gen != gen_at_snapshot"
        );
        assert_eq!(
            report.get("kind").and_then(serde_json::Value::as_str),
            Some("full"),
            "stale pull diagnostic must return a full report"
        );
        let items = report
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or("stale pull diagnostic report must include items")?;
        assert!(items.is_empty(), "input that hits the boundary: current_gen != gen_at_snapshot");
        assert!(
            report.get("resultId").is_none(),
            "stale pull diagnostic must not cache a result id"
        );
        Ok(())
    }

    /// Positive control: a valid URI for a document that was never opened must
    /// return `Ok({"kind":"full","items":[]})` — genuinely-no-diagnostics is
    /// correct and must NOT be conflated with the error cases above.
    #[test]
    fn pull_diagnostic_valid_uri_unopened_returns_empty_full_report()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let result = server.test_handle_document_diagnostic(Some(
            json!({ "textDocument": { "uri": "file:///never_opened.pl" } }),
        ));
        assert!(result.is_ok(), "valid URI for unopened file must return Ok; got: {:?}", result);
        let value = result?.unwrap_or_default();
        assert_eq!(
            value["kind"].as_str(),
            Some("full"),
            "report kind must be 'full'; got: {value:?}"
        );
        let items = value["items"].as_array().ok_or("report must have an items array")?;
        assert!(
            items.is_empty(),
            "report items must be empty for an unopened file; got: {value:?}"
        );
        Ok(())
    }

    /// Full-range code-action params for a freshly opened perl document.
    fn code_action_params(uri: &str) -> Value {
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 50, "character": 0 },
            },
            "context": { "diagnostics": [] },
        })
    }

    #[test]
    fn native_critic_code_actions_use_native_source_not_perl_critic() {
        // On the default native engine, critic quick-fixes must carry the
        // native diagnostic identity (`source: perl-lsp`, `native.*`
        // code) that the publish path emits — never the external tool's
        // `Perl::Critic` brand. This is the #3276 native-product-surface leak:
        // the code-action handler previously ran the legacy analyzer
        // unconditionally and hardcoded `source: "Perl::Critic"`.
        let (server, _buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("strict");
        let uri = "file:///native_critic_code_action.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $x = 1;\nprint $x;\n"
                }
            })))
            .expect("did_open must succeed");

        let result = server
            .test_handle_code_action(Some(code_action_params(uri)))
            .expect("code_action must succeed")
            .unwrap_or_default();
        let text = result.to_string();

        // The brand must never appear anywhere in the native response.
        assert!(
            !text.contains("Perl::Critic"),
            "native engine code actions must NOT leak the Perl::Critic brand; got: {text}"
        );

        // Structural check: SOME code action must carry an embedded diagnostic
        // whose `code` and `source` are BOTH native on the same object — a loose
        // whole-response substring match would pass even if the native code and
        // native source landed on two different actions. This is the exact
        // guarantee the PR makes (code + source line up with the published
        // native diagnostic, so the client associates the fix).
        let actions = result.as_array().cloned().unwrap_or_default();
        let has_native_diag = actions.iter().any(|a| {
            a["diagnostics"].as_array().is_some_and(|diags| {
                diags.iter().any(|d| {
                    d["code"].as_str() == Some("native.testing.require_use_strict")
                        && d["source"].as_str() == Some("perl-lsp")
                })
            })
        });
        assert!(
            has_native_diag,
            "a native code action must carry code `native.testing.require_use_strict` AND source `perl-lsp` on the SAME diagnostic; got: {text}"
        );
    }

    #[test]
    fn legacy_critic_code_actions_keep_perl_critic_source() {
        // The opt-in legacy compatibility engine still shares the external
        // tool's policy names, so its code actions keep `source: Perl::Critic`.
        // This is the compatibility adapter path — the brand is expected here.
        let (server, _buf) = make_server_with_capture();
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Legacy);
        let uri = "file:///legacy_critic_code_action.pl";
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": "my $x = 1;\nprint $x;\n"
                }
            })))
            .expect("did_open must succeed");

        let result = server
            .test_handle_code_action(Some(code_action_params(uri)))
            .expect("code_action must succeed")
            .unwrap_or_default();
        let text = result.to_string();

        assert!(
            text.contains("Perl::Critic"),
            "legacy engine code actions keep the Perl::Critic source; got: {text}"
        );
        assert!(
            text.contains("TestingAndDebugging::RequireUseStrict"),
            "legacy engine code actions keep the legacy policy code; got: {text}"
        );
    }

    fn native_critic_quickfixes_for_code<'a>(actions: &'a [Value], code: &str) -> Vec<&'a Value> {
        actions
            .iter()
            .filter(|action| {
                action.get("kind").and_then(Value::as_str) == Some("quickfix")
                    && action.get("diagnostics").and_then(Value::as_array).is_some_and(|diags| {
                        diags
                            .iter()
                            .any(|diag| diag.get("code").and_then(Value::as_str) == Some(code))
                    })
            })
            .collect()
    }

    fn open_native_critic_document(server: &LspServer, uri: &str, text: &str) {
        server.test_configure_critic_engine(perl_lsp_rs_core::config::CriticEngine::Native);
        server.test_configure_native_critic_profile("strict");
        server
            .test_handle_did_open(Some(json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": text,
                }
            })))
            .expect("did_open must succeed");
    }

    #[test]
    fn native_critic_shadowed_lexical_suggested_fix_is_not_a_quickfix() {
        let (server, _buf) = make_server_with_capture();
        let uri = "file:///native_critic_shadowed_lexical_code_action.pl";
        let text = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        open_native_critic_document(&server, uri, text);

        let result = server
            .test_handle_code_action(Some(code_action_params(uri)))
            .expect("code_action must succeed")
            .unwrap_or_default();
        let actions = result.as_array().cloned().unwrap_or_default();
        let quickfixes =
            native_critic_quickfixes_for_code(&actions, "native.variables.shadowed_lexical");

        assert!(
            quickfixes.is_empty(),
            "Suggested shadowed_lexical fixes must not surface as quickfixes; got: {result}"
        );
    }

    #[test]
    fn native_critic_duplicate_parameter_suggested_fix_is_not_a_quickfix() {
        let (server, _buf) = make_server_with_capture();
        let uri = "file:///native_critic_duplicate_parameter_code_action.pl";
        let text = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
        open_native_critic_document(&server, uri, text);

        let result = server
            .test_handle_code_action(Some(code_action_params(uri)))
            .expect("code_action must succeed")
            .unwrap_or_default();
        let actions = result.as_array().cloned().unwrap_or_default();
        let quickfixes =
            native_critic_quickfixes_for_code(&actions, "native.variables.duplicate_parameter");

        assert!(
            quickfixes.is_empty(),
            "Suggested duplicate_parameter fixes must not surface as quickfixes; got: {result}"
        );
    }

    #[test]
    fn native_critic_require_use_strict_safe_fix_remains_a_quickfix() {
        let (server, _buf) = make_server_with_capture();
        let uri = "file:///native_critic_require_use_strict_code_action.pl";
        let text = "my $x = 1;\nprint $x;\n";
        open_native_critic_document(&server, uri, text);

        let result = server
            .test_handle_code_action(Some(code_action_params(uri)))
            .expect("code_action must succeed")
            .unwrap_or_default();
        let actions = result.as_array().cloned().unwrap_or_default();
        let quickfixes =
            native_critic_quickfixes_for_code(&actions, "native.testing.require_use_strict");

        assert_eq!(
            quickfixes.len(),
            1,
            "Safe require_use_strict fixes must remain one-click quickfixes; got: {result}"
        );
        assert!(
            quickfixes[0].get("edit").is_some(),
            "require_use_strict quickfix must include a workspace edit: {result}"
        );
    }

    #[cfg(feature = "workspace")]
    fn stale_dead_code_indexed_source() -> &'static str {
        "package StaleDeadUnused;\nsub stale_unused_sub { }\n1;\n"
    }

    #[cfg(feature = "workspace")]
    fn stale_dead_code_used_source() -> &'static str {
        "package StaleDeadUnused;\nsub stale_unused_sub { } stale_unused_sub();\n1;\n"
    }

    #[cfg(feature = "workspace")]
    fn make_document_index_stale_for_diagnostics(
        server: &LspServer,
        uri: &str,
        indexed_text: &str,
        updated_text: &str,
        capture: Option<&StdArc<parking_lot::Mutex<Vec<u8>>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        server.test_apply_did_open(uri, indexed_text, 1)?;
        if let Some(buf) = capture {
            wait_for_published_diagnostics(buf, uri)?;
        }
        server
            .test_index_file_in_building_state(uri, indexed_text)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();
        server
            .test_replace_document_without_index(uri, updated_text, 2)
            .map_err(std::io::Error::other)?;

        assert!(
            server.workspace_index_stale_for_document(uri),
            "test setup must leave the open document newer than the workspace index"
        );
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "test setup must leave at least one edited open document stale"
        );

        Ok(())
    }

    #[cfg(feature = "workspace")]
    fn latest_published_diagnostics<'a>(text: &'a str, uri: &str) -> Option<&'a str> {
        let marker = "\"method\":\"textDocument/publishDiagnostics\"";
        let uri_key = format!("\"uri\":\"{uri}\"");
        let mut remaining = text;
        let mut latest = None;

        while let Some(header_start) = remaining.find("Content-Length:") {
            let Some(after_header) = remaining.get(header_start + "Content-Length:".len()..) else {
                break;
            };
            let Some((header, body)) = after_header.split_once("\r\n\r\n") else {
                break;
            };
            let Some(length_text) = header.lines().next() else {
                break;
            };
            let Ok(length) = length_text.trim().parse::<usize>() else {
                break;
            };
            let body_bytes = body.as_bytes();
            let Some(frame_bytes) = body_bytes.get(..length) else {
                break;
            };
            let Some(rest) = body_bytes.get(length..) else {
                break;
            };
            let Ok(frame) = std::str::from_utf8(frame_bytes) else {
                break;
            };
            if frame.contains(marker) && frame.contains(&uri_key) {
                latest = Some(frame);
            }
            let Ok(next) = std::str::from_utf8(rest) else {
                break;
            };
            remaining = next;
        }

        latest
    }

    #[cfg(feature = "workspace")]
    fn wait_for_published_diagnostics(
        buf: &StdArc<parking_lot::Mutex<Vec<u8>>>,
        uri: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = String::from_utf8_lossy(&buf.lock()).into_owned();
            if latest_published_diagnostics(&text, uri).is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(
                    format!("timed out waiting for publishDiagnostics frame for {uri}").into()
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[cfg(feature = "workspace")]
    fn pull_diagnostic_codes(report: &Value) -> Vec<String> {
        report
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|diag| diag.get("code").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    /// Regression (#5016 item 2): stale workspace index must not drive
    /// `detect_dead_code` on the pull diagnostic path.
    #[cfg(feature = "workspace")]
    #[test]
    fn pull_diagnostic_skips_stale_workspace_dead_code_tier()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/stale_dead_code_pull.pl";
        make_document_index_stale_for_diagnostics(
            &server,
            uri,
            stale_dead_code_indexed_source(),
            stale_dead_code_used_source(),
            None,
        )?;

        let report = server
            .test_handle_document_diagnostic(Some(json!({
                "textDocument": { "uri": uri }
            })))?
            .ok_or("pull diagnostic must return a report")?;
        let codes = pull_diagnostic_codes(&report);
        assert!(
            !codes.iter().any(|code| code == "dead-code-subroutine"),
            "stale workspace index must not emit dead-code diagnostics from outdated symbols: {codes:?}"
        );

        Ok(())
    }

    /// Positive control (#5016 item 2): fresh workspace index still reports
    /// unused subroutines via `detect_dead_code` on the publish path.
    #[cfg(feature = "workspace")]
    #[test]
    fn publish_diagnostic_reports_dead_code_when_workspace_index_is_fresh()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///workspace/fresh_dead_code_publish.pl";
        let source = stale_dead_code_indexed_source();
        server.test_apply_did_open(uri, source, 1)?;
        wait_for_published_diagnostics(&buf, uri)?;
        server.test_index_file_in_building_state(uri, source).map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        buf.lock().clear();
        server.publish_diagnostics(uri);
        wait_for_published_diagnostics(&buf, uri)?;
        drop(server);

        let text = String::from_utf8(buf.lock().clone())?;
        assert!(
            text.contains("dead-code-subroutine"),
            "fresh workspace index should publish dead-code-subroutine; got: {text:?}"
        );

        Ok(())
    }

    /// Regression (#5016 item 2): stale workspace index must not drive
    /// `detect_dead_code` on the publish diagnostic path.
    #[cfg(feature = "workspace")]
    #[test]
    fn publish_diagnostic_skips_stale_workspace_dead_code_tier()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let uri = "file:///workspace/stale_dead_code_publish.pl";
        make_document_index_stale_for_diagnostics(
            &server,
            uri,
            stale_dead_code_indexed_source(),
            stale_dead_code_used_source(),
            Some(&buf),
        )?;

        buf.lock().clear();
        server.publish_diagnostics(uri);
        wait_for_published_diagnostics(&buf, uri)?;
        drop(server);

        let text = String::from_utf8(buf.lock().clone())?;
        let latest = latest_published_diagnostics(&text, uri)
            .ok_or("stale publish regression must emit a target diagnostic frame")?;
        assert!(
            !latest.contains("dead-code-subroutine"),
            "latest stale-target publish must not contain dead-code diagnostics: {latest:?}"
        );

        Ok(())
    }

    /// Regression: workspace-wide dead-code analysis must not publish from a
    /// fresh target URI while another edited open document is stale.
    #[cfg(feature = "workspace")]
    #[test]
    fn publish_diagnostic_skips_dead_code_when_other_open_document_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, buf) = make_server_with_capture();
        let target_uri = "file:///workspace/fresh_target_dead_code_publish.pl";
        let contributor_uri = "file:///workspace/stale_contributor_dead_code_publish.pl";
        let target_source = stale_dead_code_indexed_source();

        server.test_apply_did_open(target_uri, target_source, 1)?;
        wait_for_published_diagnostics(&buf, target_uri)?;
        server
            .test_index_file_in_building_state(target_uri, target_source)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        make_document_index_stale_for_diagnostics(
            &server,
            contributor_uri,
            stale_dead_code_indexed_source(),
            stale_dead_code_used_source(),
            Some(&buf),
        )?;

        buf.lock().clear();
        server.publish_diagnostics(target_uri);
        wait_for_published_diagnostics(&buf, target_uri)?;
        drop(server);

        let text = String::from_utf8(buf.lock().clone())?;
        let latest = latest_published_diagnostics(&text, target_uri)
            .ok_or("stale contributor regression must emit a target diagnostic frame")?;
        assert!(
            !latest.contains("dead-code-subroutine"),
            "latest target publish must suppress stale-contributor dead-code diagnostics: {latest:?}"
        );

        Ok(())
    }
}
