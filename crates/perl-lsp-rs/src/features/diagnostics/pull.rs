//! Pull-based diagnostics support (LSP 3.17).

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::{
    CodeDescription, Diagnostic as LspDiagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity as LspDiagnosticSeverity, DiagnosticTag as LspDiagnosticTag,
    DocumentDiagnosticReport, FullDocumentDiagnosticReport, Location, NumberOrString, Position,
    Range, RelatedFullDocumentDiagnosticReport, RelatedUnchangedDocumentDiagnosticReport,
    UnchangedDocumentDiagnosticReport, Uri, WorkspaceDiagnosticReport,
    WorkspaceDiagnosticReportPartialResult, WorkspaceDocumentDiagnosticReport,
    WorkspaceFullDocumentDiagnosticReport, WorkspaceUnchangedDocumentDiagnosticReport,
};

use serde::{Deserialize, Serialize};

use crate::state::DocumentState;
use crate::util::uri::parse_uri;
use perl_diagnostics::codes::DiagnosticCode;
use perl_lsp_rs_core::config::{AcceptedCriticSnapshot, CriticEngine, ServerConfig};
use perl_lsp_rs_core::providers::diagnostics::{parse_error_code, parse_error_severity};
use perl_lsp_rs_core::tooling::perl_critic::Severity;
use perl_module::{
    UseLibOperation, extract_use_lib_operations_with_offsets,
    no_lib_cancelled_paths_from_operations_at_offset,
    resolve_use_lib_paths_from_operations_at_offset,
};
use perl_parser::Parser;
use perl_parser::error::{ParseError, ResolvedParseDiagnosticAnchor};
#[cfg(test)]
use perl_parser::error::{RecoveryKind, RecoverySite};
use perl_parser::position::offset_to_utf16_line_col;
use perl_parser::util::code_slice;

// Import core diagnostics types from perl-lsp-providers (via parent module re-export)
use super::report_identity::{
    DiagnosticProjectionFragment, PullPositionEncoding, PullReportResultId, compose_report_identity,
};
use super::{
    Diagnostic as InternalDiagnostic, DiagnosticSeverity as InternalDiagnosticSeverity,
    DiagnosticTag as InternalDiagnosticTag, DiagnosticsProvider, RelatedInformation,
};

/// Root authority assumed by contexts built without an explicit workspace
/// binding. The convenience constructors define their own complete (degenerate)
/// report-subject scope so equal inputs stay deterministic; production paths
/// always bind the real owning folder or leave the authority explicitly absent
/// (fail-closed, full-report-without-ID).
const PROVIDER_DEFAULT_ROOT_AUTHORITY: &str = "perl-lsp:pull-provider-default-root";

/// Live currentness predicate for the accepted critic state behind one pull
/// report (#9062/#13304).
///
/// `PullDiagnosticsContext` carries an immutable `AcceptedCriticSnapshot`
/// snapshot taken when the report subject was composed. Rule evaluation and
/// report composition both happen after that snapshot, so configuration can
/// move underneath a run in flight. This predicate is the transport's live
/// authority: production wires it to the same fingerprint comparison the push
/// path uses, and it is consulted twice — inside the native critic service at
/// its settlement barrier, and again at the report boundary before a reusable
/// result ID may be minted.
///
/// The default is `always_current`, which is honest only where no live
/// configuration exists to move: default and test contexts.
#[derive(Clone)]
pub struct AcceptedStateCurrentness(Option<std::sync::Arc<dyn Fn() -> bool + Send + Sync>>);

impl AcceptedStateCurrentness {
    /// A predicate for contexts with no live configuration authority behind
    /// them; the snapshot cannot go stale because nothing can move it.
    #[must_use]
    pub fn always_current() -> Self {
        Self(None)
    }

    /// Bind one caller-owned liveness predicate. `true` means the accepted
    /// state behind this context still equals live configuration.
    #[must_use]
    pub fn new(check: std::sync::Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        Self(Some(check))
    }

    /// Whether the accepted state behind this context is still current.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.0.as_ref().is_none_or(|check| check())
    }
}

impl std::fmt::Debug for AcceptedStateCurrentness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AcceptedStateCurrentness")
            .field(&if self.0.is_some() { "live" } else { "always-current" })
            .finish()
    }
}

/// Context for pull diagnostics operations.
///
/// Contains all configuration and state needed to compute diagnostics
/// without direct LspServer dependencies, enabling testability and
/// clean separation of concerns.
#[derive(Clone)]
pub struct PullDiagnosticsContext {
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub perlcritic_enabled: bool,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub perlcritic_severity: i32,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub perlcritic_profile: Option<String>,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub critic_engine: CriticEngine,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub native_critic_profile: String,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub native_critic_include: Vec<String>,
    /// Deprecated raw migration observation; not Critic behavior authority.
    pub native_critic_exclude: Vec<String>,
    /// Workspace root for .perlcriticrc discovery
    pub workspace_root: Option<PathBuf>,
    /// @INC paths for module resolution
    pub include_paths: Vec<String>,
    /// Whether client supports LSP 3.18 markup messages
    pub markup_message_support: bool,
    /// Optional workspace index for dead code detection
    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    pub workspace_index: Option<std::sync::Arc<perl_workspace::workspace_index::WorkspaceIndex>>,
    /// Owning folder/root authority key binding this document's report subject.
    ///
    /// `None` means no root authority could be established: the report stays
    /// valid but can never carry a reusable result ID (#7480).
    pub identity_root_key: Option<String>,
    /// Current project-fact (workspace index) generation, when the fact tier
    /// is live and fresh for this document. `None` encodes the explicit
    /// not-ready/unavailable fact state.
    pub facts_generation: Option<u64>,
    /// One sealed accepted Critic authority derived through #8253 for this
    /// document/root. Evaluation, finalization and result identity all consume
    /// this exact value. The raw sibling fields above are migration
    /// observations only: changing them cannot change Critic behaviour or
    /// identity (#9062/#12067 review).
    pub accepted_critic_snapshot: AcceptedCriticSnapshot,
    /// Live currentness authority for [`Self::accepted_critic_snapshot`]
    /// (#9062/#13304). Consulted at the native critic service's settlement
    /// barrier and again at the report boundary, so configuration movement
    /// under an in-flight run can neither publish stale native rows nor mint a
    /// reusable result ID for a report that dropped them.
    pub accepted_state_currentness: AcceptedStateCurrentness,
    /// Behavior-bearing negotiated wire-projection state (#7480).
    pub projection: DiagnosticProjectionFragment,
}

impl PullDiagnosticsContext {
    /// Derive the accepted critic state through the #8253 authority from one
    /// coherent raw sibling snapshot (#9062). Used where no live
    /// `ServerConfig` exists (default/test contexts); every production path
    /// derives straight from its live configuration.
    fn accepted_snapshot_from_defaults(
        enabled: bool,
        severity: i32,
        root: Option<&str>,
    ) -> AcceptedCriticSnapshot {
        let config = ServerConfig {
            perlcritic_enabled: enabled,
            perlcritic_severity: severity.clamp(1, 5) as u8,
            ..ServerConfig::default()
        };
        AcceptedCriticSnapshot::capture(&config, root)
    }

    /// Create a new empty context with default values.
    pub fn new() -> Self {
        Self {
            perlcritic_enabled: true,
            perlcritic_severity: 3,
            perlcritic_profile: None,
            critic_engine: CriticEngine::Native,
            native_critic_profile: "recommended".to_string(),
            native_critic_include: Vec::new(),
            native_critic_exclude: Vec::new(),
            workspace_root: None,
            include_paths: Vec::new(),
            markup_message_support: false,
            identity_root_key: Some(PROVIDER_DEFAULT_ROOT_AUTHORITY.to_string()),
            facts_generation: None,
            accepted_critic_snapshot: Self::accepted_snapshot_from_defaults(
                true,
                3,
                Some(PROVIDER_DEFAULT_ROOT_AUTHORITY),
            ),
            accepted_state_currentness: AcceptedStateCurrentness::always_current(),
            projection: DiagnosticProjectionFragment {
                position_encoding: PullPositionEncoding::Utf16,
                markup_messages: false,
            },
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            workspace_index: None,
        }
    }

    /// Create a context with workspace index for dead code detection.
    #[cfg(all(feature = "workspace", not(target_arch = "wasm32"), test))]
    pub fn with_workspace_index(
        index: std::sync::Arc<perl_workspace::workspace_index::WorkspaceIndex>,
    ) -> Self {
        Self {
            perlcritic_enabled: true,
            perlcritic_severity: 3,
            perlcritic_profile: None,
            critic_engine: CriticEngine::Native,
            native_critic_profile: "recommended".to_string(),
            native_critic_include: Vec::new(),
            native_critic_exclude: Vec::new(),
            workspace_root: None,
            include_paths: Vec::new(),
            markup_message_support: false,
            identity_root_key: Some(PROVIDER_DEFAULT_ROOT_AUTHORITY.to_string()),
            facts_generation: None,
            accepted_critic_snapshot: Self::accepted_snapshot_from_defaults(
                true,
                3,
                Some(PROVIDER_DEFAULT_ROOT_AUTHORITY),
            ),
            accepted_state_currentness: AcceptedStateCurrentness::always_current(),
            projection: DiagnosticProjectionFragment {
                position_encoding: PullPositionEncoding::Utf16,
                markup_messages: false,
            },
            workspace_index: Some(index),
        }
    }
}

impl std::fmt::Debug for PullDiagnosticsContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PullDiagnosticsContext")
            .field("perlcritic_enabled", &self.perlcritic_enabled)
            .field("perlcritic_severity", &self.perlcritic_severity)
            .field("perlcritic_profile", &self.perlcritic_profile)
            .field("critic_engine", &self.critic_engine)
            .field("native_critic_profile", &self.native_critic_profile)
            .field("native_critic_include", &self.native_critic_include)
            .field("native_critic_exclude", &self.native_critic_exclude)
            .field("workspace_root", &self.workspace_root)
            .field("include_paths", &self.include_paths)
            .field("markup_message_support", &self.markup_message_support)
            .field("identity_root_key", &self.identity_root_key)
            .field("facts_generation", &self.facts_generation)
            .field("accepted_critic_snapshot", &self.accepted_critic_snapshot)
            .field("accepted_state_currentness", &self.accepted_state_currentness)
            .field("projection", &self.projection)
            .field("workspace_index", &"<WorkspaceIndex>")
            .finish()
    }
}

/// Native Critic work evaluated but not yet committed to a pull report.
///
/// Keeping the run and its exact accepted snapshot together prevents an early
/// append from surviving policy movement between service settlement and the
/// report boundary.
struct PendingPullCriticContribution {
    snapshot: AcceptedCriticSnapshot,
    run: perl_lsp_rs_core::tooling::perl_critic::NativeCriticRun,
}

/// Diagnostics staged before the irreversible pull-report boundary.
struct PendingPullDiagnostics {
    core: Vec<InternalDiagnostic>,
    projected: Vec<LspDiagnostic>,
    critic: Option<PendingPullCriticContribution>,
}

impl PendingPullDiagnostics {
    fn projected(diagnostics: Vec<LspDiagnostic>) -> Self {
        Self { core: Vec::new(), projected: diagnostics, critic: None }
    }
}

/// Provider for pull-based diagnostics (LSP 3.17).
pub struct PullDiagnosticsProvider;

impl PullDiagnosticsProvider {
    /// Create a new pull diagnostics provider.
    pub fn new() -> Self {
        Self
    }

    /// Handle textDocument/diagnostic request.
    ///
    /// The `include_paths` parameter allows specifying @INC search paths for PL701
    /// (ModuleNotFound) diagnostics. When `None`, the context is created with empty
    /// include_paths (backward compatible with existing call sites).
    pub fn get_document_diagnostics(
        &self,
        uri: &Uri,
        content: &str,
        previous_result_id: Option<String>,
        include_paths: Option<Vec<String>>,
    ) -> DocumentDiagnosticReport {
        let mut context = PullDiagnosticsContext::new();
        if let Some(paths) = include_paths {
            context.include_paths = paths;
        }
        self.get_document_diagnostics_with_context(uri, content, previous_result_id, &context, None)
    }

