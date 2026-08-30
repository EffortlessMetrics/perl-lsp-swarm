//! Executable ownership contract for the current `LspServer` state bag.
//!
//! Issue #8383 is intentionally inventory-only: this test records the target
//! owner and lifecycle boundary for every production field before later PRs
//! move state. A new field must therefore arrive with an explicit disposition
//! rather than silently expanding the service locator.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TargetOwner {
    ClientSession,
    DocumentStore,
    WorkspaceServices,
    AnalysisServices,
    RuntimeServices,
    ClientTransport,
    ProductComposition,
}

#[derive(Debug, Clone, Copy)]
struct OwnershipRow {
    field: &'static str,
    owner: TargetOwner,
    synchronization: &'static str,
    reset_boundary: &'static str,
    identity: &'static str,
    blocking_work_reachable: bool,
    migration_issue: &'static str,
}

macro_rules! row {
    (
        $field:literal,
        $owner:ident,
        $sync:literal,
        $reset:literal,
        $identity:literal,
        $blocking:literal,
        $issue:literal
    ) => {
        OwnershipRow {
            field: $field,
            owner: TargetOwner::$owner,
            synchronization: $sync,
            reset_boundary: $reset,
            identity: $identity,
            blocking_work_reachable: $blocking,
            migration_issue: $issue,
        }
    };
}

