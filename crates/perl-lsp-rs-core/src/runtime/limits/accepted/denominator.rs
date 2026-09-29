//! Complete first-effect consumer denominator for runtime-owned limits.

use std::collections::{BTreeMap, BTreeSet};

/// Domain that owns the limit. Adjacent domains are classified, never absorbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DomainOwner {
    RuntimeLimits,
    Testing,
    Ai,
    WirePressure,
    ReloadBudget,
    WorkspaceIndexedSource,
    ProcessSupervisor,
    FeatureSemanticBudget,
}

/// Hard-envelope owner named by the denominator row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvelopeOwner {
    Issue7479,
    NotProven,
}

/// Scope recorded on the denominator row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitScope {
    Global,
    Root,
    Document,
    Operation,
}

/// Current consumer state. `Live` is the only behavior-backed production state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitConsumerState {
    Live,
    ParsedNoConsumer,
    FixedInternal,
    Transferred,
    Retired,
    NotProven,
}

/// What happens to a local clamp/default at this consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClampDisposition {
    NoneOnConsumer,
    SnapshotAlreadyMutated,
    DomainOwned,
}

/// One limit/consumer pair. A field may have several first-effect consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DenominatorRow {
    /// Stable limit/consumer id.
    pub id: &'static str,
    /// Public config field id or fixed-product identity.
    pub public_or_fixed_id: &'static str,
    pub envelope_owner: EnvelopeOwner,
    pub view_field: &'static str,
    pub scope: LimitScope,
    pub domain_owner: DomainOwner,
    pub production_site: Option<&'static str>,
    pub production_marker: Option<&'static str>,
    pub first_effect: Option<&'static str>,
    pub lifecycle_owner: &'static str,
    pub clamp_disposition: ClampDisposition,
    pub status_projection: &'static str,
    pub migration_child: Option<&'static str>,
    pub test_id: &'static str,
    pub accessor: Option<&'static str>,
    pub rust_field: Option<&'static str>,
    pub schema_key: Option<&'static str>,
    pub state: LimitConsumerState,
    pub relational_group: Option<&'static str>,
}

macro_rules! row {
    (
        $id:literal, $public:literal, $view:literal, $owner:ident, $state:ident,
        rust = $rust:expr, schema = $schema:expr, accessor = $accessor:expr,
        site = $site:expr, marker = $marker:expr, effect = $effect:expr,
        life = $life:literal, clamp = $clamp:ident, status = $status:literal,
        migrate = $migrate:expr, test = $test:literal, rel = $rel:expr
        $(, scope = $scope:ident)?
    ) => {
        DenominatorRow {
            id: $id,
            public_or_fixed_id: $public,
            envelope_owner: EnvelopeOwner::Issue7479,
            view_field: $view,
            scope: row!(@scope $($scope)?),
            domain_owner: DomainOwner::$owner,
            production_site: $site,
            production_marker: $marker,
            first_effect: $effect,
            lifecycle_owner: $life,
            clamp_disposition: ClampDisposition::$clamp,
            status_projection: $status,
            migration_child: $migrate,
            test_id: $test,
            accessor: $accessor,
            rust_field: $rust,
            schema_key: $schema,
            state: LimitConsumerState::$state,
            relational_group: $rel,
        }
    };
    (@scope) => { LimitScope::Global };
    (@scope $scope:ident) => { LimitScope::$scope };
}

const WS: &str = "crates/perl-lsp-rs/src/runtime/workspace.rs";
const REFS: &str = "crates/perl-lsp-rs/src/runtime/language/references.rs";
const COMP: &str = "crates/perl-lsp-rs/src/runtime/language/completion.rs";
const DSYM: &str = "crates/perl-lsp-rs/src/runtime/language/symbols.rs";
const MISC: &str = "crates/perl-lsp-rs/src/runtime/language/misc.rs";
const SEM: &str = "crates/perl-lsp-rs/src/runtime/language/semantic_tokens.rs";
const NAV: &str = "crates/perl-lsp-rs/src/runtime/language/navigation.rs";
const SYNC: &str = "crates/perl-lsp-rs/src/runtime/text_sync.rs";
const SEC: &str = "crates/perl-lsp-rs/src/security/config.rs";
const FILE_VAL: &str = "crates/perl-lsp-rs-core/src/runtime/input_validation/file_validation.rs";
const LSP_VAL: &str = "crates/perl-lsp-rs-core/src/runtime/input_validation/lsp_validation.rs";
const TYPEDEF: &str = "crates/perl-lsp-rs-core/src/providers/navigation/type_definition.rs";
const WS_MON: &str = "crates/perl-workspace/src/monitoring/mod.rs";