    /// Handle textDocument/diagnostic request with full context.
    ///
    /// This is the production entry point that includes all diagnostic sources:
    /// - Parse errors and AST-based diagnostics
    /// - External perlcritic integration (if enabled in context)
    /// - Dead code detection (if workspace index available)
    /// - Built-in Perl::Critic policy analysis
    /// - @INC-aware module resolution diagnostics
    pub fn get_document_diagnostics_with_context(
        &self,
        uri: &Uri,
        content: &str,
        previous_result_id: Option<String>,
        context: &PullDiagnosticsContext,
        doc_state: Option<&DocumentState>,
    ) -> DocumentDiagnosticReport {
        // The report subject encodes the accepted critic policy snapshotted
        // when this context was built. If configuration has already moved, the
        // composed identity describes a policy that is no longer live: it must
        // neither answer `Unchanged` nor be handed back as reusable (#13304).
        let accepted_state_current = context.accepted_state_currentness.holds();
        let result_id = compose_report_identity(
            &uri.to_string(),
            content,
            doc_state.map(DocumentState::current_generation).map(u64::from),
            context,
            accepted_state_current,
        );

        // `Unchanged` only for a prior ID that parses under the current schema
        // and equals the complete current report subject (#7480).
        let unchanged_prior = previous_result_id
            .as_deref()
            .and_then(PullReportResultId::from_wire)
            .filter(|prior| result_id.as_ref() == Some(prior));
        if let Some(prior) = unchanged_prior
            && context.accepted_state_currentness.holds()
        {
            return self.build_unchanged_report(prior.into_string());
        }

        let pending =
            self.collect_diagnostics_for_text_with_context(uri, content, context, doc_state);
        let (diagnostics, critic_subject_current) =
            self.finalize_pending_diagnostics(uri, content, context, pending);
        let reusable_id = critic_subject_current.then_some(result_id).flatten();
        self.build_full_report(reusable_id, diagnostics)
    }

    /// Handle workspace/diagnostic request.
    pub fn get_workspace_diagnostics(
        &self,
        documents: &HashMap<String, DocumentState>,
        previous_result_ids: Vec<(Uri, String)>,
    ) -> WorkspaceDiagnosticReport {
        let context = PullDiagnosticsContext::new();
        self.get_workspace_diagnostics_with_context(documents, previous_result_ids, &context)
    }

    /// Handle workspace/diagnostic request with full context.
    pub fn get_workspace_diagnostics_with_context(
        &self,
        documents: &HashMap<String, DocumentState>,
        previous_result_ids: Vec<(Uri, String)>,
        context: &PullDiagnosticsContext,
    ) -> WorkspaceDiagnosticReport {
        let mut items = Vec::new();
        let prev_ids: HashMap<Uri, String> = previous_result_ids.into_iter().collect();

        for (uri_str, doc_state) in documents {
            let document_context = context.clone();
            let uri = parse_uri(uri_str);
            let prev_id = prev_ids.get(&uri).cloned();

            // A pending-parse gap (#3396 PR4) is an explicit not-ready subject:
            // the report stays full but never carries a reusable ID, so it can
            // never be echoed back as `Unchanged` (#7480).
            //
            // Accepted critic policy that has already moved is the same kind of
            // not-ready subject: the composed identity would describe a dead
            // policy (#13304).
            let ready = doc_state.current_parsed().is_some()
                && document_context.accepted_state_currentness.holds();
            let result_id = compose_report_identity(
                uri_str,
                &doc_state.text,
                Some(u64::from(doc_state.current_generation())),
                &document_context,
                ready,
            );

            let unchanged_prior = if ready { prev_id.as_deref() } else { None }
                .and_then(PullReportResultId::from_wire)
                .filter(|prior| result_id.as_ref() == Some(prior));

            let report = match unchanged_prior
                .filter(|_| document_context.accepted_state_currentness.holds())
            {
                Some(prior) => self.build_unchanged_report(prior.into_string()),
                None => {
                    // Without readiness the composed identity is suppressed so
                    // the not-ready subject cannot be cached client-side.
                    let pending = self.collect_diagnostics_for_state_with_context(
                        &uri,
                        doc_state,
                        &document_context,
                    );
                    let (diagnostics, critic_subject_current) = self.finalize_pending_diagnostics(
                        &uri,
                        &doc_state.text,
                        &document_context,
                        pending,
                    );
                    let reusable_id =
                        (ready && critic_subject_current).then_some(result_id).flatten();
                    self.build_full_report(reusable_id, diagnostics)
                }
            };

            items.push(self.to_workspace_report(uri, Some(doc_state.version), report));
        }

        WorkspaceDiagnosticReport { items }
    }

    /// Handle workspace/diagnostic partial result with context.
    pub fn get_workspace_diagnostics_partial_with_context(
        &self,
        documents: &[(String, String)],
        batch_size: usize,
        context: &PullDiagnosticsContext,
    ) -> Vec<WorkspaceDiagnosticReportPartialResult> {
        let mut results = Vec::new();

        for chunk in documents.chunks(batch_size) {
            let mut items = Vec::new();

            for (uri_str, content) in chunk {
                let document_context = context.clone();
                let uri = parse_uri(uri_str);
                // Partial workspace progress items use the same per-document
                // identity authority as document and full workspace reports.
                let result_id = compose_report_identity(
                    uri_str,
                    content,
                    None,
                    &document_context,
                    document_context.accepted_state_currentness.holds(),
                );
                // For partial results, we need to parse the content
                let pending = self.collect_diagnostics_for_text_with_context(
                    &uri,
                    content,
                    &document_context,
                    None,
                );
                let (diagnostics, critic_subject_current) =
                    self.finalize_pending_diagnostics(&uri, content, &document_context, pending);
                let reusable_id = critic_subject_current.then_some(result_id).flatten();
                let report = self.build_full_report(reusable_id, diagnostics);

                items.push(self.to_workspace_report(uri, None, report));
            }

            results.push(WorkspaceDiagnosticReportPartialResult { items });
        }

        results
    }

    fn collect_diagnostics_for_text_with_context(
        &self,
        uri: &Uri,
        content: &str,
        context: &PullDiagnosticsContext,
        doc_state: Option<&DocumentState>,
    ) -> PendingPullDiagnostics {
        let code_text = code_slice(content);
        let mut parser = Parser::new(code_text);

        match parser.parse() {
            Ok(ast) => {
                // Retrieve any collected parse errors from error recovery
                let parse_errors: Vec<ParseError> = parser.errors().to_vec();
                let ast = std::sync::Arc::new(ast);
                let provider = DiagnosticsProvider::new();
                let uri_str = uri.to_string();
                let source_path = url::Url::parse(&uri_str)
                    .map_err(|e| {
                        tracing::warn!(uri = %uri_str, error = %e, "pull diagnostics: failed to parse URI");
                    })
                    .ok()
                    .and_then(|value| {
                        value.to_file_path().map_err(|()| {
                            tracing::warn!(uri = %uri_str, "pull diagnostics: URI is not a file path");
                        }).ok()
                    });
                // Build the baseline include paths (configured + PERL5LIB, without lexical
                // `use lib`/`no lib`). The resolver re-evaluates lexical paths per use-site
                // offset so that `no lib` cancellations that precede each `use` statement
                // are respected.
                let base_include_paths = context.include_paths.clone();

                // Extract lexical `use lib` / `no lib` operations once per diagnostic
                // cycle so each `use Module` resolver call filters the cached ops instead
                // of re-scanning the source prefix (#1683).
                let source_path_ref = source_path.as_deref();
                let workspace_root = context
                    .workspace_root
                    .as_deref()
                    .or_else(|| source_path_ref.and_then(std::path::Path::parent))
                    .unwrap_or(std::path::Path::new("."));
                let file_dir = source_path_ref.and_then(std::path::Path::parent);
                let use_lib_ops = extract_use_lib_operations_with_offsets(content);

                // Position-aware resolver: for each `use Module` statement, recompute the
                // effective include paths at that statement's byte offset so that `no lib`
                // directives appearing before it cancel the appropriate `use lib` paths.
                let resolver = |module: &str, use_site_offset: usize| {
                    let paths = self.effective_include_paths_at_offset(
                        &base_include_paths,
                        &use_lib_ops,
                        workspace_root,
                        file_dir,
                        use_site_offset,
                    );
                    self.resolve_module_with_paths(module, &paths, source_path_ref)
                };

                // Search context for PL701 display: compute once for the whole file (end
                // offset) so the diagnostic message shows what paths were searched overall.
                let search_paths: Vec<String> = self.effective_include_paths(
                    &base_include_paths,
                    &use_lib_ops,
                    workspace_root,
                    file_dir,
                );

                // Wire workspace semantic queries when available (pull-text path).
                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                let core_diagnostics: Vec<_> = {
                    let semantic_diags =
                        context.workspace_index.as_ref().and_then(|workspace_index| {
                            workspace_index.with_semantic_queries_for_uri(
                                &uri_str,
                                |file_id, queries| {
                                    provider.get_diagnostics_with_path_and_semantics(
                                        &ast,
                                        &parse_errors,
                                        content,
                                        Some(&resolver),
                                        &search_paths,
                                        source_path.as_deref(),
                                        file_id,
                                        &queries,
                                    )
                                },
                            )
                        });
                    semantic_diags.unwrap_or_else(|| {
                        provider.get_diagnostics_with_path(
                            &ast,
                            &parse_errors,
                            content,
                            Some(&resolver),
                            &search_paths,
                            source_path.as_deref(),
                        )
                    })
                };
                #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
                let core_diagnostics: Vec<_> = provider.get_diagnostics_with_path(
                    &ast,
                    &parse_errors,
                    content,
                    Some(&resolver),
                    &search_paths,
                    source_path.as_deref(),
                );

                let core_diagnostics = core_diagnostics;
                // Critic composition runs over the producer-owned core rows so
                // declared overlap observations can enter the normalized seam
                // before LSP projection (#11918); surviving rows are mapped
                // afterwards.
                let critic_generation =
                    doc_state.map(|state| state.current_generation()).unwrap_or(0);
                let critic = self.evaluate_policy_critic(
                    uri,
                    &ast,
                    content,
                    context,
                    perl_lsp_rs_core::tooling::perl_critic::critic_source_identity_for_uri(
                        &uri.to_string(),
                        critic_generation,
                    ),
                    &core_diagnostics,
                );

                PendingPullDiagnostics {
                    core: core_diagnostics,
                    projected: Vec::new(),
                    critic: Some(critic),
                }
            }
            Err(error) => PendingPullDiagnostics::projected(vec![
                self.parse_error_to_diagnostic_with_context(uri, content, &error, context),
            ]),
        }
    }

    /// Resolve a module to a path using the provided include paths.
    fn resolve_module_with_paths(
        &self,
        module: &str,
        include_paths: &[String],
        source_path: Option<&std::path::Path>,
    ) -> bool {
        // Convert module name to path
        let module_path = module.replace("::", "/") + ".pm";

        // Check include paths
        for path in include_paths {
            let include_root = {
                let include_path = std::path::Path::new(path);
                if include_path.is_absolute() {
                    include_path.to_path_buf()
                } else if let Some(source_parent) = source_path.and_then(std::path::Path::parent) {
                    source_parent.join(include_path)
                } else {
                    include_path.to_path_buf()
                }
            };
            let full_path = include_root.join(&module_path);
            if full_path.exists() {
                return true;
            }
        }

        // Check relative to source file
        if let Some(source) = source_path
            && let Some(parent) = source.parent()
        {
            let relative_path = parent.join(&module_path);
            if relative_path.exists() {
                return true;
            }
        }

        false
    }

    fn effective_include_paths(
        &self,
        include_paths: &[String],
        use_lib_ops: &[UseLibOperation],
        workspace_root: &std::path::Path,
        file_dir: Option<&std::path::Path>,
    ) -> Vec<String> {
        let mut effective_paths = include_paths.to_vec();
        let dynamic_paths = resolve_use_lib_paths_from_operations_at_offset(
            use_lib_ops,
            usize::MAX,
            workspace_root,
            file_dir,
        );
        for path in dynamic_paths.into_iter().rev() {
            effective_paths.retain(|existing| existing != &path);
            effective_paths.insert(0, path);
        }

        effective_paths
    }

    /// Compute effective include paths at a specific byte offset.
    ///
    /// Identical to [`effective_include_paths`] but uses position-aware
    /// `use lib` / `no lib` evaluation: only operations that precede
    /// `use_site_offset` in the source text are considered. This means a
    /// `no lib 'lib'` directive that appears before the offset correctly
    /// removes the `lib` path, even though it appears after the initial
    /// `use lib 'lib'`.
    fn effective_include_paths_at_offset(
        &self,
        include_paths: &[String],
        use_lib_ops: &[UseLibOperation],
        workspace_root: &std::path::Path,
        file_dir: Option<&std::path::Path>,
        use_site_offset: usize,
    ) -> Vec<String> {
        // Determine which configured paths were explicitly cancelled by `no lib`
        // at this offset. A `no lib 'lib'` directive removes `lib` from `@INC`
        // regardless of whether it arrived via `use lib` or workspace config.
        let cancelled = no_lib_cancelled_paths_from_operations_at_offset(
            use_lib_ops,
            use_site_offset,
            workspace_root,
            file_dir,
        );

        // Start from configured paths, excluding any that `no lib` cancelled.
        let mut effective_paths: Vec<String> =
            include_paths.iter().filter(|p| !cancelled.contains(p)).cloned().collect();

        // Prepend lexical `use lib` paths that are active at this offset.
        let dynamic_paths = resolve_use_lib_paths_from_operations_at_offset(
            use_lib_ops,
            use_site_offset,
            workspace_root,
            file_dir,
        );
        for path in dynamic_paths.into_iter().rev() {
            effective_paths.retain(|existing| existing != &path);
            effective_paths.insert(0, path);
        }

        effective_paths
    }