const OWNERSHIP: &[OwnershipRow] = &[
    row!(
        "documents",
        DocumentStore,
        "Arc<Mutex>",
        "document close / session shutdown",
        "document instance + generation",
        false,
        "#8384"
    ),
    row!(
        "backing_file_transitions",
        DocumentStore,
        "Arc<Mutex>",
        "save/close authority handoff",
        "document URI + observed transition",
        false,
        "#8041"
    ),
    row!(
        "initialize_requested",
        ClientSession,
        "AtomicBool",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "initialized",
        ClientSession,
        "AtomicBool",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "shutdown_received",
        ClientSession,
        "AtomicBool",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "pending_startup_log",
        ClientSession,
        "Arc<Mutex>",
        "initialized or connection shutdown",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "index_coordinator",
        WorkspaceServices,
        "Option<Arc>",
        "workspace root removal / runtime shutdown",
        "workspace-set generation",
        true,
        "#8385"
    ),
    row!(
        "symbol_index",
        AnalysisServices,
        "Arc<Mutex>",
        "document/root removal / analysis shutdown",
        "project fact generation",
        false,
        "#6957"
    ),
    row!(
        "config",
        ClientSession,
        "Arc<Mutex>",
        "configuration generation / connection replacement",
        "configuration generation",
        false,
        "#8386"
    ),
    row!(
        "reader",
        ClientTransport,
        "Arc<Mutex>",
        "connection shutdown",
        "connection",
        true,
        "#9509"
    ),
    row!(
        "outbound",
        ClientTransport,
        "bounded sender",
        "connection shutdown",
        "connection",
        false,
        "#9506"
    ),
    row!(
        "outbound_writer_handle",
        ClientTransport,
        "owned JoinHandle",
        "connection shutdown",
        "connection",
        true,
        "#9507"
    ),
    row!(
        "client_capabilities",
        ClientSession,
        "Mutex",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "cancelled",
        ClientSession,
        "Arc<Mutex>",
        "request terminal / connection shutdown",
        "request ID",
        false,
        "#7098"
    ),
    row!(
        "pending_request_ids",
        ClientSession,
        "Arc<Mutex>",
        "request terminal / connection shutdown",
        "request ID",
        false,
        "#7098"
    ),
    row!(
        "workspace_folders",
        WorkspaceServices,
        "Arc<Mutex>",
        "workspace-set transition",
        "workspace-set generation",
        false,
        "#8385"
    ),
    row!(
        "root_path",
        WorkspaceServices,
        "Arc<Mutex>",
        "workspace-set transition",
        "workspace-set generation",
        false,
        "#8385"
    ),
    row!(
        "discovered_perltidy_profile",
        WorkspaceServices,
        "Arc<Mutex>",
        "root/configuration transition",
        "root + configuration generation",
        true,
        "#8385"
    ),
    row!(
        "advertised_features",
        ClientSession,
        "Mutex",
        "connection replacement",
        "initialize surface identity",
        false,
        "#8386"
    ),
    row!(
        "advertised_feature_ids",
        ClientSession,
        "Mutex",
        "connection replacement",
        "initialize surface identity",
        false,
        "#8386"
    ),
    row!(
        "text_sync_session",
        ClientSession,
        "Mutex<Option<AcceptedTextSyncSession>>",
        "connection replacement",
        "accepted text-sync session contract (#9378): immutable FULL + UTF-16 authority written once at initialize acceptance",
        false,
        "#9378"
    ),
    row!(
        "client_supports_pull_diags",
        ClientSession,
        "Arc<AtomicBool>",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "workspace_config",
        WorkspaceServices,
        "Arc<Mutex>",
        "configuration generation",
        "root + configuration generation",
        false,
        "#8385"
    ),
    row!(
        "initialization_options_perl_settings",
        ClientSession,
        "Arc<Mutex>",
        "connection replacement",
        "client session + configuration generation",
        false,
        "#8386"
    ),
    row!(
        "next_request_id",
        ClientSession,
        "Arc<AtomicI32>",
        "connection replacement",
        "server request ID domain",
        false,
        "#7007"
    ),
    row!(
        "pending_workspace_configuration_requests",
        ClientSession,
        "Arc<Mutex>",
        "response/timeout/connection shutdown",
        "server request ID",
        false,
        "#7007"
    ),
    row!(
        "progress_tokens",
        ClientSession,
        "Arc<Mutex>",
        "operation terminal / connection shutdown",
        "progress token",
        false,
        "#6729"
    ),
    row!(
        "progress_token_to_request",
        ClientSession,
        "Arc<Mutex>",
        "operation terminal / connection shutdown",
        "progress token + request ID",
        false,
        "#6729"
    ),
    row!(
        "refresh_controller",
        RuntimeServices,
        "owned service",
        "runtime shutdown",
        "runtime + configuration generation",
        true,
        "#8388"
    ),
    row!(
        "diagnostic_debouncer",
        RuntimeServices,
        "Mutex<Option>",
        "application shutdown",
        "runtime + document generation",
        true,
        "#9508"
    ),
    row!(
        "parse_worker_handle",
        RuntimeServices,
        "Mutex<Option<Arc>>",
        "application shutdown",
        "runtime + document generation",
        true,
        "#9508"
    ),
    row!(
        "file_watcher_debouncer",
        RuntimeServices,
        "Mutex<Option>",
        "application shutdown",
        "runtime + root generation",
        true,
        "#9508"
    ),
    row!(
        "notebook_store",
        DocumentStore,
        "owned store",
        "notebook close / session shutdown",
        "notebook document generation",
        false,
        "#8384"
    ),
    row!(
        "trace_level",
        ClientSession,
        "Arc<Mutex>",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "resolve_session_authenticator",
        ClientTransport,
        "Mutex",
        "connection shutdown",
        "connection",
        false,
        "#8342"
    ),
    row!(
        "stream_session_manager",
        RuntimeServices,
        "owned service",
        "request/session terminal",
        "stream session + document generation",
        true,
        "#8388"
    ),
    row!(
        "feature_profile",
        ProductComposition,
        "immutable",
        "process restart",
        "product profile",
        false,
        "#8400"
    ),
    row!(
        "runtime_tuning",
        ProductComposition,
        "immutable",
        "process restart",
        "runtime profile",
        false,
        "#8400"
    ),
    row!(
        "workspace_indexing_invocation_count",
        WorkspaceServices,
        "Arc<AtomicUsize>",
        "server instance drop",
        "server instance",
        false,
        "#8385"
    ),
    row!(
        "readiness_receipt_observer_id",
        RuntimeServices,
        "AtomicU64",
        "test observer detach / server drop",
        "test observer",
        false,
        "#9510"
    ),
    row!(
        "active_document_readiness",
        RuntimeServices,
        "Mutex",
        "open-document close eviction / server instance drop",
        "open-document readiness states",
        false,
        "#11675"
    ),
    row!(
        "push_diagnostics_sink",
        RuntimeServices,
        "Mutex",
        "server instance drop",
        "per-URI committed diagnostic tickets",
        false,
        "#11673"
    ),
    row!(
        "document_symbols_sink",
        RuntimeServices,
        "Mutex",
        "server instance drop",
        "per-URI committed symbol tickets",
        false,
        "#11674"
    ),
    row!(
        "workspace_readiness_receipt",
        WorkspaceServices,
        "Arc<Mutex>",
        "workspace generation transition",
        "workspace generation",
        false,
        "#8385"
    ),
    row!(
        "workspace_indexing_start_gate",
        RuntimeServices,
        "Arc<std::sync::Mutex>",
        "test gate release / server drop",
        "test runtime",
        true,
        "#7394"
    ),
    row!(
        "pod_cache",
        AnalysisServices,
        "Arc<Mutex>",
        "source/root invalidation / analysis shutdown",
        "logical source + content revision",
        true,
        "#6957"
    ),
    row!(
        "provider_decision_traces",
        AnalysisServices,
        "Arc<Mutex>",
        "request replacement / server drop",
        "request + provider decision",
        false,
        "#6957"
    ),
    row!(
        "semantic_tokens_cache",
        DocumentStore,
        "Arc<Mutex>",
        "document close/change",
        "document generation + result ID",
        false,
        "#8384"
    ),
    row!(
        "module_scan_cache",
        WorkspaceServices,
        "Arc",
        "TTL / root transition",
        "root + configuration generation",
        true,
        "#8385"
    ),
    row!(
        "use_lib_hir_cache",
        AnalysisServices,
        "Arc<Mutex>",
        "document/root invalidation",
        "source + semantic generation",
        false,
        "#6957"
    ),
    row!(
        "pending_index_task_count",
        RuntimeServices,
        "Arc<AtomicUsize>",
        "task terminal / runtime shutdown",
        "runtime task",
        false,
        "#8388"
    ),
    row!(
        "parse_cancel_flags",
        RuntimeServices,
        "Arc<Mutex>",
        "parse terminal / document close",
        "document instance + generation",
        false,
        "#8388"
    ),
    row!(
        "pull_diagnostics_orchestrator",
        AnalysisServices,
        "owned service",
        "analysis shutdown / configuration transition",
        "diagnostic operation + document generation",
        true,
        "#6957"
    ),
    row!(
        "indexing_in_progress",
        RuntimeServices,
        "Arc<AtomicBool>",
        "index task terminal / runtime shutdown",
        "workspace generation",
        false,
        "#8388"
    ),
    row!(
        "indexing_rescan_pending",
        RuntimeServices,
        "Arc<AtomicBool>",
        "index handoff / runtime shutdown",
        "workspace generation",
        false,
        "#8388"
    ),
    row!(
        "indexing_transition_lock",
        RuntimeServices,
        "Arc<Mutex>",
        "index handoff / runtime shutdown",
        "workspace generation",
        false,
        "#8388"
    ),
    row!(
        "permission_denied_shown",
        WorkspaceServices,
        "Arc<AtomicBool>",
        "server session",
        "workspace + session",
        false,
        "#8385"
    ),
    row!(
        "root_undetected_shown",
        ClientSession,
        "Arc<AtomicBool>",
        "connection replacement",
        "client session",
        false,
        "#8386"
    ),
    row!(
        "critic_analyzer",
        AnalysisServices,
        "Mutex<Option>",
        "critic config transition / analysis shutdown",
        "configuration + document generation",
        true,
        "#7410"
    ),
    row!(
        "critic_runtime_override",
        ProductComposition,
        "Mutex<Option<Arc>>",
        "test/product composition reset",
        "process/test subject",
        true,
        "#8400"
    ),
    row!(
        "formatter_runtime_override",
        ProductComposition,
        "Mutex<Option<Arc>>",
        "test/product composition reset",
        "process/test subject",
        true,
        "#5001"
    ),
    row!(
        "skip_perlcritic_command_check",
        ProductComposition,
        "AtomicBool",
        "test server drop",
        "test subject",
        false,
        "#8400"
    ),
    row!(
        "force_perlcritic_command_unavailable",
        ProductComposition,
        "AtomicBool",
        "test server drop",
        "test subject",
        false,
        "#8400"
    ),
    row!(
        "session_warning_dedup",
        ClientSession,
        "owned store (per-family Mutex)",
        "connection replacement + family lifecycle clear",
        "typed warning family/code/subject fingerprint",
        false,
        "#9769"
    ),
    row!(
        "diagnostic_after_snapshot_hook",
        RuntimeServices,
        "Mutex<Option<Box>>",
        "test hook release / server drop",
        "test runtime",
        true,
        "#7394"
    ),
    row!(
        "document_symbols_before_commit_hook",
        RuntimeServices,
        "Mutex<Option<Box>>",
        "test hook release / server drop",
        "test runtime",
        true,
        "#11674"
    ),
    row!(
        "document_symbols_before_install_hook",
        RuntimeServices,
        "Mutex<Option<Box>>",
        "test hook release / server drop",
        "test runtime",
        true,
        "#11674"
    ),
    row!(
        "ai_inline_backend",
        ProductComposition,
        "Mutex<Option<Arc>>",
        "AI config transition / process shutdown",
        "configuration + backend subject",
        true,
        "#8400"
    ),
    row!(
        "incremental_eager",
        ProductComposition,
        "AtomicBool",
        "server drop",
        "runtime profile",
        false,
        "#8400"
    ),
];