/// Complete current-main denominator. Adjacent owners are referenced, not absorbed.
pub(crate) static RUNTIME_LIMITS_DENOMINATOR: &[DenominatorRow] = &[
    row!(
        "limits.workspace_symbol_cap.v2_handler",
        "perl.limits.workspaceSymbolCap",
        "result_caps.workspace_symbols",
        RuntimeLimits,
        Live,
        rust = Some("workspace_symbol_cap"),
        schema = Some("workspaceSymbolCap"),
        accessor = Some("workspace_symbol_cap"),
        site = Some(WS),
        marker = Some("handle_workspace_symbols_v2"),
        effect = Some("workspace/symbol v2 result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "workspace_symbol_cap_v2",
        rel = None
    ),
    row!(
        "limits.workspace_symbol_cap.v1_index",
        "perl.limits.workspaceSymbolCap",
        "result_caps.workspace_symbols",
        RuntimeLimits,
        Live,
        rust = Some("workspace_symbol_cap"),
        schema = Some("workspaceSymbolCap"),
        accessor = Some("workspace_symbol_cap"),
        site = Some(WS),
        marker = Some("search_source_symbols(query, Some(cap))"),
        effect = Some("workspace/symbol v1 index fast-path truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "workspace_symbol_cap_v1_index",
        rel = None
    ),
    row!(
        "limits.workspace_symbol_cap.open_docs",
        "perl.limits.workspaceSymbolCap",
        "result_caps.workspace_symbols",
        RuntimeLimits,
        Live,
        rust = Some("workspace_symbol_cap"),
        schema = Some("workspaceSymbolCap"),
        accessor = Some("workspace_symbol_cap"),
        site = Some(WS),
        marker = Some("WorkspaceSymbolsProvider::new"),
        effect = Some("workspace/symbol open-document fallback truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "workspace_symbol_cap_open_docs",
        rel = None
    ),
    row!(
        "limits.references_cap.handler",
        "perl.limits.referencesCap",
        "result_caps.references",
        RuntimeLimits,
        Live,
        rust = Some("references_cap"),
        schema = Some("referencesCap"),
        accessor = Some("references_cap"),
        site = Some(REFS),
        marker = Some("let cap = references_cap()"),
        effect = Some("textDocument/references result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "references_cap_handler",
        rel = None
    ),
    row!(
        "limits.completion_cap.handler",
        "perl.limits.completionCap",
        "result_caps.completion",
        RuntimeLimits,
        Live,
        rust = Some("completion_cap"),
        schema = Some("completionCap"),
        accessor = Some("completion_cap"),
        site = Some(COMP),
        marker = Some("let cap = completion_cap()"),
        effect = Some("textDocument/completion result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "completion_cap_handler",
        rel = None
    ),
    row!(
        "limits.document_symbol_cap.handler",
        "perl.limits.documentSymbolCap",
        "result_caps.document_symbols",
        RuntimeLimits,
        Live,
        rust = Some("document_symbol_cap"),
        schema = Some("documentSymbolCap"),
        accessor = Some("document_symbol_cap"),
        site = Some(DSYM),
        marker = Some("let cap = document_symbol_cap()"),
        effect = Some("textDocument/documentSymbol result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16841"),
        test = "document_symbol_cap_handler",
        rel = None
    ),
    row!(
        "limits.code_lens_cap.handler",
        "perl.limits.codeLensCap",
        "result_caps.code_lenses",
        RuntimeLimits,
        Live,
        rust = Some("code_lens_cap"),
        schema = Some("codeLensCap"),
        accessor = Some("code_lens_cap"),
        site = Some(MISC),
        marker = Some("let cap = code_lens_cap()"),
        effect = Some("textDocument/codeLens result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16842"),
        test = "code_lens_cap_handler",
        rel = None
    ),
    row!(
        "limits.diagnostics_per_file_cap.unconsumed",
        "perl.limits.diagnosticsPerFileCap",
        "result_caps.diagnostics_per_file",
        RuntimeLimits,
        ParsedNoConsumer,
        rust = Some("diagnostics_per_file_cap"),
        schema = Some("diagnosticsPerFileCap"),
        accessor = Some("diagnostics_per_file_cap"),
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = SnapshotAlreadyMutated,
        status = "parsed-no-first-effect",
        migrate = Some("#16842"),
        test = "diagnostics_per_file_cap_unconsumed",
        rel = None
    ),
    row!(
        "limits.inlay_hints_cap.handler",
        "perl.limits.inlayHintsCap",
        "result_caps.inlay_hints",
        RuntimeLimits,
        Live,
        rust = Some("inlay_hints_cap"),
        schema = Some("inlayHintsCap"),
        accessor = Some("inlay_hints_cap"),
        site = Some(MISC),
        marker = Some("let cap = inlay_hints_cap()"),
        effect = Some("textDocument/inlayHint result truncation"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-cap-not-projected",
        migrate = Some("#16842"),
        test = "inlay_hints_cap_handler",
        rel = None
    ),
    row!(
        "limits.file_size_bytes.file_content",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(FILE_VAL),
        marker = Some("let max_file_size = limits_max_file_size_bytes()"),
        effect = Some("reject oversized buffer content"),
        life = "document",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_file_content",
        rel = None
    ),
    row!(
        "limits.file_size_bytes.lsp_validation",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(LSP_VAL),
        marker = Some("let file_limit = limits_max_file_size_bytes()"),
        effect = Some("text-sync params ceiling"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_lsp_validation",
        rel = None
    ),
    row!(
        "limits.file_size_bytes.did_open",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(SYNC),
        marker = Some("fn handle_did_open_with_cancellation_inner"),
        effect = Some("didOpen skip parse for oversized files"),
        life = "document",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_did_open",
        rel = None
    ),
    row!(
        "limits.file_size_bytes.did_change",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(SYNC),
        marker = Some("fn handle_did_change_with_version_policy"),
        effect = Some("didChange skip parse for oversized files"),
        life = "document",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_did_change",
        rel = None
    ),
    row!(
        "limits.file_size_bytes.type_definition",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(TYPEDEF),
        marker = Some("fn should_parse_document"),
        effect = Some("type-definition parse admission"),
        life = "operation",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_type_definition",
        rel = None,
        scope = Operation
    ),
    row!(
        "limits.file_size_bytes.navigation_scan",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(NAV),
        marker = Some("fn is_scannable_type_definition_source"),
        effect = Some("navigation type-definition scan admission"),
        life = "operation",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_navigation_scan",
        rel = None,
        scope = Operation
    ),
    row!(
        "limits.file_size_bytes.security_config",
        "perl.limits.maxFileSizeBytes",
        "source_admission.max_file_size",
        RuntimeLimits,
        Live,
        rust = Some("max_file_size_bytes"),
        schema = Some("maxFileSizeBytes"),
        accessor = Some("max_file_size_bytes"),
        site = Some(SEC),
        marker = Some("max_file_size: perl_lsp_rs_core::runtime::limits::max_file_size_bytes()"),
        effect = Some("SecurityConfig default file size"),
        life = "process",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-size-not-projected",
        migrate = Some("#16843"),
        test = "file_size_security_config",
        rel = None
    ),
    row!(
        "limits.max_symbols_per_file.lsp_limits_unused",
        "fixed:LspLimits.max_symbols_per_file",
        "source_admission.max_symbols_per_file",
        RuntimeLimits,
        Transferred,
        rust = Some("max_symbols_per_file"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = DomainOwned,
        status = "transferred-not-consumed-from-lsp-limits",
        migrate = Some("#16843"),
        test = "max_symbols_per_file_unused",
        rel = None
    ),
    row!(
        "workspace.index.max_symbols_per_file",
        "perl-workspace::IndexResourceLimits.max_symbols_per_file",
        "source_admission.max_symbols_per_file",
        WorkspaceIndexedSource,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = Some(WS_MON),
        marker = Some("pub max_symbols_per_file: usize"),
        effect = Some("workspace index per-file symbol budget"),
        life = "index",
        clamp = DomainOwned,
        status = "workspace-owned",
        migrate = Some("#10841"),
        test = "workspace_max_symbols_per_file",
        rel = None
    ),
    row!(
        "limits.parse_storm_threshold.lsp_limits_unused",
        "fixed:LspLimits.parse_storm_threshold",
        "source_admission.parse_storm_threshold",
        RuntimeLimits,
        Transferred,
        rust = Some("parse_storm_threshold"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = DomainOwned,
        status = "transferred-not-consumed-from-lsp-limits",
        migrate = Some("#16843"),
        test = "parse_storm_unused",
        rel = None
    ),
    row!(
        "workspace.parse_storm_threshold",
        "perl-workspace::ParseStormMetrics.parse_storm_threshold",
        "source_admission.parse_storm_threshold",
        WorkspaceIndexedSource,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = Some(WS_MON),
        marker = Some("parse_storm_threshold: usize"),
        effect = Some("workspace parse-storm degradation"),
        life = "index",
        clamp = DomainOwned,
        status = "workspace-owned",
        migrate = Some("#10841"),
        test = "workspace_parse_storm",
        rel = None
    ),
    row!(
        "limits.reference_search_deadline.handler",
        "perl.limits.referenceSearchDeadlineMs",
        "provider_deadlines.reference_search",
        RuntimeLimits,
        Live,
        rust = Some("reference_search_deadline"),
        schema = Some("referenceSearchDeadlineMs"),
        accessor = Some("reference_search_deadline"),
        site = Some(REFS),
        marker = Some("let deadline = reference_search_deadline()"),
        effect = Some("textDocument/references search deadline"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-deadline-not-projected",
        migrate = Some("#16845"),
        test = "reference_search_deadline_handler",
        rel = None
    ),
    row!(
        "limits.semantic_tokens_deadline.handler",
        "fixed:LspLimits.semantic_tokens_deadline",
        "provider_deadlines.semantic_tokens",
        RuntimeLimits,
        Live,
        rust = Some("semantic_tokens_deadline"),
        schema = None,
        accessor = Some("semantic_tokens_deadline"),
        site = Some(SEM),
        marker = Some("let deadline = semantic_tokens_deadline()"),
        effect = Some("textDocument/semanticTokens deadline"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-deadline-not-projected",
        migrate = Some("#16845"),
        test = "semantic_tokens_deadline_handler",
        rel = None
    ),
    row!(
        "limits.code_lens_resolve_deadline.handler",
        "fixed:LspLimits.code_lens_resolve_deadline",
        "provider_deadlines.code_lens_resolve",
        RuntimeLimits,
        Live,
        rust = Some("code_lens_resolve_deadline"),
        schema = None,
        accessor = Some("code_lens_resolve_deadline"),
        site = Some(MISC),
        marker = Some("let deadline = code_lens_resolve_deadline()"),
        effect = Some("textDocument/codeLens resolve deadline"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-deadline-not-projected",
        migrate = Some("#16845"),
        test = "code_lens_resolve_deadline_handler",
        rel = None
    ),
    row!(
        "limits.completion_deadline.handler",
        "fixed:LspLimits.completion_deadline",
        "provider_deadlines.completion",
        RuntimeLimits,
        Live,
        rust = Some("completion_deadline"),
        schema = None,
        accessor = Some("completion_deadline"),
        site = Some(COMP),
        marker = Some("let deadline = completion_deadline()"),
        effect = Some("textDocument/completion deadline"),
        life = "request",
        clamp = SnapshotAlreadyMutated,
        status = "accepted-deadline-not-projected",
        migrate = Some("#16845"),
        test = "completion_deadline_handler",
        rel = None
    ),
    row!(
        "limits.file_index_deadline.internal",
        "fixed:LspLimits.file_index_deadline",
        "index_io_deadlines.file_index",
        RuntimeLimits,
        FixedInternal,
        rust = Some("file_index_deadline"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = NoneOnConsumer,
        status = "fixed-internal-no-reader",
        migrate = Some("#16846"),
        test = "file_index_deadline_internal",
        rel = None
    ),
    row!(
        "limits.regex_scan_deadline.unconsumed",
        "fixed:LspLimits.regex_scan_deadline",
        "index_io_deadlines.regex_scan",
        RuntimeLimits,
        ParsedNoConsumer,
        rust = Some("regex_scan_deadline"),
        schema = None,
        accessor = Some("regex_scan_deadline"),
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = SnapshotAlreadyMutated,
        status = "accessor-without-production-caller",
        migrate = Some("#16846"),
        test = "regex_scan_deadline_unconsumed",
        rel = None
    ),
    row!(
        "limits.fs_operation_deadline.internal",
        "fixed:LspLimits.fs_operation_deadline",
        "index_io_deadlines.filesystem",
        RuntimeLimits,
        FixedInternal,
        rust = Some("fs_operation_deadline"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = NoneOnConsumer,
        status = "fixed-internal-no-reader",
        migrate = Some("#16846"),
        test = "fs_operation_deadline_internal",
        rel = None
    ),
    row!(
        "limits.memory_warning.unconsumed",
        "perl.limits.memoryWarningThresholdBytes",
        "memory_cache.warning_threshold",
        RuntimeLimits,
        ParsedNoConsumer,
        rust = Some("warning_threshold_bytes"),
        schema = Some("memoryWarningThresholdBytes"),
        accessor = Some("memory_warning_threshold_bytes"),
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = SnapshotAlreadyMutated,
        status = "parsed-no-first-effect",
        migrate = Some("#16847"),
        test = "memory_warning_unconsumed",
        rel = Some("memory_thresholds")
    ),
    row!(
        "limits.memory_critical.unconsumed",
        "perl.limits.memoryCriticalThresholdBytes",
        "memory_cache.critical_threshold",
        RuntimeLimits,
        ParsedNoConsumer,
        rust = Some("critical_threshold_bytes"),
        schema = Some("memoryCriticalThresholdBytes"),
        accessor = Some("memory_critical_threshold_bytes"),
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = SnapshotAlreadyMutated,
        status = "parsed-no-first-effect",
        migrate = Some("#16847"),
        test = "memory_critical_unconsumed",
        rel = Some("memory_thresholds")
    ),
    row!(
        "limits.ast_cache.unconsumed",
        "perl.limits.astCacheMaxMemoryBytes",
        "memory_cache.ast_cache",
        RuntimeLimits,
        ParsedNoConsumer,
        rust = Some("ast_cache_max_bytes"),
        schema = Some("astCacheMaxMemoryBytes"),
        accessor = Some("ast_cache_max_memory_bytes"),
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = SnapshotAlreadyMutated,
        status = "parsed-no-first-effect",
        migrate = Some("#16847"),
        test = "ast_cache_unconsumed",
        rel = None
    ),
    row!(
        "workspace.index.max_ast_cache_bytes",
        "perl-workspace::IndexResourceLimits.max_ast_cache_bytes",
        "memory_cache.ast_cache",
        WorkspaceIndexedSource,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = Some(WS_MON),
        marker = Some("pub max_ast_cache_bytes: usize"),
        effect = Some("workspace AST cache byte budget"),
        life = "index",
        clamp = DomainOwned,
        status = "workspace-owned",
        migrate = Some("#10841"),
        test = "workspace_ast_cache",
        rel = None
    ),
    row!(
        "limits.return_partial_on_timeout.internal",
        "fixed:LspLimits.return_partial_on_timeout",
        "degradation_policy.return_partial_on_timeout",
        RuntimeLimits,
        FixedInternal,
        rust = Some("return_partial_on_timeout"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = NoneOnConsumer,
        status = "fixed-internal-no-reader",
        migrate = Some("#16843"),
        test = "return_partial_internal",
        rel = None
    ),
    row!(
        "limits.include_open_docs_when_degraded.internal",
        "fixed:LspLimits.include_open_docs_when_degraded",
        "degradation_policy.include_open_docs_when_degraded",
        RuntimeLimits,
        FixedInternal,
        rust = Some("include_open_docs_when_degraded"),
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "none",
        clamp = NoneOnConsumer,
        status = "fixed-internal-no-reader",
        migrate = Some("#16843"),
        test = "include_open_docs_internal",
        rel = None
    ),
    row!(
        "adjacent.ai.max_inflight",
        "ai.max_inflight",
        "adjacent.ai",
        Ai,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "ai",
        clamp = DomainOwned,
        status = "ai-owned",
        migrate = Some("#10909"),
        test = "adjacent_ai_max_inflight",
        rel = None
    ),
    row!(
        "adjacent.ai.max_output_tokens",
        "ai.max_output_tokens",
        "adjacent.ai",
        Ai,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "ai",
        clamp = DomainOwned,
        status = "ai-owned",
        migrate = Some("#10909"),
        test = "adjacent_ai_max_output_tokens",
        rel = None
    ),
    row!(
        "adjacent.ai.timeout_ms",
        "ai.timeout_ms",
        "adjacent.ai",
        Ai,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "ai",
        clamp = DomainOwned,
        status = "ai-owned",
        migrate = Some("#10909"),
        test = "adjacent_ai_timeout_ms",
        rel = None
    ),
    row!(
        "adjacent.ai.rate_limit_rps",
        "ai.rate_limit_rps",
        "adjacent.ai",
        Ai,
        Transferred,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "ai",
        clamp = DomainOwned,
        status = "ai-owned",
        migrate = Some("#10909"),
        test = "adjacent_ai_rate_limit_rps",
        rel = None
    ),
    row!(
        "adjacent.testing.timeouts",
        "testing-preferences-timeouts",
        "adjacent.testing",
        Testing,
        NotProven,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "testing",
        clamp = DomainOwned,
        status = "no-runtime-owned-testing-timeout-field",
        migrate = Some("#10898"),
        test = "adjacent_testing_timeouts",
        rel = None
    ),
    row!(
        "adjacent.wire_pressure",
        "runtime-queue-wire-pressure",
        "adjacent.wire_pressure",
        WirePressure,
        NotProven,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "scheduler",
        clamp = DomainOwned,
        status = "no-runtime-limits-queue-field",
        migrate = Some("#10481"),
        test = "adjacent_wire_pressure",
        rel = None
    ),
    row!(
        "adjacent.reload_budgets",
        "reload-end-to-end-operation-budgets",
        "adjacent.reload",
        ReloadBudget,
        NotProven,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "reload",
        clamp = DomainOwned,
        status = "reload-budgets-not-runtime-limits",
        migrate = Some("#9852"),
        test = "adjacent_reload_budgets",
        rel = None
    ),
    row!(
        "adjacent.process_supervisor",
        "process-supervisor-hard-limits",
        "adjacent.process",
        ProcessSupervisor,
        NotProven,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "process",
        clamp = DomainOwned,
        status = "process-owner-contract",
        migrate = None,
        test = "adjacent_process_supervisor",
        rel = None
    ),
    row!(
        "adjacent.feature_semantic_budgets",
        "feature-specific-semantic-work-budgets",
        "adjacent.semantic",
        FeatureSemanticBudget,
        NotProven,
        rust = None,
        schema = None,
        accessor = None,
        site = None,
        marker = None,
        effect = None,
        life = "semantic",
        clamp = DomainOwned,
        status = "exact-domain-owner",
        migrate = None,
        test = "adjacent_feature_semantic_budgets",
        rel = None
    ),
];

/// Machine-check the denominator against current source, schema, and accessors.
pub(crate) fn denominator_violations(rows: &[DenominatorRow]) -> Vec<String> {
    let mut violations = Vec::new();
    check_row_identity(rows, &mut violations);
    check_lsp_limits_fields(rows, &mut violations);
    check_schema_keys(rows, &mut violations);
    check_accessors(rows, &mut violations);
    check_states(rows, &mut violations);
    check_production_sites(rows, &mut violations);
    check_adjacent_not_live(rows, &mut violations);
    check_relational_group(rows, &mut violations);
    check_no_second_store(&mut violations);
    check_families_nonempty(rows, &mut violations);
    violations
}

fn check_row_identity(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let mut ids = BTreeSet::new();
    let mut tests = BTreeSet::new();
    for row in rows {
        if !ids.insert(row.id) {
            violations.push(format!("duplicate denominator id {}", row.id));
        }
        if !tests.insert(row.test_id) {
            violations.push(format!("duplicate test id {}", row.test_id));
        }
        if row.id.is_empty() || row.view_field.is_empty() {
            violations.push(format!("empty identity on {}", row.id));
        }
    }
}

fn check_lsp_limits_fields(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let expected = lsp_limits_leaf_fields();
    let mut seen = BTreeSet::new();
    for row in rows {
        if let Some(field) = row.rust_field {
            seen.insert(field);
        }
    }
    for missing in expected {
        if !seen.contains(missing.as_str()) {
            violations.push(format!(
                "public/configured limit field `{missing}` is absent from the denominator"
            ));
        }
    }
}

fn check_schema_keys(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let schema: serde_json::Value = match serde_json::from_str(include_str!(
        "../../../../../../schemas/perllsp-settings.schema.json"
    )) {
        Ok(value) => value,
        Err(error) => {
            violations.push(format!("settings schema did not parse: {error}"));
            return;
        }
    };
    let Some(properties) =
        schema["properties"]["perl"]["properties"]["limits"]["properties"].as_object()
    else {
        violations.push("schema is missing perl.limits.properties".to_string());
        return;
    };
    let schema_keys: BTreeSet<&str> = properties.keys().map(String::as_str).collect();
    let catalog_keys: BTreeSet<&str> = rows.iter().filter_map(|row| row.schema_key).collect();
    for missing in schema_keys.difference(&catalog_keys) {
        violations.push(format!("schema key `{missing}` is absent from the denominator"));
    }
    for extra in catalog_keys.difference(&schema_keys) {
        violations.push(format!(
            "denominator schema key `{extra}` is not a current perl.limits property"
        ));
    }
}

fn check_accessors(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let accessors = lsp_limits_accessors();
    let mut seen = BTreeSet::new();
    for row in rows {
        if let Some(accessor) = row.accessor {
            seen.insert(accessor);
        }
    }
    for missing in accessors {
        if !seen.contains(missing.as_str()) {
            violations.push(format!("production accessor `{missing}` has no denominator row"));
        }
    }
}

fn check_states(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    for row in rows {
        match row.state {
            LimitConsumerState::Live => {
                if row.production_site.is_none() || row.production_marker.is_none() {
                    violations.push(format!("{} is live without a production site/marker", row.id));
                }
                if row.domain_owner != DomainOwner::RuntimeLimits {
                    violations.push(format!(
                        "{} marks a non-runtime owner as live (absorbed adjacent domain)",
                        row.id
                    ));
                }
            }
            LimitConsumerState::ParsedNoConsumer => {
                if row.production_site.is_some() || row.first_effect.is_some() {
                    violations.push(format!(
                        "{} is parsed_no_consumer but names a first-effect consumer",
                        row.id
                    ));
                }
                if row.schema_key.is_none() && row.accessor.is_none() {
                    violations.push(format!(
                        "{} is parsed_no_consumer without a parse channel or accessor",
                        row.id
                    ));
                }
            }
            LimitConsumerState::NotProven => {
                if row.status_projection.contains("as-zero")
                    || row.status_projection.contains("rendered-current")
                {
                    violations.push(format!(
                        "{} renders missing instrumentation as zero/current",
                        row.id
                    ));
                }
            }
            LimitConsumerState::Transferred
            | LimitConsumerState::FixedInternal
            | LimitConsumerState::Retired => {}
        }
    }
}

fn check_production_sites(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let corpus = production_corpus();
    for row in rows {
        if row.state != LimitConsumerState::Live && row.state != LimitConsumerState::Transferred {
            continue;
        }
        let (Some(site), Some(marker)) = (row.production_site, row.production_marker) else {
            continue;
        };
        let Some(source) = corpus.get(site) else {
            violations.push(format!("{} names unknown production site {site}", row.id));
            continue;
        };
        if !source.contains(marker) {
            violations.push(format!("{} marker `{marker}` is missing from {site}", row.id));
        }
    }

    let live_accessors: BTreeSet<&str> = rows
        .iter()
        .filter(|row| row.state == LimitConsumerState::Live)
        .filter_map(|row| row.accessor)
        .collect();
    for row in rows.iter().filter(|row| row.state == LimitConsumerState::ParsedNoConsumer) {
        let Some(accessor) = row.accessor else {
            continue;
        };
        if live_accessors.contains(accessor) {
            continue;
        }
        let call = format!("{accessor}(");
        for (site, source) in &corpus {
            if *site == "crates/perl-lsp-rs-core/src/runtime/limits/mod.rs" {
                continue;
            }
            if source.contains(&call) {
                violations.push(format!(
                    "{} is parsed_no_consumer but `{call}` appears in {site}",
                    row.id
                ));
            }
        }
    }
}

fn check_adjacent_not_live(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    for row in rows {
        if row.domain_owner == DomainOwner::RuntimeLimits {
            continue;
        }
        if row.state == LimitConsumerState::Live {
            violations.push(format!(
                "adjacent owner {:?} absorbed as live row {}",
                row.domain_owner, row.id
            ));
        }
        if (row.view_field.starts_with("result_caps.")
            || row.view_field.starts_with("provider_deadlines.")
            || row.view_field.starts_with("index_io_deadlines.")
            || row.view_field.starts_with("degradation_policy."))
            && row.domain_owner != DomainOwner::RuntimeLimits
            && row.domain_owner != DomainOwner::WorkspaceIndexedSource
        {
            violations.push(format!(
                "adjacent owner {:?} absorbed into a runtime family field on {}",
                row.domain_owner, row.id
            ));
        }
    }
}

fn check_relational_group(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let grouped: Vec<_> = rows
        .iter()
        .filter(|row| row.relational_group == Some("memory_thresholds"))
        .map(|row| row.rust_field)
        .collect();
    if !grouped.contains(&Some("warning_threshold_bytes"))
        || !grouped.contains(&Some("critical_threshold_bytes"))
    {
        violations.push("memory warning/critical are not one relational group".to_string());
    }
}

fn check_no_second_store(violations: &mut Vec<String>) {
    let accepted_sources = [
        include_str!("mod.rs"),
        include_str!("assemble.rs"),
        include_str!("view.rs"),
        include_str!("change.rs"),
        include_str!("family.rs"),
        include_str!("identity.rs"),
        include_str!("lookup.rs"),
        include_str!("values.rs"),
    ];
    for source in accepted_sources {
        if source.contains("LazyLock<") || source.contains("static ACCEPTED") {
            violations.push("accepted view introduced a second mutable store".to_string());
        }
    }
    let limits = include_str!("../mod.rs");
    let stores = limits.matches("LazyLock::new").count();
    if stores != 1 {
        violations.push(format!("expected exactly one LazyLock limits store, found {stores}"));
    }
}

fn check_families_nonempty(rows: &[DenominatorRow], violations: &mut Vec<String>) {
    let mut families: BTreeSet<&str> = BTreeSet::new();
    for row in rows {
        let family = row.view_field.split('.').next().unwrap_or(row.view_field);
        families.insert(family);
    }
    let required = [
        "result_caps",
        "source_admission",
        "provider_deadlines",
        "index_io_deadlines",
        "memory_cache",
        "degradation_policy",
        "adjacent",
    ];
    for family in required {
        if !families.contains(family) {
            violations.push(format!("required family {family} is missing"));
        }
    }
}

fn lsp_limits_leaf_fields() -> BTreeSet<String> {
    let source = include_str!("../mod.rs");
    let mut fields = public_fields(source, "LspLimits");
    fields.remove("memory_budget");
    fields.extend(public_fields(source, "MemoryBudget"));
    fields
}

fn public_fields(source: &str, struct_name: &str) -> BTreeSet<String> {
    let marker = format!("pub struct {struct_name} {{");
    let Some((_, rest)) = source.split_once(&marker) else {
        return BTreeSet::new();
    };
    let Some((body, _)) = rest.split_once("\n}") else {
        return BTreeSet::new();
    };
    let mut fields = BTreeSet::new();
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() {
            fields.insert(name.to_string());
        }
    }
    fields
}

fn lsp_limits_accessors() -> BTreeSet<String> {
    let source = include_str!("../mod.rs");
    let Some((_, after_static)) = source.split_once("pub static LSP_LIMITS") else {
        return BTreeSet::new();
    };
    let after_static = after_static.split("#[cfg(test)]").next().unwrap_or(after_static);
    let mut accessors = BTreeSet::new();
    for line in after_static.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub fn ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once('(') else {
            continue;
        };
        accessors.insert(name.trim().to_string());
    }
    accessors
}

fn production_corpus() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        (WS, include_str!("../../../../../perl-lsp-rs/src/runtime/workspace.rs")),
        (REFS, include_str!("../../../../../perl-lsp-rs/src/runtime/language/references.rs")),
        (COMP, include_str!("../../../../../perl-lsp-rs/src/runtime/language/completion.rs")),
        (DSYM, include_str!("../../../../../perl-lsp-rs/src/runtime/language/symbols.rs")),
        (MISC, include_str!("../../../../../perl-lsp-rs/src/runtime/language/misc.rs")),
        (SEM, include_str!("../../../../../perl-lsp-rs/src/runtime/language/semantic_tokens.rs")),
        (NAV, include_str!("../../../../../perl-lsp-rs/src/runtime/language/navigation.rs")),
        (SYNC, include_str!("../../../../../perl-lsp-rs/src/runtime/text_sync.rs")),
        (SEC, include_str!("../../../../../perl-lsp-rs/src/security/config.rs")),
        (FILE_VAL, include_str!("../../input_validation/file_validation.rs")),
        (LSP_VAL, include_str!("../../input_validation/lsp_validation.rs")),
        (TYPEDEF, include_str!("../../../providers/navigation/type_definition.rs")),
        (WS_MON, include_str!("../../../../../perl-workspace/src/monitoring/mod.rs")),
        ("crates/perl-lsp-rs-core/src/runtime/limits/mod.rs", include_str!("../mod.rs")),
    ])
}