    /// Add configured policy critic diagnostics.
    ///
    /// Under the native engine this also consumes core diagnostics whose
    /// emitter declared a reviewed critic overlap observation: those ordinary
    /// rows are replaced by the normalized logical rows appended to
    /// `critic_rows`, merged with their native aliases (#11918).
    fn evaluate_policy_critic(
        &self,
        uri: &Uri,
        ast: &std::sync::Arc<perl_parser::ast::Node>,
        content: &str,
        context: &PullDiagnosticsContext,
        source_identity: perl_lsp_rs_core::tooling::perl_critic::CriticSourceIdentity,
        core_diagnostics: &[InternalDiagnostic],
    ) -> PendingPullCriticContribution {
        // #9062: routing authority is the accepted state (#8253), never the raw
        // engine setting. `EffectiveCriticState` is `Disabled | Native`, so a
        // deprecated `legacy`/`external`/`perlcritic` value is a migration
        // observation that cannot construct runtime state and cannot select a
        // second evaluator here. The service owns the disabled contribution
        // too, so there is no consumer-side branch left to get wrong.
        self.evaluate_native_critic(uri, ast, content, context, source_identity, core_diagnostics)
    }

    /// Add native critic policy diagnostics.
    ///
    /// Routed through the one protocol-neutral [`NativeCriticService`]
    /// (#9062). The immutable context carries the complete accepted state
    /// (#8253), so the service — not this transport — owns registry
    /// construction, candidate collection, canonical normalization, policy,
    /// and ordering. This method only extracts emitter-declared overlap
    /// observations (#11918) and projects the resulting logical rows.
    fn evaluate_native_critic(
        &self,
        uri: &Uri,
        ast: &std::sync::Arc<perl_parser::ast::Node>,
        content: &str,
        context: &PullDiagnosticsContext,
        source_identity: perl_lsp_rs_core::tooling::perl_critic::CriticSourceIdentity,
        core_diagnostics: &[InternalDiagnostic],
    ) -> PendingPullCriticContribution {
        use perl_lsp_rs_core::providers::diagnostics::critic_overlap_observations;
        use perl_lsp_rs_core::tooling::perl_critic::{
            NativeCriticService, NativeCriticSubject, RunGate,
        };

        // Core lint emitters that declared a reviewed critic overlap
        // observation surrender their ordinary diagnostic here (#11918): the
        // logical row comes out of the same normalization inside the service,
        // merged with the native alias and carrying both contributor
        // identities. Read non-destructively first (#9062): carriers are
        // surrendered only after a publishable normalized replacement exists,
        // so an unpublishable run retains every independent core row.
        let overlap_observations = critic_overlap_observations(core_diagnostics);

        // The accepted state was snapshotted when the report subject was
        // composed; configuration can move while rules evaluate. The service
        // re-checks this gate at its settlement barrier, so a run whose policy
        // moved underneath it settles `Stale` and publishes nothing (#13304).
        let accepted_state_is_current = || context.accepted_state_currentness.holds();

        let snapshot = context.accepted_critic_snapshot.clone();
        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            &uri.to_string(),
            source_identity,
            ast,
            content,
            snapshot.state().clone(),
            overlap_observations,
            RunGate::open(),
            RunGate::new(&accepted_state_is_current),
        ));

        PendingPullCriticContribution { snapshot, run }
    }

    /// Commit or withhold the staged Critic contribution at the report
    /// boundary. This is the only pull path allowed to drain overlap carriers
    /// or append normalized native rows.
    fn finalize_pending_diagnostics(
        &self,
        uri: &Uri,
        content: &str,
        context: &PullDiagnosticsContext,
        mut pending: PendingPullDiagnostics,
    ) -> (Vec<LspDiagnostic>, bool) {
        use perl_lsp_rs_core::providers::diagnostics::take_critic_overlap_observations;

        let mut critic_subject_current = context.accepted_state_currentness.holds();
        if let Some(critic) = pending.critic {
            critic_subject_current &= critic.snapshot == context.accepted_critic_snapshot;
            if !critic.run.is_publishable() {
                critic_subject_current = false;
            } else if critic_subject_current {
                if critic.run.superseded_overlap_carriers() {
                    take_critic_overlap_observations(&mut pending.core);
                }
                pending.projected.extend(critic.run.findings().iter().map(|finding| {
                    self.normalized_finding_to_lsp_diagnostic(uri, content, finding)
                }));
            }
        }

        let mut diagnostics: Vec<LspDiagnostic> = pending
            .core
            .into_iter()
            .map(|diagnostic| {
                self.to_lsp_diagnostic_with_context(uri, content, diagnostic, context)
            })
            .collect();
        diagnostics.extend(pending.projected);
        (diagnostics, critic_subject_current)
    }

    fn normalized_finding_to_lsp_diagnostic(
        &self,
        uri: &Uri,
        text: &str,
        finding: &perl_lsp_rs_core::tooling::perl_critic::NormalizedCriticFinding,
    ) -> LspDiagnostic {
        let range =
            lsp_range_from_offsets(text, finding.range().start.byte, finding.range().end.byte);
        let severity = Some(native_critic_severity_to_lsp(finding.severity()));
        let code = Some(NumberOrString::String(finding.public_code().to_string()));
        let data = Some(serde_json::json!({
            "code": finding.public_code(),
            "category": finding.category().map(|category| format!("{category:?}")).unwrap_or_else(|| "Other".to_string()),
            "fixable": finding.has_available_fix(),
            "tags": [],
            "suppressionKey": finding.public_code(),
            "explanation": finding.explanation(),
        }));

        // #12004: render the contributing ordinary producer's user-visible
        // remediation exactly the way the ordinary-row path renders it. The
        // composition is owned by the normalized finding so the code-action
        // surface renders the identical text (#13304).
        let message = finding.user_visible_message();
        let remediation_notes: Vec<RelatedInformation> = finding
            .remediation_related_information()
            .iter()
            .map(|note| RelatedInformation {
                location: (note.range.start.byte, note.range.end.byte),
                message: note.message.clone(),
            })
            .collect();
        let related_information = to_lsp_related_information(uri, text, &remediation_notes);

        LspDiagnostic {
            range,
            severity,
            code,
            code_description: None,
            source: Some("perl-lsp".to_string()),
            message,
            related_information,
            tags: None,
            data,
        }
    }

    fn collect_diagnostics_for_state_with_context(
        &self,
        uri: &Uri,
        doc_state: &DocumentState,
        context: &PullDiagnosticsContext,
    ) -> PendingPullDiagnostics {
        // No published snapshot at all (e.g. a document that never parsed --
        // large-file/binary/template guards) behaves like the pre-migration
        // default: no AST, no parse errors, nothing to report.
        let Some(parsed) = doc_state.current_parsed() else {
            return PendingPullDiagnostics::projected(Vec::new());
        };
        if let Some(ast) = parsed.ast() {
            let parse_errors = parsed.parse_errors();
            let provider = DiagnosticsProvider::new();
            let source_path =
                url::Url::parse(&uri.to_string()).ok().and_then(|value| value.to_file_path().ok());
            // Build the baseline include paths (configured + PERL5LIB, without lexical
            // `use lib`/`no lib`). The resolver re-evaluates lexical paths per use-site
            // offset so that `no lib` cancellations that precede each `use` statement
            // are respected.
            let base_include_paths = context.include_paths.clone();
            let doc_text = doc_state.text_arc.to_string();
            let source_path_ref = source_path.as_deref();

            // Extract lexical `use lib` / `no lib` operations once per diagnostic
            // cycle (#1683).
            let workspace_root = context
                .workspace_root
                .as_deref()
                .or_else(|| source_path_ref.and_then(std::path::Path::parent))
                .unwrap_or(std::path::Path::new("."));
            let file_dir = source_path_ref.and_then(std::path::Path::parent);
            let use_lib_ops = extract_use_lib_operations_with_offsets(&doc_text);

            // Position-aware resolver: for each `use Module` statement, recompute the
            // effective include paths at that statement's byte offset so that `no lib`
            // directives appearing before it cancel the appropriate `use lib` paths.
            let resolver = |module: &str, use_site_offset: usize| {
                let paths = self.effective_include_paths_at_offset(
                    &base_include_paths,
                    &use_lib_ops,
                    workspace_root,
                    file_dir,
                    use_site_offset,
                );
                self.resolve_module_with_paths(module, &paths, source_path_ref)
            };

            // Search context for PL701 display: compute once for the whole file (end
            // offset) so the diagnostic message shows what paths were searched overall.
            let search_paths: Vec<String> = self.effective_include_paths(
                &base_include_paths,
                &use_lib_ops,
                workspace_root,
                file_dir,
            );
            let uri_str = uri.to_string();

            // Wire workspace semantic queries when available (pull-state path).
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            let core_diagnostics: Vec<_> = {
                let semantic_diags = context.workspace_index.as_ref().and_then(|workspace_index| {
                    workspace_index.with_semantic_queries_for_uri(&uri_str, |file_id, queries| {
                        provider.get_diagnostics_with_path_and_semantics(
                            ast,
                            parse_errors,
                            &doc_state.text,
                            Some(&resolver),
                            &search_paths,
                            source_path.as_deref(),
                            file_id,
                            &queries,
                        )
                    })
                });
                semantic_diags.unwrap_or_else(|| {
                    provider.get_diagnostics_with_path(
                        ast,
                        parse_errors,
                        &doc_state.text,
                        Some(&resolver),
                        &search_paths,
                        source_path.as_deref(),
                    )
                })
            };
            #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
            let core_diagnostics: Vec<_> = provider.get_diagnostics_with_path(
                ast,
                parse_errors,
                &doc_state.text,
                Some(&resolver),
                &search_paths,
                source_path.as_deref(),
            );

            let mut core_diagnostics = core_diagnostics;
            // Critic composition over producer-owned core rows first (#11918);
            // surviving rows map to LSP afterwards.
            let critic = self.evaluate_policy_critic(
                uri,
                ast,
                &doc_state.text,
                context,
                perl_lsp_rs_core::tooling::perl_critic::critic_source_identity_for_uri(
                    &uri.to_string(),
                    doc_state.current_generation(),
                ),
                &core_diagnostics,
            );

            // Add dead code diagnostics from workspace-wide symbol analysis
            #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
            {
                if let Some(ref workspace_index) = context.workspace_index {
                    let dead_code_diags =
                        perl_lsp_rs_core::providers::diagnostics::detect_dead_code(
                            workspace_index,
                            &uri_str,
                            &doc_state.text,
                            &doc_state.line_starts,
                        );
                    // Convert dead code diagnostics to LSP format
                    for d in dead_code_diags {
                        core_diagnostics.push(d);
                    }
                }
            }

            PendingPullDiagnostics {
                core: core_diagnostics,
                projected: Vec::new(),
                critic: Some(critic),
            }
        } else if parsed.parse_errors().is_empty() {
            PendingPullDiagnostics::projected(Vec::new())
        } else {
            PendingPullDiagnostics::projected(
                parsed
                    .parse_errors()
                    .iter()
                    .map(|error| {
                        self.parse_error_to_diagnostic_with_context(
                            uri,
                            &doc_state.text,
                            error,
                            context,
                        )
                    })
                    .collect(),
            )
        }
    }

    fn build_unchanged_report(&self, result_id: String) -> DocumentDiagnosticReport {
        DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
            related_documents: None,
            unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport { result_id },
        })
    }

    fn build_full_report(
        &self,
        result_id: Option<PullReportResultId>,
        diagnostics: Vec<LspDiagnostic>,
    ) -> DocumentDiagnosticReport {
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                // `None` is the honest full report for a valid-but-not-reusable
                // subject (#7480): LSP result IDs are optional.
                result_id: result_id.map(PullReportResultId::into_string),
                items: diagnostics,
            },
        })
    }

    fn to_workspace_report(
        &self,
        uri: Uri,
        version: Option<i32>,
        report: DocumentDiagnosticReport,
    ) -> WorkspaceDocumentDiagnosticReport {
        let version = version.map(i64::from);

        match report {
            DocumentDiagnosticReport::Full(full) => {
                let RelatedFullDocumentDiagnosticReport { full_document_diagnostic_report, .. } =
                    full;
                WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                    uri,
                    version,
                    full_document_diagnostic_report,
                })
            }
            DocumentDiagnosticReport::Unchanged(unchanged) => {
                let RelatedUnchangedDocumentDiagnosticReport {
                    unchanged_document_diagnostic_report,
                    ..
                } = unchanged;
                WorkspaceDocumentDiagnosticReport::Unchanged(
                    WorkspaceUnchangedDocumentDiagnosticReport {
                        uri,
                        version,
                        unchanged_document_diagnostic_report,
                    },
                )
            }
        }
    }

    /// Convert internal diagnostic to LSP diagnostic with context support.
    fn to_lsp_diagnostic_with_context(
        &self,
        uri: &Uri,
        text: &str,
        diagnostic: InternalDiagnostic,
        context: &PullDiagnosticsContext,
    ) -> LspDiagnostic {
        let range = lsp_range_from_offsets(text, diagnostic.range.0, diagnostic.range.1);
        let severity = Some(to_lsp_severity(diagnostic.severity));
        let code = diagnostic.code.map(NumberOrString::String);
        let code_for_source = code.clone();
        let related_information =
            to_lsp_related_information(uri, text, &diagnostic.related_information);

        // Collect tag strings before diagnostic is partially moved by the suggestion match
        let tag_strings: Vec<String> = diagnostic
            .tags
            .iter()
            .map(|t| match t {
                InternalDiagnosticTag::Unnecessary => "Unnecessary".to_string(),
                InternalDiagnosticTag::Deprecated => "Deprecated".to_string(),
                // Forward-compatible fallback for future variants (#2898)
                _ => "Unnecessary".to_string(),
            })
            .collect();
        let tags = to_lsp_tags(&diagnostic.tags);
        let fixable = diagnostic.fixable;

        // Append the suggestion to the message when present so users see it inline
        let message = match diagnostic.suggestion {
            Some(ref suggestion) => format!("{}\nSuggestion: {}", diagnostic.message, suggestion),
            None => diagnostic.message.clone(),
        };

        let data = code.as_ref().and_then(|c| {
            if let NumberOrString::String(code_str) = c {
                let category = DiagnosticCode::parse_code(code_str)
                    .map(|dc| format!("{:?}", dc.category()))
                    .unwrap_or_else(|| {
                        // Check if it's a perlcritic policy
                        if code_str.contains("::") {
                            "PerlCritic".to_string()
                        } else {
                            "Other".to_string()
                        }
                    });
                let data_obj = DiagnosticData {
                    code: code_str.clone(),
                    category,
                    fixable,
                    tags: tag_strings.clone(),
                };

                // Add LSP 3.18 markup message support if enabled
                if context.markup_message_support {
                    let markdown = format!("**{}**: {}", code_str, diagnostic.message);
                    return serde_json::to_value(data_obj).ok().map(|mut v| {
                        v["messageMarkup"] = serde_json::json!({
                            "kind": "markdown",
                            "value": markdown
                        });
                        v
                    });
                }

                serde_json::to_value(data_obj).ok()
            } else {
                None
            }
        });

        let code_description = lsp_code_description(code.as_ref());

        LspDiagnostic {
            range,
            severity,
            code,
            code_description,
            source: diagnostic_source(code_for_source.as_ref()),
            message,
            related_information,
            tags,
            data,
        }
    }

    #[cfg(test)]
    fn parse_error_to_diagnostic(
        &self,
        uri: &Uri,
        text: &str,
        error: &ParseError,
    ) -> LspDiagnostic {
        let context = PullDiagnosticsContext::new();
        self.parse_error_to_diagnostic_with_context(uri, text, error, &context)
    }

    fn parse_error_to_diagnostic_with_context(
        &self,
        uri: &Uri,
        text: &str,
        error: &ParseError,
        context: &PullDiagnosticsContext,
    ) -> LspDiagnostic {
        // Keep message formatting local, but let parser-core own source placement.
        let base_message = match error {
            ParseError::UnexpectedToken { expected, found, .. } => {
                format!("Expected {expected}, found {found}")
            }
            ParseError::SyntaxError { message, .. } | ParseError::Advisory { message, .. } => {
                message.clone()
            }
            ParseError::Recovered { .. } => error.to_string(),
            ParseError::UnexpectedEof => "Unexpected end of input".to_string(),
            ParseError::LexerError { message } => message.clone(),
            ParseError::RecursionLimit
            | ParseError::InvalidNumber { .. }
            | ParseError::InvalidString
            | ParseError::UnclosedDelimiter { .. }
            | ParseError::InvalidRegex { .. }
            | ParseError::NestingTooDeep { .. }
            | ParseError::Cancelled => error.to_string(),
            // `ParseError` is `#[non_exhaustive]`, so a wildcard is mandatory
            // outside perl-parser-core. It is safe here because this match only
            // selects message text, and `Display` is defined for every variant,
            // present and future. Source placement deliberately does not come
            // from this match — it comes from `resolved_diagnostic_anchor`,
            // whose exhaustiveness is enforced inside the defining crate, so a
            // future variant cannot silently anchor a diagnostic at byte 0.
            _ => error.to_string(),
        };

        // Append the suggestion inline so users see actionable hints in the fallback path,
        // matching the behaviour of to_lsp_diagnostic for the AST-present path.
        let suggestion =
            perl_lsp_rs_core::providers::diagnostics::build_parse_error_hint(error, &base_message);
        let message = match suggestion.as_deref() {
            Some(hint) => format!("{base_message}\nSuggestion: {hint}"),
            None => base_message,
        };

        let offset = resolved_parse_diagnostic_offset(error, text);
        let end_offset = offset.saturating_add(1).min(text.len());
        let range = lsp_range_from_offsets(text, offset, end_offset);

        let code = parse_error_code(error);
        let code_str = code.as_str();

        let data_obj = DiagnosticData {
            code: code_str.to_string(),
            category: format!("{:?}", code.category()),
            fixable: is_fixable_diagnostic(code_str),
            tags: vec![],
        };

        // Add LSP 3.18 markup message support if enabled
        let data = if context.markup_message_support {
            let markdown = format!("**{}**: {}", code_str, message);
            serde_json::to_value(data_obj).ok().map(|mut v| {
                v["messageMarkup"] = serde_json::json!({
                    "kind": "markdown",
                    "value": markdown
                });
                v
            })
        } else {
            serde_json::to_value(data_obj).ok()
        };

        LspDiagnostic {
            range,
            severity: Some(to_lsp_severity(parse_error_severity(error))),
            code: Some(NumberOrString::String(code_str.to_string())),
            code_description: lsp_code_description_from_str(code_str),
            source: Some("perl-lsp".to_string()),
            message,
            related_information: to_lsp_related_information(uri, text, &[]),
            tags: None,
            data,
        }
    }
}