fn repo_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("xtask must live beneath the repository root"))?
        .to_path_buf())
}

/// Collapse the `LspServer` struct body into its significant declaration text.
///
/// Comments and attributes are dropped line-wise so that neither doc prose nor
/// a `#[cfg(...)]` predicate can inject braces, angle brackets, or commas into
/// the declaration scan. Anything this scan cannot classify is a hard failure
/// rather than a silent omission: a guard that quietly skips a declaration
/// would let an unowned field land while still reporting green.
fn lsp_server_body(source: &str) -> Result<String> {
    let marker = "pub struct LspServer {";
    let start = source
        .find(marker)
        .ok_or_else(|| anyhow!("LspServer declaration must remain discoverable"))?;

    let mut body = String::new();
    let mut terminated = false;
    for line in source[start + marker.len()..].lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        ensure!(
            !line.starts_with("/*"),
            "block comments are not supported by the #8383 ownership scan: {line}"
        );
        if let Some(attribute) = line.strip_prefix("#[") {
            ensure!(
                attribute.ends_with(']'),
                "multi-line attributes are not supported by the #8383 ownership scan: {line}"
            );
            continue;
        }
        if line == "}" {
            terminated = true;
            break;
        }

        body.push_str(line);
        body.push(' ');
    }

    ensure!(terminated, "LspServer declaration must close with a sole `}}` line");
    Ok(body)
}

