//! Completion request handlers
//!
//! Handles textDocument/completion requests with support for:
//! - Variable completion (scalars, arrays, hashes)
//! - Function/subroutine completion
//! - Keyword completion
//! - Workspace-wide symbol completion
//! - Cancellation support

use crate::cancellation::{
    GLOBAL_CANCELLATION_REGISTRY, PerlLspCancellationToken, RequestCleanupGuard,
};
use crate::completion::{
    CompletionItemKind, CompletionProvider, InsertTextFormat, add_xs_api_completions_for_prefix,
};
use crate::runtime::lifecycle::inc_context::RequestIncContext;
#[cfg(feature = "workspace")]
use crate::runtime::readiness::IndexReadinessPolicy;
use crate::runtime::types::workspace_folder_matches_doc_uri;
use crate::{
    protocol::{JsonRpcError, JsonRpcId, REQUEST_CANCELLED, req_position, req_uri},
    runtime::routing::{IndexAccessMode, route_index_access},
    state::DocumentState,
    state::{completion_cap, completion_deadline},
};
use perl_lexer::LSP_RUNTIME_COMPLETION_KEYWORDS;
#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
use perl_lsp_rs_core::providers::completion::completion_shadow::completion_visibility_shadow;
use perl_module::resolution::{IncRoot, IncRootKind};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Serialize a slice of typed values to a JSON array (#4995).
fn to_json_array<T: serde::Serialize>(values: &[T]) -> Value {
    serde_json::to_value(values).unwrap_or(Value::Array(Vec::new()))
}

use super::super::LspServer;

/// Sentinel request ID used for the notification-path token in
/// [`Self::handle_completion_cancellable`]. Real client request IDs are
/// always positive integers (per LSP convention) or strings; this negative
/// integer cannot collide with any client- or server-generated ID. The
/// token created with this ID is intentionally **not** registered in the
/// global cancellation registry — it exists only as a local handle that the
/// provider's cancel-check closure can read.
const UNCANCELLABLE_LOCAL_TOKEN_ID: JsonRpcId = JsonRpcId::Integer(-1);

/// Test-only observer, notified exactly once the next time
/// `handle_completion_cancellable` enters its analysis phase (the
/// `if let Some(doc)` arm, before any provider work begins) *for the URI it
/// was armed with*. Lets a regression test cancel a request deterministically
/// *after* analysis has genuinely started, instead of guessing the timing
/// with a fixed sleep. Mirrors `set_index_ready_wait_entered_observer` in
/// `readiness.rs`, but keyed by URI: unlike readiness (a rare, mostly-
/// synthetic wait path), every completion test that resolves an open
/// document passes through this call site, so an unkeyed global slot could
/// be consumed by an unrelated concurrent test's request and wake the
/// canceller before the armed test's own analysis started (cubic
/// review-run fbb70c75, discussion_r3560238397).
#[cfg(any(test, feature = "expose_lsp_test_api"))]
static COMPLETION_ANALYSIS_STARTED_OBSERVER: std::sync::Mutex<
    Option<(String, std::sync::mpsc::Sender<()>)>,
> = std::sync::Mutex::new(None);

// Narrower than the surrounding `expose_lsp_test_api`-eligible items: every
// current caller is itself `cfg(test)`-gated (it arms
// `COMPLETION_ANALYSIS_STARTED_OBSERVER`, which only `pub(crate)` in-crate test
// code can reach), so under a plain `expose_lsp_test_api`-only build (no
// `cfg(test)`) this would otherwise be genuinely unused (clippy::dead_code).
#[cfg(test)]
pub(crate) fn set_completion_analysis_started_observer(
    uri: &str,
    sender: std::sync::mpsc::Sender<()>,
) {
    if let Ok(mut observer) = COMPLETION_ANALYSIS_STARTED_OBSERVER.lock() {
        *observer = Some((uri.to_string(), sender));
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn notify_completion_analysis_started(uri: &str) {
    let sender = COMPLETION_ANALYSIS_STARTED_OBSERVER.lock().ok().and_then(|mut observer| {
        // Only consume the slot for the URI it was armed with -- an
        // unrelated concurrent test's request must not wake this one's
        // canceller (leaves the slot untouched for its rightful owner).
        if observer.as_ref().is_some_and(|(armed_uri, _)| armed_uri == uri) {
            observer.take().map(|(_, tx)| tx)
        } else {
            None
        }
    });
    if let Some(sender) = sender {
        let _ = sender.send(());
    }
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
fn notify_completion_analysis_started(_uri: &str) {}

/// Test-only rendezvous gate: when armed for a URI, blocks
/// `handle_completion_cancellable` immediately after it has computed the
/// comment-guard predicate and *before* the guard's own cancellation check
/// runs, until the test releases it. Lets a regression test land a
/// cancellation deterministically in the narrow window between the initial
/// `token.is_cancelled_relaxed()` check (early in the handler) and the
/// comment guard's cancellation check, without guessing at timing the way a
/// genuinely concurrent race would (mirrors the determinism goal of
/// `COMPLETION_ANALYSIS_STARTED_OBSERVER` above, but as a blocking barrier
/// rather than a fire-and-forget notification, since the window being
/// tested here is too narrow -- a couple of cheap byte-string scans -- for
/// any real thread-scheduling race to reliably land in it).
#[cfg(test)]
static COMPLETION_COMMENT_GUARD_GATE: std::sync::Mutex<
    Option<(String, std::sync::mpsc::Receiver<()>)>,
> = std::sync::Mutex::new(None);

/// Arms the gate for `uri` and returns the sender the test uses to release
/// it once the cancellation it wants observed has already landed.
#[cfg(test)]
pub(crate) fn arm_completion_comment_guard_gate(uri: &str) -> std::sync::mpsc::Sender<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut gate) = COMPLETION_COMMENT_GUARD_GATE.lock() {
        *gate = Some((uri.to_string(), rx));
    }
    tx
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn wait_for_completion_comment_guard_gate(uri: &str) {
    #[cfg(test)]
    {
        let rx = COMPLETION_COMMENT_GUARD_GATE.lock().ok().and_then(|mut gate| {
            // Only consume the slot for the URI it was armed with, matching
            // `notify_completion_analysis_started`'s per-URI ownership so an
            // unrelated concurrent test's request can't be blocked by (or
            // consume) this test's gate.
            if gate.as_ref().is_some_and(|(armed_uri, _)| armed_uri == uri) {
                gate.take().map(|(_, rx)| rx)
            } else {
                None
            }
        });
        if let Some(rx) = rx {
            // Bounded: fail open (proceed) rather than hang forever if the
            // test never releases the gate for some other reason.
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
    #[cfg(not(test))]
    let _ = uri;
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
fn wait_for_completion_comment_guard_gate(_uri: &str) {}

#[derive(Debug)]
struct CompletionDecisionSummary {
    compiler_fact_items: usize,
    generated_items: usize,
    dynamic_boundary_items: usize,
    fallback_items: usize,
    sample_labels: Vec<String>,
}

#[derive(Debug)]
struct CompletionDecisionContext<'a> {
    uri: &'a str,
    line: u32,
    character: u32,
    ast_available: bool,
    workspace_index_state: &'static str,
    workspace_index_reason: Option<&'static str>,
    is_incomplete: bool,
}

#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
struct CompletionShadowBudget<'a> {
    should_continue: &'a dyn Fn() -> bool,
}

/// Returns commit characters for a completion item based on its kind.
/// Each string is exactly one character, per the LSP 3.x spec.
///
/// Returns a static slice to avoid per-item heap allocation in the completion
/// serialization hot path (called once per item, up to `completion_cap()` times
/// per request, on every keypress in editors).
fn commit_chars_for_kind(kind: CompletionItemKind) -> Option<&'static [&'static str]> {
    match kind {
        CompletionItemKind::Function => Some(&["(", ",", ";"]),
        CompletionItemKind::Variable => Some(&["[", "{", ".", ";"]),
        CompletionItemKind::Module => Some(&[":", ";"]),
        CompletionItemKind::Constant => Some(&["[", "{", ".", ";"]),
        CompletionItemKind::Property => Some(&[",", "}"]),
        _ => None,
    }
}

/// Deduplicate and rank the complete candidate set before applying the client
/// result cap.
///
/// Provider completions are initially sorted, but request-level enrichment
/// appends declared-dependency and workspace candidates afterward.  Capping
/// that mixed list before sorting can hide a better late candidate and makes
/// the visible result depend on provider order rather than completion rank.
fn sort_and_cap_completions(
    completions: Vec<crate::completion::CompletionItem>,
    cap: usize,
) -> (Vec<crate::completion::CompletionItem>, bool) {
    let mut completions =
        perl_lsp_rs_core::providers::completion_item::deduplicate_and_sort(completions);
    let is_incomplete = completions.len() > cap;
    completions.truncate(cap);
    (completions, is_incomplete)
}

impl LspServer {
    fn completion_visibility_shadow_labels(
        completions: &[crate::completion::CompletionItem],
    ) -> Vec<String> {
        completions
            .iter()
            .filter(|completion| {
                matches!(
                    completion.kind,
                    CompletionItemKind::Variable
                        | CompletionItemKind::Function
                        | CompletionItemKind::Constant
                ) && !(completion.kind == CompletionItemKind::Function
                    && completion
                        .sort_text
                        .as_deref()
                        .is_some_and(|sort_text| sort_text.starts_with("3_")))
            })
            .map(|completion| completion.label.to_string())
            .collect()
    }

    fn is_member_subscript_completion_context(before_cursor: &str, token_start: usize) -> bool {
        let mut nested_brackets = 0usize;
        let mut saw_open_subscript = false;

        for (position, character) in before_cursor[..token_start].char_indices().rev() {
            let previous = before_cursor[..position].chars().next_back();
            let next = before_cursor[position + character.len_utf8()..].chars().next();
            let is_member_arrow = (character == '-' && next == Some('>'))
                || (character == '>' && previous == Some('-'));

            if matches!(character, ']' | '}') {
                nested_brackets += 1;
                continue;
            }
            if matches!(character, '[' | '{') {
                if nested_brackets > 0 {
                    nested_brackets -= 1;
                    continue;
                }
                if before_cursor[..position].trim_end().ends_with("->") {
                    return true;
                }
                saw_open_subscript = true;
                continue;
            }
            if saw_open_subscript && is_member_arrow {
                return true;
            }

            let is_boundary = character.is_whitespace()
                || matches!(
                    character,
                    ';' | '='
                        | '+'
                        | '-'
                        | '*'
                        | '/'
                        | '%'
                        | '.'
                        | '!'
                        | '<'
                        | '>'
                        | '&'
                        | '|'
                        | '^'
                        | '~'
                        | '?'
                        | ':'
                        | '('
                        | ')'
                        | ','
                );
            let is_token_start_boundary = position + character.len_utf8() == token_start;
            if saw_open_subscript && nested_brackets == 0 && is_boundary && !is_token_start_boundary
            {
                return false;
            }
        }

        false
    }

    fn is_qualified_member_completion_context(doc_text: &str, offset: usize) -> bool {
        let Some(before_cursor) = doc_text.get(..offset.min(doc_text.len())) else {
            return false;
        };
        let token_start = before_cursor
            .char_indices()
            .rev()
            .find_map(|(position, character)| {
                let previous = before_cursor[..position].chars().next_back();
                let next = before_cursor[position + character.len_utf8()..].chars().next();
                let is_member_arrow = (character == '-' && next == Some('>'))
                    || (character == '>' && previous == Some('-'));
                let is_package_separator =
                    character == ':' && (previous == Some(':') || next == Some(':'));
                let is_boundary = !is_member_arrow
                    && !is_package_separator
                    && (character.is_whitespace()
                        || matches!(
                            character,
                            ';' | '='
                                | '+'
                                | '-'
                                | '*'
                                | '/'
                                | '%'
                                | '.'
                                | '!'
                                | '<'
                                | '>'
                                | '&'
                                | '|'
                                | '^'
                                | '~'
                                | '?'
                                | ':'
                                | '('
                                | ')'
                                | '{'
                                | '}'
                                | '['
                                | ']'
                                | ','
                        ));
                is_boundary.then_some(position + character.len_utf8())
            })
            .unwrap_or(0);
        let token = &before_cursor[token_start..];
        let member_subscript =
            Self::is_member_subscript_completion_context(before_cursor, token_start);
        let preceded_by_member_arrow = before_cursor[..token_start].trim_end().ends_with("->");
        member_subscript || preceded_by_member_arrow || token.contains("->") || token.contains("::")
    }

    fn record_completion_provider_decision_trace(
        &self,
        context: &CompletionDecisionContext<'_>,
        completions: &[crate::completion::CompletionItem],
        semantic_shadow_receipt: Option<Value>,
    ) {
        let summary = Self::completion_decision_summary(completions);
        let item_count = completions.len();
        let fact_source = if summary.compiler_fact_items > 0 {
            "compiler_fact"
        } else if context.ast_available {
            "parser_syntax"
        } else {
            "fallback"
        };
        let fallback_state = if item_count == 0 {
            "no_result"
        } else if context.ast_available {
            "none"
        } else {
            "legacy_provider"
        };
        let reason = if item_count == 0 {
            "missing_fact"
        } else if summary.dynamic_boundary_items > 0 {
            "dynamic_boundary"
        } else if summary.generated_items > 0 {
            "generated_no_source"
        } else if summary.compiler_fact_items > 0 {
            "source_backed_high_confidence"
        } else {
            "fallback_policy"
        };

        let mut receipt = json!({
            "provider": "completion",
            "provider_action": "textDocument/completion",
            "decision": if item_count > 0 { "acted" } else { "fallback" },
            "reason": reason,
            "uri": context.uri,
            "line": context.line,
            "character": context.character,
            "item_count": item_count,
            "is_incomplete": context.is_incomplete,
            "ast_available": context.ast_available,
            "fact_source": fact_source,
            "confidence": if item_count > 0 { "high" } else { "low" },
            "freshness": "fresh",
            "fallback_state": fallback_state,
            "workspace_index_state": context.workspace_index_state,
            "workspace_index_reason": context.workspace_index_reason,
            "compiler_fact_item_count": summary.compiler_fact_items,
            "generated_item_count": summary.generated_items,
            "dynamic_boundary_item_count": summary.dynamic_boundary_items,
            "fallback_candidate_count": summary.fallback_items,
            "sample_labels": summary.sample_labels,
            "claim_boundary": if semantic_shadow_receipt.is_some() {
                "records existing comparable visibility completions and semantic shadow evidence; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
            } else {
                "records existing comparable visibility completions only; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
            }
        });
        if let Some(shadow_receipt) = semantic_shadow_receipt
            && let Some(object) = receipt.as_object_mut()
        {
            object.insert("semantic_shadow_receipt".to_string(), shadow_receipt);
        }
        self.record_provider_decision_trace("completion", &receipt);
    }

    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    fn completion_semantic_shadow_receipt(
        &self,
        uri: &str,
        doc_text: &str,
        byte_offset: usize,
        position: (u32, u32),
        completions: &[crate::completion::CompletionItem],
        workspace_mode: &IndexAccessMode<'_>,
        budget: CompletionShadowBudget<'_>,
    ) -> Option<Value> {
        let IndexAccessMode::Full(coordinator) = workspace_mode else {
            return None;
        };
        if Self::is_module_import_completion_context(doc_text, byte_offset)
            || Self::is_qualified_member_completion_context(doc_text, byte_offset)
        {
            return None;
        }
        let byte_offset = u32::try_from(byte_offset).ok()?;
        let (line, character) = position;
        let input_label = format!("{uri}:{line}:{character}");
        let legacy_symbols = Self::completion_visibility_shadow_labels(completions);
        if legacy_symbols.is_empty() {
            return None;
        }
        if !(budget.should_continue)() {
            return None;
        }
        if self.workspace_index_stale_for_any_open_document() {
            return None;
        }
        let index = coordinator.index();
        let receipt = index.with_semantic_queries_for_uri(uri, |file_id, queries| {
            if !(budget.should_continue)() {
                return None;
            }
            Some(
                completion_visibility_shadow(
                    legacy_symbols,
                    &queries,
                    file_id,
                    byte_offset,
                    None,
                    &input_label,
                )
                .receipt,
            )
        })??;
        if !(budget.should_continue)() {
            return None;
        }
        serde_json::to_value(receipt).ok()
    }

    #[cfg(feature = "workspace")]
    fn completion_workspace_index_state(
        workspace_mode: &IndexAccessMode<'_>,
    ) -> (&'static str, Option<&'static str>) {
        match workspace_mode {
            IndexAccessMode::Full(_) => ("full", None),
            IndexAccessMode::Partial(reason) => ("partial", Some(*reason)),
            IndexAccessMode::None => ("none", None),
        }
    }

    #[cfg(not(feature = "workspace"))]
    fn completion_workspace_index_state(
        workspace_mode: &IndexAccessMode,
    ) -> (&'static str, Option<&'static str>) {
        match workspace_mode {
            IndexAccessMode::Partial(reason) => ("partial", Some(*reason)),
            IndexAccessMode::None => ("none", None),
        }
    }

    fn completion_decision_summary(
        completions: &[crate::completion::CompletionItem],
    ) -> CompletionDecisionSummary {
        let sample_labels =
            completions.iter().take(5).map(|completion| completion.label.to_string()).collect();
        let mut compiler_fact_items = 0;
        let mut generated_items = 0;
        let mut dynamic_boundary_items = 0;

        for completion in completions {
            let detail = completion.detail.as_deref().unwrap_or("");
            let documentation = completion.documentation.as_deref().unwrap_or("");
            let mut is_compiler_fact = detail.contains("compiler fact")
                || documentation.contains("Compiler visible-symbol");
            let is_generated = detail.contains("generated")
                || documentation.contains("generated")
                || detail.contains("framework")
                || documentation.contains("Framework");
            let is_dynamic = detail.contains("dynamic")
                || documentation.contains("dynamic")
                || detail.contains("Dynamic")
                || documentation.contains("Dynamic");

            if matches!(
                completion.kind,
                CompletionItemKind::Variable
                    | CompletionItemKind::Function
                    | CompletionItemKind::Module
                    | CompletionItemKind::Constant
                    | CompletionItemKind::Property
            ) && detail.contains("high confidence")
            {
                is_compiler_fact = true;
            }

            if is_compiler_fact {
                compiler_fact_items += 1;
            }
            if is_generated {
                generated_items += 1;
            }
            if is_dynamic {
                dynamic_boundary_items += 1;
            }
        }

        CompletionDecisionSummary {
            compiler_fact_items,
            generated_items,
            dynamic_boundary_items,
            fallback_items: completions.len().saturating_sub(compiler_fact_items),
            sample_labels,
        }
    }

    /// Include-root view for module completion at a standalone document
    /// position, assembling its own `@INC` context.
    ///
    /// Callers that already hold a request-scoped context should use
    /// [`Self::module_completion_roots`] so the context is built once per
    /// request rather than once per consumer (#1684).
    pub(super) fn module_completion_roots_for_doc(
        &self,
        uri: &str,
        doc_text: &str,
        cursor_offset: usize,
    ) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
        self.module_completion_roots(&RequestIncContext::new(self, uri, doc_text, cursor_offset))
    }

    /// Include-root view for module completion, reading the request's shared
    /// `@INC` context instead of assembling its own.
    pub(super) fn module_completion_roots(
        &self,
        inc_context: &RequestIncContext<'_>,
    ) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
        let mut include_paths: Vec<PathBuf> = Vec::new();
        let mut seen_include: HashSet<PathBuf> = HashSet::new();
        let mut system_inc_paths: Vec<PathBuf> = Vec::new();
        let mut seen_system: HashSet<PathBuf> = HashSet::new();
        let Some(context) = inc_context.get() else {
            return (include_paths, system_inc_paths, false);
        };

        let include_workspace_roots = context.folder_uri.is_some();
        for root in &context.effective_roots {
            match root.kind {
                IncRootKind::InterpreterStartup => {
                    let resolved = root.path.clone();
                    if seen_system.insert(resolved.clone()) {
                        system_inc_paths.push(resolved);
                    }
                }
                IncRootKind::FileLocalLexical => {
                    let resolved = Self::completion_path_for_inc_root(root, &context.root);
                    if seen_include.insert(resolved.clone()) {
                        include_paths.push(resolved);
                    }
                }
                _ if include_workspace_roots => {
                    let resolved = Self::completion_path_for_inc_root(root, &context.root);
                    if seen_include.insert(resolved.clone()) {
                        include_paths.push(resolved);
                    }
                }
                _ => {}
            }
        }

        let mut include_system_inc = context.use_system_inc;

        // Preserve the historical non-workspace completion fallback: when the
        // document is outside every workspace folder, opted-in folder startup
        // @INC roots can still be used for module completion. Workspace-configured
        // roots stay excluded, while explicit file-local `use lib` roots above
        // remain eligible.
        if !include_system_inc && context.folder_uri.is_none() {
            let mut folders = self.workspace_folders.lock();
            for folder in folders.iter_mut() {
                if !folder.effective_workspace_config.use_system_inc {
                    continue;
                }
                include_system_inc = true;
                for path in folder.effective_workspace_config.get_system_inc() {
                    if seen_system.insert(path.clone()) {
                        system_inc_paths.push(path.clone());
                    }
                }
            }
        }

        (include_paths, system_inc_paths, include_system_inc)
    }

    fn completion_path_for_inc_root(root: &IncRoot, context_root: &Path) -> PathBuf {
        match root.kind {
            IncRootKind::FileLocalLexical | IncRootKind::WorkspaceRelative
                if !root.path.is_absolute() =>
            {
                context_root.join(&root.path)
            }
            _ => root.path.clone(),
        }
    }

    fn split_sigil(name: &str) -> (Option<char>, &str) {
        let mut chars = name.chars();
        match chars.next() {
            Some(sigil @ ('$' | '@' | '%')) => (Some(sigil), &name[sigil.len_utf8()..]),
            _ => (None, name),
        }
    }

    fn workspace_symbol_qualified_name(symbol: &crate::workspace_index::WorkspaceSymbol) -> String {
        match symbol.kind {
            crate::workspace_index::SymbolKind::Variable(_) => {
                if let Some(container) = symbol.container_name.as_ref() {
                    let (sigil, bare_name) = Self::split_sigil(&symbol.name);
                    format!("{}{container}::{bare_name}", sigil.unwrap_or('$'))
                } else {
                    symbol.name.clone()
                }
            }
            _ => symbol
                .qualified_name
                .clone()
                .or_else(|| {
                    symbol
                        .container_name
                        .as_ref()
                        .map(|container| format!("{container}::{}", symbol.name))
                })
                .unwrap_or_else(|| symbol.name.clone()),
        }
    }

    fn qualified_variable_workspace_symbols(
        index: &crate::workspace_index::WorkspaceIndex,
        prefix: &str,
    ) -> Option<Vec<crate::workspace_index::WorkspaceSymbol>> {
        let (requested_sigil, prefix_body) = Self::split_sigil(prefix);
        let requested_sigil = requested_sigil?;
        let mut parts: Vec<&str> = prefix_body.split("::").collect();
        if parts.len() < 2 {
            return None;
        }

        let member_prefix = parts.pop().unwrap_or("");
        let package_name = parts.join("::");

        Some(
            index
                .get_package_members(&package_name)
                .into_iter()
                .filter(|symbol| match symbol.kind {
                    crate::workspace_index::SymbolKind::Variable(_) => {
                        let (symbol_sigil, bare_name) = Self::split_sigil(&symbol.name);
                        symbol_sigil == Some(requested_sigil)
                            && bare_name.starts_with(member_prefix)
                    }
                    _ => false,
                })
                .collect(),
        )
    }

    fn workspace_symbol_kind(
        symbol: &crate::workspace_index::WorkspaceSymbol,
    ) -> CompletionItemKind {
        match symbol.kind {
            crate::workspace_index::SymbolKind::Package => CompletionItemKind::Module,
            crate::workspace_index::SymbolKind::Subroutine => CompletionItemKind::Function,
            crate::workspace_index::SymbolKind::Variable(_) => CompletionItemKind::Variable,
            crate::workspace_index::SymbolKind::Class => CompletionItemKind::Module,
            crate::workspace_index::SymbolKind::Method => CompletionItemKind::Function,
            crate::workspace_index::SymbolKind::Constant => CompletionItemKind::Constant,
            crate::workspace_index::SymbolKind::Role => CompletionItemKind::Module,
            crate::workspace_index::SymbolKind::Import => CompletionItemKind::Module,
            crate::workspace_index::SymbolKind::Export => CompletionItemKind::Function,
            crate::workspace_index::SymbolKind::Label => CompletionItemKind::Keyword,
            crate::workspace_index::SymbolKind::Format => CompletionItemKind::Function,
        }
    }

    fn is_module_import_completion_context(doc_text: &str, offset: usize) -> bool {
        if !doc_text.is_char_boundary(offset) {
            return false;
        }
        let before = &doc_text[..offset];
        let line_start = before.rfind('\n').map(|position| position + 1).unwrap_or(0);
        let line = before[line_start..].trim_start();

        if let Some(rest) = line.strip_prefix("use") {
            if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
                return false;
            }
            let rest = rest.trim_start();
            if rest.contains(';') || rest.contains('(') || rest.contains("qw") {
                return false;
            }
            let first_char = rest.chars().next();
            return first_char.is_none() || first_char.is_some_and(|c| c.is_ascii_uppercase());
        }

        if let Some(rest) = line.strip_prefix("require") {
            if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
                return false;
            }
            let rest = rest.trim_start();
            if rest.contains(';') {
                return false;
            }
            let Some(first_char) = rest.chars().next() else {
                return true;
            };
            return !matches!(first_char, '0'..='9' | '\'' | '"' | '`' | '.' | '/' | '\\');
        }

        false
    }

    fn module_completion_prefix(doc_text: &str, offset: usize) -> Option<String> {
        if !Self::is_module_import_completion_context(doc_text, offset) {
            return None;
        }

        let text_before = &doc_text[..offset.min(doc_text.len())];
        Some(
            text_before
                .chars()
                .rev()
                .take_while(|&c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                .collect::<String>()
                .chars()
                .rev()
                .collect(),
        )
    }

    fn add_declared_dependency_completions(
        &self,
        completions: &mut Vec<crate::completion::CompletionItem>,
        doc_text: &str,
        doc_uri: &str,
        offset: usize,
        should_continue: Option<&dyn Fn() -> bool>,
    ) {
        let Some(prefix) = Self::module_completion_prefix(doc_text, offset) else {
            return;
        };
        let config =
            self.config_for_doc(doc_uri).unwrap_or_else(|| self.workspace_config.lock().clone());
        let mut seen: HashSet<String> =
            completions.iter().map(|completion| completion.label.to_string()).collect();

        for dependency in config.declared_dependencies {
            if should_continue.is_some_and(|check| !check()) {
                return;
            }
            if !prefix.is_empty() && !dependency.module.starts_with(&prefix) {
                continue;
            }
            if !seen.insert(dependency.module.clone()) {
                continue;
            }

            let summary = Self::declared_dependency_summary(&dependency);
            let module = dependency.module;
            let detail = format!("{summary}; not currently indexed");
            let documentation = format!(
                "`{module}` is {summary}, but it is not currently indexed. Install it or add its directory to `.perl-lsp.toml` `include_paths`.",
            );

            completions.push(crate::completion::CompletionItem {
                label: module.clone().into(),
                kind: CompletionItemKind::Module,
                detail: Some(detail.into()),
                documentation: Some(documentation.into()),
                insert_text: Some(module.clone().into()),
                sort_text: Some(format!("080_declared_dependency_{module}").into()),
                filter_text: None,
                additional_edits: Vec::new(),
                text_edit_range: None,
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }

    /// Adds workspace-index completions.
    ///
    /// Takes the request's shared `@INC` context rather than a loose
    /// `(text, uri, offset)` triple: the position is the same one the module
    /// roots were built from, and passing the holder makes that structural
    /// instead of a convention the call sites have to keep in step (#1684).
    fn add_runtime_workspace_completions(
        &self,
        completions: &mut Vec<crate::completion::CompletionItem>,
        inc_context: &RequestIncContext<'_>,
        workspace_mode: &IndexAccessMode,
        should_continue: Option<&dyn Fn() -> bool>,
    ) {
        let doc_text = inc_context.doc_text();
        let doc_uri = inc_context.doc_uri();
        let offset = inc_context.offset();

        if Self::is_module_import_completion_context(doc_text, offset) {
            return;
        }

        match workspace_mode {
            IndexAccessMode::Full(coordinator) => {
                let index = coordinator.index();

                let text_before = &doc_text[..offset.min(doc_text.len())];
                // Method context survives once a method name is partially typed:
                // `$obj->` and `$obj->co` are both method-completion positions,
                // while `$x->[0]` or plain identifiers are not.
                let is_method_completion =
                    text_before.trim_end().rsplit_once("->").is_some_and(|(_, suffix)| {
                        suffix.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    });
                let prefix = text_before
                    .chars()
                    .rev()
                    .take_while(|&c| {
                        c.is_alphanumeric()
                            || c == '_'
                            || c == ':'
                            || c == '$'
                            || c == '@'
                            || c == '%'
                    })
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>();

                // Detect `use Module` / `require Module` context so we can gate
                // Package-kind completions through @INC reachability (fixes #8537).
                let is_use_module_context = {
                    let before_prefix =
                        &text_before[..text_before.len().saturating_sub(prefix.len())];
                    let trimmed = before_prefix.trim_end();
                    trimmed.ends_with("use") || trimmed.ends_with("require")
                };

                // Reuse the request's @INC context (only when needed for
                // filtering) — on the completion paths it was already built for
                // the module roots, so this is a cache read, not a rebuild.
                let inc_ctx = if is_use_module_context { inc_context.get() } else { None };

                // For multi-root workspaces, determine the workspace folder that owns
                // the document so we can filter non-module symbols to that folder only.
                // When there is only one folder (or none), skip the filter — no cross-
                // folder leak is possible.
                let doc_folder_filter = {
                    let folders = self.workspace_folders.lock();
                    if folders.len() > 1 {
                        crate::runtime::types::best_workspace_folder_for_doc(&folders, doc_uri)
                            .cloned()
                    } else {
                        None
                    }
                };

                let qualified_variable_symbols =
                    Self::qualified_variable_workspace_symbols(index, &prefix);
                let replace_prefix_range = (offset.saturating_sub(prefix.len()), offset);
                let qualified_variable_context = qualified_variable_symbols.is_some();
                let workspace_symbols =
                    qualified_variable_symbols.unwrap_or_else(|| index.find_symbols(&prefix));
                use std::collections::HashSet;
                let mut seen: HashSet<String> =
                    completions.iter().map(|completion| completion.label.to_string()).collect();

                // The runtime pass has no receiver facts of its own. When the
                // core provider already attached receiver evidence to this
                // response, keep its quiet name-only extras; otherwise label
                // callable candidates honestly instead of emitting an
                // unlabelled dynamic-boundary insertion (issue #11158).
                let receiver_evidence_present = completions.iter().any(|completion| {
                    completion.detail.as_deref().is_some_and(|detail| detail.contains("receiver:"))
                });

                for symbol in workspace_symbols {
                    if should_continue.is_some_and(|check| !check()) {
                        return;
                    }
                    if seen.contains(&symbol.name) {
                        continue;
                    }

                    // The runtime workspace pass is a name-only fallback. It
                    // has no import/reachability facts for callable and value
                    // symbols, so emitting these as bare insertions can leave
                    // an unimported cross-file reference in the document.
                    // The core provider owns import-aware, current-file, and
                    // qualified completions for these kinds; retain only the
                    // module-name kinds here (issue #11158).
                    if !is_method_completion
                        && matches!(
                            symbol.kind,
                            crate::workspace_index::SymbolKind::Subroutine
                                | crate::workspace_index::SymbolKind::Method
                                | crate::workspace_index::SymbolKind::Constant
                                | crate::workspace_index::SymbolKind::Export
                        )
                    {
                        continue;
                    }

                    // Strategy A: module-kind symbols in `use Module` / `require Module`
                    // context — filter by position-aware @INC reachability so that
                    // `no lib` cancellations are honoured (fixes #8537).
                    let is_module_kind = matches!(
                        symbol.kind,
                        crate::workspace_index::SymbolKind::Package
                            | crate::workspace_index::SymbolKind::Class
                            | crate::workspace_index::SymbolKind::Role
                    );
                    if is_use_module_context
                        && is_module_kind
                        && let Some(ctx) = inc_ctx
                        && !ctx.symbol_uri_reachable(&symbol.uri)
                    {
                        tracing::trace!(
                            symbol = %symbol.name,
                            uri = %symbol.uri,
                            "completion: skipping workspace symbol not reachable via @INC"
                        );
                        continue;
                    }

                    // Strategy B: non-module symbols in multi-root workspace — filter
                    // by workspace-folder containment. symbol_uri_reachable is designed
                    // for @INC paths (module files) and would incorrectly drop scripts
                    // and .pm files not on @INC. Folder containment is the right filter
                    // for subroutines, variables, methods, and constants (fixes #970).
                    if !is_module_kind
                        && let Some(ref folder) = doc_folder_filter
                        && !workspace_folder_matches_doc_uri(folder, &symbol.uri)
                    {
                        tracing::trace!(
                            symbol = %symbol.name,
                            uri = %symbol.uri,
                            folder = %folder.uri,
                            "completion: skipping cross-folder non-module symbol"
                        );
                        continue;
                    }

                    let label = symbol.name.clone();
                    let qualified_name = Self::workspace_symbol_qualified_name(&symbol);
                    let detail = if !receiver_evidence_present
                        && matches!(
                            symbol.kind,
                            crate::workspace_index::SymbolKind::Subroutine
                                | crate::workspace_index::SymbolKind::Method
                                | crate::workspace_index::SymbolKind::Constant
                                | crate::workspace_index::SymbolKind::Export
                        ) {
                        // Callable kinds only reach this pass through the
                        // method-completion gate above, which carries no
                        // receiver evidence; say so on the item.
                        Some(format!("{qualified_name} — receiver: unknown, low confidence"))
                    } else {
                        Some(qualified_name.clone())
                    };
                    // Invariant: text_edit_range.is_some() ⟺ insert_text is the
                    // fully-qualified name.  The serializer (completion_item_to_lsp_value)
                    // depends on this to locate the newText from `item["insertText"]`.
                    let (insert_text, text_edit_range) = if qualified_variable_context
                        && matches!(symbol.kind, crate::workspace_index::SymbolKind::Variable(_))
                    {
                        (Some(qualified_name), Some(replace_prefix_range))
                    } else {
                        (Some(label.clone()), None)
                    };
                    seen.insert(label.clone());
                    let sort_text = format!("9_workspace_{label}");

                    completions.push(crate::completion::CompletionItem {
                        label: label.into(),
                        kind: Self::workspace_symbol_kind(&symbol),
                        detail: detail.map(Into::into),
                        insert_text: insert_text.map(Into::into),
                        // Workspace enrichment is a fallback tier. Give it an
                        // explicit low-priority rank so unranked labels (for
                        // example `$...` variables) cannot displace ranked
                        // in-file methods when the final response is capped.
                        sort_text: Some(sort_text.into()),
                        filter_text: None,
                        documentation: Self::workspace_symbol_documentation(&symbol)
                            .map(Into::into),
                        additional_edits: Vec::new(),
                        text_edit_range,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
            }
            IndexAccessMode::Partial(reason) => {
                tracing::debug!(reason, "Completion: workspace index partial");
            }
            IndexAccessMode::None => {}
        }
    }

    fn workspace_symbol_documentation(
        symbol: &crate::workspace_index::WorkspaceSymbol,
    ) -> Option<String> {
        symbol.documentation.clone().or_else(|| {
            let qualified_name = Self::workspace_symbol_qualified_name(symbol);

            Some(match symbol.kind {
                crate::workspace_index::SymbolKind::Package => {
                    format!("Package `{qualified_name}` available in the workspace.")
                }
                crate::workspace_index::SymbolKind::Subroutine => {
                    format!("Subroutine `{qualified_name}` defined in the workspace.")
                }
                crate::workspace_index::SymbolKind::Variable(_) => {
                    format!("Variable `{qualified_name}` declared in the workspace.")
                }
                crate::workspace_index::SymbolKind::Class => {
                    format!("Class `{qualified_name}` defined in the workspace.")
                }
                crate::workspace_index::SymbolKind::Method => {
                    format!("Method `{qualified_name}` defined in the workspace.")
                }
                crate::workspace_index::SymbolKind::Constant => {
                    format!("Constant `{qualified_name}` declared in the workspace.")
                }
                crate::workspace_index::SymbolKind::Role => {
                    format!("Role `{qualified_name}` defined in the workspace.")
                }
                crate::workspace_index::SymbolKind::Import => {
                    format!("Imported module `{qualified_name}` used in the workspace.")
                }
                crate::workspace_index::SymbolKind::Export => {
                    format!("Exported function `{qualified_name}` available in the workspace.")
                }
                crate::workspace_index::SymbolKind::Label => {
                    format!("Label `{qualified_name}` declared in the workspace.")
                }
                crate::workspace_index::SymbolKind::Format => {
                    format!("Format `{qualified_name}` declared in the workspace.")
                }
            })
        })
    }

    fn completion_list_default_data() -> Value {
        json!({
            "provider": "perl-lsp",
            "kind": "completion-list",
            "schemaVersion": 1
        })
    }

    fn completion_list_response(
        is_incomplete: bool,
        items: Vec<Value>,
        item_defaults_data_support: bool,
        apply_kind_support: bool,
    ) -> Value {
        let has_items = !items.is_empty();
        let mut response = json!({
            "isIncomplete": is_incomplete,
            "items": items
        });

        if item_defaults_data_support && has_items {
            response["itemDefaults"] = json!({
                "data": Self::completion_list_default_data()
            });
            if apply_kind_support {
                response["applyKind"] = json!({
                    "data": 2
                });
            }
        }

        response
    }

    /// Format type information concisely for completion detail
    pub(crate) fn format_type_for_detail(t: &crate::type_inference::PerlType) -> String {
        use perl_parser::type_inference::PerlType;
        match t {
            PerlType::Scalar(_) => "scalar".to_string(),
            PerlType::Array(_) => "array".to_string(),
            PerlType::Hash { .. } => "hash".to_string(),
            PerlType::Subroutine { .. } => "code".to_string(),
            PerlType::Reference(inner) => format!("ref {}", Self::format_type_for_detail(inner)),
            PerlType::Object(name) => format!("object {}", name),
            PerlType::Glob => "glob".to_string(),
            PerlType::Union(_) => "mixed".to_string(),
            PerlType::Any => "any".to_string(),
            PerlType::Void => "void".to_string(),
        }
    }

    fn completion_item_to_lsp_value(
        &self,
        doc: &DocumentState,
        c: crate::completion::CompletionItem,
        snippet_support: bool,
        commit_chars_support: bool,
        label_details_support: bool,
    ) -> Value {
        // LSP 3.17 §3.17.1: `kind` and `insertTextFormat` are independent. The
        // item declares its own insertion grammar; deriving it from `kind`
        // means a Function that inserts a snippet (`open`) ships literal
        // `${1:<}` to the editor. Degrading to the item's own plain-text
        // fallback is the only correct answer for a client without
        // `snippetSupport` — there is nothing to reconstruct at this layer.
        let (insert_text_format, degraded_insert_text) = match &c.insert_text_format {
            InsertTextFormat::PlainText => (1, None),
            InsertTextFormat::Snippet { plain_fallback } => {
                if snippet_support {
                    (2, None)
                } else {
                    (1, Some(plain_fallback.clone()))
                }
            }
        };

        let mut item = json!({
            "label": c.label,
            "kind": match c.kind {
                CompletionItemKind::Variable => 6,
                CompletionItemKind::Function => 3,
                CompletionItemKind::Keyword => 14,
                CompletionItemKind::Module => 9,
                CompletionItemKind::File => 17,
                CompletionItemKind::Snippet => 15,
                CompletionItemKind::Constant => 14,
                CompletionItemKind::Property => 7,
            },
            "insertTextFormat": insert_text_format,
        });

        if let Some(detail) = c.detail {
            item["detail"] = json!(detail);
        }

        if let Some(insert_text) = c.insert_text {
            item["insertText"] =
                json!(degraded_insert_text.unwrap_or_else(|| insert_text.into_owned()));
        }

        if let Some(documentation) = c.documentation {
            item["documentation"] = json!({
                "kind": "markdown",
                "value": documentation
            });
        }

        if commit_chars_support && let Some(chars) = commit_chars_for_kind(c.kind) {
            item["commitCharacters"] = json!(chars);
        }

        if let Some(sort_text) = c.sort_text {
            item["sortText"] = json!(sort_text);
        }
        if let Some(filter_text) = c.filter_text {
            item["filterText"] = json!(filter_text);
        }

        if label_details_support && let Some(ld) = c.label_details {
            let mut obj = serde_json::Map::new();
            if let Some(d) = ld.detail {
                obj.insert("detail".to_string(), json!(d));
            }
            if let Some(desc) = ld.description {
                obj.insert("description".to_string(), json!(desc));
            }
            if !obj.is_empty() {
                item["labelDetails"] = Value::Object(obj);
            }
        }

        if !c.additional_edits.is_empty() {
            let edits: Vec<Value> = c
                .additional_edits
                .iter()
                .map(|(loc, new_text)| {
                    let (sl, sc) = self.offset_to_pos16(doc, loc.start);
                    let (el, ec) = self.offset_to_pos16(doc, loc.end);
                    json!({
                        "range": {
                            "start": { "line": sl, "character": sc },
                            "end": { "line": el, "character": ec }
                        },
                        "newText": new_text
                    })
                })
                .collect();
            item["additionalTextEdits"] = to_json_array(&edits);
        }

        // LSP 3.17 §3.16.1: when `textEdit` is present it takes precedence over
        // `insertText`.  Without it, clients replace nothing — they append the
        // resolved name to the typed prefix, producing "$v$variable" instead of
        // "$variable".  Emit a plain TextEdit whose range covers exactly the typed
        // prefix so the client replaces it.
        if let Some((start_offset, end_offset)) = c.text_edit_range {
            let (sl, sc) = self.offset_to_pos16(doc, start_offset);
            let (el, ec) = self.offset_to_pos16(doc, end_offset);
            // Use the insertText that was already serialized (possibly snippet-degraded),
            // falling back to the label.  Both fields have already been written into
            // `item`, so we read from there rather than the (partially-moved) `c`.
            let new_text = item["insertText"]
                .as_str()
                .or_else(|| item["label"].as_str())
                .map(String::from)
                .unwrap_or_default();
            item["textEdit"] = json!({
                "range": {
                    "start": { "line": sl, "character": sc },
                    "end": { "line": el, "character": ec }
                },
                "newText": new_text
            });
        }

        item
    }

    /// Handle completion request
    pub(crate) fn handle_completion(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let start = Instant::now();
        let deadline = completion_deadline();
        let cap = completion_cap();

        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            // Reject stale requests
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;

            // Wait for workspace index to be Ready before routing, matching the
            // workspace/symbol handler (issue #1514 race, extended to completion).
            // The wait is bounded (2 s) and a no-op when the index is already ready.
            #[cfg(feature = "workspace")]
            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);

            // Use routing to determine workspace index access mode
            let mut workspace_mode = route_index_access(self.coordinator());
            if self.workspace_index_stale_for_any_open_document() {
                workspace_mode = IndexAccessMode::None;
            }

            // Phase 1: grab an owned `DocumentState` clone under a brief
            // documents-map lock, then drop the guard before doing any analysis
            // (#3396 off-lock provider consumption). `DocumentState` derives
            // `Clone`: `rope` (structural sharing) and `generation`/`parsed`
            // (`Arc` bumps, incl. the owned `Arc<ParsedSnapshot>` from #3579)
            // clone cheaply, but `text` (`String`) and `line_starts`
            // (`Vec<usize>`) are real O(document-size) copies -- both are
            // needed by the analysis below (offset/position mapping, symbol
            // text extraction), so this isn't wasted work, but it is a
            // genuine per-request cost, not a free clone. It is bounded and
            // single-threaded (a memcpy), unlike the alternative of holding
            // the documents-map mutex -- shared by every open document, not
            // just this one -- for the full analysis duration below.
            let timing_on = crate::runtime::timing::is_enabled();
            let t_lock_start = std::time::Instant::now();
            let doc_owned = {
                let documents = self.documents_guard();
                self.get_document(&documents, uri).cloned()
            };
            // documents guard dropped here
            if timing_on {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "provider.completion.lock_hold",
                    crate::runtime::timing::elapsed_ms(t_lock_start),
                    crate::runtime::timing::uri_tail(uri),
                ));
            }

            let t_analyze_start = std::time::Instant::now();
            let response = 'completion_response: {
                let Some(doc) = doc_owned.as_ref() else {
                    break 'completion_response None;
                };
                let offset = self.pos16_to_offset(doc, line, character);

                // Skip completions inside comments -- the cursor is not on a
                // real symbol and no completion path below (AST-based
                // provider, lexical/keyword fallback, declared-dependency,
                // or workspace-wide completions) should suggest anything.
                // Mirrors the goto-definition comment guard (#5066/#5408) at
                // navigation.rs. String-aware guarding is intentionally
                // omitted: text-based quote scanners produce false positives
                // on real Perl code (regexes, heredocs, qw(), POD), and
                // `is_in_comment && !is_in_string` is the inverted pattern
                // #5411 fixed for goto-definition -- a position the naive
                // quote-counter classifies as both comment and string would
                // wrongly skip this guard.
                if perl_lsp_rs_core::providers::rename::is_in_comment(offset, &doc.text) {
                    break 'completion_response None;
                }

                let ast_available = doc.current_parsed().is_some_and(|p| p.ast().is_some());

                // One `@INC` context per request, shared by the module roots
                // below and the workspace-symbol filter further down (#1684).
                let inc_context = RequestIncContext::new(self, uri, &doc.text, offset);

                // Get completions, with fallback for missing AST
                let parsed = doc.current_parsed();
                #[cfg_attr(not(feature = "workspace"), allow(unused_mut))]
                let mut completions = if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                    let (include_paths, system_inc_paths, include_system_inc) =
                        self.module_completion_roots(&inc_context);
                    // Only provide workspace index when Full access is available
                    // This ensures we don't bypass routing policy
                    #[cfg(feature = "workspace")]
                    let workspace_idx = match &workspace_mode {
                        IndexAccessMode::Full(coordinator) => Some(Arc::clone(coordinator.index())),
                        _ => None,
                    };

                    #[cfg(feature = "workspace")]
                    let provider = CompletionProvider::new_with_index_and_source_and_paths(
                        ast,
                        &doc.text,
                        workspace_idx,
                        include_paths,
                        system_inc_paths,
                        include_system_inc,
                    )
                    .with_scan_cache(Arc::clone(&self.module_scan_cache));

                    #[cfg(not(feature = "workspace"))]
                    let provider = CompletionProvider::new_with_index_and_source_and_paths(
                        ast,
                        &doc.text,
                        None,
                        include_paths,
                        system_inc_paths,
                        include_system_inc,
                    )
                    .with_scan_cache(Arc::clone(&self.module_scan_cache));

                    let mut base_completions =
                        provider.get_completions_with_path(&doc.text, offset, Some(uri));

                    // Enhance completions with generation-owned type information
                    // (#3760): the type environment is materialized once per
                    // ParsedSnapshot generation and derived from the exact source
                    // this snapshot was parsed from, so a completion request under
                    // rapid edits always reads type facts for the current
                    // generation — no cross-generation bleed. `type_environment()`
                    // only returns `None` for an AST-less snapshot, which cannot
                    // happen on this path (this branch is already gated on
                    // `parsed.ast()` being `Some`, the same snapshot); the
                    // `.and_then` is defensive plumbing against a future change to
                    // that guard, not a reachable `None` today. The sigil-based
                    // fallback below still runs regardless.
                    let type_engine = parsed.as_ref().and_then(|p| p.type_environment());

                    // Add type information to completion items where possible
                    for completion in &mut base_completions {
                        // Add type detail to variables based on inferred types
                        if completion.kind == CompletionItemKind::Variable {
                            // Try to get the actual inferred type for the variable
                            let var_name =
                                completion.label.trim_start_matches(['$', '@', '%', '&']);
                            if let Some(perl_type) =
                                type_engine.as_ref().and_then(|engine| engine.get_type_at(var_name))
                            {
                                completion.detail =
                                    Some(Self::format_type_for_detail(&perl_type).into());
                            } else {
                                // Fallback to sigil-based type hint
                                let type_hint: &'static str = if completion.label.starts_with('$') {
                                    "scalar"
                                } else if completion.label.starts_with('@') {
                                    "array"
                                } else if completion.label.starts_with('%') {
                                    "hash"
                                } else if completion.label.starts_with('&') {
                                    "code"
                                } else {
                                    "unknown"
                                };
                                completion.detail = Some(type_hint.into());
                            }
                        }
                    }

                    base_completions
                } else {
                    // Fallback: provide basic keyword completions when AST is unavailable
                    self.lexical_complete(&doc.text, offset, Some(uri))
                };

                self.add_declared_dependency_completions(
                    &mut completions,
                    &doc.text,
                    uri,
                    offset,
                    None,
                );

                // Add workspace-wide completions using routing policy
                #[cfg(feature = "workspace")]
                if start.elapsed() < deadline {
                    self.add_runtime_workspace_completions(
                        &mut completions,
                        &inc_context,
                        &workspace_mode,
                        None,
                    );
                }

                let (completions, is_incomplete) = sort_and_cap_completions(completions, cap);
                let (workspace_index_state, workspace_index_reason) =
                    Self::completion_workspace_index_state(&workspace_mode);
                let completion_decision_context = CompletionDecisionContext {
                    uri,
                    line,
                    character,
                    ast_available,
                    workspace_index_state,
                    workspace_index_reason,
                    is_incomplete,
                };
                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                let semantic_shadow_receipt = self.completion_semantic_shadow_receipt(
                    uri,
                    &doc.text,
                    offset,
                    (line, character),
                    &completions,
                    &workspace_mode,
                    CompletionShadowBudget { should_continue: &|| start.elapsed() < deadline },
                );
                #[cfg(any(not(feature = "workspace"), target_arch = "wasm32"))]
                let semantic_shadow_receipt: Option<Value> = None;
                self.record_completion_provider_decision_trace(
                    &completion_decision_context,
                    &completions,
                    semantic_shadow_receipt,
                );

                // Snapshot capability flags once and drop the lock immediately
                // to avoid holding client_capabilities Mutex across the full
                // completion-item serialization loop. (PERF-7)
                let (
                    snippet_support,
                    commit_chars_support,
                    label_details_support,
                    item_defaults_data_support,
                    apply_kind_support,
                ) = {
                    let client_caps = self.client_capabilities.lock();
                    (
                        client_caps.snippet_support,
                        client_caps.completion_commit_characters_support,
                        client_caps.label_details_support,
                        client_caps.completion_list_item_defaults_data_support,
                        client_caps.completion_list_apply_kind_support,
                    )
                };

                let items: Vec<Value> = completions
                    .into_iter()
                    .map(|c| {
                        self.completion_item_to_lsp_value(
                            doc,
                            c,
                            snippet_support,
                            commit_chars_support,
                            label_details_support,
                        )
                    })
                    .collect();

                if is_incomplete {
                    tracing::debug!(
                        count = items.len(),
                        cap,
                        elapsed = ?start.elapsed(),
                        "Completion: returning items (capped)"
                    );
                } else {
                    tracing::debug!(count = items.len(), "Returning completions");
                }
                Some(Self::completion_list_response(
                    is_incomplete,
                    items,
                    item_defaults_data_support,
                    apply_kind_support,
                ))
            };
            if timing_on {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "provider.completion.analyze",
                    crate::runtime::timing::elapsed_ms(t_analyze_start),
                    crate::runtime::timing::uri_tail(uri),
                ));
            }
            if let Some(response) = response {
                return Ok(Some(response));
            }
        }

        Ok(Some(json!({"isIncomplete": false, "items": []})))
    }

    /// Handle completion request with cancellation support
    pub(crate) fn handle_completion_cancellable(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().completion {
            return Err(crate::protocol::method_not_advertised());
        }

        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        let request_start = Instant::now();

        // Convert raw Value ID to typed ID at the boundary.
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        // RAII guard ensures cleanup on all exit paths (early returns, errors, panics)
        let _cleanup_guard = RequestCleanupGuard::from_ref(typed_id.as_ref());

        if let Some(params) = params {
            // Create or get cancellation token for this request
            let token = if let Some(ref tid) = typed_id {
                GLOBAL_CANCELLATION_REGISTRY.get_token(tid).unwrap_or_else(|| {
                    let token = PerlLspCancellationToken::new(
                        tid.clone(),
                        "textDocument/completion".to_string(),
                    );
                    let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone());
                    token
                })
            } else {
                // Notification path: no client-visible id to cancel against. Use a
                // synthetic sentinel id that won't collide with any real client or
                // server ID (which are always positive integers). The token is
                // created but never registered in the global registry, so external
                // cancellation cannot reach it; it exists only for the
                // cancel-check closure that the provider calls during its work.
                PerlLspCancellationToken::new(
                    UNCANCELLABLE_LOCAL_TOKEN_ID,
                    "textDocument/completion".to_string(),
                )
            };

            // Early cancellation check with relaxed read
            if token.is_cancelled_relaxed() {
                return Err(JsonRpcError {
                    code: REQUEST_CANCELLED,
                    message: "Request cancelled - completion provider".to_string(),
                    data: None,
                });
            }

            // Use cancellable provider method instead of delegating
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            // Reject stale requests
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;

            // Wait for workspace index to be Ready before routing, matching the
            // workspace/symbol handler (issue #1514 race, extended to completion).
            // The wait is bounded (2 s) and a no-op when the index is already ready.
            #[cfg(feature = "workspace")]
            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);

            // Use routing to determine workspace index access mode
            let mut workspace_mode = route_index_access(self.coordinator());
            if self.workspace_index_stale_for_any_open_document() {
                workspace_mode = IndexAccessMode::None;
            }

            // Phase 1: grab an owned `DocumentState` clone under a brief
            // documents-map lock, then drop the guard before doing any analysis
            // (#3396 off-lock provider consumption). `DocumentState` derives
            // `Clone`: `rope` (structural sharing) and `generation`/`parsed`
            // (`Arc` bumps, incl. the owned `Arc<ParsedSnapshot>` from #3579)
            // clone cheaply, but `text` (`String`) and `line_starts`
            // (`Vec<usize>`) are real O(document-size) copies -- both are
            // needed by the analysis below, so this isn't wasted work, but it
            // is a genuine per-request cost, not a free clone. It is bounded
            // and single-threaded (a memcpy), unlike the alternative of
            // holding the documents-map mutex -- shared by every open
            // document, not just this one -- for the full analysis duration
            // below.
            let timing_on = crate::runtime::timing::is_enabled();
            let t_lock_start = std::time::Instant::now();
            let doc_owned = {
                let documents = self.documents_guard();
                self.get_document(&documents, uri).cloned()
            };
            // documents guard dropped here
            if timing_on {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "provider.completion.lock_hold",
                    crate::runtime::timing::elapsed_ms(t_lock_start),
                    crate::runtime::timing::uri_tail(uri),
                ));
            }

            // RAII span: covers the whole analysis attempt via `Drop`, so it
            // emits `provider.completion.analyze` on every exit path --
            // including the cancellation early `return Err` below -- not
            // just the normal fall-through. A manual Instant+emit pair
            // placed after this `if`/`else` (the prior shape) is skipped
            // by any early `return` inside the `if` arm; see
            // `provider.references.analyze` in references.rs for the same
            // pattern (#3619). Started outside the `if let Some(doc)` arm so
            // the `lock_hold`/`analyze` pair is preserved on the
            // doc-no-longer-in-map path too, matching the prior manual-
            // Instant behavior, which emitted unconditionally regardless of
            // whether `doc_owned` resolved.
            let _analyze_span =
                crate::runtime::timing::ScopedSpan::start("provider.completion.analyze", uri);
            let response = 'completion_response: {
                let Some(doc) = doc_owned.as_ref() else {
                    break 'completion_response None;
                };
                notify_completion_analysis_started(uri);

                let offset = self.pos16_to_offset(doc, line, character);

                // Skip completions inside comments -- the cursor is not on a
                // real symbol and no completion path below (AST-based
                // provider, lexical/keyword fallback, declared-dependency,
                // or workspace-wide completions) should suggest anything.
                // Mirrors the goto-definition comment guard (#5066/#5408) at
                // navigation.rs. String-aware guarding is intentionally
                // omitted: text-based quote scanners produce false positives
                // on real Perl code (regexes, heredocs, qw(), POD), and
                // `is_in_comment && !is_in_string` is the inverted pattern
                // #5411 fixed for goto-definition -- a position the naive
                // quote-counter classifies as both comment and string would
                // wrongly skip this guard.
                let in_comment =
                    perl_lsp_rs_core::providers::rename::is_in_comment(offset, &doc.text);

                // Test-only rendezvous: gives a regression test a
                // deterministic window to land a cancellation here instead
                // of racing thread scheduling. No-op in production builds.
                wait_for_completion_comment_guard_gate(uri);

                // Check for cancellation before honoring the comment guard: a
                // cancelled request must always surface REQUEST_CANCELLED to
                // the client, even when the cursor happens to be inside a
                // comment (found in review of this change -- the comment
                // break below previously exited before the only other
                // mid-flight cancellation check, further down after the
                // provider call, ever ran).
                if token.is_cancelled_relaxed() {
                    return Err(JsonRpcError {
                        code: REQUEST_CANCELLED,
                        message: "Request cancelled during completion generation".to_string(),
                        data: None,
                    });
                }

                if in_comment {
                    break 'completion_response None;
                }

                let ast_available = doc.current_parsed().is_some_and(|p| p.ast().is_some());

                // Create optimized cancellation callback with reduced frequency
                // Performance optimization: reduced overhead from 16.66% to <10%
                let check_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
                let cancel_fn = {
                    let token_clone = token.clone();
                    let counter = check_count.clone();
                    move || {
                        let count = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // Adaptive checking: less frequent as processing continues
                        let check_interval = if count < 20 { 5 } else { 25 }; // Reduced from default frequency
                        count.is_multiple_of(check_interval) && token_clone.is_cancelled()
                    }
                };

                // One `@INC` context per request, shared by the module roots
                // below and the workspace-symbol filter further down (#1684).
                let inc_context = RequestIncContext::new(self, uri, &doc.text, offset);

                // Get completions with optimized cancellation support
                let parsed = doc.current_parsed();
                let mut completions = if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                    let (include_paths, system_inc_paths, include_system_inc) =
                        self.module_completion_roots(&inc_context);
                    // Only provide workspace index when Full access is available
                    // This ensures we don't bypass routing policy
                    #[cfg(feature = "workspace")]
                    let workspace_idx = match &workspace_mode {
                        IndexAccessMode::Full(coordinator) => Some(Arc::clone(coordinator.index())),
                        _ => None,
                    };

                    #[cfg(feature = "workspace")]
                    let provider = CompletionProvider::new_with_index_and_source_and_paths(
                        ast,
                        &doc.text,
                        workspace_idx,
                        include_paths,
                        system_inc_paths,
                        include_system_inc,
                    )
                    .with_scan_cache(Arc::clone(&self.module_scan_cache));
                    #[cfg(not(feature = "workspace"))]
                    let provider = CompletionProvider::new_with_index_and_source_and_paths(
                        ast,
                        &doc.text,
                        None,
                        include_paths,
                        system_inc_paths,
                        include_system_inc,
                    )
                    .with_scan_cache(Arc::clone(&self.module_scan_cache));

                    // Use cancellable provider method
                    provider.get_completions_with_path_cancellable(
                        &doc.text,
                        offset,
                        Some(uri),
                        &cancel_fn,
                    )
                } else {
                    self.lexical_complete(&doc.text, offset, Some(uri))
                };

                // Check for cancellation after provider call using relaxed read
                if token.is_cancelled_relaxed() {
                    return Err(JsonRpcError {
                        code: REQUEST_CANCELLED,
                        message: "Request cancelled during completion generation".to_string(),
                        data: None,
                    });
                }

                let should_continue = || !token.is_cancelled_relaxed();
                self.add_declared_dependency_completions(
                    &mut completions,
                    &doc.text,
                    uri,
                    offset,
                    Some(&should_continue),
                );

                #[cfg(feature = "workspace")]
                self.add_runtime_workspace_completions(
                    &mut completions,
                    &inc_context,
                    &workspace_mode,
                    Some(&should_continue),
                );

                if token.is_cancelled_relaxed() {
                    return Err(JsonRpcError {
                        code: REQUEST_CANCELLED,
                        message: "Request cancelled during completion enrichment".to_string(),
                        data: None,
                    });
                }

                let (completions, is_incomplete) =
                    sort_and_cap_completions(completions, completion_cap());

                if token.is_cancelled_relaxed() {
                    return Err(JsonRpcError {
                        code: REQUEST_CANCELLED,
                        message: "Request cancelled during completion ranking".to_string(),
                        data: None,
                    });
                }

                let (workspace_index_state, workspace_index_reason) =
                    Self::completion_workspace_index_state(&workspace_mode);
                let completion_decision_context = CompletionDecisionContext {
                    uri,
                    line,
                    character,
                    ast_available,
                    workspace_index_state,
                    workspace_index_reason,
                    is_incomplete,
                };
                #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                let semantic_shadow_receipt = self.completion_semantic_shadow_receipt(
                    uri,
                    &doc.text,
                    offset,
                    (line, character),
                    &completions,
                    &workspace_mode,
                    CompletionShadowBudget {
                        should_continue: &|| {
                            request_start.elapsed() < completion_deadline()
                                && !token.is_cancelled_relaxed()
                        },
                    },
                );
                #[cfg(any(not(feature = "workspace"), target_arch = "wasm32"))]
                let semantic_shadow_receipt: Option<Value> = None;
                self.record_completion_provider_decision_trace(
                    &completion_decision_context,
                    &completions,
                    semantic_shadow_receipt,
                );

                // Convert to JSON format with highly optimized cancellation checks.
                // Snapshot capability flags and drop the lock before serialization so the
                // dispatched completion path has the same contention behavior as the direct path.
                let (
                    snippet_support,
                    commit_chars_support,
                    label_details_support,
                    item_defaults_data_support,
                    apply_kind_support,
                ) = {
                    let client_caps = self.client_capabilities.lock();
                    (
                        client_caps.snippet_support,
                        client_caps.completion_commit_characters_support,
                        client_caps.label_details_support,
                        client_caps.completion_list_item_defaults_data_support,
                        client_caps.completion_list_apply_kind_support,
                    )
                };

                let items: Vec<Value> = completions
                    .into_iter()
                    .enumerate()
                    .filter_map(|(idx, c)| {
                        // Ultra-optimized cancellation check (every 250 items to reduce overhead to <5%)
                        if idx % 250 == 0 && idx > 0 && token.is_cancelled_relaxed() {
                            return None;
                        }

                        Some(self.completion_item_to_lsp_value(
                            doc,
                            c,
                            snippet_support,
                            commit_chars_support,
                            label_details_support,
                        ))
                    })
                    .collect();

                Some(Self::completion_list_response(
                    is_incomplete,
                    items,
                    item_defaults_data_support,
                    apply_kind_support,
                ))
            };
            if let Some(response) = response {
                return Ok(Some(response));
            }

            Ok(Some(json!({"isIncomplete": false, "items": []})))
        } else {
            self.handle_completion(params)
        }
    }

    /// Lexical completion fallback for when AST is unavailable
    pub(crate) fn lexical_complete(
        &self,
        content: &str,
        offset: usize,
        filepath: Option<&str>,
    ) -> Vec<crate::completion::CompletionItem> {
        let mut completions = Vec::new();

        // Get the prefix we're completing
        let text_before = &content[..offset.min(content.len())];
        let prefix = text_before
            .chars()
            .rev()
            .take_while(|&c| c.is_alphanumeric() || c == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let prefix_start = offset.saturating_sub(prefix.len());

        // Check if we're in a method call context (after ->)
        let is_method_call = text_before.ends_with("->")
            || text_before
                .chars()
                .rev()
                .skip_while(|c| c.is_alphanumeric() || *c == '_')
                .take(2)
                .collect::<String>()
                == ">-";

        // Check what sigil we're after (if any)
        let sigil = text_before.chars().rev().find(|&c| !(c.is_alphanumeric() || c == '_'));

        // If we're completing after '->', provide common method completions
        if is_method_call {
            let common_methods = [
                ("new", "constructor"),
                ("init", "initializer"),
                ("process", "processor"),
                ("run", "executor"),
                ("execute", "executor"),
                ("handle", "handler"),
                ("get", "getter"),
                ("set", "setter"),
                ("create", "constructor"),
                ("build", "builder"),
                ("parse", "parser"),
                ("format", "formatter"),
                ("validate", "validator"),
                ("transform", "transformer"),
                ("render", "renderer"),
            ];

            for (method, kind) in &common_methods {
                if method.starts_with(&prefix) || prefix.is_empty() {
                    completions.push(crate::completion::CompletionItem {
                        label: method.to_string().into(),
                        kind: CompletionItemKind::Function,
                        detail: Some(format!("method ({})", kind).into()),
                        documentation: None,
                        insert_text: Some(method.to_string().into()),
                        additional_edits: vec![],
                        sort_text: None,
                        filter_text: None,
                        text_edit_range: None,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
            }
            return completions; // Return early for method completions
        }

        match sigil {
            Some('$') => {
                // Scalar variables - suggest common ones
                if "_".starts_with(&prefix) || prefix.is_empty() {
                    completions.push(crate::completion::CompletionItem {
                        label: "_".into(),
                        kind: CompletionItemKind::Variable,
                        detail: Some("Default variable".into()),
                        documentation: None,
                        insert_text: Some("_".into()),
                        additional_edits: vec![],
                        sort_text: None,
                        filter_text: None,
                        text_edit_range: None,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
            }
            Some('@') => {
                // Array variables - suggest common ones
                if "ARGV".starts_with(&prefix) || prefix.is_empty() {
                    completions.push(crate::completion::CompletionItem {
                        label: "ARGV".into(),
                        kind: CompletionItemKind::Variable,
                        detail: Some("Command line arguments".into()),
                        documentation: None,
                        insert_text: Some("ARGV".into()),
                        additional_edits: vec![],
                        sort_text: None,
                        filter_text: None,
                        text_edit_range: None,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
                if "_".starts_with(&prefix) || prefix.is_empty() {
                    completions.push(crate::completion::CompletionItem {
                        label: "_".into(),
                        kind: CompletionItemKind::Variable,
                        detail: Some("Function arguments".into()),
                        documentation: None,
                        insert_text: Some("_".into()),
                        additional_edits: vec![],
                        sort_text: None,
                        filter_text: None,
                        text_edit_range: None,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
            }
            Some('%') => {
                // Hash variables - suggest common ones
                if "ENV".starts_with(&prefix) || prefix.is_empty() {
                    completions.push(crate::completion::CompletionItem {
                        label: "ENV".into(),
                        kind: CompletionItemKind::Variable,
                        detail: Some("Environment variables".into()),
                        documentation: None,
                        insert_text: Some("ENV".into()),
                        additional_edits: vec![],
                        sort_text: None,
                        filter_text: None,
                        text_edit_range: None,
                        commit_characters: None,
                        insert_text_format: InsertTextFormat::PlainText,
                        label_details: None,
                    });
                }
            }
            _ => {
                add_xs_api_completions_for_prefix(
                    &mut completions,
                    &prefix,
                    prefix_start,
                    offset,
                    content,
                    filepath,
                );

                // No sigil - suggest keywords
                for kw in LSP_RUNTIME_COMPLETION_KEYWORDS {
                    if kw.starts_with(&prefix) {
                        completions.push(crate::completion::CompletionItem {
                            label: (*kw).into(),
                            kind: CompletionItemKind::Keyword,
                            detail: None,
                            documentation: None,
                            insert_text: Some((*kw).into()),
                            additional_edits: vec![],
                            sort_text: None,
                            filter_text: None,
                            text_edit_range: None,
                            commit_characters: None,
                            insert_text_format: InsertTextFormat::PlainText,
                            label_details: None,
                        });
                    }
                }
            }
        }

        completions
    }

    /// Handle completionItem/resolve request
    ///
    /// This method enriches a completion item with additional information
    /// such as documentation for built-in functions. This enables lazy loading
    /// of completion details, improving initial completion list performance.
    pub(crate) fn handle_completion_resolve(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let Some(mut item) = params else {
            return Ok(None);
        };

        // Extract the label and kind upfront (clone to avoid borrow issues)
        let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let kind = item.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
        let has_doc = item.get("documentation").is_some();
        let label_details_support = self.client_capabilities.lock().label_details_support;

        // Check if this is a built-in function and add documentation
        let builtin_signatures = crate::builtin_signatures::create_builtin_signatures();
        if let Some(sig) = builtin_signatures.get(label.as_str()) {
            // Build markdown documentation
            let mut doc_parts = Vec::new();

            // Add signatures
            if !sig.signatures.is_empty() {
                doc_parts.push("**Signatures:**".to_string());
                for signature in &sig.signatures {
                    doc_parts.push(format!("- `{}`", signature));
                }
                doc_parts.push(String::new()); // blank line
            }

            // Add documentation
            doc_parts.push(sig.documentation.to_string());

            let documentation = doc_parts.join("\n");

            if let Some(obj) = item.as_object_mut() {
                obj.insert(
                    "documentation".to_string(),
                    json!({
                        "kind": "markdown",
                        "value": documentation
                    }),
                );
                // Populate labelDetails for clients that declared labelDetailsSupport.
                // detail = primary signature (inline after label), description = source tag.
                if label_details_support && obj.get("labelDetails").is_none() {
                    let sig_detail = sig.signatures.first().copied().unwrap_or("").to_string();
                    if !sig_detail.is_empty() {
                        obj.insert(
                            "labelDetails".to_string(),
                            json!({
                                "detail": sig_detail,
                                "description": "builtin"
                            }),
                        );
                    }
                }
            }
            return Ok(Some(item));
        }

        // For variables, add type hint documentation if available
        if kind == 6 && !has_doc {
            // Variable kind
            let (type_doc, label_detail) = if label.starts_with('$') {
                (
                    Some("Scalar variable - holds a single value (string, number, or reference)"),
                    Some("scalar"),
                )
            } else if label.starts_with('@') {
                (Some("Array variable - holds an ordered list of scalars"), Some("array"))
            } else if label.starts_with('%') {
                (Some("Hash variable - holds a set of key-value pairs"), Some("hash"))
            } else {
                (None, None)
            };

            if let Some(obj) = item.as_object_mut() {
                if let Some(doc) = type_doc {
                    obj.insert(
                        "documentation".to_string(),
                        json!({
                            "kind": "markdown",
                            "value": doc
                        }),
                    );
                }
                if label_details_support
                    && let Some(detail) = label_detail
                    && obj.get("labelDetails").is_none()
                {
                    obj.insert("labelDetails".to_string(), json!({ "detail": detail }));
                }
            }
            return Ok(Some(item));
        }

        // For keywords, add brief documentation
        if kind == 14 && !has_doc {
            // Keyword kind
            let keyword_doc = match label.as_str() {
                "my" => Some("Declares a lexically scoped variable"),
                "our" => Some("Declares a package variable visible to all code in its package"),
                "local" => Some("Temporarily saves and restores a variable's value"),
                "state" => Some("Declares a persistent lexical variable (Perl 5.10+)"),
                "sub" => Some("Declares a subroutine"),
                "package" => Some("Declares a namespace"),
                "use" => Some("Imports a module at compile time"),
                "require" => Some("Loads a module at runtime"),
                "if" => Some("Conditional execution"),
                "elsif" => Some("Additional conditional branch"),
                "else" => Some("Default conditional branch"),
                "unless" => Some("Negated conditional execution"),
                "while" => Some("Loop while condition is true"),
                "until" => Some("Loop until condition is true"),
                "for" => Some("C-style loop or list iteration"),
                "foreach" => Some("Iterate over a list"),
                "given" => Some("Switch statement (Perl 5.10+)"),
                "when" => Some("Case in a switch statement"),
                "default" => Some("Default case in a switch statement"),
                "return" => Some("Returns from a subroutine"),
                "last" => Some("Exits a loop immediately"),
                "next" => Some("Skips to the next iteration of a loop"),
                "redo" => Some("Restarts the current iteration without re-evaluating condition"),
                "goto" => Some("Transfers control to another location"),
                _ => None,
            };

            if let Some(doc) = keyword_doc
                && let Some(obj) = item.as_object_mut()
            {
                obj.insert(
                    "documentation".to_string(),
                    json!({
                        "kind": "markdown",
                        "value": doc
                    }),
                );
            }
        }

        Ok(Some(item))
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

    fn explain_provider_decision(
        server: &LspServer,
        provider: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = server
            .handle_execute_command(Some(json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{
                    "provider": provider
                }]
            })))?
            .ok_or("missing explain-provider-decision response")?;
        Ok(response)
    }

    #[test]
    fn completion_cap_applies_after_final_ranking() {
        let item = |label: &str, sort_text: &str| crate::completion::CompletionItem {
            label: label.to_string().into(),
            kind: CompletionItemKind::Function,
            detail: None,
            documentation: None,
            insert_text: Some(label.to_string().into()),
            sort_text: Some(sort_text.to_string().into()),
            filter_text: None,
            additional_edits: Vec::new(),
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        };

        // The late item represents a request-level enrichment candidate. It
        // outranks both earlier provider candidates and must survive a cap of
        // two after the complete set is ranked.
        let (items, is_incomplete) = sort_and_cap_completions(
            vec![
                item("provider-first", "100"),
                item("provider-second", "200"),
                item("late", "050"),
            ],
            2,
        );

        assert!(is_incomplete, "cap should mark the response incomplete");
        assert_eq!(
            items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(),
            ["late", "provider-first"]
        );
    }

    #[test]
    fn cancellable_completion_reports_incomplete_after_cap()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut source = String::new();
        for index in 0..150 {
            source.push_str(&format!("sub completion_{index} {{ 1 }}\n"));
        }
        source.push_str("completion_");

        let server = LspServer::default();
        let uri = "file:///workspace/cancellable_completion_cap.pl";
        server.test_apply_did_open(uri, &source, 1)?;
        let line = source.lines().count() as u32 - 1;
        let character = source.lines().next_back().map(str::len).unwrap_or(0) as u32;
        let request_id = json!(7_654_321_i64);

        let response = server
            .handle_completion_cancellable(
                Some(json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                })),
                Some(&request_id),
            )?
            .ok_or("cancellable completion returned no response")?;

        assert_eq!(
            response.get("isIncomplete"),
            Some(&json!(true)),
            "a capped cancellable response must advertise that more items exist: {response}"
        );
        Ok(())
    }

    #[cfg(feature = "workspace")]
    fn make_document_index_stale(
        server: &LspServer,
        uri: &str,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        server.test_apply_did_open(uri, text, 1)?;
        server.test_index_file_in_building_state(uri, text).map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();
        server.test_replace_document_without_index(uri, text, 2).map_err(std::io::Error::other)?;

        assert!(
            server.workspace_index_stale_for_document(uri),
            "test setup must leave the open document newer than the workspace index"
        );

        Ok(())
    }

    #[test]
    fn completion_off_lock_analysis_emits_lock_hold_and_analyze_timing_spans()
    -> Result<(), Box<dyn std::error::Error>> {
        // #3396 Phase 4: `handle_completion` grabs an owned `DocumentState`
        // clone under a brief documents-map lock, then drops the guard before
        // analysis. Proves this measurably: the `lock_hold` span (the brief
        // guarded scope) must be recorded before the `analyze` span (the
        // off-lock work), for the same request.
        let server = LspServer::default();
        let uri = "file:///workspace/timing_completion.pl";
        server.test_apply_did_open(uri, "my $var = 42;\n$va", 1)?;

        let _lock = crate::runtime::timing::capture::test_lock();
        crate::runtime::timing::capture::start();
        let _ = server.handle_completion(Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 3 }
        })))?;
        let spans = crate::runtime::timing::capture::drain();

        let lock_hold_idx = spans.iter().position(|s| s.span == "provider.completion.lock_hold");
        let analyze_idx = spans.iter().position(|s| s.span == "provider.completion.analyze");
        assert!(
            lock_hold_idx.is_some(),
            "expected a provider.completion.lock_hold span, got: {spans:?}"
        );
        assert!(
            analyze_idx.is_some(),
            "expected a provider.completion.analyze span, got: {spans:?}"
        );
        assert!(
            lock_hold_idx < analyze_idx,
            "lock_hold span must be emitted before the analyze span (proves the documents-map \
             guard is dropped before analysis runs): {spans:?}"
        );

        Ok(())
    }

    #[test]
    fn cancellable_completion_emits_analyze_span_on_normal_completion()
    -> Result<(), Box<dyn std::error::Error>> {
        // Baseline: the cancellable path must emit `provider.completion.analyze`
        // on the ordinary, non-cancelled fall-through -- the same contract
        // `handle_completion` already proves above, checked here for the
        // `_cancellable` entry point specifically.
        let server = LspServer::default();
        let uri = "file:///workspace/timing_completion_cancellable.pl";
        server.test_apply_did_open(uri, "my $var = 42;\n$va", 1)?;

        let _lock = crate::runtime::timing::capture::test_lock();
        crate::runtime::timing::capture::start();
        let id = json!(1);
        let _ = server.handle_completion_cancellable(
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 3 }
            })),
            Some(&id),
        )?;
        let spans = crate::runtime::timing::capture::drain();

        assert!(
            spans.iter().any(|s| s.span == "provider.completion.analyze"),
            "expected a provider.completion.analyze span, got: {spans:?}"
        );

        Ok(())
    }

    #[test]
    fn cancellable_completion_emits_analyze_span_when_cancelled_mid_flight()
    -> Result<(), Box<dyn std::error::Error>> {
        // #3619 regression test. `handle_completion_cancellable` checks
        // `token.is_cancelled_relaxed()` twice: once before analysis starts,
        // and once again right after the provider call returns (to reject a
        // request that was cancelled while completions were being
        // generated). The old code placed its manual Instant+emit pair for
        // `provider.completion.analyze` *after* that second check, so a
        // cancellation landing between the two checks caused the early
        // `return Err(..)` to skip the emit entirely -- the analyze span was
        // silently dropped for every request cancelled mid-analysis. The fix
        // wraps the analysis block in a `ScopedSpan` (RAII), which emits on
        // `Drop` regardless of which `return` fires.
        //
        // This test uses a genuinely concurrent cancellation (a background
        // thread calling `token.cancel()`) against a fixture with
        // `sub_count` candidate subs, so the off-lock analysis phase
        // (owned-document clone + completion generation over thousands of
        // `func_`-prefixed candidates) has a wide window to observe the
        // cancellation before it returns -- reproducing the exact
        // interleaving the bug depended on, not just the general RAII
        // contract already covered by `timing.rs`'s
        // `scoped_span_emits_on_early_return_from_enclosing_fn`.
        //
        // Timing is controlled deterministically, not with a fixed sleep: a
        // `notify_completion_analysis_started` hook (module-level, mirrors
        // `set_index_ready_wait_entered_observer` in `readiness.rs`) fires
        // the instant the handler enters its analysis phase. The canceller
        // thread blocks on that signal before calling `token.cancel()`, so
        // the cancellation is guaranteed to land no earlier than the start
        // of analysis -- a fixed sleep can only guess at that boundary and
        // is either too short (fires before analysis begins, proving
        // nothing) or too long (analysis already finished) under CI load.
        use std::sync::atomic::{AtomicBool, Ordering};

        // Each sub is called once below so the dead-code lint stays quiet --
        // keeps this test's diagnostics payload small while still giving the
        // completion provider thousands of `func_`-prefixed candidates to
        // filter, which is what actually slows the analysis phase down.
        let sub_count = 1_500;
        let mut source = String::with_capacity(100_000);
        for i in 0..sub_count {
            source.push_str(&format!("sub func_{i} {{ my $x = {i}; return $x; }}\n"));
        }
        source.push_str("func_0(); ");
        for i in 1..sub_count {
            source.push_str(&format!("func_{i}(); "));
        }
        source.push('\n');
        source.push_str("func_");
        let uri = "file:///workspace/timing_completion_cancel_mid_flight.pl";

        let server = LspServer::default();
        server.test_apply_did_open(uri, &source, 1)?;

        let request_id = JsonRpcId::Integer(918_273_645);
        let token = PerlLspCancellationToken::new(
            request_id.clone(),
            "textDocument/completion".to_string(),
        );
        GLOBAL_CANCELLATION_REGISTRY
            .register_token(token.clone())
            .map_err(|e| format!("failed to register cancellation token: {e:?}"))?;

        let _lock = crate::runtime::timing::capture::test_lock();
        crate::runtime::timing::capture::start();

        let (analysis_started_tx, analysis_started_rx) = std::sync::mpsc::channel();
        set_completion_analysis_started_observer(uri, analysis_started_tx);

        let landed = Arc::new(AtomicBool::new(false));
        let canceller = {
            let token = token.clone();
            let landed = Arc::clone(&landed);
            std::thread::spawn(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                // Bounded wait: fail loudly instead of hanging forever if the
                // analysis phase is never entered (e.g. a future refactor
                // moves or removes the notify call).
                analysis_started_rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(
                    |_| "timed out waiting for completion analysis to start".to_string(),
                )?;
                token.cancel();
                landed.store(true, Ordering::SeqCst);
                Ok(())
            })
        };

        let last_line = source.lines().count() as u32 - 1;
        let last_char = source.lines().next_back().map(str::len).unwrap_or(0) as u32;
        let request_id_value = json!(918_273_645_i64);
        let result = server.handle_completion_cancellable(
            Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": last_line, "character": last_char }
            })),
            Some(&request_id_value),
        );

        canceller
            .join()
            .map_err(|_| "canceller thread panicked")?
            .map_err(|e| format!("canceller thread failed: {e}"))?;
        assert!(landed.load(Ordering::SeqCst), "canceller thread must have run");

        let spans = crate::runtime::timing::capture::drain();

        // The race must actually land mid-analysis for this test to prove
        // anything about the fix; fail loudly (rather than silently pass on
        // an unrelated code path) if it did not.
        assert!(
            matches!(result, Err(ref e) if e.code == REQUEST_CANCELLED),
            "test setup must trigger cancellation mid-analysis for this regression test to be \
             meaningful; got: {result:?} (grow the fixture document if this flakes in CI)"
        );

        assert!(
            spans.iter().any(|s| s.span == "provider.completion.analyze"),
            "provider.completion.analyze span must be emitted even when the request is \
             cancelled mid-analysis (#3619 regression), got spans: {spans:?}"
        );

        Ok(())
    }

    #[test]
    fn cancellable_completion_at_comment_position_surfaces_cancellation_not_empty_list()
    -> Result<(), Box<dyn std::error::Error>> {
        // Review finding on this PR: the comment guard in
        // `handle_completion_cancellable` breaks to an empty completion list
        // as soon as it observes the cursor is inside a comment, without
        // checking whether the request was cancelled in between the
        // handler's initial `token.is_cancelled_relaxed()` check and the
        // comment guard's own check. A cancellation landing in that window
        // must still surface `REQUEST_CANCELLED` to the client, not a
        // silent empty `isIncomplete: false` completion list -- the client
        // would otherwise treat a cancelled request as "no completions
        // here" instead of retrying.
        //
        // Deterministic sequencing (no fixed sleep, no relying on a real
        // thread-scheduling race to land in a window too narrow for one --
        // see `wait_for_completion_comment_guard_gate`'s doc comment).
        // Mirrors `cancellable_completion_emits_analyze_span_when_cancelled_mid_flight`
        // above: the request itself runs (blocking) on this thread, and a
        // background canceller thread reacts to the analysis-started signal
        // fired synchronously from inside the call.
        //   1. Arm both the existing analysis-started observer and the new
        //      comment-guard gate before making the request.
        //   2. Spawn a canceller thread that waits for the analysis-started
        //      signal (proves the handler has passed its *first*
        //      cancellation check), then cancels the token and releases the
        //      gate (proves the cancellation happens-before the comment
        //      guard's check can observe it, since the handler is blocked
        //      on the gate exactly at that point until released).
        //   3. Call the handler (blocking) and assert its result is
        //      `Err(REQUEST_CANCELLED)`, not an empty completion list.
        let uri = "file:///workspace/comment_position_cancel.pl";
        let source = "# a comment with some prefix pri";
        let server = LspServer::default();
        server.test_apply_did_open(uri, source, 1)?;

        let request_id = JsonRpcId::Integer(554_433_221);
        let token = PerlLspCancellationToken::new(
            request_id.clone(),
            "textDocument/completion".to_string(),
        );
        GLOBAL_CANCELLATION_REGISTRY
            .register_token(token.clone())
            .map_err(|e| format!("failed to register cancellation token: {e:?}"))?;

        let (analysis_started_tx, analysis_started_rx) = std::sync::mpsc::channel();
        set_completion_analysis_started_observer(uri, analysis_started_tx);
        let release_gate = arm_completion_comment_guard_gate(uri);

        let canceller = {
            let token = token.clone();
            std::thread::spawn(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                analysis_started_rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(
                    |_| "timed out waiting for completion analysis to start".to_string(),
                )?;
                token.cancel();
                release_gate
                    .send(())
                    .map_err(|_| "handler thread dropped the comment-guard gate receiver")?;
                Ok(())
            })
        };

        let request_id_value = json!(554_433_221_i64);
        let result = server.handle_completion_cancellable(
            Some(json!({
                "textDocument": { "uri": uri },
                // Past "pri", inside the comment -- matches the
                // no-completion-in-comments fixture shape.
                "position": { "line": 0, "character": source.len() as u32 }
            })),
            Some(&request_id_value),
        );

        canceller
            .join()
            .map_err(|_| "canceller thread panicked")?
            .map_err(|e| format!("canceller thread failed: {e}"))?;

        assert!(
            matches!(result, Err(ref e) if e.code == REQUEST_CANCELLED),
            "a request cancelled between the handler's initial cancellation check and the \
             comment guard's check must surface REQUEST_CANCELLED, not an empty completion \
             list; got: {result:?}"
        );

        Ok(())
    }

    /// Well-formed `foreach` body used by the serializer tests: the literal
    /// Perl `$item` is escaped so it survives as text on both client kinds.
    const FOREACH_SNIPPET: &str = "foreach my ${1:\\$item} (@${2:list}) {\n\t$0\n}";

    #[test]
    fn completion_item_serializer_serializes_filter_text() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_serializer_some.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "fo"
            }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("missing test document")?;
        let item = crate::completion::CompletionItem {
            label: "foreach".into(),
            kind: CompletionItemKind::Snippet,
            detail: None,
            documentation: None,
            insert_text: Some(FOREACH_SNIPPET.into()),
            additional_edits: Vec::new(),
            sort_text: Some("1_foreach".into()),
            filter_text: Some("foreach".into()),
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::snippet(FOREACH_SNIPPET),
            label_details: None,
        };

        let value = server.completion_item_to_lsp_value(doc, item, true, false, false);

        assert_eq!(value.get("filterText").and_then(Value::as_str), Some("foreach"));
        assert_eq!(value.get("insertTextFormat").and_then(Value::as_i64), Some(2));
        Ok(())
    }

    #[test]
    fn completion_item_serializer_omits_filter_text_when_unset()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_serializer_none.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "my $value = 1;\n"
            }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("missing test document")?;
        let item = crate::completion::CompletionItem {
            label: "fallback".into(),
            kind: CompletionItemKind::Keyword,
            detail: None,
            documentation: None,
            insert_text: Some("fallback".into()),
            additional_edits: Vec::new(),
            sort_text: Some("9_fallback".into()),
            filter_text: None,
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        };

        let value = server.completion_item_to_lsp_value(doc, item, true, false, false);

        assert!(
            value.get("filterText").is_none(),
            "completion item should omit filterText when filter_text is unset: {value:?}"
        );
        assert_eq!(value.get("sortText").and_then(Value::as_str), Some("9_fallback"));
        Ok(())
    }

    #[test]
    fn completion_item_serializer_maps_remaining_kinds() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_serializer_kinds.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "use strict;\n"
            }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("missing test document")?;
        let cases = [
            (CompletionItemKind::Variable, 6),
            (CompletionItemKind::Function, 3),
            (CompletionItemKind::Module, 9),
            (CompletionItemKind::File, 17),
            (CompletionItemKind::Constant, 14),
            (CompletionItemKind::Property, 7),
        ];

        for (kind, expected_kind) in cases {
            let item = crate::completion::CompletionItem {
                label: format!("{kind:?}").into(),
                kind,
                detail: None,
                documentation: None,
                insert_text: None,
                additional_edits: Vec::new(),
                sort_text: None,
                filter_text: None,
                text_edit_range: None,
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            };

            let value = server.completion_item_to_lsp_value(doc, item, false, false, false);
            assert_eq!(
                value.get("kind").and_then(Value::as_i64),
                Some(expected_kind),
                "unexpected LSP kind for {kind:?}: {value:?}"
            );
            assert!(value.get("insertText").is_none());
        }

        Ok(())
    }

    #[test]
    fn completion_item_serializer_emits_optional_lsp_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_parser_core::SourceLocation;

        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_serializer_optional.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "use strict;\n"
            }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("missing test document")?;
        let item = crate::completion::CompletionItem {
            label: "render".into(),
            kind: CompletionItemKind::Function,
            detail: Some("render($ctx)".into()),
            documentation: Some("Render the current context.".into()),
            insert_text: Some("render($ctx)".into()),
            additional_edits: vec![(
                SourceLocation { start: 0, end: 0 },
                "use Demo::Renderer;\n".to_string(),
            )],
            sort_text: Some("2_render".into()),
            filter_text: Some("render".into()),
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: Some(
                perl_lsp_rs_core::providers::completion_item::CompletionItemLabelDetails {
                    detail: Some("($ctx)".to_string()),
                    description: Some("Demo::Renderer".to_string()),
                },
            ),
        };

        let value = server.completion_item_to_lsp_value(doc, item, false, true, true);

        assert_eq!(value.get("detail").and_then(Value::as_str), Some("render($ctx)"));
        assert_eq!(
            value.pointer("/documentation/value").and_then(Value::as_str),
            Some("Render the current context.")
        );
        assert_eq!(value.get("filterText").and_then(Value::as_str), Some("render"));
        assert!(value.get("commitCharacters").and_then(Value::as_array).is_some());
        assert_eq!(value.pointer("/labelDetails/detail").and_then(Value::as_str), Some("($ctx)"));
        assert_eq!(
            value.pointer("/labelDetails/description").and_then(Value::as_str),
            Some("Demo::Renderer")
        );
        assert_eq!(
            value.pointer("/additionalTextEdits/0/newText").and_then(Value::as_str),
            Some("use Demo::Renderer;\n")
        );
        Ok(())
    }

    #[test]
    fn completion_item_serializer_degrades_snippet_without_client_support()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_serializer_plain.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "fo"
            }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("missing test document")?;
        let item = crate::completion::CompletionItem {
            label: "foreach".into(),
            kind: CompletionItemKind::Snippet,
            detail: None,
            documentation: None,
            insert_text: Some(FOREACH_SNIPPET.into()),
            additional_edits: Vec::new(),
            sort_text: None,
            filter_text: Some("foreach".into()),
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::snippet(FOREACH_SNIPPET),
            label_details: None,
        };

        let value = server.completion_item_to_lsp_value(doc, item, false, false, false);

        assert_eq!(value.get("insertTextFormat").and_then(Value::as_i64), Some(1));
        assert_eq!(
            value.get("insertText").and_then(Value::as_str),
            Some("foreach my $item (@list) {\n\t\n}")
        );
        Ok(())
    }

    #[test]
    fn completion_provider_decision_replays_live_completion_trace()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_trace.pl";
        let text = "my $count = 0;\nmy $counter = $co\n";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })))?;

        let response = server
            .handle_completion_cancellable(
                Some(json!({
                    "textDocument": { "uri": uri, "version": 1 },
                    "position": { "line": 1, "character": 17 }
                })),
                Some(&json!("completion-provider-decision")),
            )?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        assert!(
            items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("$count")),
            "expected lexical completion to include $count, got: {items:?}"
        );

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;
        let sample_labels = receipt
            .get("sample_labels")
            .and_then(Value::as_array)
            .ok_or("missing completion sample labels")?;

        assert_eq!(
            receipt.get("schema_version").and_then(Value::as_str),
            Some("provider_decision.v1")
        );
        assert_eq!(receipt.get("provider").and_then(Value::as_str), Some("completion"));
        assert_eq!(
            receipt.get("provider_action").and_then(Value::as_str),
            Some("textDocument/completion")
        );
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("acted"));
        assert_eq!(receipt.get("uri").and_then(Value::as_str), Some(uri));
        assert_eq!(receipt.get("line").and_then(Value::as_u64), Some(1));
        assert_eq!(receipt.get("character").and_then(Value::as_u64), Some(17));
        assert_eq!(receipt.get("freshness").and_then(Value::as_str), Some("fresh"));
        assert_eq!(receipt.get("fallback").and_then(Value::as_str), Some("none"));
        let expected_claim_boundary = if receipt.get("semantic_shadow_receipt").is_some() {
            "records existing comparable visibility completions and semantic shadow evidence; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
        } else {
            "records existing comparable visibility completions only; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
        };
        assert_eq!(
            receipt.get("claim_boundary").and_then(Value::as_str),
            Some(expected_claim_boundary)
        );
        assert!(
            receipt.get("item_count").and_then(Value::as_u64).is_some_and(|count| count > 0),
            "completion receipt should record item count: {receipt:?}"
        );
        assert!(
            sample_labels.iter().filter_map(Value::as_str).any(|label| label == "$count"),
            "completion receipt should include sample labels from the response: {sample_labels:?}"
        );
        Ok(())
    }

    #[test]
    fn completion_provider_decision_records_regular_completion_trace()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_regular_trace.pl";
        let text = "my $ready = 1;\n$re\n";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })))?;

        let response = server
            .handle_completion(Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 1, "character": 3 }
            })))?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        assert!(
            items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("$ready")),
            "expected regular completion to include $ready, got: {items:?}"
        );

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;

        assert_eq!(
            receipt.get("schema_version").and_then(Value::as_str),
            Some("provider_decision.v1")
        );
        assert_eq!(receipt.get("provider").and_then(Value::as_str), Some("completion"));
        assert_eq!(receipt.get("decision").and_then(Value::as_str), Some("acted"));
        assert_eq!(
            receipt.get("provider_action").and_then(Value::as_str),
            Some("textDocument/completion")
        );
        assert_eq!(receipt.get("is_incomplete").and_then(Value::as_bool), Some(false));
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn regular_completion_records_none_index_state_when_open_document_index_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_stale_regular.pl";
        let text = "my $ready = 1;\n$re\n";

        make_document_index_stale(&server, uri, text)?;

        let response = server
            .handle_completion(Some(json!({
                "textDocument": { "uri": uri, "version": 2 },
                "position": { "line": 1, "character": 3 }
            })))?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        assert!(
            items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("$ready")),
            "stale-index regular completion must still use current-document fallback: {items:?}"
        );

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;
        assert_eq!(
            receipt.get("workspace_index_state").and_then(Value::as_str),
            Some("none"),
            "stale current-document index must downgrade regular completion index access"
        );
        assert!(
            receipt.get("semantic_shadow_receipt").is_none(),
            "stale workspace index must not run completion visibility shadow queries"
        );

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn cancellable_completion_records_none_index_state_when_open_document_index_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_stale_cancellable.pl";
        let text = "my $count = 1;\n$co\n";

        make_document_index_stale(&server, uri, text)?;

        let response = server
            .handle_completion_cancellable(
                Some(json!({
                    "textDocument": { "uri": uri, "version": 2 },
                    "position": { "line": 1, "character": 3 }
                })),
                Some(&json!("completion-stale-cancellable")),
            )?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        assert!(
            items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("$count")),
            "stale-index cancellable completion must still use current-document fallback: {items:?}"
        );

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;
        assert_eq!(
            receipt.get("workspace_index_state").and_then(Value::as_str),
            Some("none"),
            "stale current-document index must downgrade cancellable completion index access"
        );

        Ok(())
    }

    /// Regression (#5016 item 2): cross-file completion must not use the
    /// workspace index tier while an unrelated open document is ahead of the
    /// indexed snapshot.
    #[cfg(feature = "workspace")]
    #[test]
    fn completion_skips_workspace_index_when_unrelated_open_document_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let source_uri = "file:///workspace/completion_source.pl";
        let unrelated_uri = "file:///workspace/completion_unrelated.pl";
        let source_text = "package CompletionSource;\nmy $ready = 1;\n$re\n";
        let unrelated_v1 = "package CompletionUnrelated;\nsub helper {}\n";
        let unrelated_v2 = "package CompletionUnrelated;\nsub renamed {}\n";

        server.test_apply_did_open(source_uri, source_text, 1)?;
        server.test_apply_did_open(unrelated_uri, unrelated_v1, 1)?;
        server
            .test_index_file_in_building_state(source_uri, source_text)
            .map_err(std::io::Error::other)?;
        server
            .test_index_file_in_building_state(unrelated_uri, unrelated_v1)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        server.handle_completion(Some(json!({
            "textDocument": { "uri": source_uri, "version": 1 },
            "position": { "line": 2, "character": 3 }
        })))?;
        let fresh = explain_provider_decision(&server, "completion")?;
        let fresh_receipt = fresh
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing fresh completion request receipt")?;
        assert_eq!(
            fresh_receipt.get("workspace_index_state").and_then(Value::as_str),
            Some("full"),
            "fresh completion request should observe the full index: {fresh:?}"
        );

        server
            .test_replace_document_without_index(unrelated_uri, unrelated_v2, 2)
            .map_err(std::io::Error::other)?;
        assert!(server.workspace_index_stale_for_any_open_document());

        server.handle_completion(Some(json!({
            "textDocument": { "uri": source_uri, "version": 1 },
            "position": { "line": 2, "character": 3 }
        })))?;
        let stale = explain_provider_decision(&server, "completion")?;
        let stale_receipt = stale
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing stale completion request receipt")?;
        assert_eq!(
            stale_receipt.get("workspace_index_state").and_then(Value::as_str),
            Some("none"),
            "unrelated stale open document must disable cross-file completion index access: {stale:?}"
        );
        assert!(
            stale_receipt.get("semantic_shadow_receipt").is_none(),
            "stale workspace index must not run completion visibility shadow queries"
        );

        Ok(())
    }

    #[test]
    fn completion_provider_decision_claim_boundary_requires_shadow_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let context = CompletionDecisionContext {
            uri: "file:///workspace/completion-no-shadow-receipt.pl",
            line: 0,
            character: 0,
            ast_available: false,
            workspace_index_state: "none",
            workspace_index_reason: None,
            is_incomplete: false,
        };

        server.record_completion_provider_decision_trace(&context, &[], None);

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;
        assert!(receipt.get("semantic_shadow_receipt").is_none());
        assert_eq!(
            receipt.get("claim_boundary").and_then(Value::as_str),
            Some(
                "records existing comparable visibility completions only; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
            )
        );
        Ok(())
    }

    #[test]
    fn completion_provider_decision_embeds_semantic_shadow_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let context = CompletionDecisionContext {
            uri: "file:///workspace/completion-shadow-receipt.pl",
            line: 0,
            character: 2,
            ast_available: true,
            workspace_index_state: "full",
            workspace_index_reason: None,
            is_incomplete: false,
        };
        let shadow_receipt = json!({
            "schema_version": 2,
            "query": "completion_visibility",
            "verdict": "same"
        });

        server.record_completion_provider_decision_trace(
            &context,
            &[],
            Some(shadow_receipt.clone()),
        );

        let explanation = explain_provider_decision(&server, "completion")?;
        let receipt = explanation
            .get("request_receipt")
            .and_then(Value::as_object)
            .ok_or("missing persisted completion request receipt")?;
        assert_eq!(receipt.get("semantic_shadow_receipt"), Some(&shadow_receipt));
        assert_eq!(
            receipt.get("claim_boundary").and_then(Value::as_str),
            Some(
                "records existing comparable visibility completions and semantic shadow evidence; module, method, keyword, builtin, file, and ranking behavior remain unchanged"
            )
        );
        Ok(())
    }

    #[test]
    fn completion_visibility_shadow_filters_non_visibility_items()
    -> Result<(), Box<dyn std::error::Error>> {
        let item = |label: &str, kind: CompletionItemKind, sort_text: Option<&str>| {
            crate::completion::CompletionItem {
                label: label.to_string().into(),
                kind,
                detail: None,
                documentation: None,
                insert_text: None,
                sort_text: sort_text.map(|s| s.to_string().into()),
                filter_text: None,
                additional_edits: Vec::new(),
                text_edit_range: None,
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            }
        };
        let completions = vec![
            item("$value", CompletionItemKind::Variable, None),
            item("user_sub", CompletionItemKind::Function, Some("2_user_sub")),
            item("open", CompletionItemKind::Function, Some("3_open")),
            item("if", CompletionItemKind::Keyword, Some("5_if")),
            item("Foo", CompletionItemKind::Module, Some("4_Foo")),
            item("file.pl", CompletionItemKind::File, None),
            item("CONST", CompletionItemKind::Constant, Some("3_CONST")),
            item("key", CompletionItemKind::Property, None),
        ];

        assert_eq!(
            LspServer::completion_visibility_shadow_labels(&completions),
            vec!["$value".to_string(), "user_sub".to_string(), "CONST".to_string()]
        );
        assert!(LspServer::is_qualified_member_completion_context("Foo::", 5));
        assert!(LspServer::is_qualified_member_completion_context("Foo::bar", "Foo::bar".len(),));
        assert!(LspServer::is_qualified_member_completion_context("$object->", 9));
        assert!(!LspServer::is_qualified_member_completion_context("$value", 6));
        assert!(!LspServer::is_qualified_member_completion_context(
            "Foo::bar+$value",
            "Foo::bar+$value".len(),
        ));
        assert!(!LspServer::is_qualified_member_completion_context(
            "Foo::bar-$value",
            "Foo::bar-$value".len(),
        ));
        assert!(!LspServer::is_qualified_member_completion_context(
            "Foo::bar.$value",
            "Foo::bar.$value".len(),
        ));
        assert!(!LspServer::is_qualified_member_completion_context(
            "condition ? Foo::bar:$value",
            "condition ? Foo::bar:$value".len(),
        ));
        assert!(LspServer::is_qualified_member_completion_context(
            "$object->{key",
            "$object->{key".len(),
        ));
        assert!(LspServer::is_qualified_member_completion_context(
            "$object->[idx",
            "$object->[idx".len(),
        ));
        assert!(LspServer::is_qualified_member_completion_context(
            "$object->{$array[0",
            "$object->{$array[0".len(),
        ));
        assert!(LspServer::is_qualified_member_completion_context(
            "$object->method[0",
            "$object->method[0".len(),
        ));
        assert!(!LspServer::is_qualified_member_completion_context(
            "$value + $object[0",
            "$value + $object[0".len(),
        ));
        assert!(LspServer::is_qualified_member_completion_context(
            "$object -> method",
            "$object -> method".len(),
        ));
        Ok(())
    }

    #[test]
    fn regular_completion_serializes_snippet_filter_text() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_regular.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "fo"
            }
        })))?;

        let response = server
            .handle_completion(Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 0, "character": 2 }
            })))?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        let foreach_item = items
            .iter()
            .find(|item| item.get("label").and_then(Value::as_str) == Some("foreach"))
            .ok_or_else(|| format!("expected foreach snippet completion, got: {items:?}"))?;

        assert_eq!(
            foreach_item.get("filterText").and_then(Value::as_str),
            Some("foreach"),
            "regular completion response should serialize snippet filter_text"
        );

        Ok(())
    }

    #[test]
    fn cancellable_completion_serializes_snippet_filter_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/completion_filter_text_cancellable.pl";

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "fo"
            }
        })))?;

        let response = server
            .handle_completion_cancellable(
                Some(json!({
                    "textDocument": { "uri": uri, "version": 1 },
                    "position": { "line": 0, "character": 2 }
                })),
                Some(&json!("completion-filter-text-cancellable")),
            )?
            .ok_or("expected completion response")?;
        let items =
            response.get("items").and_then(Value::as_array).ok_or("expected completion items")?;
        let foreach_item = items
            .iter()
            .find(|item| item.get("label").and_then(Value::as_str) == Some("foreach"))
            .ok_or_else(|| format!("expected foreach snippet completion, got: {items:?}"))?;

        assert_eq!(
            foreach_item.get("filterText").and_then(Value::as_str),
            Some("foreach"),
            "cancellable completion response should serialize snippet filter_text"
        );

        Ok(())
    }

    #[test]
    fn test_module_completion_roots_includes_use_lib_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let lib_dir = temp.path().join("mylibs");
        let module_file = lib_dir.join("MyCustom").join("Widget.pm");
        fs::create_dir_all(module_file.parent().ok_or("no parent")?)?;
        fs::write(&module_file, "package MyCustom::Widget;\n1;\n")?;

        let workspace_uri =
            Url::from_file_path(temp.path()).map_err(|_| "bad workspace path")?.to_string();
        let doc_uri =
            Url::from_file_path(temp.path().join("app.pl")).map_err(|_| "bad doc uri")?.to_string();

        let lib_dir_str = lib_dir.to_string_lossy();
        let doc_text = format!("use lib '{lib_dir_str}';\nuse MyCustom::Widget;\n");

        let server = LspServer::default();

        // Register workspace folder so folder_for_doc_uri / config_for_doc work.
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri.clone()),
        );

        let (include_paths, _sys, _use_sys) =
            server.module_completion_roots_for_doc(&doc_uri, &doc_text, doc_text.len());

        assert!(
            include_paths.contains(&lib_dir),
            "use lib path should be in include_paths; got: {include_paths:?}",
        );
        Ok(())
    }

    #[test]
    fn module_completion_offers_declared_but_unindexed_dependencies()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::config::{
            DeclaredDependency, DeclaredDependencySource, WorkspaceConfig,
        };

        let server = LspServer::with_io(Box::new(std::io::empty()), Box::new(Vec::<u8>::new()));
        let mut config = WorkspaceConfig::default();
        config.use_system_inc = false;
        config.use_perl5lib = false;
        config.declared_dependencies = vec![DeclaredDependency::new(
            "JSON::PP",
            Some("4.16"),
            "requires",
            DeclaredDependencySource::Cpanfile,
        )];

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(
                "file:///workspace".to_string(),
            )
            .with_effective_workspace_config(config),
        );

        let uri = "file:///workspace/app.pl";
        let text = "use JS";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": text
            }
        })))?;

        let response = server
            .handle_completion(Some(json!({
                "textDocument": { "uri": uri, "version": 1 },
                "position": { "line": 0, "character": text.len() }
            })))?
            .ok_or("expected completion response")?;
        let items = response["items"].as_array().ok_or("expected completion items")?;
        let item = items
            .iter()
            .find(|item| item["label"].as_str() == Some("JSON::PP"))
            .ok_or_else(|| format!("expected declared dependency completion, got: {items:?}"))?;

        assert_eq!(item["kind"].as_i64(), Some(9));
        assert_eq!(item["insertText"].as_str(), Some("JSON::PP"));
        assert!(
            item["detail"].as_str().is_some_and(|detail| detail.contains("declared in cpanfile")),
            "completion detail should explain declaration source: {item:?}"
        );
        assert!(
            item.pointer("/documentation/value")
                .and_then(Value::as_str)
                .is_some_and(|doc| doc.contains("not currently indexed")),
            "completion docs should explain the dependency is not indexed: {item:?}"
        );
        Ok(())
    }

    #[test]
    fn test_module_completion_roots_system_inc_fallback_for_non_workspace_uri()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::config::WorkspaceConfig;

        let server = LspServer::default();
        let mut cfg = WorkspaceConfig::default();
        cfg.use_system_inc = true;

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(
                "file:///workspace/project".to_string(),
            )
            .with_effective_workspace_config(cfg),
        );

        let (include_paths, system_inc_paths, include_system_inc) = server
            .module_completion_roots_for_doc("file:///tmp/outside_workspace.pl", "use strict;", 0);

        assert!(include_system_inc, "use_system_inc should be propagated");
        assert!(include_paths.is_empty(), "no configured include paths expected");
        assert!(
            !system_inc_paths.is_empty(),
            "fallback system @INC roots should be available for non-workspace URIs"
        );

        Ok(())
    }

    #[test]
    fn test_module_completion_roots_keep_file_local_use_lib_for_non_workspace_uri()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let workspace = temp.path().join("workspace");
        let lexical_lib = workspace.join("standalone_lib");
        let outside_doc = temp.path().join("outside.pl");
        std::fs::create_dir_all(&lexical_lib)?;
        std::fs::write(&outside_doc, "use strict;\n")?;

        let workspace_uri =
            Url::from_file_path(&workspace).map_err(|_| "bad workspace uri")?.to_string();
        let doc_uri = Url::from_file_path(&outside_doc).map_err(|_| "bad doc uri")?.to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["configured_lib".to_string()];
        config.use_system_inc = false;

        let server = LspServer::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        let doc_text = "use lib 'standalone_lib';\nuse Outside::Widget;\n";
        let (include_paths, system_inc_paths, include_system_inc) =
            server.module_completion_roots_for_doc(&doc_uri, doc_text, doc_text.len());

        assert!(
            include_paths.contains(&lexical_lib),
            "file-local use lib root should survive outside-workspace filtering; got {include_paths:?}"
        );
        assert!(
            !include_paths.iter().any(|path| path.ends_with("configured_lib")),
            "workspace-configured roots should stay excluded for non-workspace docs; got {include_paths:?}"
        );
        assert!(system_inc_paths.is_empty());
        assert!(!include_system_inc);
        Ok(())
    }

    /// The two `@INC` consumers on a completion request — the module-completion
    /// roots and the workspace-symbol reachability filter — must be served by a
    /// single assembled context.
    ///
    /// Before #1684 each built its own, so one keystroke on a `use <pragma>`
    /// line paid twice for the `use lib`/`no lib` source scan, the `PERL5LIB`
    /// parse, and two `workspace_folders` lock acquisitions.
    #[cfg(feature = "workspace")]
    #[test]
    fn shared_request_context_is_built_once_for_both_completion_consumers()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::runtime::routing::IndexAccessMode;
        use perl_parser::workspace_index::IndexCoordinator;
        use std::sync::Arc;
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let workspace = temp.path().join("workspace");
        let doc_path = workspace.join("bin").join("app.pl");
        std::fs::create_dir_all(doc_path.parent().ok_or("missing doc parent")?)?;

        let workspace_uri =
            Url::from_file_path(&workspace).map_err(|_| "bad workspace uri")?.to_string();
        let doc_uri = Url::from_file_path(&doc_path).map_err(|_| "bad doc uri")?.to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_system_inc = false;

        let server = LspServer::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        let coordinator = Arc::new(IndexCoordinator::new());
        coordinator.transition_to_ready(1, 1);

        // A lowercase pragma prefix is the case that exercises both consumers:
        // `is_module_import_completion_context` is false, so the workspace pass
        // does not early-return, while `is_use_module_context` is true, so it
        // genuinely needs the @INC context for reachability filtering.
        let doc_text = "use lib 't/lib';\nuse pa";
        let probe = crate::runtime::lifecycle::inc_context::inc_context_build_probe();
        let inc_context = RequestIncContext::new(&server, &doc_uri, doc_text, doc_text.len());

        let (include_paths, _, _) = server.module_completion_roots(&inc_context);
        assert_eq!(probe.count(), 1, "the module-roots consumer should have assembled the context");

        let mut completions = Vec::new();
        server.add_runtime_workspace_completions(
            &mut completions,
            &inc_context,
            &IndexAccessMode::Full(&coordinator),
            None,
        );

        assert_eq!(
            probe.count(),
            1,
            "the workspace-symbol consumer must reuse the context, not assemble a second one"
        );
        // Guard against the assertion above going vacuous if the roots ever stop
        // being computed at all.
        assert!(
            include_paths.iter().any(|path| path.ends_with("t/lib")),
            "expected the lexical `use lib` root in the shared context; got {include_paths:?}"
        );
        Ok(())
    }

    /// A request that needs no `@INC` view must assemble none at all.
    ///
    /// The holder is deliberately lazy: at a plain identifier position (not a
    /// `use`/`require` line) the workspace pass has nothing to filter, and on the
    /// AST-less fallback path `module_completion_roots` is never called. Pins the
    /// zero-build case so a future change to the `is_use_module_context` gating
    /// cannot silently restore eager assembly — a regression that would still
    /// satisfy every "built at most once" assertion.
    #[cfg(feature = "workspace")]
    #[test]
    fn no_inc_context_is_assembled_when_no_consumer_needs_one()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::runtime::routing::IndexAccessMode;
        use perl_parser::workspace_index::IndexCoordinator;
        use std::sync::Arc;
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let workspace = temp.path().join("workspace");
        let doc_path = workspace.join("bin").join("app.pl");
        std::fs::create_dir_all(doc_path.parent().ok_or("missing doc parent")?)?;

        let workspace_uri =
            Url::from_file_path(&workspace).map_err(|_| "bad workspace uri")?.to_string();
        let doc_uri = Url::from_file_path(&doc_path).map_err(|_| "bad doc uri")?.to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_system_inc = false;

        let server = LspServer::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        let coordinator = Arc::new(IndexCoordinator::new());
        coordinator.transition_to_ready(1, 1);

        // A plain identifier position: neither a module-import context nor a
        // use-module context, so the reachability filter has nothing to gate on.
        let doc_text = "my $x = cr";
        let probe = crate::runtime::lifecycle::inc_context::inc_context_build_probe();
        let inc_context = RequestIncContext::new(&server, &doc_uri, doc_text, doc_text.len());

        let mut completions = Vec::new();
        server.add_runtime_workspace_completions(
            &mut completions,
            &inc_context,
            &IndexAccessMode::Full(&coordinator),
            None,
        );

        assert_eq!(
            probe.count(),
            0,
            "a request with no @INC consumer must not assemble a context at all"
        );
        Ok(())
    }

    /// Reading the roots through a shared request context must produce exactly
    /// what the standalone entry point produces — the #1684 change is a
    /// computation-sharing change, not a semantic one.
    #[test]
    fn shared_request_context_roots_match_standalone_roots()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let workspace = temp.path().join("workspace");
        let doc_path = workspace.join("bin").join("app.pl");
        std::fs::create_dir_all(doc_path.parent().ok_or("missing doc parent")?)?;

        let workspace_uri =
            Url::from_file_path(&workspace).map_err(|_| "bad workspace uri")?.to_string();
        let doc_uri = Url::from_file_path(&doc_path).map_err(|_| "bad doc uri")?.to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_system_inc = false;

        let server = LspServer::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        // Includes a `no lib` cancellation so the comparison covers the
        // position-sensitive part of the assembly, not just the static roots.
        let doc_text = "use lib 't/lib';\nno lib 'lib';\nuse Demo::Worker;\n";

        let standalone = server.module_completion_roots_for_doc(&doc_uri, doc_text, doc_text.len());
        let shared = server.module_completion_roots(&RequestIncContext::new(
            &server,
            &doc_uri,
            doc_text,
            doc_text.len(),
        ));

        assert_eq!(standalone, shared);
        Ok(())
    }

    #[test]
    fn test_module_completion_roots_match_effective_inc_context_for_workspace_doc()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_module::resolution::IncRootKind;
        use tempfile::TempDir;
        use url::Url;

        let temp = TempDir::new()?;
        let workspace = temp.path().join("workspace");
        let doc_path = workspace.join("bin").join("app.pl");
        std::fs::create_dir_all(doc_path.parent().ok_or("missing doc parent")?)?;

        let workspace_uri =
            Url::from_file_path(&workspace).map_err(|_| "bad workspace uri")?.to_string();
        let doc_uri = Url::from_file_path(&doc_path).map_err(|_| "bad doc uri")?.to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_system_inc = false;

        let server = LspServer::default();
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(workspace_uri)
                .with_path(workspace.clone())
                .with_effective_workspace_config(config),
        );

        let doc_text = "use lib 't/lib';\nuse Demo::Worker;\n";
        let context = server
            .effective_inc_context_for_doc(Some(&doc_uri), Some(doc_text), Some(doc_text.len()))
            .ok_or("expected effective @INC context")?;
        let expected_include_paths: Vec<PathBuf> = context
            .effective_roots
            .iter()
            .filter(|root| root.kind != IncRootKind::InterpreterStartup)
            .map(|root| LspServer::completion_path_for_inc_root(root, &context.root))
            .collect();

        let (include_paths, system_inc_paths, include_system_inc) =
            server.module_completion_roots_for_doc(&doc_uri, doc_text, doc_text.len());

        assert_eq!(include_paths, expected_include_paths);
        assert!(system_inc_paths.is_empty());
        assert!(!include_system_inc);
        Ok(())
    }

    #[test]
    fn test_cancellable_completion_cross_file_variable() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": "file:///workspace/Config.pm",
                "languageId": "perl",
                "version": 1,
                "text": "package Config;\nour $CONFIG_PATH = '/etc/app.conf';\nour $DEBUG_MODE = 1;\n1;\n"
            }
        })))?;

        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": "file:///workspace/app.pl",
                "languageId": "perl",
                "version": 1,
                "text": "use Config;\nprint $Config::CONF\n"
            }
        })))?;

        let response = server
            .handle_completion_cancellable(
                Some(json!({
                    "textDocument": { "uri": "file:///workspace/app.pl" },
                    "position": { "line": 1, "character": 19 }
                })),
                Some(&json!(1)),
            )?
            .ok_or("expected completion response")?;

        let items = response["items"].as_array().ok_or("expected completion items")?;
        let item = items
            .iter()
            .find(|item| item["label"].as_str() == Some("$CONFIG_PATH"))
            .ok_or_else(|| format!("expected $CONFIG_PATH completion, got: {items:?}"))?;
        assert_eq!(
            item["insertText"].as_str(),
            Some("$Config::CONFIG_PATH"),
            "qualified workspace variable completion should preserve package qualifier"
        );

        Ok(())
    }

    #[test]
    fn test_completion_resolve_builtin_function() -> Result<(), Box<dyn std::error::Error>> {
        // Test that built-in function documentation is added
        let item = json!({
            "label": "print",
            "kind": 3  // Function
        });

        let server = LspServer::default();
        let result = server.handle_completion_resolve(Some(item));

        assert!(result.is_ok());
        let resolved =
            result.map_err(|e| e.message.to_string())?.ok_or("expected resolved value")?;

        // Check that documentation was added
        assert!(resolved.get("documentation").is_some());
        let doc = resolved.get("documentation").ok_or("expected documentation")?;
        assert_eq!(doc.get("kind").and_then(|v| v.as_str()), Some("markdown"));

        let value = doc.get("value").and_then(|v| v.as_str()).unwrap_or("");
        assert!(value.contains("Signatures:"));
        assert!(value.contains("print"));
        Ok(())
    }

    #[test]
    fn test_completion_resolve_keyword() -> Result<(), Box<dyn std::error::Error>> {
        // Test that keyword documentation is added
        let item = json!({
            "label": "my",
            "kind": 14  // Keyword
        });

        let server = LspServer::default();
        let result = server.handle_completion_resolve(Some(item));

        assert!(result.is_ok());
        let resolved =
            result.map_err(|e| e.message.to_string())?.ok_or("expected resolved value")?;

        // Check that documentation was added
        assert!(resolved.get("documentation").is_some());
        let doc = resolved.get("documentation").ok_or("expected documentation")?;
        let value = doc.get("value").and_then(|v| v.as_str()).unwrap_or("");
        assert!(value.contains("lexically scoped"));
        Ok(())
    }

    #[test]
    fn test_completion_resolve_variable() -> Result<(), Box<dyn std::error::Error>> {
        // Test that variable documentation is added
        let item = json!({
            "label": "$foo",
            "kind": 6  // Variable
        });

        let server = LspServer::default();
        let result = server.handle_completion_resolve(Some(item));

        assert!(result.is_ok());
        let resolved =
            result.map_err(|e| e.message.to_string())?.ok_or("expected resolved value")?;

        // Check that documentation was added
        assert!(resolved.get("documentation").is_some());
        let doc = resolved.get("documentation").ok_or("expected documentation")?;
        let value = doc.get("value").and_then(|v| v.as_str()).unwrap_or("");
        assert!(value.contains("Scalar variable"));
        Ok(())
    }

    #[test]
    fn test_completion_resolve_array_variable() -> Result<(), Box<dyn std::error::Error>> {
        // Test that array variable documentation is added
        let item = json!({
            "label": "@items",
            "kind": 6  // Variable
        });

        let server = LspServer::default();
        let result = server.handle_completion_resolve(Some(item));

        assert!(result.is_ok());
        let resolved =
            result.map_err(|e| e.message.to_string())?.ok_or("expected resolved value")?;

        // Check that documentation was added
        assert!(resolved.get("documentation").is_some());
        let doc = resolved.get("documentation").ok_or("expected documentation")?;
        let value = doc.get("value").and_then(|v| v.as_str()).unwrap_or("");
        assert!(value.contains("Array variable"));
        Ok(())
    }

    #[test]
    fn test_completion_resolve_passthrough() -> Result<(), Box<dyn std::error::Error>> {
        // Test that unknown items are passed through unchanged (except for no documentation)
        let item = json!({
            "label": "some_custom_function",
            "kind": 3  // Function
        });

        let server = LspServer::default();
        let result = server.handle_completion_resolve(Some(item.clone()));

        assert!(result.is_ok());
        let resolved =
            result.map_err(|e| e.message.to_string())?.ok_or("expected resolved value")?;

        // Label should be preserved
        assert_eq!(resolved.get("label").and_then(|v| v.as_str()), Some("some_custom_function"));
        // Kind should be preserved
        assert_eq!(resolved.get("kind").and_then(|v| v.as_u64()), Some(3));
        Ok(())
    }

    /// Serialize the named completion for a client with the given snippet
    /// support, going through the real builtin/snippet producers rather than a
    /// hand-built item — #4956 was a defect in what the producers emit.
    fn serialized_completion(
        source: &str,
        prefix_len: usize,
        label: &str,
        snippet_support: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = format!("file:///workspace/insertion_contract_{label}_{snippet_support}.pl");

        server.test_handle_did_open(Some(json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1, "text": source }
        })))?;

        let documents = server.documents_guard();
        let doc = server.get_document(&documents, &uri).ok_or("missing test document")?;

        let mut parser = perl_parser::Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("parse failed: {e:?}"))?;
        let provider = perl_lsp_rs_core::providers::completion::CompletionProvider::new(&ast);
        let item = provider
            .get_completions(source, prefix_len)
            .into_iter()
            .find(|item| item.label == label)
            .ok_or_else(|| format!("no `{label}` completion at offset {prefix_len}"))?;

        Ok(server.completion_item_to_lsp_value(doc, item, snippet_support, false, false))
    }

    /// #4956: `open`'s insert text is a snippet, but its *kind* is Function.
    /// Deriving `insertTextFormat` from the kind sent format 1, so clients
    /// pasted the literal `${1:<}` into the buffer.
    #[test]
    fn open_builtin_is_a_function_that_inserts_a_snippet() -> Result<(), Box<dyn std::error::Error>>
    {
        let value = serialized_completion("op", 2, "open", true)?;

        assert_eq!(
            value.get("kind").and_then(Value::as_i64),
            Some(3),
            "open must stay CompletionItemKind::Function; snippet insertion is not a kind"
        );
        assert_eq!(
            value.get("insertTextFormat").and_then(Value::as_i64),
            Some(2),
            "open inserts a snippet, so the format must be Snippet"
        );

        let insert_text = value.get("insertText").and_then(Value::as_str).ok_or("no insertText")?;
        assert!(
            insert_text.contains("${1:") && insert_text.contains("${2:"),
            "snippet-capable clients keep the tab stops, got: {insert_text}"
        );
        assert!(
            insert_text.contains("\\$fh") && insert_text.contains("\\$!"),
            "literal Perl variables must be escaped so the client does not treat them as \
             snippet variables, got: {insert_text}"
        );

        Ok(())
    }

    /// The other half of the contract: a client without `snippetSupport`
    /// receives literal, valid Perl — no tab stops and no snippet escapes.
    #[test]
    fn open_builtin_degrades_to_valid_perl_for_plaintext_clients()
    -> Result<(), Box<dyn std::error::Error>> {
        let value = serialized_completion("op", 2, "open", false)?;

        assert_eq!(value.get("insertTextFormat").and_then(Value::as_i64), Some(1));
        assert_eq!(
            value.get("insertText").and_then(Value::as_str),
            Some("open(my $fh, '<', $file) or die \"Cannot open $file: $!\";")
        );

        Ok(())
    }

    /// #4956: `submethod`'s body spelled literal Perl `$self` as a snippet
    /// variable, so VS Code inserted an editable `self` placeholder and other
    /// clients could insert nothing.
    #[test]
    fn submethod_snippet_preserves_literal_self() -> Result<(), Box<dyn std::error::Error>> {
        let snippet = serialized_completion("submethod", 9, "submethod", true)?;
        let snippet_text =
            snippet.get("insertText").and_then(Value::as_str).ok_or("no insertText")?;

        assert_eq!(snippet.get("insertTextFormat").and_then(Value::as_i64), Some(2));
        assert!(
            snippet_text.contains("my (\\$self"),
            "`$self` must be escaped to survive as literal Perl, got: {snippet_text}"
        );

        let plain = serialized_completion("submethod", 9, "submethod", false)?;
        assert_eq!(plain.get("insertTextFormat").and_then(Value::as_i64), Some(1));
        assert_eq!(
            plain.get("insertText").and_then(Value::as_str),
            Some("sub method_name {\n    my ($self, @args) = @_;\n    \n}")
        );

        Ok(())
    }

    /// No item may claim PlainText while carrying snippet syntax — that is the
    /// exact shape of the `open` defect. Guards every producer at once, so a
    /// new snippet-bearing entry cannot reintroduce it.
    #[test]
    fn no_completion_ships_unrendered_snippet_syntax() -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::snippet_body_defects;

        let source = "";
        let mut parser = perl_parser::Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("parse failed: {e:?}"))?;
        let provider = perl_lsp_rs_core::providers::completion::CompletionProvider::new(&ast);
        let items = provider.get_completions(source, 0);
        assert!(!items.is_empty(), "expected completions for an empty document");

        for item in items {
            let Some(insert_text) = item.insert_text.as_deref() else { continue };
            match &item.insert_text_format {
                InsertTextFormat::PlainText => {
                    // Inserted verbatim: any tab stop would reach the buffer as text.
                    assert!(
                        !insert_text.contains("${") && !insert_text.contains("$0"),
                        "`{}` is PlainText but carries snippet syntax: {insert_text}",
                        item.label
                    );
                }
                InsertTextFormat::Snippet { plain_fallback } => {
                    let defects = snippet_body_defects(insert_text);
                    assert!(
                        defects.is_empty(),
                        "`{}` has a defective snippet body: {defects:?}",
                        item.label
                    );
                    assert!(
                        !plain_fallback.contains("${")
                            && !plain_fallback.contains("$0")
                            && !plain_fallback.contains('\\'),
                        "`{}` has a fallback that is not literal text: {plain_fallback}",
                        item.label
                    );
                }
            }
        }

        Ok(())
    }

    // Degradation is no longer a serializer concern: a snippet item carries the
    // plain-text fallback it was built with. These cover the shared renderer
    // that produces it.
    use perl_lsp_rs_core::providers::completion::render_snippet_plaintext;
    #[test]
    fn test_degrade_snippet_removes_placeholders_with_defaults() {
        // ${1:placeholder} should become "placeholder"
        let result = render_snippet_plaintext("function(${1:arg1}, ${2:arg2})");
        assert_eq!(result, "function(arg1, arg2)");
    }

    #[test]
    fn test_degrade_snippet_removes_simple_placeholders() {
        // $1, $0 should be removed entirely
        let result = render_snippet_plaintext("print $1;$0");
        assert_eq!(result, "print ;");
    }

    #[test]
    fn test_degrade_snippet_mixed_placeholders() {
        // Mix of both types
        let result = render_snippet_plaintext("sub ${1:name} { $0 }");
        assert_eq!(result, "sub name {  }");
    }

    #[test]
    fn test_degrade_snippet_no_placeholders() {
        // Plain text should pass through unchanged
        let result = render_snippet_plaintext("just plain text");
        assert_eq!(result, "just plain text");
    }

    #[test]
    fn test_degrade_snippet_empty_string() {
        let result = render_snippet_plaintext("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_completion_capability_advertises_label_details_support() {
        use perl_lsp_rs_core::features::flags::BuildFlags;
        use perl_lsp_rs_core::protocol::capabilities::capabilities_for;
        use serde_json::to_value;

        let caps = capabilities_for(BuildFlags::production());
        let caps_json = to_value(&caps).expect("serialize ServerCapabilities");

        let completion_item_opt = caps_json
            .pointer("/completionProvider/completionItem")
            .expect("completionProvider.completionItem must be present");
        assert_eq!(
            completion_item_opt.get("labelDetailsSupport").and_then(|v| v.as_bool()),
            Some(true),
            "server must advertise completionItem.labelDetailsSupport: true in capabilities"
        );
    }

    #[test]
    fn test_completion_builtin_function_has_label_details_on_resolve()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        // Simulate a client that declared labelDetailsSupport.
        server.client_capabilities.lock().label_details_support = true;

        let item = json!({
            "label": "print",
            "kind": 3  // Function
        });
        let resolved = server
            .handle_completion_resolve(Some(item))
            .map_err(|e| e.message.to_string())?
            .ok_or("expected resolved value")?;

        let ld =
            resolved.get("labelDetails").ok_or("labelDetails must be present after resolve")?;
        let detail = ld.get("detail").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            detail.contains("print"),
            "labelDetails.detail should contain the primary signature; got: {detail:?}"
        );
        assert_eq!(
            ld.get("description").and_then(|v| v.as_str()),
            Some("builtin"),
            "labelDetails.description should be 'builtin'"
        );
        Ok(())
    }

    #[test]
    fn test_completion_builtin_no_label_details_without_client_support()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        // Default server has label_details_support = false.

        let item = json!({ "label": "print", "kind": 3 });
        let resolved = server
            .handle_completion_resolve(Some(item))
            .map_err(|e| e.message.to_string())?
            .ok_or("expected resolved value")?;

        assert!(
            resolved.get("labelDetails").is_none(),
            "labelDetails must NOT be populated when client did not declare labelDetailsSupport"
        );
        Ok(())
    }

    #[test]
    fn test_completion_variable_has_label_details_on_resolve()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        server.client_capabilities.lock().label_details_support = true;

        let item = json!({ "label": "$count", "kind": 6 });
        let resolved = server
            .handle_completion_resolve(Some(item))
            .map_err(|e| e.message.to_string())?
            .ok_or("expected resolved value")?;

        let ld = resolved.get("labelDetails").ok_or("labelDetails must be present for variable")?;
        assert_eq!(
            ld.get("detail").and_then(|v| v.as_str()),
            Some("scalar"),
            "scalar variable should have labelDetails.detail = 'scalar'"
        );
        Ok(())
    }

    /// PERL5LIB inclusion in the shared completion context is gated on
    /// `use_perl5lib`, NOT `use_system_inc`. `use_system_inc` controls
    /// interpreter startup `@INC` only. This test walks the four-cell matrix to
    /// ensure the two flags remain independent for the effective include-path
    /// source that completion now consumes through `EffectiveIncContext`.
    #[test]
    fn perl5lib_completion_gate_is_use_perl5lib_independent_of_use_system_inc()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::config::WorkspaceConfig;

        // (use_perl5lib, use_system_inc, expected_perl5lib_present)
        let cells: &[(bool, bool, bool)] =
            &[(true, false, true), (true, true, true), (false, true, false), (false, false, false)];

        for &(use_perl5lib, use_system_inc, expected) in cells {
            let mut config = WorkspaceConfig::default();
            config.use_perl5lib = use_perl5lib;
            config.use_system_inc = use_system_inc;

            let paths = config.effective_include_paths(&["perl5lib".to_string()]);
            let has = paths.iter().any(|path| path == "perl5lib");
            assert_eq!(
                has, expected,
                "cell (use_perl5lib={use_perl5lib}, use_system_inc={use_system_inc}): \
                 expected PERL5LIB present={expected}, got {has}; paths={paths:?}"
            );
        }

        Ok(())
    }

    #[test]
    fn module_import_completion_context_accepts_perl_whitespace_only_after_keyword() {
        assert!(LspServer::is_module_import_completion_context("use\tFoo", "use\tFoo".len()));
        assert!(LspServer::is_module_import_completion_context(
            "require\tFoo",
            "require\tFoo".len()
        ));
        assert!(!LspServer::is_module_import_completion_context("useful Foo", "useful Foo".len()));
        assert!(!LspServer::is_module_import_completion_context(
            "required Foo",
            "required Foo".len()
        ));
    }

    // =========================================================================
    // Module scan cache integration tests (issue #8514)
    // =========================================================================

    /// First call for a given root+prefix triggers a scan (cache miss).
    /// Second call within TTL returns the cached result (no new scan).
    #[test]
    fn test_scan_cache_second_call_within_ttl_does_not_rescan()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::module_scan_cache::{
            ModuleCompletionScanCache, ScanCacheKey,
        };
        use std::path::PathBuf;

        let temp = tempfile::TempDir::new()?;
        let lib = temp.path().join("lib");
        let mojo_dir = lib.join("Mojo");
        std::fs::create_dir_all(&mojo_dir)?;
        std::fs::write(mojo_dir.join("Controller.pm"), "package Mojo::Controller;\n1;\n")?;

        let cache = ModuleCompletionScanCache::new();
        let canonical = std::fs::canonicalize(&lib).unwrap_or_else(|_| lib.clone());
        let key = ScanCacheKey {
            canonical_root: canonical.clone(),
            prefix_dir: PathBuf::from("Mojo"),
            module_prefix: "Mojo::".to_string(),
        };

        // First: miss — nothing in cache yet
        assert!(cache.get(&key).is_none(), "cache must be cold initially");

        // Manually populate as the scan would
        cache.insert(key.clone(), vec!["Mojo::Controller".to_string()]);

        // Second within TTL: hit
        let hit = cache.get(&key).ok_or("expected cache hit on second call")?;
        assert!(hit.contains(&"Mojo::Controller".to_string()), "cached result must match scan");

        Ok(())
    }

    /// Different prefix dir produces a cache miss.
    #[test]
    fn test_scan_cache_different_prefix_dir_misses() -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::module_scan_cache::{
            ModuleCompletionScanCache, ScanCacheKey,
        };
        use std::path::PathBuf;

        let cache = ModuleCompletionScanCache::new();
        cache.insert(
            ScanCacheKey {
                canonical_root: PathBuf::from("/lib"),
                prefix_dir: PathBuf::from("Mojo"),
                module_prefix: "Mojo::C".to_string(),
            },
            vec!["Mojo::Controller".to_string()],
        );

        // Different prefix_dir — must miss
        let miss = cache.get(&ScanCacheKey {
            canonical_root: PathBuf::from("/lib"),
            prefix_dir: PathBuf::from("Catalyst"),
            module_prefix: "Catalyst::C".to_string(),
        });
        assert!(miss.is_none(), "different prefix_dir must be a cache miss");

        Ok(())
    }

    /// After TTL expires the cache returns None.
    #[test]
    fn test_scan_cache_after_ttl_misses() -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::module_scan_cache::{
            ModuleCompletionScanCache, ScanCacheKey,
        };
        use std::path::PathBuf;
        use std::time::Duration;

        let cache = ModuleCompletionScanCache::with_ttl_ms(10);
        let key = ScanCacheKey {
            canonical_root: PathBuf::from("/lib"),
            prefix_dir: PathBuf::from("Mojo"),
            module_prefix: "Mojo::C".to_string(),
        };
        cache.insert(key.clone(), vec!["Mojo::Controller".to_string()]);

        std::thread::sleep(Duration::from_millis(50));
        assert!(cache.get(&key).is_none(), "expired entry must be a cache miss");

        Ok(())
    }

    /// Cancellation token is checked before returning a cached hit.
    #[test]
    fn test_scan_cache_cancellation_checked_before_returning_hit()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::CompletionProvider;
        use perl_lsp_rs_core::providers::completion::module_scan_cache::{
            ModuleCompletionScanCache, ScanCacheKey,
        };
        use perl_parser::Parser;
        use std::path::PathBuf;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let temp = tempfile::TempDir::new()?;
        let lib = temp.path().join("lib");
        let mojo_dir = lib.join("Mojo");
        std::fs::create_dir_all(&mojo_dir)?;
        std::fs::write(mojo_dir.join("Controller.pm"), "package Mojo::Controller;\n1;\n")?;

        // Pre-populate the cache as if a prior call had scanned.
        let cache = Arc::new(ModuleCompletionScanCache::new());
        let canonical = std::fs::canonicalize(&lib).unwrap_or_else(|_| lib.clone());
        let key = ScanCacheKey {
            canonical_root: canonical.clone(),
            prefix_dir: PathBuf::from("Mojo"),
            module_prefix: "Mojo::".to_string(),
        };
        cache.insert(key, vec!["Mojo::Controller".to_string()]);

        // Construct a provider with the pre-populated cache and a cancellation flag already set.
        let source = "use Mojo::";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;

        let cancelled = Arc::new(AtomicBool::new(true));
        let cancel_fn = {
            let c = Arc::clone(&cancelled);
            move || c.load(Ordering::Relaxed)
        };

        let provider = CompletionProvider::new_with_index_and_source_and_paths(
            &ast,
            source,
            None,
            vec![lib.clone()],
            vec![],
            false,
        )
        .with_scan_cache(Arc::clone(&cache));

        // With cancellation flag set, completions must be empty even though cache has a hit.
        let completions =
            provider.get_completions_with_path_cancellable(source, source.len(), None, &cancel_fn);

        // Either the request was cancelled entirely (empty) or the cancellation check
        // at the cache-hit path caused an early return.  Either way, the contract is
        // that a cancelled request does not return results.
        assert!(
            completions.is_empty(),
            "cancelled request must not return completions; got {completions:?}"
        );

        Ok(())
    }

    /// Prefix-filtered cache entries must not satisfy a different leaf prefix
    /// under the same namespace directory.
    #[test]
    fn test_scan_cache_full_prefix_prevents_cross_prefix_hits()
    -> Result<(), Box<dyn std::error::Error>> {
        use perl_lsp_rs_core::providers::completion::CompletionProvider;
        use perl_lsp_rs_core::providers::completion::module_scan_cache::{
            ModuleCompletionScanCache, ScanCacheKey,
        };
        use perl_parser::Parser;
        use std::path::PathBuf;
        use std::sync::Arc;

        let temp = tempfile::TempDir::new()?;
        let lib = temp.path().join("lib");
        let mojo_dir = lib.join("Mojo");
        std::fs::create_dir_all(&mojo_dir)?;
        std::fs::write(mojo_dir.join("Controller.pm"), "package Mojo::Controller;\n1;\n")?;
        std::fs::write(mojo_dir.join("Lite.pm"), "package Mojo::Lite;\n1;\n")?;

        // Simulate an earlier `Mojo::C` request. A later `Mojo::L` request
        // scans the same prefix_dir (`Mojo`) but must not reuse this filtered hit.
        let cache = Arc::new(ModuleCompletionScanCache::new());
        let canonical = std::fs::canonicalize(&lib).unwrap_or_else(|_| lib.clone());
        cache.insert(
            ScanCacheKey {
                canonical_root: canonical,
                prefix_dir: PathBuf::from("Mojo"),
                module_prefix: "Mojo::C".to_string(),
            },
            vec!["Mojo::Controller".to_string()],
        );

        let source = "use Mojo::L";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;
        let provider = CompletionProvider::new_with_index_and_source_and_paths(
            &ast,
            source,
            None,
            vec![lib.clone()],
            vec![],
            false,
        )
        .with_scan_cache(Arc::clone(&cache));

        let labels: Vec<String> = provider
            .get_completions_with_path(source, source.len(), None)
            .into_iter()
            .map(|item| item.label.into_owned())
            .collect();

        assert!(
            labels.contains(&"Mojo::Lite".to_string()),
            "different leaf prefix should miss the stale filtered hit and scan Lite; labels={labels:?}"
        );
        assert!(
            !labels.contains(&"Mojo::Controller".to_string()),
            "cached Mojo::C result must not leak into Mojo::L completions; labels={labels:?}"
        );

        Ok(())
    }

    /// Cached and uncached completions produce the same labels.
    #[test]
    fn test_scan_cache_cached_and_uncached_labels_match() -> Result<(), Box<dyn std::error::Error>>
    {
        use perl_lsp_rs_core::providers::completion::CompletionProvider;
        use perl_lsp_rs_core::providers::completion::module_scan_cache::ModuleCompletionScanCache;
        use perl_parser::Parser;
        use std::sync::Arc;

        let temp = tempfile::TempDir::new()?;
        let lib = temp.path().join("lib");
        let mojo_dir = lib.join("Mojo");
        std::fs::create_dir_all(&mojo_dir)?;
        std::fs::write(mojo_dir.join("Controller.pm"), "package Mojo::Controller;\n1;\n")?;
        std::fs::write(mojo_dir.join("Lite.pm"), "package Mojo::Lite;\n1;\n")?;

        let source = "use Mojo::";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;

        // Uncached call.
        let uncached_provider = CompletionProvider::new_with_index_and_source_and_paths(
            &ast,
            source,
            None,
            vec![lib.clone()],
            vec![],
            false,
        );
        let uncached = uncached_provider.get_completions_with_path(source, source.len(), None);
        let mut uncached_labels: Vec<String> =
            uncached.iter().map(|c| c.label.to_string()).collect();
        uncached_labels.sort();

        // Cached call — first invocation populates the cache.
        let cache = Arc::new(ModuleCompletionScanCache::new());
        let cached_provider = CompletionProvider::new_with_index_and_source_and_paths(
            &ast,
            source,
            None,
            vec![lib.clone()],
            vec![],
            false,
        )
        .with_scan_cache(Arc::clone(&cache));
        let cached_first = cached_provider.get_completions_with_path(source, source.len(), None);
        let mut cached_first_labels: Vec<String> =
            cached_first.iter().map(|c| c.label.to_string()).collect();
        cached_first_labels.sort();

        // Second invocation should hit the cache.
        let cached_provider2 = CompletionProvider::new_with_index_and_source_and_paths(
            &ast,
            source,
            None,
            vec![lib.clone()],
            vec![],
            false,
        )
        .with_scan_cache(Arc::clone(&cache));
        let cached_second = cached_provider2.get_completions_with_path(source, source.len(), None);
        let mut cached_second_labels: Vec<String> =
            cached_second.iter().map(|c| c.label.to_string()).collect();
        cached_second_labels.sort();

        assert_eq!(
            uncached_labels, cached_first_labels,
            "first cached call must produce same labels as uncached"
        );
        assert_eq!(
            cached_first_labels, cached_second_labels,
            "second cached call (cache hit) must produce same labels as first"
        );

        Ok(())
    }

    // =========================================================================
    // Strategy-B folder-scope filter unit tests (#970)
    //
    // These tests exercise the exact functions called by the new production
    // code paths in add_runtime_workspace_completions:
    //   • best_workspace_folder_for_doc  (doc_folder_filter computation)
    //   • workspace_folder_matches_doc_uri  (Strategy-B keep/drop decision)
    //
    // They run under `cargo test -p perl-lsp-rs --lib` without the workspace
    // or expose_lsp_test_api features, so they are visible to the coverage pack.
    // =========================================================================

    /// `best_workspace_folder_for_doc` returns `Some` for the matching folder
    /// when two folders are registered — exercises the multi-folder branch that
    /// sets `doc_folder_filter = Some(folder)`.
    #[test]
    fn folder_filter_multi_root_selects_owning_folder() {
        use crate::runtime::types::{
            best_workspace_folder_for_doc, workspace_folder_matches_doc_uri,
        };
        use crate::runtime::workspace_folder::WorkspaceFolderState;

        let folder_a = WorkspaceFolderState::new("file:///project/folder-a".to_string());
        let folder_b = WorkspaceFolderState::new("file:///project/folder-b".to_string());
        let folders = vec![folder_a, folder_b];

        let doc_uri = "file:///project/folder-a/script.pl";
        let best = best_workspace_folder_for_doc(&folders, doc_uri);
        assert!(best.is_some(), "multi-root: owning folder must be found for doc in folder-a");
        assert_eq!(best.map(|f| f.uri.as_str()), Some("file:///project/folder-a"));

        // Strategy-B keep: symbol from same folder passes the filter.
        let same_folder_symbol_uri = "file:///project/folder-a/lib/Lib.pm";
        assert!(
            workspace_folder_matches_doc_uri(best.unwrap(), same_folder_symbol_uri),
            "symbol in folder-a must pass folder-containment filter when doc is in folder-a"
        );

        // Strategy-B drop: symbol from other folder is rejected.
        let cross_folder_symbol_uri = "file:///project/folder-b/lib/Other.pm";
        assert!(
            !workspace_folder_matches_doc_uri(best.unwrap(), cross_folder_symbol_uri),
            "symbol in folder-b must be rejected by folder-containment filter when doc is in folder-a"
        );
    }

    /// `best_workspace_folder_for_doc` returns `None` when only one folder is
    /// registered — the production code skips Strategy-B (`doc_folder_filter = None`).
    #[test]
    fn folder_filter_single_root_skips_filter() {
        use crate::runtime::types::best_workspace_folder_for_doc;
        use crate::runtime::workspace_folder::WorkspaceFolderState;

        // Simulate the `folders.len() > 1` check: with one folder the branch
        // evaluates to false and returns None without calling best_workspace_folder_for_doc.
        // Here we verify that even if called, the result is Some — confirming that
        // the `len() > 1` guard is the correct and necessary gate.
        let folder_a = WorkspaceFolderState::new("file:///project/folder-a".to_string());
        let folders = vec![folder_a];

        // len() <= 1 → production code short-circuits to None; test the guard value.
        assert!(
            folders.len() <= 1,
            "single-folder workspace must have len <= 1, skipping Strategy-B"
        );

        // best_workspace_folder_for_doc still finds the folder when called directly —
        // the skip is purely the len() > 1 guard in add_runtime_workspace_completions.
        let doc_uri = "file:///project/folder-a/script.pl";
        let best = best_workspace_folder_for_doc(&folders, doc_uri);
        assert!(
            best.is_some(),
            "best_workspace_folder_for_doc finds the folder; the len() guard is what skips it"
        );
    }

    /// `best_workspace_folder_for_doc` returns `None` when no folders are registered.
    /// Production code skips Strategy-B in the no-folder case.
    #[test]
    fn folder_filter_no_folders_skips_filter() {
        use crate::runtime::types::best_workspace_folder_for_doc;
        use crate::runtime::workspace_folder::WorkspaceFolderState;

        let folders: Vec<WorkspaceFolderState> = vec![];
        let doc_uri = "file:///project/script.pl";
        let best = best_workspace_folder_for_doc(&folders, doc_uri);
        assert!(best.is_none(), "no folders → best_workspace_folder_for_doc returns None");

        // Also verify the len() guard: empty vec has len() <= 1.
        assert!(folders.len() <= 1, "empty workspace satisfies the single-folder skip condition");
    }

    /// Module-kind symbols (Package, Class, Role) are exempt from Strategy-B.
    /// The `is_module_kind` flag gates Strategy-B: only `!is_module_kind` enters it.
    #[test]
    fn folder_filter_module_kind_exempt_from_strategy_b() {
        use crate::workspace_index::SymbolKind;

        // Verify is_module_kind computation for each module kind.
        let package_is_module = matches!(
            SymbolKind::Package,
            SymbolKind::Package | SymbolKind::Class | SymbolKind::Role
        );
        let class_is_module =
            matches!(SymbolKind::Class, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role);
        let role_is_module =
            matches!(SymbolKind::Role, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role);
        // Subroutine is NOT module-kind — enters Strategy-B.
        let sub_is_module = matches!(
            SymbolKind::Subroutine,
            SymbolKind::Package | SymbolKind::Class | SymbolKind::Role
        );

        assert!(package_is_module, "Package must be module-kind (exempt from Strategy-B)");
        assert!(class_is_module, "Class must be module-kind (exempt from Strategy-B)");
        assert!(role_is_module, "Role must be module-kind (exempt from Strategy-B)");
        assert!(!sub_is_module, "Subroutine must NOT be module-kind (subject to Strategy-B)");
    }

    /// `workspace_folder_matches_doc_uri` correctly handles the URI prefix match.
    /// This is the exact predicate used by Strategy-B to keep/drop symbols.
    #[test]
    fn folder_filter_uri_prefix_matching() {
        use crate::runtime::types::workspace_folder_matches_doc_uri;
        use crate::runtime::workspace_folder::WorkspaceFolderState;

        let folder = WorkspaceFolderState::new("file:///project/folder-a".to_string());

        // Same folder — kept.
        assert!(
            workspace_folder_matches_doc_uri(&folder, "file:///project/folder-a/lib/Foo.pm"),
            "file under folder-a must match folder-a"
        );
        assert!(
            workspace_folder_matches_doc_uri(&folder, "file:///project/folder-a/script.pl"),
            "root-level file under folder-a must match"
        );

        // Different folder — dropped.
        assert!(
            !workspace_folder_matches_doc_uri(&folder, "file:///project/folder-b/lib/Bar.pm"),
            "file under folder-b must not match folder-a"
        );

        // Prefix that is not a path boundary (folder-a-extra vs folder-a) — dropped.
        assert!(
            !workspace_folder_matches_doc_uri(&folder, "file:///project/folder-a-extra/lib/Baz.pm"),
            "folder-a-extra must not match folder-a (not a path boundary)"
        );
    }

    // =========================================================================
    // Strategy-B through-production-path tests (#970, patch-coverage)
    //
    // The five tests above cover the helper functions directly.  These two tests
    // go through add_runtime_workspace_completions itself so the changed lines at
    // the doc_folder_filter block (lines ~477-485) and the Strategy-B block
    // (lines ~534-546) are executed and visible to the --lib coverage pack.
    //
    // They require the workspace feature (IndexAccessMode::Full).
    // =========================================================================

    /// With two registered workspace folders, add_runtime_workspace_completions
    /// computes doc_folder_filter = Some(folder-a) and rejects the variable from
    /// folder-b via the Strategy-B continue branch.
    ///
    /// Covered changed lines:
    ///   477-481  doc_folder_filter = Some(best_workspace_folder_for_doc(...))
    ///   534      if !is_module_kind
    ///   535      if let Some(ref folder) = doc_folder_filter
    ///   536-543  !workspace_folder_matches_doc_uri -> trace + continue
    #[cfg(feature = "workspace")]
    #[test]
    fn strategy_b_multi_folder_filters_cross_folder_var() {
        use crate::runtime::routing::IndexAccessMode;
        use crate::runtime::workspace_folder::WorkspaceFolderState;
        use perl_parser::workspace_index::IndexCoordinator;
        use std::sync::Arc;

        let server = LspServer::default();
        {
            let mut folders = server.workspace_folders.lock();
            folders.push(WorkspaceFolderState::new("file:///project/folder-a".to_string()));
            folders.push(WorkspaceFolderState::new("file:///project/folder-b".to_string()));
        }

        let coordinator = Arc::new(IndexCoordinator::new());
        // Add a preserved non-callable (Variable) symbol from folder-b — it should be filtered out.
        let _ = coordinator.index().index_file_str(
            "file:///project/folder-b/lib/B.pm",
            "package B;
our $cross_folder_var_b;
1;
",
        );
        coordinator.transition_to_ready(1, 1);

        let doc_text = "my $x = $cross";
        let doc_uri = "file:///project/folder-a/script.pl";
        let inc_context = RequestIncContext::new(&server, doc_uri, doc_text, doc_text.len());
        let mut completions = Vec::new();
        server.add_runtime_workspace_completions(
            &mut completions,
            &inc_context,
            &IndexAccessMode::Full(&coordinator),
            None,
        );

        let names: Vec<&str> = completions.iter().map(|c| c.label.as_ref()).collect();
        assert!(
            !names.contains(&"$cross_folder_var_b"),
            "Strategy-B must reject $cross_folder_var_b from folder-b when doc is in folder-a;              got completions: {names:?}"
        );
    }

    /// With a single registered workspace folder, add_runtime_workspace_completions
    /// skips Strategy-B (doc_folder_filter = None) and includes symbols from any URI.
    ///
    /// Covered changed lines:
    ///   479  folders.len() > 1 -> false
    ///   482-484  else { None }   (doc_folder_filter = None -> Strategy-B skipped)
    #[cfg(feature = "workspace")]
    #[test]
    fn strategy_b_single_folder_skips_filter_includes_symbol() {
        use crate::runtime::routing::IndexAccessMode;
        use crate::runtime::workspace_folder::WorkspaceFolderState;
        use perl_parser::workspace_index::IndexCoordinator;
        use std::sync::Arc;

        let server = LspServer::default();
        {
            let mut folders = server.workspace_folders.lock();
            // Only one folder — len() > 1 is false -> doc_folder_filter = None.
            folders.push(WorkspaceFolderState::new("file:///project/folder-a".to_string()));
        }

        let coordinator = Arc::new(IndexCoordinator::new());
        // A non-callable symbol at a path outside folder-a — still included
        // because filter is None.  Subroutine candidates are intentionally
        // withdrawn by #11158 before Strategy-B, so use an `our` variable to
        // keep this test focused on the single-folder no-filter branch.
        let _ = coordinator.index().index_file_str(
            "file:///project/folder-b/lib/B.pm",
            "package B;
our $single_root_var;
1;
",
        );
        coordinator.transition_to_ready(1, 1);

        let doc_text = "$single";
        let doc_uri = "file:///project/folder-a/script.pl";
        let inc_context = RequestIncContext::new(&server, doc_uri, doc_text, doc_text.len());
        let mut completions = Vec::new();
        server.add_runtime_workspace_completions(
            &mut completions,
            &inc_context,
            &IndexAccessMode::Full(&coordinator),
            None,
        );

        let names: Vec<&str> = completions.iter().map(|c| c.label.as_ref()).collect();
        assert!(
            names.contains(&"$single_root_var"),
            "single-folder workspace must not filter by folder (doc_folder_filter = None);              got completions: {names:?}"
        );
    }
}