fn lsp_code_description(code: Option<&NumberOrString>) -> Option<CodeDescription> {
    match code {
        Some(NumberOrString::String(code_str)) => lsp_code_description_from_str(code_str),
        _ => None,
    }
}

fn lsp_code_description_from_str(code_str: &str) -> Option<CodeDescription> {
    DiagnosticCode::parse_code(code_str)
        .and_then(|code| code.documentation_url())
        .and_then(|url| url.parse::<Uri>().ok())
        .map(|href| CodeDescription { href })
}

fn lsp_range_from_offsets(text: &str, start: usize, end: usize) -> Range {
    let (start, end) = if start <= end { (start, end) } else { (end, start) };
    let (start_line, start_col) = offset_to_utf16_line_col(text, start);
    let (end_line, end_col) = offset_to_utf16_line_col(text, end);
    Range::new(Position::new(start_line, start_col), Position::new(end_line, end_col))
}

fn resolved_parse_diagnostic_offset(error: &ParseError, text: &str) -> usize {
    match error.resolved_diagnostic_anchor(text) {
        ResolvedParseDiagnosticAnchor::Exact(offset) => offset,
        ResolvedParseDiagnosticAnchor::EndOfInput(offset) => offset,
        ResolvedParseDiagnosticAnchor::NoSource => 0,
        ResolvedParseDiagnosticAnchor::InvalidOffset { reported, source_len } => {
            tracing::error!(
                reported,
                source_len,
                "parser returned an out-of-range diagnostic anchor"
            );
            source_len
        }
        ResolvedParseDiagnosticAnchor::InvalidUtf8Boundary { reported, source_len } => {
            tracing::error!(
                reported,
                source_len,
                "parser returned a UTF-8 interior diagnostic anchor"
            );
            source_len
        }
    }
}

fn to_lsp_severity(severity: InternalDiagnosticSeverity) -> LspDiagnosticSeverity {
    match severity {
        InternalDiagnosticSeverity::Error => LspDiagnosticSeverity::ERROR,
        InternalDiagnosticSeverity::Warning => LspDiagnosticSeverity::WARNING,
        InternalDiagnosticSeverity::Information => LspDiagnosticSeverity::INFORMATION,
        InternalDiagnosticSeverity::Hint => LspDiagnosticSeverity::HINT,
        // Forward-compatible fallback for future variants (#2898)
        _ => LspDiagnosticSeverity::ERROR,
    }
}

fn native_critic_severity_to_lsp(severity: Severity) -> LspDiagnosticSeverity {
    severity.to_diagnostic_severity()
}

fn to_lsp_tags(tags: &[InternalDiagnosticTag]) -> Option<Vec<LspDiagnosticTag>> {
    if tags.is_empty() {
        return None;
    }

    Some(
        tags.iter()
            .map(|tag| match tag {
                InternalDiagnosticTag::Unnecessary => LspDiagnosticTag::UNNECESSARY,
                InternalDiagnosticTag::Deprecated => LspDiagnosticTag::DEPRECATED,
                // Forward-compatible fallback for future variants (#2898)
                _ => LspDiagnosticTag::UNNECESSARY,
            })
            .collect(),
    )
}

fn to_lsp_related_information(
    uri: &Uri,
    text: &str,
    infos: &[RelatedInformation],
) -> Option<Vec<DiagnosticRelatedInformation>> {
    if infos.is_empty() {
        return None;
    }

    Some(
        infos
            .iter()
            .map(|info| DiagnosticRelatedInformation {
                location: Location {
                    uri: uri.clone(),
                    range: lsp_range_from_offsets(text, info.location.0, info.location.1),
                },
                message: info.message.clone(),
            })
            .collect(),
    )
}

/// Structured data attached to each LSP diagnostic for client integration.
///
/// Serialized into the `data` field of `lsp_types::Diagnostic` so that clients can
/// identify fixable diagnostics, filter by category, and integrate with code actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticData {
    /// The diagnostic code string (e.g., "PL001")
    pub code: String,
    /// Category name derived from `DiagnosticCode::category()` (e.g., "Parser", "StrictWarnings")
    pub category: String,
    /// Whether a quick-fix code action is currently available for this diagnostic
    pub fixable: bool,
    /// Tag names (e.g., ["Unnecessary"], ["Deprecated"])
    pub tags: Vec<String>,
}