/// Split a struct body into one string per field declaration.
///
/// Splitting happens only on commas outside every bracket pair, so a trailing
/// comma inside a multi-line generic argument list does not manufacture a
/// second declaration. `->` is consumed as one token because the `>` of an
/// `Fn` return arrow is not a closing angle bracket.
fn split_declarations(body: &str) -> Result<Vec<String>> {
    let mut declarations = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut characters = body.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '<' | '(' | '[' => {
                depth += 1;
                current.push(character);
            }
            '>' | ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            '-' if characters.peek() == Some(&'>') => {
                characters.next();
                current.push_str("->");
            }
            ',' if depth == 0 => {
                declarations.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(character),
        }
    }

    let trailing = current.trim();
    ensure!(
        trailing.is_empty(),
        "every LspServer declaration must end with a top-level comma: {trailing}"
    );

    declarations.retain(|declaration| !declaration.is_empty());
    Ok(declarations)
}

/// Remove any leading visibility qualifier from one declaration.
///
/// Every restricted form counts: `pub`, `pub(crate)`, `pub(super)`, and
/// `pub(in some::path)`. Only `pub` and `pub(crate)` appear today, but a field
/// added under another form must not slip past the recurrence guard.
fn strip_visibility(declaration: &str) -> Result<&str> {
    let Some(rest) = declaration.strip_prefix("pub") else {
        return Ok(declaration);
    };

    match rest.chars().next() {
        Some('(') => {
            let close = rest
                .find(')')
                .ok_or_else(|| anyhow!("unterminated visibility qualifier: {declaration}"))?;
            Ok(rest[close + 1..].trim_start())
        }
        Some(character) if character.is_whitespace() => Ok(rest.trim_start()),
        // A field whose own name merely starts with `pub`, such as `published`.
        _ => Ok(declaration),
    }
}