/// Returns `true` when a quick-fix code action exists for the given diagnostic code.
///
/// The authoritative source is `crates/perl-lsp-code-actions/src/code_actions.rs`.
fn is_fixable_diagnostic(code: &str) -> bool {
    if matches!(
        code,
        "TestingAndDebugging::RequireUseStrict"
            | "TestingAndDebugging::RequireUseWarnings"
            | "native.testing.require_use_strict"
            | "native.testing.require_use_warnings"
            | "native.common.deprecated_defined"
            | "native.common.undef_comparison"
            | "native.common.unreachable_code"
            | "native.io.bareword_filehandle"
            | "native.io.two_arg_open"
            | "InputOutput::ProhibitBarewordFileHandles"
            | "InputOutput::RequireBriefOpen"
            | "InputOutput::RequireThreeArgOpen"
            | "Variables::ProhibitUnusedVariables"
    ) {
        return true;
    }

    matches!(
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

/// Determine the diagnostic source based on the code.
///
/// Source taxonomy (see issue #4627):
/// - `perl-lsp` — all built-in diagnostics: parse errors (`PL***` codes),
///   built-in lints, and native critic findings (`native.*` codes).
/// - `perl-lsp-critic` — findings from the external `perlcritic` binary,
///   whose codes are fully-qualified Perl::Critic policy names (`Policy::Name`).
///
/// This resolves the former fragmentation across four strings
/// (`perl-lsp`, `perl-lsp-critic`, `perlcritic`, `perl-parser`) so that the
/// same logical finding carries the same source regardless of transport path.
fn diagnostic_source(code: Option<&NumberOrString>) -> Option<String> {
    match code {
        Some(NumberOrString::String(code_str)) => {
            // External Perl::Critic policies contain "::" and are not in our
            // DiagnosticCode enum. Native critic codes (`native.*`) and built-in
            // lint codes (`PL***`) are both built-in and use `perl-lsp`.
            if code_str.contains("::") && DiagnosticCode::parse_code(code_str).is_none() {
                Some("perl-lsp-critic".to_string())
            } else {
                Some("perl-lsp".to_string())
            }
        }
        _ => Some("perl-lsp".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DocumentDiagnosticReport, NumberOrString};
    /// Derive an accepted critic snapshot through the #8253 authority from the
    /// raw siblings a test wants to exercise (#9062). Mirrors exactly what a
    /// live `ServerConfig` snapshot does in production.
    fn accepted_state(
        profile: &str,
        severity: u8,
        include: Vec<String>,
        exclude: Vec<String>,
    ) -> AcceptedCriticSnapshot {
        let config = perl_lsp_rs_core::config::ServerConfig {
            native_critic_profile: profile.to_string(),
            perlcritic_severity: severity,
            native_critic_include: include,
            native_critic_exclude: exclude,
            ..perl_lsp_rs_core::config::ServerConfig::default()
        };
        AcceptedCriticSnapshot::capture(&config, Some(PROVIDER_DEFAULT_ROOT_AUTHORITY))
    }

    fn strict_accepted_state(severity: u8) -> AcceptedCriticSnapshot {
        accepted_state("strict", severity, Vec::new(), Vec::new())
    }

    fn get_full_items(report: DocumentDiagnosticReport) -> Vec<lsp_types::Diagnostic> {
        match report {
            DocumentDiagnosticReport::Full(full) => full.full_document_diagnostic_report.items,
            _ => vec![],
        }
    }

    #[test]
    fn diagnostic_data_for_parse_error() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let items =
            get_full_items(provider.get_document_diagnostics(&uri, "my $x = ;", None, None));
        assert!(!items.is_empty());
        // Find the PL001 (ParseError) diagnostic — ordering may vary depending on
        // which lints run first (e.g., PL100 MissingStrict may precede PL001).
        let diag = items
            .iter()
            .find(|d| d.data.as_ref().and_then(|v| v["code"].as_str()) == Some("PL001"))
            .ok_or("expected a PL001 ParseError diagnostic in the results")?;
        let data = diag.data.as_ref().ok_or("data should be populated")?;
        assert_eq!(data["code"], "PL001");
        assert_eq!(data["category"], "Parser");
        assert_eq!(data["fixable"], true);
        let code_description = diag
            .code_description
            .as_ref()
            .ok_or("codeDescription should be populated for PL001")?;
        let expected_url =
            DiagnosticCode::ParseError.documentation_url().ok_or("PL001 should have docs")?;
        assert_eq!(code_description.href.to_string(), expected_url);
        let tags = data["tags"].as_array().ok_or("tags should be an array")?;
        assert!(tags.is_empty());
        Ok(())
    }

    #[test]
    fn diagnostic_data_none_when_no_code() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let report = provider.get_document_diagnostics(&uri, "my $x = 1;\n", None, None);
        let items = get_full_items(report);
        // Any diagnostic without a code must also have data: None
        assert!(items.iter().all(|d| d.code.is_some() || d.data.is_none()));
        Ok(())
    }

    #[test]
    fn diagnostic_data_for_missing_strict() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let code = "print 'hello';\n";
        let items = get_full_items(provider.get_document_diagnostics(&uri, code, None, None));
        let diag = items
            .iter()
            .find(|d| {
                d.code.as_ref().map(|c| matches!(c, NumberOrString::String(s) if s == "PL100"))
                    == Some(true)
            })
            .ok_or("expected PL100 (missing strict) diagnostic for bare print statement")?;
        let data = diag.data.as_ref().ok_or("data should be Some for PL100")?;
        assert_eq!(data["code"], "PL100");
        assert_eq!(data["category"], "StrictWarnings");
        assert_eq!(data["fixable"], true);
        let code_description = diag
            .code_description
            .as_ref()
            .ok_or("codeDescription should be populated for PL100")?;
        let expected_url =
            DiagnosticCode::MissingStrict.documentation_url().ok_or("PL100 should have docs")?;
        assert_eq!(code_description.href.to_string(), expected_url);
        Ok(())
    }

    #[test]
    fn code_description_is_catalog_backed_and_fail_closed() -> Result<(), Box<dyn std::error::Error>>
    {
        let parse_error_description =
            lsp_code_description_from_str("PL001").ok_or("PL001 should have codeDescription")?;
        let parse_error_url =
            DiagnosticCode::ParseError.documentation_url().ok_or("PL001 should have docs")?;
        assert_eq!(parse_error_description.href.to_string(), parse_error_url);

        assert!(lsp_code_description_from_str("TestingAndDebugging::RequireUseStrict").is_none());
        assert!(lsp_code_description_from_str("PC101").is_none());
        assert!(lsp_code_description(Some(&NumberOrString::Number(101))).is_none());
        assert!(lsp_code_description(None).is_none());
        Ok(())
    }

    #[test]
    fn diagnostic_data_fixable_true_for_variable_redeclaration()
    -> Result<(), Box<dyn std::error::Error>> {
        // PL105 (VariableRedeclaration) offers a quick-fix that removes the duplicate `my`,
        // so the enriched diagnostic data must advertise it as fixable.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        // Redeclare $x in the same scope to trigger PL105
        let code = "use strict; use warnings; my $x = 1; my $x = 2;\n";
        let items = get_full_items(provider.get_document_diagnostics(&uri, code, None, None));
        if let Some(diag) = items.iter().find(|d| {
            d.code.as_ref().map(|c| matches!(c, NumberOrString::String(s) if s == "PL105"))
                == Some(true)
        }) {
            let data = diag.data.as_ref().ok_or("data should be Some for PL105")?;
            assert_eq!(data["code"], "PL105");
            assert_eq!(data["fixable"], true, "PL105 now has a quick-fix; fixable must stay true");
        }
        // Also verify that every diagnostic with a code has a valid data object
        for d in &items {
            if d.code.is_some() {
                let data = d.data.as_ref().ok_or("data must be Some when code is Some")?;
                assert!(data["fixable"].is_boolean(), "fixable must always be a boolean");
            }
        }
        Ok(())
    }

    #[test]
    fn diagnostic_data_is_valid_json_object() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let items =
            get_full_items(provider.get_document_diagnostics(&uri, "my $x = ;", None, None));
        for diag in &items {
            if diag.code.is_some() {
                let data = diag.data.as_ref().ok_or("data must be Some when code is Some")?;
                assert!(data.is_object(), "data must be a JSON object");
                assert!(data["code"].is_string());
                assert!(data["category"].is_string());
                assert!(data["fixable"].is_boolean());
                assert!(data["tags"].is_array());
            }
        }
        Ok(())
    }

    #[test]
    fn invalid_prototype_syntax_error_maps_to_pl302_warning()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let diagnostic = provider.parse_error_to_diagnostic(
            &uri,
            "sub foo (XYZ) {}",
            &ParseError::SyntaxError {
                location: 8,
                message: "Invalid prototype character(s) 'X'".to_string(),
            },
        );

        assert_eq!(diagnostic.code, Some(NumberOrString::String("PL302".to_string())));
        assert_eq!(diagnostic.severity, Some(LspDiagnosticSeverity::WARNING));
        let data = diagnostic.data.as_ref().ok_or("data should be populated")?;
        assert_eq!(data["code"], "PL302");
        Ok(())
    }

    #[test]
    fn pull_diagnostic_preserves_recovered_anchor() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let diagnostic = provider.parse_error_to_diagnostic(
            &uri,
            "ab + ;",
            &ParseError::Recovered {
                site: RecoverySite::InfixRhs,
                kind: RecoveryKind::MissingOperand,
                location: 2,
            },
        );

        assert_eq!(diagnostic.range.start, Position::new(0, 2));
        Ok(())
    }

    #[test]
    fn pull_diagnostic_rejects_out_of_range_anchor() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let diagnostic = provider.parse_error_to_diagnostic(
            &uri,
            "abc",
            &ParseError::SyntaxError { location: 42, message: "bad syntax".to_string() },
        );

        assert_eq!(diagnostic.range.start, Position::new(0, 3));
        Ok(())
    }

    #[test]
    fn pull_diagnostic_rejects_utf8_interior_anchor() -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let diagnostic = provider.parse_error_to_diagnostic(
            &uri,
            "💖",
            &ParseError::SyntaxError { location: 1, message: "bad syntax".to_string() },
        );

        assert_eq!(diagnostic.range.start, Position::new(0, 2));
        Ok(())
    }

    #[test]
    fn perlcritic_policy_codes_are_marked_fixable_in_diagnostic_data() {
        assert!(is_fixable_diagnostic("PL502"));
        assert!(is_fixable_diagnostic("PL503"));
        assert!(is_fixable_diagnostic("TestingAndDebugging::RequireUseStrict"));
        assert!(is_fixable_diagnostic("TestingAndDebugging::RequireUseWarnings"));
        assert!(is_fixable_diagnostic("native.common.undef_comparison"));
        assert!(is_fixable_diagnostic("InputOutput::RequireThreeArgOpen"));
        assert!(is_fixable_diagnostic("Variables::ProhibitUnusedVariables"));
    }

    #[test]
    fn native_critic_engine_emits_opt_in_lsp_diagnostics() -> Result<(), Box<dyn std::error::Error>>
    {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();
        context.accepted_critic_snapshot = strict_accepted_state(3);
        context.perlcritic_severity = 3;

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\nmy $x = 2;\nmy $unused = 3;\nmy $shadow = 4;\nmy $outer_param = 0;\nmy $cond = 0;\nmy $path = 'file.txt';\nmy @items = (1, 2);\nmy $eval_code = 'print 1';\nmy $cmd_out = `ls`;\nmy $qx_out = qx(date);\nmy $readpipe_out = readpipe($path);\nif ($cond = 1) { print $cond; }\nif (defined @items) { print @items; }\nif ($path == undef) { print $path; }\neval { die $path; };\nif ($@) { warn $@; }\nprintf \"%s %s\", $path;\nopen(FH, '<', 'file.txt');\nopen(my $log_fh, $path);\nopen(my $pipe_fh, '-|', 'ls');\neval $eval_code;\nsystem($path);\nexec('ls', '-la');\nprint $log_fh;\nprint $pipe_fh;\n{ my $shadow = 5; print $shadow; }\nsub helper($used_param, $unused_param) { return $used_param; }\nsub duplicate_param($dup_param, $dup_param) { return $dup_param; }\nsub shadow_param($outer_param) { return $outer_param; }\nsub unreachable_helper { return 1; my $dead_after_return = 2; }\nprint $x + $shadow + $outer_param + $cond + $cmd_out + $qx_out + $readpipe_out;\n=head1 NAME\n\nDemo\n\n=cut\n",
            None,
            &context,
            None,
        ));

        let strict = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_strict"))
            })
            .ok_or("expected native strict finding")?;
        assert_eq!(strict.source.as_deref(), Some("perl-lsp"));
        assert_eq!(strict.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(strict.message, "Code does not use strict");
        let data = strict.data.as_ref().ok_or("native critic data should be populated")?;
        assert_eq!(data["code"], "native.testing.require_use_strict");
        assert_eq!(data["suppressionKey"], "native.testing.require_use_strict");
        assert_eq!(data["fixable"], true);

        let warnings = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_warnings"))
            })
            .ok_or("expected native warnings finding")?;
        assert_eq!(warnings.source.as_deref(), Some("perl-lsp"));

        let assignment = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.common.assignment_in_condition"))
            })
            .ok_or("expected native assignment-in-condition finding")?;
        assert_eq!(assignment.source.as_deref(), Some("perl-lsp"));
        assert_eq!(assignment.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(assignment.message, "Assignment in condition - did you mean '=='?");
        let data = assignment
            .data
            .as_ref()
            .ok_or("native assignment-in-condition data should be populated")?;
        assert_eq!(data["code"], "native.common.assignment_in_condition");
        assert_eq!(data["suppressionKey"], "native.common.assignment_in_condition");
        assert_eq!(data["fixable"], true);

        let printf_format = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.printf_format_arity"),
                )
            })
            .ok_or("expected native printf format arity finding")?;
        assert_eq!(printf_format.source.as_deref(), Some("perl-lsp"));
        assert_eq!(printf_format.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(
            printf_format.message,
            "`printf` format string has 2 specifiers but 1 argument supplied"
        );
        let data = printf_format
            .data
            .as_ref()
            .ok_or("native printf format arity data should be populated")?;
        assert_eq!(data["code"], "native.common.printf_format_arity");
        assert_eq!(data["suppressionKey"], "native.common.printf_format_arity");
        assert_eq!(data["fixable"], false);

        let deprecated_defined = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.deprecated_defined"),
                )
            })
            .ok_or("expected native deprecated-defined finding")?;
        assert_eq!(deprecated_defined.source.as_deref(), Some("perl-lsp"));
        assert_eq!(deprecated_defined.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(deprecated_defined.message, "Use of 'defined @items' is deprecated");
        let data = deprecated_defined
            .data
            .as_ref()
            .ok_or("native deprecated-defined data should be populated")?;
        assert_eq!(data["code"], "native.common.deprecated_defined");
        assert_eq!(data["suppressionKey"], "native.common.deprecated_defined");
        assert_eq!(data["fixable"], true);

        // #11918: the literal-undef comparison merges with its built-in PL404
        // observation into one logical row presented with the built-in
        // spelling; the native spelling rides inside the row, not beside it.
        let undef_comparison = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL404"),
                )
            })
            .ok_or("expected merged undef-comparison row")?;
        assert_eq!(undef_comparison.source.as_deref(), Some("perl-lsp"));
        assert!(
            undef_comparison.message.contains("defined"),
            "merged row carries the producer message: {}",
            undef_comparison.message
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.undef_comparison"),
                )
            }),
            "native undef-comparison spelling must not appear as a separate row"
        );

        let stale_dollar_at = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.stale_dollar_at"),
                )
            })
            .ok_or("expected native stale-dollar-at finding")?;
        assert_eq!(stale_dollar_at.source.as_deref(), Some("perl-lsp"));
        assert_eq!(stale_dollar_at.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(stale_dollar_at.message, "Checking $@ after eval can observe a stale error");
        let data = stale_dollar_at
            .data
            .as_ref()
            .ok_or("native stale-dollar-at data should be populated")?;
        assert_eq!(data["code"], "native.common.stale_dollar_at");
        assert_eq!(data["suppressionKey"], "native.common.stale_dollar_at");
        assert_eq!(data["fixable"], false);

        let unreachable_code = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.unreachable_code"),
                )
            })
            .ok_or("expected native unreachable-code finding")?;
        assert_eq!(unreachable_code.source.as_deref(), Some("perl-lsp"));
        assert_eq!(unreachable_code.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(unreachable_code.message, "Unreachable code: this statement cannot be executed");
        let data = unreachable_code
            .data
            .as_ref()
            .ok_or("native unreachable-code data should be populated")?;
        assert_eq!(data["code"], "native.common.unreachable_code");
        assert_eq!(data["suppressionKey"], "native.common.unreachable_code");
        assert_eq!(data["fixable"], true);

        let bareword_filehandle = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.io.bareword_filehandle"))
            })
            .ok_or("expected native bareword filehandle finding")?;
        assert_eq!(bareword_filehandle.source.as_deref(), Some("perl-lsp"));
        assert_eq!(bareword_filehandle.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(bareword_filehandle.message, "Bareword filehandle 'FH' should be lexical");
        let data = bareword_filehandle
            .data
            .as_ref()
            .ok_or("native bareword filehandle data should be populated")?;
        assert_eq!(data["code"], "native.io.bareword_filehandle");
        assert_eq!(data["suppressionKey"], "native.io.bareword_filehandle");
        assert_eq!(data["fixable"], true);

        let two_arg_open = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.io.two_arg_open"))
            })
            .ok_or("expected native two-arg open finding")?;
        assert_eq!(two_arg_open.source.as_deref(), Some("perl-lsp"));
        assert_eq!(two_arg_open.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(two_arg_open.message, "Two-argument open should use an explicit mode");
        let data =
            two_arg_open.data.as_ref().ok_or("native two-arg open data should be populated")?;
        assert_eq!(data["code"], "native.io.two_arg_open");
        assert_eq!(data["suppressionKey"], "native.io.two_arg_open");
        assert_eq!(data["fixable"], true);

        let pipe_open = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.io.pipe_open"))
            })
            .ok_or("expected native pipe-open finding")?;
        assert_eq!(pipe_open.source.as_deref(), Some("perl-lsp"));
        assert_eq!(pipe_open.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(pipe_open.message, "Pipe-open executes a shell command");
        let data = pipe_open.data.as_ref().ok_or("native pipe-open data should be populated")?;
        assert_eq!(data["code"], "native.io.pipe_open");
        assert_eq!(data["suppressionKey"], "native.io.pipe_open");
        assert_eq!(data["fixable"], false);

        let unchecked_open_close = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.io.unchecked_open_close"),
                )
            })
            .ok_or("expected native unchecked open/close finding")?;
        assert_eq!(unchecked_open_close.source.as_deref(), Some("perl-lsp"));
        assert_eq!(unchecked_open_close.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(unchecked_open_close.message, "open() return value should be checked");
        let data = unchecked_open_close
            .data
            .as_ref()
            .ok_or("native unchecked open/close data should be populated")?;
        assert_eq!(data["code"], "native.io.unchecked_open_close");
        assert_eq!(data["suppressionKey"], "native.io.unchecked_open_close");
        assert_eq!(data["fixable"], false);

        // #11918: backtick and qx each keep one merged PL601 row per reviewed
        // shape; the native spellings ride inside the merged rows.
        let pl601_rows = items
            .iter()
            .filter(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL601"),
                )
            })
            .count();
        assert_eq!(pl601_rows, 2, "backtick and qx each merge into one PL601 row");
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.security.backtick_exec"),
                )
            }),
            "native backtick spelling must not appear as a separate row"
        );

        let readpipe = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL606"),
                )
            })
            .ok_or("expected merged readpipe row")?;
        assert_eq!(readpipe.source.as_deref(), Some("perl-lsp"));
        // #12004: the merged row renders the retired ordinary twin's
        // suggestion text exactly as that twin always rendered it.
        assert_eq!(
            readpipe.message,
            "readpipe() executes a shell command (equivalent to qx//). Ensure input is \
             sanitized.\nSuggestion: Use open(my $fh, '-|', @cmd) or IPC::Run for safer \
             command execution"
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.security.qx_readpipe"),
                )
            }),
            "native qx/readpipe spelling must not appear as a separate row"
        );

        let string_eval = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.security.string_eval"),
                )
            })
            .ok_or("expected native string eval finding")?;
        assert_eq!(string_eval.source.as_deref(), Some("perl-lsp"));
        assert_eq!(string_eval.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(string_eval.message, "String eval is a security risk");
        let data =
            string_eval.data.as_ref().ok_or("native string eval data should be populated")?;
        assert_eq!(data["code"], "native.security.string_eval");
        assert_eq!(data["suppressionKey"], "native.security.string_eval");
        assert_eq!(data["fixable"], false);

        // #11918: system and exec merge with their built-in PL603/PL604
        // observations; the native spelling rides inside the merged rows.
        let system_exec = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL603"),
                )
            })
            .ok_or("expected merged system row")?;
        assert_eq!(system_exec.source.as_deref(), Some("perl-lsp"));
        assert_eq!(
            system_exec.message,
            "system() executes a shell command. Ensure input is sanitized.\nSuggestion: Use the list form: system($cmd, @args) instead of system(\"$cmd @args\") to avoid shell injection"
        );
        assert!(
            items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL604"),
                )
            }),
            "expected merged exec row"
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.security.system_exec"),
                )
            }),
            "native system/exec spelling must not appear as a separate row"
        );

        let unused = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.unused_lexical"))
                    && diag.message == "Lexical variable '$unused' is declared but never used"
            })
            .ok_or("expected native unused lexical finding")?;
        assert_eq!(unused.source.as_deref(), Some("perl-lsp"));
        assert_eq!(unused.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(unused.message, "Lexical variable '$unused' is declared but never used");
        let data = unused.data.as_ref().ok_or("native unused lexical data should be populated")?;
        assert_eq!(data["code"], "native.variables.unused_lexical");
        assert_eq!(data["suppressionKey"], "native.variables.unused_lexical");
        assert_eq!(data["fixable"], true);

        let unused_parameter = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.unused_parameter"))
            })
            .ok_or("expected native unused parameter finding")?;
        assert_eq!(unused_parameter.source.as_deref(), Some("perl-lsp"));
        assert_eq!(unused_parameter.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(unused_parameter.message, "Parameter '$unused_param' is never used");
        let data = unused_parameter
            .data
            .as_ref()
            .ok_or("native unused parameter data should be populated")?;
        assert_eq!(data["code"], "native.variables.unused_parameter");
        assert_eq!(data["suppressionKey"], "native.variables.unused_parameter");
        assert_eq!(data["fixable"], true);

        let duplicate_parameter = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.duplicate_parameter"))
            })
            .ok_or("expected native duplicate parameter finding")?;
        assert_eq!(duplicate_parameter.source.as_deref(), Some("perl-lsp"));
        assert_eq!(duplicate_parameter.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(
            duplicate_parameter.message,
            "Parameter '$dup_param' appears more than once in this signature"
        );
        let data = duplicate_parameter
            .data
            .as_ref()
            .ok_or("native duplicate parameter data should be populated")?;
        assert_eq!(data["code"], "native.variables.duplicate_parameter");
        assert_eq!(data["suppressionKey"], "native.variables.duplicate_parameter");
        assert_eq!(data["fixable"], true);

        let parameter_shadow = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.parameter_shadows_global"))
            })
            .ok_or("expected native parameter shadowing finding")?;
        assert_eq!(parameter_shadow.source.as_deref(), Some("perl-lsp"));
        assert_eq!(parameter_shadow.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(
            parameter_shadow.message,
            "Parameter '$outer_param' shadows an outer declaration"
        );
        let data = parameter_shadow
            .data
            .as_ref()
            .ok_or("native parameter shadowing data should be populated")?;
        assert_eq!(data["code"], "native.variables.parameter_shadows_global");
        assert_eq!(data["suppressionKey"], "native.variables.parameter_shadows_global");
        assert_eq!(data["fixable"], true);

        let duplicate = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.duplicate_lexical"))
            })
            .ok_or("expected native duplicate lexical finding")?;
        assert_eq!(duplicate.source.as_deref(), Some("perl-lsp"));
        assert_eq!(duplicate.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(
            duplicate.message,
            "Lexical variable '$x' is declared more than once in the same scope"
        );
        let data =
            duplicate.data.as_ref().ok_or("native duplicate lexical data should be populated")?;
        assert_eq!(data["code"], "native.variables.duplicate_lexical");
        assert_eq!(data["suppressionKey"], "native.variables.duplicate_lexical");
        assert_eq!(data["fixable"], true);

        let shadowed = items
            .iter()
            .find(|diag| {
                diag.code
                    .as_ref()
                    .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.variables.shadowed_lexical"))
            })
            .ok_or("expected native shadowed lexical finding")?;
        assert_eq!(shadowed.source.as_deref(), Some("perl-lsp"));
        assert_eq!(shadowed.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(shadowed.message, "Lexical variable '$shadow' shadows an outer declaration");
        let data =
            shadowed.data.as_ref().ok_or("native shadowed lexical data should be populated")?;
        assert_eq!(data["code"], "native.variables.shadowed_lexical");
        assert_eq!(data["suppressionKey"], "native.variables.shadowed_lexical");
        assert_eq!(data["fixable"], true);

        let require_pod_sections = items
            .iter()
            .find(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.documentation.require_pod_sections"),
                )
            })
            .ok_or("expected native required POD sections finding")?;
        assert_eq!(require_pod_sections.source.as_deref(), Some("perl-lsp"));
        assert_eq!(require_pod_sections.severity, Some(LspDiagnosticSeverity::WARNING));
        assert_eq!(
            require_pod_sections.message,
            "POD is missing required =head1 DESCRIPTION section"
        );
        let data = require_pod_sections
            .data
            .as_ref()
            .ok_or("native required POD sections data should be populated")?;
        assert_eq!(data["code"], "native.documentation.require_pod_sections");
        assert_eq!(data["suppressionKey"], "native.documentation.require_pod_sections");
        assert_eq!(data["fixable"], false);
        Ok(())
    }

    #[test]
    fn native_critic_recommended_profile_filters_pull_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "recommended".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "my $unused = 1;\nmy $cond = 0;\nif ($cond = 1) { print $cond; }\n",
            None,
            &context,
            None,
        ));

        assert!(
            items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_strict"),
                )
            }),
            "recommended native critic profile should keep strict finding: {items:?}"
        );
        assert!(
            items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.assignment_in_condition"),
                )
            }),
            "recommended native critic profile should keep common findings: {items:?}"
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.variables.unused_lexical"),
                )
            }),
            "recommended native critic profile should omit broader variable findings: {items:?}"
        );

        Ok(())
    }

    #[test]
    fn accepted_critic_state_normalizes_profile_case_and_whitespace()
    -> Result<(), Box<dyn std::error::Error>> {
        // Intended #8253/#9062 behavior change: migrated transports no longer
        // reparse a raw profile carrier with exact-token legacy semantics
        // (invalid case => strict fallback). The accepted state derivation
        // normalizes case and surrounding whitespace, so this carrier yields
        // the recommended profile: the strict-only unused-lexical rule must
        // stay absent while native rows keep flowing.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = " RECOMMENDED ".to_string();
        context.accepted_critic_snapshot =
            accepted_state(" RECOMMENDED ", 3, Vec::new(), Vec::new());

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n",
            None,
            &context,
            None,
        ));

        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.variables.unused_lexical"),
                )
            }),
            "accepted normalization must not widen to the strict profile: {items:?}"
        );

        Ok(())
    }

    #[test]
    fn native_critic_runtime_context_honors_include_and_exclude_filters()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "recommended".to_string();
        context.native_critic_include = vec!["native.testing.require_use_strict".to_string()];
        context.native_critic_exclude = vec!["native.common.assignment_in_condition".to_string()];
        context.accepted_critic_snapshot = accepted_state(
            "recommended",
            3,
            vec!["native.testing.require_use_strict".to_string()],
            vec!["native.common.assignment_in_condition".to_string()],
        );

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "my $cond = 0;\nif ($cond = 1) { print $cond; }\n",
            None,
            &context,
            None,
        ));

        assert!(
            items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_strict"),
                )
            }),
            "native include should keep selected strict rule: {items:?}"
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.common.assignment_in_condition"),
                )
            }),
            "native include/exclude filters should suppress assignment rule: {items:?}"
        );
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_warnings"),
                )
            }),
            "native include should suppress non-included warning rule: {items:?}"
        );

        Ok(())
    }

    #[test]
    fn native_critic_include_enables_a_strict_only_rule_under_recommended()
    -> Result<(), Box<dyn std::error::Error>> {
        // `native.variables.unused_lexical` is strict-only. Naming it in
        // `include` used to yield no diagnostics at all under the recommended
        // profile, because the profile registry never carried the rule.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "recommended".to_string();
        context.native_critic_include = vec!["native.variables.unused_lexical".to_string()];
        context.accepted_critic_snapshot = accepted_state(
            "recommended",
            3,
            vec!["native.variables.unused_lexical".to_string()],
            Vec::new(),
        );

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n",
            None,
            &context,
            None,
        ));

        assert!(
            items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "native.variables.unused_lexical"),
                )
            }),
            "strict-only include should run under the recommended profile: {items:?}"
        );

        Ok(())
    }

    #[test]
    fn native_critic_engine_is_default_for_pull_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let items =
            get_full_items(provider.get_document_diagnostics(&uri, "my $x = 1;\n", None, None));

        assert!(items.iter().any(|diag| {
            diag.code
                .as_ref()
                .is_some_and(|code| matches!(code, NumberOrString::String(value) if value == "native.testing.require_use_strict"))
        }));
        assert!(!items.iter().any(|diag| {
            diag.code.as_ref().is_some_and(|code| {
                matches!(code, NumberOrString::String(value) if value == "TestingAndDebugging::RequireUseStrict")
            })
        }));
        Ok(())
    }

    #[test]
    fn pull_overlap_rows_merge_into_one_product_row_before_projection()
    -> Result<(), Box<dyn std::error::Error>> {
        // #11918: the pull transport historically had no XOR dedup at all, so
        // the core PL603 row and the native system row appeared as duplicates.
        // The normalized seam now merges them for both transports.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///overlap.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nsystem('ls');\n",
            None,
            &context,
            None,
        ));

        let pl603 = items
            .iter()
            .filter(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL603"),
                )
            })
            .count();
        assert_eq!(pl603, 1, "exactly one merged logical row carries PL603: {items:?}");
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(|code| {
                    matches!(code, NumberOrString::String(value) if value == "native.security.system_exec")
                })
            }),
            "the native spelling must ride inside the merged row: {items:?}"
        );
        Ok(())
    }

    #[test]
    fn pull_merged_system_row_preserves_the_retired_twin_suggestion_verbatim()
    -> Result<(), Box<dyn std::error::Error>> {
        // #12004: retiring the ordinary overlap twin must not retire its
        // user-visible remediation. The merged row renders the producer's
        // exact Suggestion text and related information.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///overlap_remediation_system.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nsystem('ls -la');\n",
            None,
            &context,
            None,
        ));

        let pl603_rows: Vec<_> = items
            .iter()
            .filter(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL603"),
                )
            })
            .collect();
        assert_eq!(pl603_rows.len(), 1, "one merged PL603 row: {items:?}");
        assert!(
            pl603_rows[0].message.contains(
                "system() executes a shell command. Ensure input is sanitized.\nSuggestion: Use the list form: system($cmd, @args) instead of system(\"$cmd @args\") to avoid shell injection",
            ),
            "merged message must carry the twin's verbatim Suggestion text: {}",
            pl603_rows[0].message
        );
        let related = pl603_rows[0].related_information.as_deref().unwrap_or_default();
        assert!(
            related.iter().any(|info| info.message
                == "Use the list form system($cmd, @args) to avoid shell injection when arguments come from user input"),
            "merged row must keep the twin's related information: {related:?}"
        );
        Ok(())
    }

    #[test]
    fn pull_merged_undef_comparison_row_preserves_the_retired_twin_suggestion_verbatim()
    -> Result<(), Box<dyn std::error::Error>> {
        // #12004: the literal-undef PL404 row merges with its native alias;
        // the ordinary emitter's suggestion text and related information must
        // survive that merge byte-for-byte on the pull transport too.
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///overlap_remediation_undef.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nif (5 == undef) { }\n",
            None,
            &context,
            None,
        ));

        let pl404_rows: Vec<_> = items
            .iter()
            .filter(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL404"),
                )
            })
            .collect();
        assert_eq!(pl404_rows.len(), 1, "one merged literal-undef PL404 row: {items:?}");
        assert!(
            pl404_rows[0].message.contains(
                "Using '==' with potentially undefined value -- use 'defined()' to check first\nSuggestion: Guard with 'defined($var)' or use the '//' (defined-or) operator",
            ),
            "merged message must carry the twin's verbatim Suggestion text: {}",
            pl404_rows[0].message
        );
        let related = pl404_rows[0].related_information.as_deref().unwrap_or_default();
        assert!(
            related
                .iter()
                .any(|info| info.message == "Consider using 'defined' check or '//' operator"),
            "merged row must keep the twin's related information: {related:?}"
        );
        Ok(())
    }

    #[test]
    fn pull_overlap_merges_respect_reviewed_shapes_and_distinct_findings()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///shapes.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "use strict;\nuse warnings;\nmy $a = `ls`;\nmy $b = qx(date);\nmy $c = readpipe('id');\nif (5 == undef) { }\nif ($undeclared_var == 5) { }\nprint $a . $b . $c;\n",
            None,
            &context,
            None,
        ));

        let count_code = |needle: &str| {
            items
                .iter()
                .filter(|diag| {
                    diag.code.as_ref().is_some_and(
                        |code| matches!(code, NumberOrString::String(value) if value == needle),
                    )
                })
                .count()
        };
        assert_eq!(count_code("PL601"), 2, "backtick and qx each keep one row: {items:?}");
        assert_eq!(count_code("PL606"), 1, "readpipe keeps its own row: {items:?}");
        assert_eq!(count_code("PL604"), 0, "no exec finding in this document: {items:?}");
        // Literal-undef PL404 merges with its native alias into one row; the
        // data-flow PL404 stays a distinct built-in-only row.
        assert_eq!(count_code("PL404"), 2, "literal and data-flow PL404 rows: {items:?}");
        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(|code| {
                    matches!(code, NumberOrString::String(value) if value == "native.common.undef_comparison")
                })
            }),
            "the native literal spelling rides inside its merged row: {items:?}"
        );
        Ok(())
    }

    #[test]
    fn pull_suppression_by_compat_spelling_removes_the_complete_alias_row()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///suppress.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();

        let items = get_full_items(provider.get_document_diagnostics_with_context(
            &uri,
            "## no critic PL603\nuse strict;\nuse warnings;\nsystem('ls');\n",
            None,
            &context,
            None,
        ));

        assert!(
            !items.iter().any(|diag| {
                diag.code.as_ref().is_some_and(
                    |code| matches!(code, NumberOrString::String(value) if value == "PL603"),
                )
            }),
            "suppression must remove the whole logical row in the pull path too: {items:?}"
        );
        Ok(())
    }

    #[test]
    fn unknown_subroutine_attribute_syntax_error_stays_warning()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///test.pl".parse()?;
        let diagnostic = provider.parse_error_to_diagnostic(
            &uri,
            "sub foo :wat {}",
            &ParseError::SyntaxError {
                location: 8,
                message: "unknown subroutine attribute ':wat'".to_string(),
            },
        );

        assert_eq!(diagnostic.code, Some(NumberOrString::String("PL002".to_string())));
        assert_eq!(diagnostic.severity, Some(LspDiagnosticSeverity::WARNING));
        let data = diagnostic.data.as_ref().ok_or("data should be populated")?;
        assert_eq!(data["code"], "PL002");
        Ok(())
    }

    // ── pending-parse gap (#3396 PR4 / #7480) ─────────────────────────────
    //
    // `get_workspace_diagnostics_with_context` is not reachable from the live
    // `workspace/diagnostic` JSON-RPC dispatch today (the hand-rolled
    // `LspServer::handle_workspace_diagnostic` in `runtime/diagnostics.rs`
    // handles that request directly and is exercised in
    // `tests/pull_diagnostics_freshness_tests.rs`). It remains public API on
    // `PullDiagnosticsProvider`, so it must uphold the pending-parse policy:
    // a `DocumentState` with no current-generation `ParsedSnapshot` is an
    // explicitly not-ready subject — its report is returned in full but never
    // carries a reusable result ID and never comes back as `Unchanged`,
    // even when the client echoes a known prior ID.
    // `DocumentState::new` never publishes a snapshot, so `current_parsed()`
    // is `None` by construction -- exactly the gap state.

    #[test]
    fn workspace_diagnostics_returns_full_without_result_id_for_gapped_doc_with_known_prior()
    -> Result<(), Box<dyn std::error::Error>> {
        let doc = DocumentState::new("my $x = 1;\n", 1);
        assert!(doc.current_parsed().is_none(), "fresh DocumentState must have no snapshot yet");

        let mut documents = HashMap::new();
        documents.insert("file:///gap_known.pl".to_string(), doc);
        let previous_result_ids =
            vec![("file:///gap_known.pl".parse()?, "stale-result-id".to_string())];

        let provider = PullDiagnosticsProvider::new();
        let report = provider.get_workspace_diagnostics(&documents, previous_result_ids);
        assert_eq!(report.items.len(), 1);
        match &report.items[0] {
            WorkspaceDocumentDiagnosticReport::Full(full) => {
                assert!(
                    full.full_document_diagnostic_report.result_id.is_none(),
                    "a not-ready (gapped) subject must not receive a reusable resultId"
                );
                Ok(())
            }
            other => Err(format!(
                "expected a Full report without resultId for a pending-parse-gap document \
                 with a known prior ID, got: {other:?}"
            )
            .into()),
        }
    }

    #[test]
    fn workspace_diagnostics_falls_through_for_gapped_doc_without_known_result_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let doc = DocumentState::new("my $x = 1;\n", 1);
        assert!(doc.current_parsed().is_none(), "fresh DocumentState must have no snapshot yet");

        let mut documents = HashMap::new();
        documents.insert("file:///gap_unknown.pl".to_string(), doc);

        let provider = PullDiagnosticsProvider::new();
        let report = provider.get_workspace_diagnostics(&documents, Vec::new());
        assert_eq!(report.items.len(), 1);
        match &report.items[0] {
            WorkspaceDocumentDiagnosticReport::Full(full) => {
                assert!(
                    full.full_document_diagnostic_report.result_id.is_none(),
                    "a not-ready (gapped) subject must not receive a reusable resultId"
                );
                assert!(
                    full.full_document_diagnostic_report.items.is_empty(),
                    "no current-generation AST means no diagnostics can be computed"
                );
                Ok(())
            }
            other => Err(format!(
                "expected a (empty) Full report without resultId when there is no previous \
                 resultId to protect, got: {other:?}"
            )
            .into()),
        }
    }

    // ── complete-subject result identity (#7480) ──────────────────────────

    fn full_result_id(report: &DocumentDiagnosticReport) -> Option<String> {
        match report {
            DocumentDiagnosticReport::Full(full) => {
                full.full_document_diagnostic_report.result_id.clone()
            }
            DocumentDiagnosticReport::Unchanged(unchanged) => {
                Some(unchanged.unchanged_document_diagnostic_report.result_id.clone())
            }
        }
    }

    /// Review probe: a disabled accepted state must not delete ordinary core
    /// rows. `PL603` is a core security lint about shell injection; it carries
    /// a critic overlap observation (#11918) only so a merged logical row can
    /// replace it when the native critic actually runs. With critic disabled no
    /// replacement is produced, so the ordinary row must survive.
    #[test]
    fn disabled_critic_state_retains_core_overlap_carrier_rows()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///disabled_carrier.pl".parse()?;
        let source = "my $path = 'f.txt';
system($path);
";

        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.accepted_critic_snapshot = AcceptedCriticSnapshot::capture(
            &ServerConfig { perlcritic_enabled: false, ..ServerConfig::default() },
            Some(PROVIDER_DEFAULT_ROOT_AUTHORITY),
        );
        context.perlcritic_enabled = false;

        let report =
            provider.get_document_diagnostics_with_context(&uri, source, None, &context, None);
        if full_result_id(&report).is_none() {
            return Err("a current Disabled snapshot must remain reusable".into());
        }
        let items = get_full_items(report);

        if has_native_critic_row(&items) {
            return Err("a Disabled snapshot must not publish native rows".into());
        }

        let has_pl603 = items.iter().any(|diag| {
            diag.code
                .as_ref()
                .is_some_and(|code| matches!(code, NumberOrString::String(v) if v == "PL603"))
        });
        if !has_pl603 {
            return Err(format!(
                "disabling native Critic must retain PL603; got: {:?}",
                items.iter().filter_map(|d| d.code.clone()).collect::<Vec<_>>()
            )
            .into());
        }
        Ok(())
    }

    // ── accepted-state currentness at the pull result boundary (#13304) ────

    /// Build the strict native context the currentness proofs share.
    fn strict_native_context() -> PullDiagnosticsContext {
        let mut context = PullDiagnosticsContext::new();
        context.critic_engine = CriticEngine::Native;
        context.native_critic_profile = "strict".to_string();
        context.accepted_critic_snapshot = strict_accepted_state(3);
        context.perlcritic_severity = 3;
        context
    }

    fn has_native_critic_row(items: &[LspDiagnostic]) -> bool {
        items.iter().any(|diag| {
            diag.code.as_ref().is_some_and(|code| {
                matches!(code, NumberOrString::String(value) if value.starts_with("native."))
            })
        })
    }

    /// #13304: a pull run whose accepted policy moved underneath it must
    /// neither publish its native rows nor hand back a reusable result ID. An
    /// implementation that passes `RunGate::open()` for currentness (the state
    /// this repaired) returns the dead-policy rows and caches them.
    #[test]
    fn moved_policy_withholds_native_rows_and_reusable_result_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///pull_currentness.pl".parse()?;
        let source = "my $x = 1;