fn is_field_identifier(candidate: &str) -> bool {
    let candidate = candidate.strip_prefix("r#").unwrap_or(candidate);

    !candidate.is_empty()
        && !candidate.starts_with(|character: char| character.is_ascii_digit())
        && candidate.chars().all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn declaration_field_name(declaration: &str) -> Result<String> {
    let (name, _) = strip_visibility(declaration)?
        .split_once(':')
        .ok_or_else(|| anyhow!("LspServer declaration must name a field: {declaration}"))?;
    let name = name.trim();

    ensure!(is_field_identifier(name), "unrecognized LspServer declaration: {declaration}");
    Ok(name.to_string())
}

fn discover_lsp_server_fields(source: &str) -> Result<BTreeSet<String>> {
    let names = split_declarations(&lsp_server_body(source)?)?
        .iter()
        .map(|declaration| declaration_field_name(declaration))
        .collect::<Result<Vec<String>>>()?;

    let unique: BTreeSet<String> = names.iter().cloned().collect();
    ensure!(names.len() == unique.len(), "LspServer declares a duplicate field name");
    Ok(unique)
}

fn governed_fields() -> BTreeSet<String> {
    OWNERSHIP.iter().map(|row| row.field.to_string()).collect()
}

/// Fields present in the source that no ownership row claims.
fn unclassified_fields(source: &str) -> Result<BTreeSet<String>> {
    Ok(discover_lsp_server_fields(source)?.difference(&governed_fields()).cloned().collect())
}

/// Ownership rows describing a field the source no longer declares.
fn stale_ownership_rows(source: &str) -> Result<BTreeSet<String>> {
    Ok(governed_fields().difference(&discover_lsp_server_fields(source)?).cloned().collect())
}

#[test]
fn ownership_map_covers_every_current_lsp_server_field() -> Result<()> {
    let source = fs::read_to_string(repo_root()?.join("crates/perl-lsp-rs/src/runtime/mod.rs"))?;

    assert_eq!(
        discover_lsp_server_fields(&source)?,
        governed_fields(),
        "LspServer fields and the #8383 ownership map must move together"
    );

    Ok(())
}

/// `LspServer` fields that legitimately own reviewed string-keyed sets/maps
/// under their own #8383 rows. Any OTHER string-keyed `HashSet`/`HashMap`
/// declaration on the struct is raw unbounded state and fails the #9769
/// negative control -- whatever it is named.
const STRING_KEYED_STATE_FIELDS: &[&str] = &[
    // Transport/session bookkeeping with per-item removal semantics.
    "progress_tokens",
    "progress_token_to_request",
    // Per-document stores governed by #8384/#8388.
    "documents",
    "parse_cancel_flags",
    "backing_file_transitions",
    // Reviewed analysis surfaces governed by #6957/#8384.
    "provider_decision_traces",
    "semantic_tokens_cache",
];

/// The declared type text of one field declaration (everything after the
/// first top-level `:`), or `None` when the declaration has no field shape.
fn declaration_type(declaration: &str) -> Option<&str> {
    strip_visibility(declaration).ok()?.split_once(':').map(|(_, rest)| rest.trim())
}

/// True when a declaration's type is a string-keyed set or map -- the raw
/// shape that #9769 removed from warning suppression. Name-independent: a
/// renamed `dedup_keys: Mutex<HashSet<String>>` is still raw state.
fn is_string_keyed_collection(declaration: &str) -> bool {
    declaration_type(declaration)
        .is_some_and(|ty| ty.contains("HashSet<String") || ty.contains("HashMap<String"))
}

/// Extract one struct body (between the marker line and its closing brace).
fn struct_body(source: &str, marker: &str) -> Result<String> {
    let start = source
        .find(marker)
        .ok_or_else(|| anyhow!("struct declaration must remain discoverable: {marker}"))?;
    let mut body = String::new();
    for line in source[start + marker.len()..].lines() {
        let line = line.trim();
        if line == "}" {
            return Ok(body);
        }
        body.push_str(line);
        body.push(' ');
    }
    Err(anyhow!("struct declaration must close with a sole `}}` line: {marker}"))
}

/// #9769 negative control: a governed session-warning family may not return
/// as a raw string-keyed set. The check is structural, not name-based: every
/// string-keyed `HashSet`/`HashMap` declaration on `LspServer` must belong to
/// the reviewed allowlist above, and `PullDiagnosticsOrchestrator` (which
/// previously kept its own unbounded `warnings_sent` set beside the governed
/// critic family) may declare no collection state at all.
#[test]
fn session_warning_dedup_is_not_a_raw_string_set() -> Result<()> {
    let source = fs::read_to_string(repo_root()?.join("crates/perl-lsp-rs/src/runtime/mod.rs"))?;
    let body = lsp_server_body(&source)?;
    let declarations = split_declarations(&body)?;

    for declaration in &declarations {
        if !is_string_keyed_collection(declaration) {
            continue;
        }
        let name = declaration_field_name(declaration)?;
        ensure!(
            STRING_KEYED_STATE_FIELDS.contains(&name.as_str()),
            "LspServer must not retain raw string-keyed collection state outside the reviewed allowlist (#9769): {declaration}"
        );
    }

    let diagnostics =
        fs::read_to_string(repo_root()?.join("crates/perl-lsp-rs/src/runtime/diagnostics.rs"))?;
    let orchestrator = struct_body(&diagnostics, "pub struct PullDiagnosticsOrchestrator {")?;
    ensure!(
        !orchestrator.contains("HashSet") && !orchestrator.contains("HashMap"),
        "the pull diagnostics orchestrator must route warning suppression through the bounded #9769 store, not its own collection state: {orchestrator}"
    );
    ensure!(
        !diagnostics.contains("warnings_sent"),
        "the #9769 pull-path warning store name must not return"
    );

    ensure!(
        declarations.iter().any(|declaration| declaration.starts_with(
            "pub(crate) session_warning_dedup: session_warning_dedup::SessionWarningDedupStore"
        )),
        "the bounded #9769 warning-dedup store must remain a governed field"
    );

    Ok(())
}

/// Regression fixtures for the structural scan: renamed raw warning stores
/// must be flagged whatever they are called, and allowlisted reviewed state
/// must keep passing.
#[test]
fn renamed_raw_warning_stores_cannot_escape_the_scan() -> Result<()> {
    // A renamed raw string set evades name-based checks but not the
    // structural type scan.
    let renamed = r#"
pub struct LspServer {
    documents: Arc<Mutex<HashMap<String, DocumentState>>>,
    dedup_keys: Mutex<HashSet<String>>,
}
"#;
    let body = lsp_server_body(renamed)?;
    let declarations = split_declarations(&body)?;
    let flagged: Vec<String> = declarations
        .iter()
        .filter(|declaration| {
            is_string_keyed_collection(declaration)
                && !STRING_KEYED_STATE_FIELDS
                    .contains(&declaration_field_name(declaration).unwrap_or_default().as_str())
        })
        .cloned()
        .collect();
    assert_eq!(flagged.len(), 1, "the renamed raw store must be flagged");
    assert!(flagged[0].contains("dedup_keys"));

    // Reviewed allowlisted state keeps passing the same scan.
    let governed = r#"
pub struct LspServer {
    progress_tokens: Arc<Mutex<HashSet<String>>>,
    provider_decision_traces: Arc<Mutex<HashMap<String, Value>>>,
}
"#;
    let governed_declarations = split_declarations(&lsp_server_body(governed)?)?;
    assert!(governed_declarations.iter().all(|declaration| {
        !is_string_keyed_collection(declaration)
            || STRING_KEYED_STATE_FIELDS
                .contains(&declaration_field_name(declaration).unwrap_or_default().as_str())
    }));

    // The orchestrator scan rejects a renamed set too.
    let orchestrator_fixture = r#"
pub struct PullDiagnosticsOrchestrator {
    critic_analyzer: Mutex<Option<CriticAnalyzer>>,
    warning_keys: Mutex<HashSet<String>>,
}
"#;
    let orchestrator_body =
        struct_body(orchestrator_fixture, "pub struct PullDiagnosticsOrchestrator {")?;
    assert!(
        orchestrator_body.contains("HashSet"),
        "fixture must carry the renamed store for the assertion below"
    );

    Ok(())
}

#[test]
fn ownership_rows_are_unique_and_complete() {
    let mut rows = BTreeMap::new();
    for row in OWNERSHIP {
        assert!(rows.insert(row.field, row).is_none(), "duplicate ownership row for {}", row.field);
        assert!(!row.synchronization.trim().is_empty());
        assert!(!row.reset_boundary.trim().is_empty());
        assert!(!row.identity.trim().is_empty());
        assert!(row.migration_issue.starts_with('#'));

        if row.blocking_work_reachable {
            assert!(
                matches!(
                    row.owner,
                    TargetOwner::AnalysisServices
                        | TargetOwner::RuntimeServices
                        | TargetOwner::ClientTransport
                        | TargetOwner::WorkspaceServices
                        | TargetOwner::ProductComposition
                ),
                "blocking work for {} must have an execution-owning target",
                row.field
            );
        }
    }
}

#[test]
fn new_fields_keep_the_decided_ownership() {
    let backing_file = OWNERSHIP.iter().find(|row| row.field == "backing_file_transitions");
    assert_eq!(
        backing_file.map(|row| (
            row.owner,
            row.synchronization,
            row.reset_boundary,
            row.identity,
            row.blocking_work_reachable,
            row.migration_issue
        )),
        Some((
            TargetOwner::DocumentStore,
            "Arc<Mutex>",
            "save/close authority handoff",
            "document URI + observed transition",
            false,
            "#8041"
        ))
    );

    let formatter = OWNERSHIP.iter().find(|row| row.field == "formatter_runtime_override");
    assert_eq!(
        formatter.map(|row| (
            row.owner,
            row.synchronization,
            row.reset_boundary,
            row.identity,
            row.blocking_work_reachable,
            row.migration_issue
        )),
        Some((
            TargetOwner::ProductComposition,
            "Mutex<Option<Arc>>",
            "test/product composition reset",
            "process/test subject",
            true,
            "#5001"
        ))
    );
}

#[test]
fn a_new_unowned_field_is_rejected() -> Result<()> {
    let source = r#"
pub struct LspServer {
    documents: Store,
    initialize_requested: Flag,
    newly_added_state: State,
}
"#;

    assert_eq!(unclassified_fields(source)?, BTreeSet::from(["newly_added_state".to_string()]));

    Ok(())
}

#[test]
fn a_restricted_visibility_field_is_rejected() -> Result<()> {
    let source = r#"
pub struct LspServer {
    documents: Store,
    pub(crate) initialize_requested: Flag,
    pub(super) sibling_visible_state: State,
    pub(in crate::runtime) path_visible_state: State,
}
"#;

    assert_eq!(
        unclassified_fields(source)?,
        BTreeSet::from(["sibling_visible_state".to_string(), "path_visible_state".to_string()]),
        "restricted visibility must not hide a field from the ownership guard"
    );

    Ok(())
}

#[test]
fn a_removed_field_is_reported_as_a_stale_row() -> Result<()> {
    let source = r#"
pub struct LspServer {
    documents: Store,
}
"#;

    let stale = stale_ownership_rows(source)?;

    assert!(
        stale.contains("initialize_requested"),
        "a dropped field must leave its ownership row visible as stale"
    );
    assert!(!stale.contains("documents"), "a retained field must not be reported as stale");

    Ok(())
}

#[test]
fn declaration_shapes_do_not_invent_or_lose_fields() -> Result<()> {
    let source = r#"
pub struct LspServer {
    /// Doc prose with a colon: and a `#[cfg(...)]` mention, plus <angles>.
    #[cfg(all(feature = "workspace", any(test, feature = "expose_lsp_test_api")))]
    documents: Store,
    pub(crate) wrapped:
        std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>,
    pub(crate) callback: Mutex<Option<Box<dyn Fn(&str) -> bool + Send + Sync>>>,
    pub(crate) nested: Mutex<
        Option<std::sync::Arc<dyn Backend>>,
    >,
}
"#;

    assert_eq!(
        discover_lsp_server_fields(source)?,
        BTreeSet::from([
            "documents".to_string(),
            "wrapped".to_string(),
            "callback".to_string(),
            "nested".to_string(),
        ]),
        "path-qualified, multi-line, and function-typed fields must resolve exactly"
    );

    Ok(())
}