";

        // Control: with the accepted policy still live, this subject really
        // does produce native rows and a reusable ID — so the assertions below
        // are discriminating rather than vacuous.
        let current = strict_native_context();
        let live =
            provider.get_document_diagnostics_with_context(&uri, source, None, &current, None);
        assert!(
            has_native_critic_row(&get_full_items(live.clone())),
            "the fixture must produce native critic rows while the policy is live"
        );
        assert!(
            full_result_id(&live).is_some(),
            "a fully current subject must carry a reusable result ID"
        );

        // The accepted policy is dead for the whole run.
        let mut moved = strict_native_context();
        moved.accepted_state_currentness =
            AcceptedStateCurrentness::new(std::sync::Arc::new(|| false));
        let report =
            provider.get_document_diagnostics_with_context(&uri, source, None, &moved, None);

        assert!(
            !has_native_critic_row(&get_full_items(report.clone())),
            "rows produced under a dead policy must not reach the client"
        );
        assert!(
            full_result_id(&report).is_none(),
            "a report that dropped its critic rows must not be cacheable as current"
        );
        Ok(())
    }

    /// #13304: the gate must also close for a policy that was live when the
    /// report subject was composed and moved while rules evaluated. Checking
    /// currentness only before collection leaves this run cacheable.
    #[test]
    fn policy_moving_during_collection_withholds_the_reusable_result_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///pull_currentness_race.pl".parse()?;

        // Current for identity composition and service settlement, dead only
        // at the report boundary. This is the exact early-append falsifier.
        let observations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = std::sync::Arc::clone(&observations);
        let mut context = strict_native_context();
        context.accepted_state_currentness =
            AcceptedStateCurrentness::new(std::sync::Arc::new(move || {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2
            }));

        let report = provider.get_document_diagnostics_with_context(
            &uri,
            "my $path = 'f.txt';
system($path);
",
            None,
            &context,
            None,
        );

        if observations.load(std::sync::atomic::Ordering::SeqCst) <= 1 {
            return Err("the currentness authority must be consulted again after collection".into());
        }
        if full_result_id(&report).is_some() {
            return Err(
                "a policy that moved during collection must suppress the reusable result ID".into(),
            );
        }
        let items = get_full_items(report);
        if has_native_critic_row(&items) {
            return Err(
                "service-current native rows must remain staged when the report boundary is stale"
                    .into(),
            );
        }
        if !items.iter().any(|diagnostic| {
            matches!(&diagnostic.code, Some(NumberOrString::String(code)) if code == "PL603")
        }) {
            return Err(
                "withholding the staged Critic contribution must preserve the core overlap carrier"
                    .into(),
            );
        }
        Ok(())
    }

    /// Exact same complete subject/profile → deterministic same ID and a valid
    /// `Unchanged` on the next pull (#7480 fixture).
    #[test]
    fn pull_document_unchanged_for_identical_complete_subject()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_stable.pl".parse()?;
        let context = PullDiagnosticsContext::new();

        let first = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            None,
            &context,
            None,
        );
        let result_id = full_result_id(&first).ok_or("full report must carry a reusable ID")?;

        let second = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            Some(result_id.clone()),
            &context,
            None,
        );
        match &second {
            DocumentDiagnosticReport::Unchanged(unchanged) => {
                assert_eq!(
                    unchanged.unchanged_document_diagnostic_report.result_id, result_id,
                    "unchanged response must echo the composed subject ID"
                );
            }
            other => Err(format!("expected Unchanged for identical subject, got: {other:?}"))?,
        }

        Ok(())
    }

    #[test]
    fn matching_previous_id_with_moved_subject_never_returns_unchanged()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_moved_before_unchanged.pl".parse()?;
        let source = "my $x = 1;\n";
        let baseline_context = strict_native_context();
        let first = provider.get_document_diagnostics_with_context(
            &uri,
            source,
            None,
            &baseline_context,
            None,
        );
        let previous = full_result_id(&first).ok_or("baseline result ID missing")?;

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = std::sync::Arc::clone(&calls);
        let mut moved = baseline_context;
        moved.accepted_state_currentness =
            AcceptedStateCurrentness::new(std::sync::Arc::new(move || {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
            }));
        let report = provider.get_document_diagnostics_with_context(
            &uri,
            source,
            Some(previous),
            &moved,
            None,
        );
        if !matches!(report, DocumentDiagnosticReport::Full(_)) {
            return Err("a moved accepted snapshot must never return Unchanged".into());
        }
        Ok(())
    }

    /// Source-identical but later document instance (generation advance) is a
    /// different subject than the pre-edit instance (#7480 fixture).
    #[test]
    fn pull_document_full_after_generation_advance_with_identical_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_generation.pl".parse()?;
        let context = PullDiagnosticsContext::new();

        let before = DocumentState::new("my $x = 1;\n", 1);
        let first = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            None,
            &context,
            Some(&before),
        );
        let before_id = full_result_id(&first).ok_or("expected reusable ID before edit")?;

        // Simulate edit + revert to identical bytes: generation advanced.
        let after = DocumentState::new("my $y = 9;\nmy $x = 1;\n", 3);
        let second = provider.get_document_diagnostics_with_context(
            &uri,
            "my $y = 9;\nmy $x = 1;\n",
            Some(before_id.clone()),
            &context,
            Some(&after),
        );
        let after_id =
            full_result_id(&second).ok_or("edited content must produce a fresh reusable ID")?;
        assert_ne!(before_id, after_id, "a source edit must supersede the prior result");

        Ok(())
    }

    /// Behavior-bearing configuration movement over unchanged bytes must move
    /// the ID (negative control: keeping the old ID would authorize false
    /// `Unchanged`) (#7480 fixture/negative control).
    #[test]
    fn pull_document_supersedes_on_config_movement_over_unchanged_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_config.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();

        let first = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            None,
            &context,
            None,
        );
        let baseline_id = full_result_id(&first).ok_or("expected reusable baseline ID")?;

        // Accepted severity movement with identical bytes.
        context.accepted_critic_snapshot = strict_accepted_state(4);
        let severity_moved = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            Some(baseline_id.clone()),
            &context,
            None,
        );
        let severity_id =
            full_result_id(&severity_moved).ok_or("config-moved report must stay reusable")?;
        assert_ne!(
            baseline_id, severity_id,
            "accepted-config movement must invalidate the prior result ID"
        );

        // Negotiated projection movement (markup support) with identical bytes.
        let mut context = PullDiagnosticsContext::new();
        context.projection.markup_messages = true;
        let markup_moved = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            Some(severity_id.clone()),
            &context,
            None,
        );
        assert!(matches!(markup_moved, DocumentDiagnosticReport::Full(_)));
        assert_ne!(
            Some(severity_id),
            full_result_id(&markup_moved),
            "projection-profile movement must invalidate the prior result ID"
        );

        Ok(())
    }

    /// Deprecated raw selector fields are migration observations only. They
    /// cannot select product behaviour or move the accepted Critic result ID.
    #[test]
    fn raw_legacy_selector_movement_changes_neither_rows_nor_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_raw_observation.pl".parse()?;
        let source = "my $x = 1;\n";
        let baseline_context = strict_native_context();
        let baseline = provider.get_document_diagnostics_with_context(
            &uri,
            source,
            None,
            &baseline_context,
            None,
        );
        let baseline_id = full_result_id(&baseline).ok_or("baseline result ID missing")?;
        let baseline_items = get_full_items(baseline);

        let mut observed = baseline_context.clone();
        observed.critic_engine = CriticEngine::Legacy;
        observed.perlcritic_enabled = false;
        observed.perlcritic_severity = 5;
        observed.perlcritic_profile = Some("/ignored/.perlcriticrc".to_string());
        observed.native_critic_profile = "recommended".to_string();
        observed.native_critic_include = vec!["ignored.include".to_string()];
        observed.native_critic_exclude = vec!["ignored.exclude".to_string()];

        let report =
            provider.get_document_diagnostics_with_context(&uri, source, None, &observed, None);
        if full_result_id(&report).as_deref() != Some(baseline_id.as_str()) {
            return Err("raw selector observations must not move the result ID".into());
        }
        if get_full_items(report) != baseline_items {
            return Err("raw selector observations must not move product behaviour".into());
        }
        Ok(())
    }

    /// Include/resolver environment movement over unchanged bytes must move
    /// the ID (#7480 fixture).
    #[test]
    fn pull_document_supersedes_on_resolver_environment_movement()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_resolver.pl".parse()?;
        let mut context = PullDiagnosticsContext::new();

        let first = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            None,
            &context,
            None,
        );
        let baseline_id = full_result_id(&first).ok_or("expected reusable baseline ID")?;

        context.include_paths = vec!["/opt/site/lib".to_string()];
        let moved = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            Some(baseline_id),
            &context,
            None,
        );

        assert_ne!(
            PullDiagnosticsContext::new().include_paths,
            context.include_paths,
            "fixture must actually move the resolver environment"
        );
        assert!(
            matches!(moved, DocumentDiagnosticReport::Full(_)),
            "resolver-environment movement must supersede, never return Unchanged"
        );

        Ok(())
    }

    /// A prior ID minted under a foreign/older scheme never authorizes
    /// `Unchanged`, even over identical bytes (#7480 fixture/negative control).
    #[test]
    fn pull_document_treats_foreign_schema_prior_as_full() -> Result<(), Box<dyn std::error::Error>>
    {
        let provider = PullDiagnosticsProvider::new();
        let uri: Uri = "file:///identity_foreign_prior.pl".parse()?;
        let context = PullDiagnosticsContext::new();

        let report = provider.get_document_diagnostics_with_context(
            &uri,
            "my $x = 1;\n",
            Some("5d41402abc4b2a76b9719d911017c592".to_string()),
            &context,
            None,
        );

        assert!(
            matches!(report, DocumentDiagnosticReport::Full(_)),
            "an unknown-schema prior ID must produce full, not unchanged"
        );

        Ok(())
    }

    /// Document and partial-workspace transports mint identical per-document
    /// IDs through the same identity authority (#7480 fixture).
    #[test]
    fn document_and_workspace_transports_share_per_document_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = PullDiagnosticsProvider::new();
        let uri_str = "file:///identity_shared.pl";
        let uri: Uri = uri_str.parse()?;
        let content = "my $x = 1;\n";
        let context = PullDiagnosticsContext::new();

        let document_report =
            provider.get_document_diagnostics_with_context(&uri, content, None, &context, None);
        let document_id =
            full_result_id(&document_report).ok_or("document transport must mint a reusable ID")?;

        let partial = provider.get_workspace_diagnostics_partial_with_context(
            &[(uri_str.into(), content.into())],
            8,
            &context,
        );
        let [chunk] = partial.as_slice() else {
            return Err("expected exactly one partial chunk".into());
        };
        let [item] = chunk.items.as_slice() else {
            return Err("expected exactly one partial item".into());
        };
        let WorkspaceDocumentDiagnosticReport::Full(workspace_full) = item else {
            return Err(format!("expected workspace Full report, got: {item:?}").into());
        };

        assert_eq!(
            workspace_full.full_document_diagnostic_report.result_id.as_deref(),
            Some(document_id.as_str()),
            "workspace partial items must reuse the document identity authority"
        );

        Ok(())
    }
}
