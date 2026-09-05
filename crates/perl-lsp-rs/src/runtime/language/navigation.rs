//! Navigation handlers for go-to-definition, declaration, and related features
//!
//! Handles textDocument/declaration, textDocument/definition, textDocument/typeDefinition,
//! and textDocument/implementation requests.

use super::super::{
    Arc, DocumentState, GLOBAL_CANCELLATION_REGISTRY, ImplementationProvider, JsonRpcError,
    JsonRpcId, LspServer, ParentMap, Parser, PerlLspCancellationToken, REQUEST_CANCELLED, Value,
    json,
};
use crate::cancellation::RequestCleanupGuard;
use crate::protocol::{req_position, req_uri};
use crate::util::{read_text_file_with_encoding, token_under_cursor};
use perl_lsp_rs_core::providers::ProviderDecisionFreshness;
use perl_parser_core::source_file::is_binary_content;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

// Only the test-fallbacks compatibility handler (`on_definition`) needs this
// helper; production definition dispatch is a transparent adapter (#5108).
#[cfg(any(test, feature = "test-fallbacks"))]
use super::super::location_from_path;

/// Serialize a slice of typed values to a JSON array (#4995).
fn to_json_array<T: serde::Serialize>(values: &[T]) -> Value {
    serde_json::to_value(values).unwrap_or(Value::Array(Vec::new()))
}

#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
use perl_lsp_rs_core::providers::navigation::definition_shadow::{
    DefinitionCutoverResult, goto_definition_live_exact_or_imported,
};
#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
use perl_workspace::semantic::queries::QueryContext;

#[cfg(feature = "workspace")]
use crate::runtime::readiness::IndexReadinessPolicy;
#[cfg(feature = "workspace")]
use crate::runtime::routing::{IndexAccessMode, route_index_access};

mod core_modules;
#[cfg(feature = "workspace")]
mod mojolicious_routes;
mod xs_bootstrap;

use self::core_modules::is_core_perl_module;
#[cfg(feature = "workspace")]
use self::mojolicious_routes::resolve_mojolicious_route_definition;
use self::xs_bootstrap::{extract_xs_bootstrap_target, xs_bootstrap_location};

// Ungated with `fqn_component_at_cursor`, which is reached from the rename and
// find-references refusal guards in every build (#14757).
static FQN_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static ARROW_METHOD_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static PACKAGE_ARROW_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static VAR_METHOD_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static SUPER_METHOD_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static GOTO_LABEL_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

#[cfg(feature = "workspace")]
static LABEL_DECLARATION_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

static QUOTED_FRAMEWORK_MODULE_RE: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

/// Test-only synchronization point for deterministic same-document TOCTOU
/// regression tests (#3613).
///
/// `handle_type_definition` and `handle_implementation` capture the request
/// document's ast/text under one lock acquisition, then later re-read all
/// open documents via `documents_text_snapshot()` for the fallback
/// cross-file scan. This hook -- fired (and consumed) exactly once, right
/// after the up-front capture and right before that later re-read -- lets a
/// test pause the handler mid-flight, apply a real edit to the same
/// document on another thread, then release the handler, so the assertion
/// proves the fallback used the captured (generation-N) text and not the
/// newer (generation-N+1) text a racing `didChange` produced. No sleeps: the
/// handler blocks on `resume.recv()` until the test explicitly signals it to
/// continue.
#[cfg(any(test, feature = "expose_lsp_test_api"))]
static NAVIGATION_SAME_DOC_FALLBACK_GAP: std::sync::Mutex<
    Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
> = std::sync::Mutex::new(None);

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn wait_at_same_doc_fallback_gap() {
    // A poisoned mutex (some earlier test panicked while holding the lock)
    // must not silently disable this synchronization point: `.lock().ok()`
    // would turn `Err` into `None` and the gate would just not fire, so a
    // later race test could false-pass without ever exercising the race it
    // claims to prove. Recover the guard instead -- the hook slot's own
    // invariants (armed at most once, consumed via `take()`) stay intact
    // even if a prior holder panicked.
    let hook = match NAVIGATION_SAME_DOC_FALLBACK_GAP.lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    if let Some((reached, resume)) = hook {
        let _ = reached.send(());
        let _ = resume.recv();
    }
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
#[inline]
fn wait_at_same_doc_fallback_gap() {}

fn lsp_location_count(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(obj)) if obj.contains_key("uri") || obj.contains_key("targetUri") => 1,
        _ => 0,
    }
}

/// Naive comment-only heuristic for the goto-definition and completion
/// guards (#5066/#5408/#5411): `true` when a `#` appears earlier on the
/// same line.
///
/// This is deliberately NOT the rename candidate classifier and is
/// deliberately not string-aware: a `#` inside a string literal still reads
/// as a comment to this guard, and that trade-off is pinned by the guard
/// regression tests. Rename's edit policy uses the generation-bound
/// `SourceRegionIndex` instead (#4964).
pub(crate) fn is_in_comment_naive(position: usize, source: &str) -> bool {
    let line_start =
        if position == 0 { 0 } else { source[..position].rfind('\n').map_or(0, |p| p + 1) };
    let line = &source[line_start..];

    if let Some(comment_pos) = line.find('#') {
        let comment_absolute = line_start + comment_pos;
        position >= comment_absolute
    } else {
        false
    }
}

#[derive(Debug)]
struct NavigationDecisionTraceContext {
    provider: &'static str,
    provider_action: &'static str,
    uri: String,
    line: u32,
    character: u32,
    include_declaration: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct TypeDefinitionFallbackTrace {
    decision: &'static str,
    reason: &'static str,
    blocker: &'static str,
    source_backed_state: &'static str,
    fact_source: &'static str,
    fallback: &'static str,
    dynamic_boundary: bool,
    request_version: Option<i32>,
    current_document_version: Option<i32>,
    trace_only_no_live_behavior_change: bool,
}

impl Default for TypeDefinitionFallbackTrace {
    fn default() -> Self {
        Self {
            decision: "fallback",
            reason: "missing_fact",
            blocker: "missing_fact",
            source_backed_state: "type_definition_not_proven",
            fact_source: "fallback",
            fallback: "no_result",
            dynamic_boundary: false,
            request_version: None,
            current_document_version: None,
            trace_only_no_live_behavior_change: true,
        }
    }
}

fn stale_type_definition_fallback_trace(
    request_version: i32,
    current_document_version: i32,
) -> TypeDefinitionFallbackTrace {
    TypeDefinitionFallbackTrace {
        decision: "blocked",
        reason: "stale_fact",
        blocker: "stale_fact",
        source_backed_state: "stale_type_definition_request",
        fact_source: "request_version",
        fallback: "refresh_workspace_facts",
        dynamic_boundary: false,
        request_version: Some(request_version),
        current_document_version: Some(current_document_version),
        trace_only_no_live_behavior_change: false,
    }
}

fn unsupported_type_definition_source_trace() -> TypeDefinitionFallbackTrace {
    TypeDefinitionFallbackTrace {
        decision: "blocked",
        reason: "unsupported",
        blocker: "unsupported_fact_class",
        source_backed_state: "unscannable_type_definition_source",
        fact_source: "fallback",
        fallback: "no_result",
        dynamic_boundary: false,
        request_version: None,
        current_document_version: None,
        trace_only_no_live_behavior_change: true,
    }
}

fn classify_type_definition_fallback_trace(
    source_text: &str,
    line: u32,
    character: u32,
) -> TypeDefinitionFallbackTrace {
    let Some(line_text) = usize::try_from(line).ok().and_then(|line| source_text.lines().nth(line))
    else {
        return TypeDefinitionFallbackTrace::default();
    };

    let character = usize::try_from(character).unwrap_or_default();
    let compact_before_cursor =
        line_text.chars().take(character).filter(|ch| !ch.is_whitespace()).collect::<String>();
    let compact_from_cursor = line_text
        .chars()
        .skip(character)
        .take(64)
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();

    if compact_from_cursor.starts_with("->$")
        || (compact_before_cursor.ends_with("->") && compact_from_cursor.starts_with('$'))
        || (compact_before_cursor.ends_with("isa=>") && compact_from_cursor.starts_with('$'))
        || (compact_before_cursor.ends_with("bless{},") && compact_from_cursor.starts_with('$'))
    {
        return TypeDefinitionFallbackTrace {
            decision: "fallback",
            reason: "dynamic_boundary",
            blocker: "dynamic_boundary",
            source_backed_state: "dynamic_type_definition_boundary",
            fact_source: "dynamic_boundary",
            fallback: "no_result",
            dynamic_boundary: true,
            request_version: None,
            current_document_version: None,
            trace_only_no_live_behavior_change: true,
        };
    }

    TypeDefinitionFallbackTrace::default()
}

fn classify_type_definition_fallback_trace_with_documents(
    source_text: &str,
    line: u32,
    character: u32,
    documents: &HashMap<String, String>,
) -> TypeDefinitionFallbackTrace {
    let fallback_trace = classify_type_definition_fallback_trace(source_text, line, character);
    if fallback_trace.blocker != "missing_fact" {
        return fallback_trace;
    }

    let Some(type_name) = type_definition_candidate_at_position(source_text, line, character)
    else {
        return fallback_trace;
    };
    if documents.values().any(|document_text| {
        !is_scannable_type_definition_source(document_text)
            && document_text.contains(&format!("package {type_name}"))
    }) {
        return unsupported_type_definition_source_trace();
    }

    fallback_trace
}

fn type_definition_candidate_at_position(
    source_text: &str,
    line: u32,
    character: u32,
) -> Option<String> {
    let line_text = usize::try_from(line).ok().and_then(|line| source_text.lines().nth(line))?;
    let character = usize::try_from(character).ok()?;
    let mut token_start = None;
    let mut token = String::new();

    for (index, ch) in line_text.chars().chain(std::iter::once(' ')).enumerate() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':') {
            if token_start.is_none() {
                token_start = Some(index);
            }
            token.push(ch);
            continue;
        }

        if let Some(start) = token_start.take() {
            let end = index;
            if character >= start
                && character <= end
                && token.contains("::")
                && token.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
            {
                return Some(token);
            }
        }
        token.clear();
    }

    None
}

fn is_scannable_type_definition_source(source_text: &str) -> bool {
    source_text.len() <= perl_lsp_rs_core::runtime::limits::max_file_size_bytes()
        && !is_binary_content(source_text)
}

/// Receipt-wire spelling of `ProviderDecisionFreshness`.
///
/// Bound to the enum's serde `snake_case` vocabulary (`fresh` | `stale` |
/// `unknown` | `not_applicable`) rather than the human-readable explanation
/// label, which spells `NotApplicable` as `"not applicable"`.
fn provider_decision_freshness_wire(freshness: ProviderDecisionFreshness) -> &'static str {
    match freshness {
        ProviderDecisionFreshness::Fresh => "fresh",
        ProviderDecisionFreshness::Stale => "stale",
        ProviderDecisionFreshness::Unknown => "unknown",
        ProviderDecisionFreshness::NotApplicable => "not_applicable",
        // The enum is non-exhaustive. A future variant is evidence we do not yet
        // know how to name, so the receipt fails closed rather than claiming
        // freshness or inventing a private spelling.
        _ => "unknown",
    }
}

/// Freshness of a goto-definition receipt, derived from what the handler
/// actually answered from (#14162).
///
/// Locations are only returned from live open-document facts or from a
/// workspace-index lookup that already passed the staleness gate, so a
/// non-empty answer is current for the request. An empty answer over a stale
/// workspace index cannot vouch that the workspace was searched and reports
/// `unknown`. An empty answer when the index is current (or the workspace
/// feature is off) is a negative over current sources.
fn goto_definition_receipt_freshness(
    result_count: usize,
    workspace_index_stale: bool,
) -> ProviderDecisionFreshness {
    if result_count == 0 && workspace_index_stale {
        ProviderDecisionFreshness::Unknown
    } else {
        ProviderDecisionFreshness::Fresh
    }
}

/// Freshness of a type-definition receipt, derived from the fact source the
/// handler actually answered from (#14162).
///
/// `request_version` is only recorded when the request is behind the live
/// document, so that source is `stale`. Open-document parser facts, a dynamic
/// boundary classified from the current buffer, and a fallback scan of current
/// open documents are current for the request. Any other source fails closed
/// to `unknown`.
fn type_definition_receipt_freshness(fact_source: &'static str) -> ProviderDecisionFreshness {
    match fact_source {
        "request_version" => ProviderDecisionFreshness::Stale,
        "parser_syntax" | "dynamic_boundary" | "fallback" => ProviderDecisionFreshness::Fresh,
        _ => ProviderDecisionFreshness::Unknown,
    }
}

pub(super) fn get_fqn_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    FQN_RE
        .get_or_init(|| regex::Regex::new(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)"))
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize fully-qualified symbol regex: {err}"
            ))
        })
}

#[cfg(feature = "workspace")]
fn get_arrow_method_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    ARROW_METHOD_RE
        .get_or_init(|| {
            regex::Regex::new(
                r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)\s*->\s*([A-Za-z_][A-Za-z0-9_]*)",
            )
        })
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize method-call regex: {err}"
            ))
        })
}

#[cfg(feature = "workspace")]
fn get_package_arrow_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    PACKAGE_ARROW_RE
        .get_or_init(|| {
            regex::Regex::new(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)\s*->")
        })
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize package navigation regex: {err}"
            ))
        })
}

/// Get regex for matching `$var->method` patterns (variable-based method calls).
///
/// Captures: group 1 = variable name (without sigil), group 2 = method name.
#[cfg(feature = "workspace")]
fn get_var_method_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    VAR_METHOD_RE
        .get_or_init(|| {
            regex::Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)\s*->\s*([A-Za-z_][A-Za-z0-9_]*)")
        })
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize variable method-call regex: {err}"
            ))
        })
}

#[cfg(feature = "workspace")]
fn get_super_method_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    SUPER_METHOD_RE
        .get_or_init(|| regex::Regex::new(r"\bSUPER::([A-Za-z_][A-Za-z0-9_]*)"))
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize SUPER method-call regex: {err}"
            ))
        })
}

#[cfg(feature = "workspace")]
fn get_goto_label_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    GOTO_LABEL_RE
        .get_or_init(|| regex::Regex::new(r"\bgoto\s+([A-Za-z_][A-Za-z0-9_]*)"))
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize goto label regex: {err}"
            ))
        })
}

#[cfg(feature = "workspace")]
fn get_label_declaration_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    LABEL_DECLARATION_RE
        .get_or_init(|| regex::Regex::new(r"(?m)^\s*([A-Za-z_][A-Za-z0-9_]*)\s*:"))
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize label declaration regex: {err}"
            ))
        })
}

fn get_quoted_framework_module_regex() -> Result<&'static regex::Regex, JsonRpcError> {
    QUOTED_FRAMEWORK_MODULE_RE
        .get_or_init(|| {
            regex::Regex::new(
                r#"\b(with|extends|enable)\s+(?:'([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)'|"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)")"#,
            )
        })
        .as_ref()
        .map_err(|err| {
            crate::protocol::internal_error(&format!(
                "Failed to initialize framework module regex: {err}"
            ))
        })
}

fn quoted_framework_module_at_cursor(
    text: &str,
    cursor: usize,
) -> Result<Option<FrameworkModuleReference>, JsonRpcError> {
    for cap in get_quoted_framework_module_regex()?.captures_iter(text) {
        let Some(keyword) = cap.get(1) else {
            continue;
        };
        let Some(module_match) = cap.get(2).or_else(|| cap.get(3)) else {
            continue;
        };
        if cursor < module_match.start() || cursor > module_match.end() {
            continue;
        }

        return Ok(Some(normalize_framework_module_reference(
            keyword.as_str(),
            module_match.as_str(),
        )));
    }

    Ok(None)
}

fn normalize_framework_module_reference(
    keyword: &str,
    module_name: &str,
) -> FrameworkModuleReference {
    let module_name = if keyword == "enable" && !module_name.contains("::") {
        format!("Plack::Middleware::{module_name}")
    } else {
        module_name.to_string()
    };
    let kind = if keyword == "enable" && module_name.starts_with("Plack::Middleware::") {
        FrameworkModuleKind::PlackMiddleware
    } else {
        FrameworkModuleKind::Package
    };

    FrameworkModuleReference { module_name, kind }
}

/// Extract a module name from a literal-path `require "Foo/Bar.pm"` statement
/// whose quoted path literal spans `offset` (#12559).
///
/// The bareword `use`/`require` extraction chain
/// (`extract_module_reference_extended` → `parse_module_token`) requires an
/// identifier-start byte, so the leading quote of a file-path require never
/// yields a module target and the @INC-aware resolver is unreachable for this
/// form. This helper closes that gap by reusing the in-repo normalization rule
/// — [`perl_module::ModuleImportHead::token_as_module_name`], the same rule
/// document links and completion consume — instead of forking another
/// path-to-module spelling.
///
/// Boundaries (mirroring hover's `find_require_module_at_offset` discipline):
/// - the full logical line containing `offset` is parsed with
///   `parse_module_import_head`; only `RequireForm::FilePath` statements
///   qualify;
/// - the cursor must sit on or inside the quoted literal (inclusive of both
///   quote bytes), matching the bareword form's inclusive token-span rule;
/// - non-`.pm` paths (e.g. `require "script.pl"`) and dynamic forms
///   (`require $var`) never resolve here, so they keep their documented
///   non-resolution behavior.
fn literal_require_path_module_at_offset(text: &str, offset: usize) -> Option<String> {
    let mut cursor = offset.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }

    let line_start = text[..cursor].rfind('\n').map_or(0, |idx| idx + 1);
    let line_end = text[cursor..].find('\n').map_or(text.len(), |idx| cursor + idx);
    let line = &text[line_start..line_end];
    let cursor_in_line = cursor.saturating_sub(line_start);

    let head = perl_module::parse_module_import_head(line)?;
    if head.kind != perl_module::ModuleImportKind::Require
        || head.require_form() != Some(perl_module::RequireForm::FilePath)
    {
        return None;
    }

    // `token_start`/`token_end` span the unquoted path content, so the opening
    // quote sits one byte before and the closing quote one byte at `token_end`.
    // Accept the cursor from the opening quote through just past the closing
    // quote (the bareword arm likewise accepts one past its token end).
    let open_quote = head.token_start.saturating_sub(1);
    let close_quote = head.token_end;
    if cursor_in_line < open_quote || cursor_in_line > close_quote + 1 {
        return None;
    }

    let module_name = head.token_as_module_name();
    // `token_as_module_name` leaves non-`.pm` file paths (and therefore
    // non-module tokens) unchanged; those are not module names and must not
    // enter module resolution. Guard with `is_lookup_safe_module_name` as
    // well: quoted paths are the only way this arm can receive dot/traversal-
    // shaped text (e.g. `require "../../x.pm"`), and unsafe values must not
    // reach the resolver's filesystem existence checks (external @INC roots
    // join the mapped relative path without traversal validation).
    if module_name == head.token || !perl_module::is_lookup_safe_module_name(&module_name) {
        return None;
    }

    Some(module_name)
}

#[derive(Debug, Clone, Copy)]
enum FrameworkModuleKind {
    Package,
    PlackMiddleware,
}

#[derive(Debug, Clone)]
struct FrameworkModuleReference {
    module_name: String,
    kind: FrameworkModuleKind,
}

#[cfg(feature = "workspace")]
impl FrameworkModuleReference {
    fn definition_location(
        &self,
        workspace_index: &crate::workspace_index::WorkspaceIndex,
    ) -> Option<crate::workspace_index::Location> {
        match self.kind {
            FrameworkModuleKind::Package => {
                find_package_definition_location(workspace_index, &self.module_name)
            }
            FrameworkModuleKind::PlackMiddleware => {
                find_plack_middleware_definition_location(workspace_index, &self.module_name)
            }
        }
    }
}

#[cfg(feature = "workspace")]
fn find_label_declaration_span(
    text: &str,
    label: &str,
) -> Result<Option<(usize, usize)>, JsonRpcError> {
    let label_re = get_label_declaration_regex()?;
    Ok(label_re.captures_iter(text).find_map(|cap| {
        let declared_label = cap.get(1)?;
        (declared_label.as_str() == label).then_some((declared_label.start(), declared_label.end()))
    }))
}

#[derive(Debug, Clone)]
enum EarlyDefinitionTarget {
    /// Cursor is on a `use Module` / `require Module` statement, or on the
    /// quoted path literal of `require "Path/To/Module.pm"` (#12559).
    /// @INC filtering applies: if file-system resolution fails, the workspace
    /// index must also be filtered through `EffectiveIncContext`.
    UseModule(String),
    /// Cursor is on a bare `Package->method` reference.
    /// @INC filtering does not apply — workspace-index method lookup is correct.
    Module(String),
    /// Cursor is on a quoted framework package reference, such as Moo/Moose
    /// `with`/`extends` or Plack Builder `enable`.
    FrameworkModule(FrameworkModuleReference),
    XsBootstrap(String),
}

/// Look up a symbol definition in the workspace index.
///
/// Tries two lookup strategies:
/// 1. `find_def()` with a structured `SymbolKey`
/// 2. `find_definition()` with a formatted `Package::name` string
///
/// Returns the LSP location if found, or `None` to fall through to same-file resolution.
#[cfg(feature = "workspace")]
fn find_workspace_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    pkg: &str,
    name: &str,
) -> Option<crate::workspace_index::Location> {
    let key = crate::workspace_index::SymbolKey {
        pkg: pkg.to_string().into(),
        name: name.to_string().into(),
        sigil: None,
        kind: crate::workspace_index::SymKind::Sub,
    };

    workspace_index
        .find_def(&key)
        .or_else(|| workspace_index.find_definition(&format!("{pkg}::{name}")))
}

#[cfg(feature = "workspace")]
fn autoload_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    receiver_pkg: &str,
    include_receiver: bool,
) -> Option<crate::workspace_index::Location> {
    include_receiver
        .then(|| find_workspace_definition_location(workspace_index, receiver_pkg, "AUTOLOAD"))
        .flatten()
        .or_else(|| inherited_method_definition_location(workspace_index, receiver_pkg, "AUTOLOAD"))
}

#[cfg(feature = "workspace")]
fn find_plack_middleware_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    module_name: &str,
) -> Option<crate::workspace_index::Location> {
    let expected_suffix =
        std::path::PathBuf::from(format!("{}.pm", module_name.replace("::", "/")));

    for symbol in workspace_index.all_symbols() {
        if symbol.kind != crate::workspace_index::SymbolKind::Package {
            continue;
        }

        let matches_name =
            symbol.name == module_name || symbol.qualified_name.as_deref() == Some(module_name);
        if !matches_name {
            continue;
        }

        if let Some(fs_path) = crate::workspace_index::uri_to_fs_path(&symbol.uri)
            && fs_path.ends_with(&expected_suffix)
        {
            return Some(crate::workspace_index::Location { uri: symbol.uri, range: symbol.range });
        }
    }

    None
}

#[cfg(feature = "workspace")]
fn find_package_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    module_name: &str,
) -> Option<crate::workspace_index::Location> {
    workspace_index
        .all_symbols()
        .into_iter()
        .find(|symbol| {
            symbol.kind == crate::workspace_index::SymbolKind::Package
                && (symbol.name == module_name
                    || symbol.qualified_name.as_deref() == Some(module_name))
        })
        .map(|symbol| crate::workspace_index::Location { uri: symbol.uri, range: symbol.range })
}

#[cfg(feature = "workspace")]
pub(super) fn workspace_document_text(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    uri: &str,
) -> Option<String> {
    workspace_index.document_store().get_text(uri).or_else(|| {
        crate::workspace_index::uri_to_fs_path(uri)
            .and_then(|path| read_text_file_with_encoding(&path).ok())
    })
}

#[cfg(feature = "workspace")]
fn inherited_method_definition_location(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    receiver_pkg: &str,
    method_name: &str,
) -> Option<crate::workspace_index::Location> {
    let mut visited = HashSet::from([receiver_pkg.to_string()]);
    let mut queue = VecDeque::new();
    let mut related_package_cache: HashMap<String, Vec<String>> = HashMap::new();

    let mut enqueue_related_packages =
        |package_name: &str, queue: &mut VecDeque<String>, visited: &HashSet<String>| {
            let related_packages = related_package_cache
                .entry(package_name.to_string())
                .or_insert_with(|| {
                    let Some(package_location) = workspace_index.find_definition(package_name)
                    else {
                        return Vec::new();
                    };
                    let Some(text) =
                        workspace_document_text(workspace_index, &package_location.uri)
                    else {
                        return Vec::new();
                    };

                    let mut parser = Parser::new(&text);
                    let Ok(ast) = parser.parse() else {
                        return Vec::new();
                    };

                    crate::semantic::SemanticAnalyzer::analyze_with_source(&ast, &text)
                        .class_models
                        .into_iter()
                        .find(|model| model.name == package_name)
                        .map(|model| {
                            // Include both parent classes and composed roles in the BFS
                            // so that `with 'Role'` methods are resolved alongside
                            // `extends`/`use parent` methods.
                            // NOTE: BFS visited-set (above) handles diamond and circular inheritance.
                            // NOTE: C3 MRO ordering is a pre-existing approximation; BFS does not
                            // honour strict C3 order. Filed as follow-up (see issue #3482).
                            model
                                .parents
                                .iter()
                                .chain(model.roles.iter())
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .clone();

            for related_package in related_packages {
                if !visited.contains(&related_package) {
                    queue.push_back(related_package);
                }
            }
        };

    enqueue_related_packages(receiver_pkg, &mut queue, &visited);

    while let Some(package_name) = queue.pop_front() {
        if !visited.insert(package_name.clone()) {
            continue;
        }

        if let Some(location) =
            find_workspace_definition_location(workspace_index, &package_name, method_name)
        {
            tracing::debug!(
                receiver_pkg,
                package_name,
                method_name,
                "resolved inherited/role method definition"
            );
            return Some(location);
        }

        enqueue_related_packages(&package_name, &mut queue, &visited);
    }

    None
}

#[cfg(feature = "workspace")]
fn find_symbol_key_definition_locations(
    workspace_index: &crate::workspace_index::WorkspaceIndex,
    symbol_key: &crate::workspace_index::SymbolKey,
) -> Vec<crate::workspace_index::Location> {
    if symbol_key.kind == crate::workspace_index::SymKind::Pack
        && symbol_key.pkg.starts_with("Plack::Middleware::")
        && let Some(location) =
            find_plack_middleware_definition_location(workspace_index, symbol_key.pkg.as_ref())
    {
        return vec![location];
    }

    if symbol_key.kind == crate::workspace_index::SymKind::Sub && symbol_key.sigil.is_none() {
        // For subroutines, try workspace definitions (may include multiple across packages),
        // then fall back to inherited method resolution (single location).
        let direct = workspace_index.find_defs(symbol_key);
        if !direct.is_empty() {
            return direct;
        }
        inherited_method_definition_location(workspace_index, &symbol_key.pkg, &symbol_key.name)
            .into_iter()
            .collect()
    } else {
        workspace_index.find_defs(symbol_key)
    }
}

#[cfg(feature = "workspace")]
fn lookup_workspace_definition(
    coordinator: Option<&std::sync::Arc<crate::workspace_index::IndexCoordinator>>,
    pkg: &str,
    name: &str,
    doc_uri: Option<&str>,
) -> Option<Value> {
    let coord = coordinator?;

    let workspace_index = coord.index();

    // Search for symbols with folder-aware ranking if we have document context
    let ranked_symbols = if let Some(uri) = doc_uri {
        workspace_index.search_symbols_ranked(name, uri)
    } else {
        workspace_index.search_symbols(name)
    };

    // Find the first matching symbol that matches the package.
    //
    // The qualified-name comparison must be anchored on the `::` package
    // separator. A boundary-less `q.starts_with(pkg)` matches any package
    // whose qualified name merely has `pkg` as a *string* prefix — e.g. with
    // pkg="Foo" it matches "FooBar::new", silently navigating `Foo->new` to
    // the unrelated `FooBar` package. Perl method resolution walks `@ISA`,
    // never a string-prefix of package names (perlobj); `Foo` and `FooBar`
    // are unrelated packages, so such a jump is definitively wrong. Anchor on
    // the exact `pkg::name` symbol or the `pkg::` package boundary instead.
    //
    // The `pkg::` boundary alone is still not sufficient: `q.starts_with(pkg::)`
    // also matches a symbol in a *nested subpackage*, e.g. pkg="Foo" matches
    // "Foo::Bar::new". `Foo::Bar` is a distinct, unrelated package from `Foo`
    // (Perl namespace nesting is purely lexical/cosmetic — it implies no
    // `@ISA` relationship), so `Foo->new` must not resolve there either.
    // Require the remainder after the `pkg::` prefix to contain no further
    // `::`, i.e. the symbol's container is exactly `pkg`, not a subpackage.
    let qualified_exact = format!("{pkg}::{name}");
    let package_prefix = format!("{pkg}::");
    for symbol in ranked_symbols {
        // Check if this symbol matches our package
        if (symbol.container_name.as_deref() == Some(pkg)
            || symbol
                .qualified_name
                .as_ref()
                .map(|q| {
                    *q == qualified_exact
                        || q.strip_prefix(package_prefix.as_str())
                            .is_some_and(|rest| !rest.contains("::"))
                })
                .unwrap_or(false))
            && let Some(lsp_location) = crate::workspace_index::lsp_adapter::to_lsp_location(
                &crate::workspace_index::Location { uri: symbol.uri.clone(), range: symbol.range },
            )
        {
            return Some(json!([lsp_location]));
        }
    }

    // Fallback to original lookup methods for backward compatibility
    if let Some(def_location) = find_workspace_definition_location(workspace_index, pkg, name)
        .or_else(|| inherited_method_definition_location(workspace_index, pkg, name))
        .or_else(|| {
            if is_universal_method(name) {
                find_workspace_definition_location(workspace_index, "UNIVERSAL", name)
            } else {
                None
            }
        })
        && let Some(lsp_location) =
            crate::workspace_index::lsp_adapter::to_lsp_location(&def_location)
    {
        return Some(json!([lsp_location]));
    }

    None
}

/// Real subs in `package UNIVERSAL` per perldoc.perl.org/UNIVERSAL: `isa`,
/// `can`, `DOES`, `VERSION`. Goto-definition may fall back to
/// `UNIVERSAL::<name>` for these because that symbol genuinely exists.
///
/// `DESTROY` (garbage-collection destructor hook) and `AUTOLOAD` (failed
/// method-lookup hook) are deliberately excluded: per perlobj, they are
/// interpreter special-method hooks, not subs shipped in `UNIVERSAL`. There
/// is no `UNIVERSAL::DESTROY` or `UNIVERSAL::AUTOLOAD` to navigate to, so
/// they must never drive the `UNIVERSAL::<name>` goto-definition fallback
/// below. They are still recognized for completion (see
/// `perl-lsp-rs-core/.../completion/methods.rs`) and for hover when an
/// actual `AUTOLOAD`/`DESTROY` sub is found via real inheritance lookup
/// (see `autoload_definition_location`, `hover.rs`).
const UNIVERSAL_METHODS: [&str; 4] = ["can", "isa", "DOES", "VERSION"];

fn is_universal_method(name: &str) -> bool {
    UNIVERSAL_METHODS.contains(&name)
}

#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
fn semantic_definition_symbol(key: &crate::workspace_index::SymbolKey) -> String {
    if key.kind == crate::workspace_index::SymKind::Pack
        || key.pkg.is_empty()
        || key.name.contains("::")
    {
        key.name.to_string()
    } else {
        format!("{}::{}", key.pkg, key.name)
    }
}

#[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
fn semantic_definition_query_symbol(
    key: &crate::workspace_index::SymbolKey,
    current_package: &str,
    import_source: Option<&str>,
) -> String {
    let is_import_resolved_bare_sub = key.kind == crate::workspace_index::SymKind::Sub
        && key.sigil.is_none()
        && !key.name.contains("::")
        && key.pkg.as_ref() != current_package
        && import_source.is_some_and(|source| source == key.pkg.as_ref());

    if is_import_resolved_bare_sub { key.name.to_string() } else { semantic_definition_symbol(key) }
}

#[cfg(feature = "workspace")]
fn cursor_in_regex_capture(regex: &regex::Regex, text: &str, cursor: usize, group: usize) -> bool {
    regex
        .captures_iter(text)
        .any(|cap| cap.get(group).is_some_and(|m| cursor >= m.start() && cursor <= m.end()))
}

/// Which `::`-separated component of a fully-qualified name the cursor is on.
///
/// Shared with `references.rs` so go-to-definition and find-references answer the
/// same question with one implementation instead of two drifting copies (#1849).
///
/// Not `#[cfg(feature = "workspace")]`: the classification is text-level -- one
/// regex over one line, then a `::` split -- and consults no workspace index.
/// `rename.rs` and `references.rs` both refuse wrong-symbol edits on it, and
/// those refusals must not disappear from a build that merely lacks the index
/// (#14757).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum FqnCursorComponent {
    /// The cursor is on a package component or on a `::` separator -- not on the
    /// final component, so the match does not name the sub the caller is after.
    Prefix,
    /// The cursor is on the final component, which names the sub.
    Final { package: String, name: String },
}

/// Resolve which component of the fully-qualified name under `cursor` the cursor
/// is on, or `None` when the cursor is not inside a `::`-qualified match.
///
/// `text` must contain the *complete* qualified name around `cursor`. The final
/// component is identified by the last `::` in the match, so a `text` that clips
/// the name partway through a component makes that component look final and
/// reports `Final` where the truth is `Prefix`. Pass a whole line
/// (`util::line_window_around_offset`) rather than a fixed-radius window: a Perl
/// qualified name cannot span a line break, but it can easily be longer than a
/// radius.
pub(super) fn fqn_component_at_cursor(
    regex: &regex::Regex,
    text: &str,
    cursor: usize,
) -> Option<FqnCursorComponent> {
    regex.captures_iter(text).find_map(|cap| {
        let matched = cap.get(1)?;
        if cursor < matched.start() || cursor > matched.end() {
            return None;
        }

        let value = matched.as_str();
        let parts: Vec<&str> = value.split("::").collect();
        if parts.len() < 2 {
            return None;
        }

        let cursor_relative = cursor.saturating_sub(matched.start());
        let final_component_start = value.rfind("::").map_or(0, |offset| offset + 2);
        if cursor_relative < final_component_start {
            Some(FqnCursorComponent::Prefix)
        } else {
            let name = parts.last().copied().unwrap_or_default().to_string();
            let package = parts[..parts.len() - 1].join("::");
            Some(FqnCursorComponent::Final { package, name })
        }
    })
}

/// Whether the cursor at `offset` sits *off* the token that names `symbol_name`.
///
/// Rename and find-references both need this before acting on a resolved symbol:
/// for a qualified name the resolver answers with the callable wherever the
/// cursor is, so a cursor that is not on the callable's own token would edit --
/// or report references for -- a symbol the user never pointed at (#9827,
/// #1849). Both providers asked it with their own copy until #14757; this is the
/// single implementation they now share.
///
/// Two cursor positions are off the named symbol:
///
/// * a **prefix** component, which never names the callable -- `Alpha` in
///   `Alpha::target()` resolves to the sub `target`;
/// * a **final** component whose text disagrees with the resolved symbol. In
///   `Some::Module->new()` the qualified-name match stops at the `->`, so
///   `Module` is that match's final component while the key names the method
///   `new`. Testing only for `Prefix` lets that receiver through.
///
/// Everything else is on the symbol, or not a question this predicate answers:
/// a final component that agrees, and a cursor outside any `::`-qualified match,
/// are both `false`. `symbol_name` of `None` is `false` for a final component
/// too -- with nothing to disagree with there is no disagreement to report.
///
/// Inherits `fqn_component_at_cursor`'s ASCII-only bound (#14616); fixing that
/// now touches one place instead of three.
pub(super) fn cursor_is_off_named_symbol(
    text: &str,
    offset: usize,
    symbol_name: Option<&str>,
) -> bool {
    let Ok(regex) = get_fqn_regex() else {
        return false;
    };
    // Classify over the whole line, not a radius window: a window can end inside
    // a long middle component, which makes that component look like the final
    // one and lets the wrong target through. A Perl qualified name cannot span a
    // line break, so the line always contains the whole name.
    let (line_start, line_text) = crate::util::line_window_around_offset(text, offset);
    let cursor_in_line = offset.saturating_sub(line_start);
    match fqn_component_at_cursor(regex, line_text, cursor_in_line) {
        Some(FqnCursorComponent::Prefix) => true,
        Some(FqnCursorComponent::Final { name, .. }) => {
            symbol_name.is_some_and(|symbol_name| name.as_str() != symbol_name)
        }
        None => false,
    }
}

impl LspServer {
    fn navigation_decision_trace_context(
        params: Option<&Value>,
        provider: &'static str,
        provider_action: &'static str,
        include_declaration: Option<bool>,
    ) -> Result<Option<NavigationDecisionTraceContext>, JsonRpcError> {
        let Some(params) = params else {
            return Ok(None);
        };
        let uri = req_uri(params)?.to_string();
        let (line, character) = req_position(params)?;
        Ok(Some(NavigationDecisionTraceContext {
            provider,
            provider_action,
            uri,
            line,
            character,
            include_declaration,
        }))
    }

    fn record_navigation_provider_decision_trace(
        &self,
        context: Option<&NavigationDecisionTraceContext>,
        result: Option<&Value>,
        semantic_shadow_receipt: Option<Value>,
    ) {
        let Some(context) = context else {
            return;
        };
        let result_count = lsp_location_count(result);
        #[cfg(feature = "workspace")]
        let workspace_index_stale = self.workspace_index_stale_for_any_open_document();
        #[cfg(not(feature = "workspace"))]
        let workspace_index_stale = false;
        let freshness = provider_decision_freshness_wire(goto_definition_receipt_freshness(
            result_count,
            workspace_index_stale,
        ));
        let (decision, reason, fallback_state) = if result_count == 0 {
            ("fallback", "no_result", "no_result")
        } else {
            ("acted", "live_provider_result", "live_provider")
        };

        let mut receipt = json!({
                "provider": context.provider,
                "provider_action": context.provider_action,
                "decision": decision,
                "reason": reason,
                "uri": context.uri,
                "line": context.line,
                "character": context.character,
                "include_declaration": context.include_declaration,
                "result_count": result_count,
                "fact_source": "navigation_provider",
                "confidence": "low",
                "freshness": freshness,
                "source_backed": false,
                "source_backed_state": "not_proven_by_provider_trace",
                "fallback_state": fallback_state,
                "dynamic_boundary": false,
                "trace_only_no_live_behavior_change": true,
                "claim_boundary": "records existing navigation response only; no broader live navigation cutover"
        });
        if let Some(semantic_shadow_receipt) = semantic_shadow_receipt
            && let Some(receipt_object) = receipt.as_object_mut()
        {
            receipt_object.insert("semantic_shadow_receipt".to_string(), semantic_shadow_receipt);
        }
        self.record_provider_decision_trace(context.provider, &receipt);
    }

    /// Handle textDocument/declaration request
    pub(crate) fn handle_declaration(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().declaration {
            return Err(crate::protocol::method_not_advertised());
        }

        let t0 = std::time::Instant::now();

        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            // Reject stale requests (parity with hover.rs:51-53 and completion.rs:312)
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;

            // Phase 1: grab an owned `DocumentState` clone under a brief
            // documents-map lock, then drop the guard before doing any
            // analysis (#3396 off-lock provider consumption).
            let timing_on = crate::runtime::timing::is_enabled();
            let t_lock_start = std::time::Instant::now();
            let doc_owned = {
                let documents = self.documents_guard();
                self.get_document(&documents, uri).cloned()
            };
            // documents guard dropped here
            if timing_on {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "provider.navigation.lock_hold",
                    crate::runtime::timing::elapsed_ms(t_lock_start),
                    crate::runtime::timing::uri_tail(uri),
                ));
            }
            if let Some(doc) = doc_owned.as_ref() {
                // Covers the whole analysis block via `Drop`, so it emits
                // correctly regardless of which `return` below fires.
                let _analyze_span =
                    crate::runtime::timing::ScopedSpan::start("provider.navigation.analyze", uri);
                let parsed = doc.current_parsed();
                if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                    let offset = self.pos16_to_offset(doc, line, character);

                    // Use the Declaration provider - ast is already an Arc.
                    // `parsed` is guaranteed `Some` here since `ast` was
                    // derived from it.
                    let empty_parent_map = ParentMap::default();
                    let parent_map = parsed.as_ref().map_or(&empty_parent_map, |p| p.parent_map());
                    let provider = crate::declaration::DeclarationProvider::new(
                        Arc::clone(ast),
                        doc.text_arc.to_string(),
                        uri.to_string(),
                    )
                    .with_parent_map(parent_map)
                    .with_doc_version(doc.version);

                    // Find declaration at the position
                    if let Some(location_links) = provider.find_declaration(offset, doc.version) {
                        // Check client capability and return appropriate format
                        if self.client_capabilities.lock().declaration_link_support {
                            // Return LocationLink format
                            let result: Vec<Value> = location_links
                                .iter()
                                .map(|link| {
                                    let (orig_start_line, orig_start_char) =
                                        self.offset_to_pos16(doc, link.origin_selection_range.0);
                                    let (orig_end_line, orig_end_char) =
                                        self.offset_to_pos16(doc, link.origin_selection_range.1);

                                    let (target_start_line, target_start_char) =
                                        self.offset_to_pos16(doc, link.target_range.0);
                                    let (target_end_line, target_end_char) =
                                        self.offset_to_pos16(doc, link.target_range.1);

                                    let (sel_start_line, sel_start_char) =
                                        self.offset_to_pos16(doc, link.target_selection_range.0);
                                    let (sel_end_line, sel_end_char) =
                                        self.offset_to_pos16(doc, link.target_selection_range.1);

                                    json!({
                                            "originSelectionRange": {
                                                "start": {
                                                    "line": orig_start_line,
                                                    "character": orig_start_char,
                                                },
                                                "end": {
                                                    "line": orig_end_line,
                                                    "character": orig_end_char,
                                                },
                                            },
                                            "targetUri": link.target_uri,
                                            "targetRange": {
                                            "start": {
                                                "line": target_start_line,
                                                "character": target_start_char,
                                            },
                                            "end": {
                                                "line": target_end_line,
                                                "character": target_end_char,
                                            },
                                        },
                                        "targetSelectionRange": {
                                            "start": {
                                                "line": sel_start_line,
                                                "character": sel_start_char,
                                            },
                                            "end": {
                                                "line": sel_end_line,
                                                "character": sel_end_char,
                                            },
                                        },
                                    })
                                })
                                .collect();

                            return Ok(Some(json!(result)));
                        } else {
                            // Down-convert to Location format for clients that don't support LocationLink
                            let result: Vec<Value> = location_links
                                .iter()
                                .map(|link| {
                                    let (sel_start_line, sel_start_char) =
                                        self.offset_to_pos16(doc, link.target_selection_range.0);
                                    let (sel_end_line, sel_end_char) =
                                        self.offset_to_pos16(doc, link.target_selection_range.1);

                                    json!({
                                        "uri": link.target_uri,
                                        "range": {
                                            "start": {
                                                "line": sel_start_line,
                                                "character": sel_start_char,
                                            },
                                            "end": {
                                                "line": sel_end_line,
                                                "character": sel_end_char,
                                            },
                                        },
                                    })
                                })
                                .collect();

                            return Ok(Some(json!(result)));
                        }
                    }
                }

                // Performance monitoring
                let dt = t0.elapsed();
                if doc.text.len() < 50_000 && dt > std::time::Duration::from_millis(50) {
                    tracing::warn!(elapsed = ?dt, uri, "slow declaration");
                }
            }
        }
        Ok(Some(json!([])))
    }

    /// Handle textDocument/definition request
    #[tracing::instrument(skip(self, params), name = "textDocument/definition")]
    pub(crate) fn handle_definition(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let trace_context = Self::navigation_decision_trace_context(
            params.as_ref(),
            "goto_definition",
            "textDocument/definition",
            None,
        )?;
        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        let semantic_shadow_receipt =
            params.as_ref().and_then(|params| self.definition_semantic_shadow_receipt(params));
        #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
        let semantic_shadow_receipt = None;
        let result = self.handle_definition_inner(params)?;
        self.record_navigation_provider_decision_trace(
            trace_context.as_ref(),
            result.as_ref(),
            semantic_shadow_receipt,
        );
        Ok(result)
    }

    fn handle_definition_inner(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            // Reject stale requests (parity with hover.rs:51-53 and completion.rs:312)
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            self.ensure_latest(uri, req_version)?;
            #[cfg(feature = "workspace")]
            let workspace_index_is_fresh = || !self.workspace_index_stale_for_any_open_document();
            #[cfg(not(feature = "workspace"))]
            let workspace_index_is_fresh = || true;

            // First, extract module reference info while holding the document lock briefly
            // We need to release the lock before calling resolve_module_to_path to avoid deadlock
            type Dancer2DefinitionProbe =
                Option<(usize, std::sync::Arc<crate::state::ParsedSnapshot>, String)>;
            let (module_lookup_info, dancer2_probe): (
                Option<(EarlyDefinitionTarget, String, usize)>,
                Dancer2DefinitionProbe,
            ) = {
                let documents = self.documents_guard();
                let dancer2_probe = self.get_document(&documents, uri).and_then(|doc| {
                    let offset = self.pos16_to_offset(doc, line, character);
                    doc.current_parsed()
                        .map(|snapshot| (offset, snapshot, doc.text_arc.to_string()))
                });
                let module_lookup = if let Some(doc) = self.get_document(&documents, uri) {
                    let offset = self.pos16_to_offset(doc, line, character);

                    // Skip go-to-definition inside comments — the cursor is not
                    // on a real symbol.  Without this guard the AST resolver may
                    // jump to an unrelated symbol on the same line.  (#5066)
                    //
                    // This guard runs BEFORE any resolution path (module lookup,
                    // AST resolver, goto-label, Mason, etc.) so it covers all
                    // navigation, not just the module-reference path.
                    //
                    // String-aware guarding is intentionally omitted: text-based
                    // quote scanners produce false positives on real Perl code
                    // (regexes, heredocs, qw(), POD).  The original guard used
                    // `is_in_comment && !is_in_string`, which was logically
                    // inverted — it only blocked when in a comment that was NOT
                    // also classified as a string.  This now blocks whenever the
                    // offset is inside a comment.
                    let text = &doc.text;
                    if is_in_comment_naive(offset, text) {
                        return Ok(None);
                    }

                    let radius = 50;
                    let (text_start, text_around) =
                        self.get_text_window_around_offset(&doc.text, offset, radius);
                    let cursor_in_text = offset.min(doc.text.len()).saturating_sub(text_start);
                    let current_package =
                        doc.current_parsed().and_then(|p| p.ast().cloned()).map_or_else(
                            || "main".to_string(),
                            |ast| crate::declaration::current_package_at(&ast, offset).to_string(),
                        );

                    if let Some(module_name) =
                        extract_xs_bootstrap_target(&text_around, cursor_in_text, &current_package)
                    {
                        Some((
                            EarlyDefinitionTarget::XsBootstrap(module_name),
                            doc.text_arc.to_string(),
                            offset,
                        ))
                    } else if let Some(module_name) =
                        self.extract_module_reference_extended(&text_around, cursor_in_text)
                    {
                        Some((
                            EarlyDefinitionTarget::UseModule(module_name),
                            doc.text_arc.to_string(),
                            offset,
                        ))
                    } else if let Some(module_name) =
                        literal_require_path_module_at_offset(text, offset)
                    {
                        // Literal-path require (`require "Foo/Bar.pm"`): the
                        // quoted form cannot enter the bareword extraction
                        // chain, so normalize it here (#12559).
                        Some((
                            EarlyDefinitionTarget::UseModule(module_name),
                            doc.text_arc.to_string(),
                            offset,
                        ))
                    } else if let Some(module_name) =
                        quoted_framework_module_at_cursor(&text_around, cursor_in_text)?
                    {
                        Some((
                            EarlyDefinitionTarget::FrameworkModule(module_name),
                            doc.text_arc.to_string(),
                            offset,
                        ))
                    } else {
                        // Also check if we're on a package name followed by ->
                        let mut package_name_result = None;
                        let package_pattern = get_package_arrow_regex()?;
                        for cap in package_pattern.captures_iter(&text_around) {
                            if let Some(package_match) = cap.get(1) {
                                let match_start = package_match.start();
                                let match_end = package_match.end();
                                if cursor_in_text >= match_start && cursor_in_text <= match_end {
                                    package_name_result = Some((
                                        EarlyDefinitionTarget::Module(
                                            package_match.as_str().to_string(),
                                        ),
                                        doc.text_arc.to_string(),
                                        offset,
                                    ));
                                    break;
                                }
                            }
                        }
                        package_name_result
                    }
                } else {
                    None
                };
                (module_lookup, dancer2_probe)
            };
            // Lock is released here

            // Canonical Dancer2 definition (#8928): a canonical route
            // declaration resolves to its exact inline handler anchor (or
            // resolved static-coderef declaration). One selected authority;
            // no string-handler subroutine path exists. Computed after the
            // lock is released because module resolution re-locks.
            if let Some((dancer2_offset, dancer2_snapshot, dancer2_text)) = dancer2_probe
                && let Some(ast) = dancer2_snapshot.ast()
                && let Some((context, _package)) = self.dancer2_package_at(
                    uri,
                    &dancer2_text,
                    dancer2_snapshot.content_hash(),
                    ast,
                    dancer2_offset,
                )
                && let Some(perl_lsp_rs_core::providers::dancer2::Dancer2DefinitionTarget::Anchor {
                    start,
                    end,
                    ..
                }) = perl_lsp_rs_core::providers::dancer2::definition_target_at(
                    &context.activations,
                    &context.facts,
                    dancer2_offset,
                )
            {
                let ((sl, sc), (el, ec)) = {
                    let documents = self.documents_guard();
                    self.get_document(&documents, uri)
                        .map(|doc| {
                            // Clamp against the CURRENT text: a didChange
                            // racing the released lock must never push a
                            // stale snapshot's byte offsets out of range.
                            let text_len = doc.text.len();
                            let clamp =
                                |value: u32| usize::try_from(value).unwrap_or(0).min(text_len);
                            (
                                self.offset_to_pos16(doc, clamp(start)),
                                self.offset_to_pos16(doc, clamp(end)),
                            )
                        })
                        .unwrap_or(((0, 0), (0, 0)))
                };
                return Ok(Some(json!([{
                    "uri": uri,
                    "range": {
                        "start": { "line": sl, "character": sc },
                        "end": { "line": el, "character": ec },
                    },
                }])));
            }

            // Now resolve module to path WITHOUT holding the document lock
            if let Some((lookup_target, doc_text, doc_offset)) = module_lookup_info {
                match lookup_target {
                    EarlyDefinitionTarget::XsBootstrap(module_name) => {
                        if let Some(xs_path) = self.resolve_xs_bootstrap_path_with_uri(
                            &module_name,
                            Some(&doc_text),
                            Some(uri),
                        ) {
                            return Ok(Some(json!([xs_bootstrap_location(
                                &xs_path,
                                &module_name
                            )])));
                        }
                    }
                    EarlyDefinitionTarget::UseModule(module_name) => {
                        // Cursor is on a `use Module` / `require Module` statement, or on
                        // the quoted path of `require "Path/To/Module.pm"` (normalized to a
                        // module name upstream). Resolution is authoritative: if the
                        // file-system resolver (which honours position-aware @INC including
                        // `no lib` cancellations) finds a path, return it. If not, return
                        // empty rather than falling through to the workspace-index lookup —
                        // the index is @INC-unaware and would surface files that `no lib`
                        // has cancelled. Fixes #8537.
                        if let Some(module_path) = self.resolve_module_to_path_with_doc_at_offset(
                            &module_name,
                            Some(&doc_text),
                            Some(uri),
                            Some(doc_offset),
                        ) {
                            return Ok(Some(json!([{
                                "uri": module_path,
                                "range": {
                                    "start": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                    "end": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                },
                            }])));
                        } else if is_core_perl_module(&module_name) {
                            // Core pragma — not on disk in the user's workspace, so no file jump
                            // is possible.  Log an info message to the LSP output channel
                            // (visible in the VSCode Output panel) so users can discover that
                            // hover (K) shows documentation for core modules.
                            let _ = self.log_message(
                                crate::runtime::window::MessageType::Info,
                                &format!(
                                    "'{module_name}' is a Perl core module. \
                                     No source file is available for goto-definition. \
                                     Use hover (K) to view documentation."
                                ),
                            );
                            tracing::debug!(
                                module = %module_name,
                                "core pragma requested via goto-def — no file target"
                            );
                        }
                        // Return early: file-system resolution is authoritative for `use Module`.
                        // Do NOT fall through to workspace-index lookup, which is @INC-unaware.
                        return Ok(Some(json!([])));
                    }
                    EarlyDefinitionTarget::Module(module_name) => {
                        if let Some(module_path) = self.resolve_module_to_path_with_doc_at_offset(
                            &module_name,
                            Some(&doc_text),
                            Some(uri),
                            Some(doc_offset),
                        ) {
                            return Ok(Some(json!([{
                                "uri": module_path,
                                "range": {
                                    "start": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                    "end": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                },
                            }])));
                        }
                    }
                    EarlyDefinitionTarget::FrameworkModule(module_ref) => {
                        #[cfg(feature = "workspace")]
                        if workspace_index_is_fresh()
                            && let Some(coordinator) = self.coordinator()
                            && let Some(def_location) =
                                module_ref.definition_location(coordinator.index())
                            && let Some(lsp_location) =
                                crate::workspace_index::lsp_adapter::to_lsp_location(&def_location)
                            && workspace_index_is_fresh()
                        {
                            return Ok(Some(json!([lsp_location])));
                        }

                        if let Some(module_path) = self.resolve_module_to_path_with_doc_at_offset(
                            &module_ref.module_name,
                            Some(&doc_text),
                            Some(uri),
                            Some(doc_offset),
                        ) {
                            return Ok(Some(json!([{
                                "uri": module_path,
                                "range": {
                                    "start": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                    "end": {
                                        "line": 0,
                                        "character": 0,
                                    },
                                },
                            }])));
                        }
                    }
                }
            }

            // Continue with remaining definition lookup logic that needs document access.
            // Grab an owned `DocumentState` clone under a brief documents-map
            // lock, then drop the guard before doing any analysis (#3396
            // off-lock provider consumption).
            let timing_on = crate::runtime::timing::is_enabled();
            let t_lock_start = std::time::Instant::now();
            let doc_owned = {
                let documents = self.documents_guard();
                self.get_document(&documents, uri).cloned()
            };
            // documents guard dropped here
            if timing_on {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "provider.navigation.lock_hold",
                    crate::runtime::timing::elapsed_ms(t_lock_start),
                    crate::runtime::timing::uri_tail(uri),
                ));
            }
            if let Some(doc) = doc_owned.as_ref() {
                // Covers the whole analysis block via `Drop`, so it emits
                // correctly regardless of which `return` below fires.
                let _analyze_span =
                    crate::runtime::timing::ScopedSpan::start("provider.navigation.analyze", uri);
                let offset = self.pos16_to_offset(doc, line, character);
                let radius = 50;
                let (text_start, text_around) =
                    self.get_text_window_around_offset(&doc.text, offset, radius);
                let cursor_in_text = offset.min(doc.text.len()).saturating_sub(text_start);

                let goto_label_re = get_goto_label_regex()?;
                for cap in goto_label_re.captures_iter(&text_around) {
                    if let Some(label_match) = cap.get(1)
                        && cursor_in_text >= label_match.start()
                        && cursor_in_text <= label_match.end()
                        && let Some((target_start, target_end)) =
                            find_label_declaration_span(&doc.text, label_match.as_str())?
                    {
                        let (def_line, def_char) = self.offset_to_pos16(doc, target_start);
                        let (def_end_line, def_end_char) = self.offset_to_pos16(doc, target_end);
                        return Ok(Some(json!([{
                            "uri": uri,
                            "range": {
                                "start": {
                                    "line": def_line,
                                    "character": def_char,
                                },
                                "end": {
                                    "line": def_end_line,
                                    "character": def_end_char,
                                },
                            },
                        }])));
                    }
                }

                if let Some(mason_location) = self.resolve_mason_definition(uri, &doc.text, offset)
                    && let Some(lsp_location) =
                        crate::workspace_index::lsp_adapter::to_lsp_location(&mason_location)
                {
                    return Ok(Some(json!([lsp_location])));
                }

                #[cfg(feature = "workspace")]
                if workspace_index_is_fresh() {
                    let parsed = doc.current_parsed();
                    if let Some(ast) = parsed.as_ref().and_then(|p| p.ast())
                        && let Some(coordinator) = self.coordinator()
                    {
                        let workspace_index = coordinator.index();
                        let current_package = crate::declaration::current_package_at(ast, offset);
                        if let Some(def_location) = resolve_mojolicious_route_definition(
                            workspace_index,
                            current_package,
                            &text_around,
                            cursor_in_text,
                        ) && let Some(lsp_location) =
                            crate::workspace_index::lsp_adapter::to_lsp_location(&def_location)
                            && workspace_index_is_fresh()
                        {
                            return Ok(Some(json!([lsp_location])));
                        }
                    }

                    // Attempt to resolve `SUPER::method` calls using the current package's
                    // inheritance chain before falling back to generic fully-qualified lookup.
                    let current_package = parsed
                        .as_ref()
                        .and_then(|p| p.ast())
                        .map(|ast| {
                            let byte_offset = self.pos16_to_offset(doc, line, character);
                            crate::declaration::current_package_at(ast, byte_offset)
                        })
                        .unwrap_or("main");

                    let super_re = get_super_method_regex()?;
                    for cap in super_re.captures_iter(&text_around) {
                        if let Some(method_match) = cap.get(1)
                            && cursor_in_text >= method_match.start()
                            && cursor_in_text <= method_match.end()
                        {
                            let parsed = doc.current_parsed();
                            if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                                let analyzer =
                                    crate::semantic::SemanticAnalyzer::analyze_with_source(
                                        ast, &doc.text,
                                    );
                                if let Some(location) = analyzer.resolve_inherited_method_location(
                                    current_package,
                                    method_match.as_str(),
                                ) {
                                    let lsp_start = self.offset_to_pos16(doc, location.start);
                                    let lsp_end = self.offset_to_pos16(doc, location.end);
                                    return Ok(Some(json!([{
                                        "uri": uri,
                                        "range": {
                                            "start": {"line": lsp_start.0, "character": lsp_start.1},
                                            "end": {"line": lsp_end.0, "character": lsp_end.1},
                                        },
                                    }])));
                                }
                            }

                            #[cfg(feature = "workspace")]
                            {
                                if let Some(coordinator) = self.coordinator()
                                    && let Some(def_location) =
                                        inherited_method_definition_location(
                                            coordinator.index(),
                                            current_package,
                                            method_match.as_str(),
                                        )
                                        .or_else(|| {
                                            autoload_definition_location(
                                                coordinator.index(),
                                                current_package,
                                                false,
                                            )
                                        })
                                    && let Some(lsp_location) =
                                        crate::workspace_index::lsp_adapter::to_lsp_location(
                                            &def_location,
                                        )
                                    && workspace_index_is_fresh()
                                {
                                    return Ok(Some(json!([lsp_location])));
                                }
                            }
                        }
                    }
                }

                // Attempt to resolve fully-qualified symbols like Package::sub
                //
                // When cursor is on a package-prefix component (e.g. `Foo` in
                // `Foo::bar`), we must NOT fall through to the AST-based workspace
                // lookup below — `symbol_at_cursor_with_source` and
                // `DeclarationProvider` always extract the LAST component of a
                // qualified name regardless of cursor position and would navigate
                // to the wrong symbol.  Track whether the cursor is on a prefix
                // and return early if so.
                //
                // This classification is deliberately OUTSIDE the workspace-index
                // freshness gate. It is a cursor-position fact about the buffer's
                // own text, not an index lookup, and the `Prefix` arm exists to
                // *suppress* a wrong target rather than to offer one. Gating it on
                // freshness would let a stale index re-enable the very wrong jump
                // this arm was written to prevent. Only the `Final` arm — which
                // consults the workspace index — stays gated.
                #[cfg(feature = "workspace")]
                {
                    let fqn_regex = get_fqn_regex()?;
                    if let Some(component) =
                        fqn_component_at_cursor(fqn_regex, &text_around, cursor_in_text)
                    {
                        match component {
                            FqnCursorComponent::Final { package, name } => {
                                if workspace_index_is_fresh()
                                    && let Some(result) = lookup_workspace_definition(
                                        self.coordinator(),
                                        &package,
                                        &name,
                                        Some(uri),
                                    )
                                    && workspace_index_is_fresh()
                                {
                                    return Ok(Some(result));
                                }
                            }
                            FqnCursorComponent::Prefix => return Ok(None),
                        }
                    }
                }

                #[cfg(feature = "workspace")]
                if workspace_index_is_fresh() {
                    // Attempt to resolve Package->method calls
                    let arrow_re = get_arrow_method_regex()?;
                    for cap in arrow_re.captures_iter(&text_around) {
                        if let (Some(package_match), Some(method_match)) = (cap.get(1), cap.get(2))
                            && cursor_in_text >= method_match.start()
                            && cursor_in_text <= method_match.end()
                        {
                            let package_name = package_match.as_str();
                            let method_name = method_match.as_str();

                            if let Some(result) = lookup_workspace_definition(
                                self.coordinator(),
                                package_name,
                                method_name,
                                Some(uri),
                            ) && workspace_index_is_fresh()
                            {
                                return Ok(Some(result));
                            }
                            #[cfg(feature = "workspace")]
                            {
                                if let Some(coordinator) = self.coordinator()
                                    && let Some(def_location) = autoload_definition_location(
                                        coordinator.index(),
                                        package_name,
                                        true,
                                    )
                                    && let Some(lsp_location) =
                                        crate::workspace_index::lsp_adapter::to_lsp_location(
                                            &def_location,
                                        )
                                    && workspace_index_is_fresh()
                                {
                                    return Ok(Some(json!([lsp_location])));
                                }
                            }
                            if is_universal_method(method_name)
                                && let Some(result) = lookup_workspace_definition(
                                    self.coordinator(),
                                    "UNIVERSAL",
                                    method_name,
                                    Some(uri),
                                )
                                && workspace_index_is_fresh()
                            {
                                return Ok(Some(result));
                            }
                            // Partial/None: fall through to same-file resolution
                            break;
                        }
                    }

                    // Attempt to resolve $var->method() calls (e.g., $self->method())
                    // For $self/$this/$class, resolve using the current package context
                    let var_method_re = get_var_method_regex()?;
                    for cap in var_method_re.captures_iter(&text_around) {
                        if let (Some(var_match), Some(method_match)) = (cap.get(1), cap.get(2))
                            && cursor_in_text >= method_match.start()
                            && cursor_in_text <= method_match.end()
                        {
                            let var_name = var_match.as_str();
                            let method_name = method_match.as_str();

                            // For $self/$this/$class, resolve using current package
                            if var_name == "self" || var_name == "this" || var_name == "class" {
                                let self_method_parsed = doc.current_parsed();
                                if let Some(ast) = self_method_parsed.as_ref().and_then(|p| p.ast())
                                {
                                    let byte_offset = self.pos16_to_offset(doc, line, character);
                                    let current_package =
                                        crate::declaration::current_package_at(ast, byte_offset);
                                    if let Some(result) = lookup_workspace_definition(
                                        self.coordinator(),
                                        current_package,
                                        method_name,
                                        Some(uri),
                                    ) && workspace_index_is_fresh()
                                    {
                                        return Ok(Some(result));
                                    }
                                    #[cfg(feature = "workspace")]
                                    {
                                        if let Some(coordinator) = self.coordinator()
                                            && let Some(def_location) = autoload_definition_location(
                                                coordinator.index(),
                                                current_package,
                                                true,
                                            )
                                            && let Some(lsp_location) =
                                                crate::workspace_index::lsp_adapter::to_lsp_location(
                                                    &def_location,
                                                )
                                            && workspace_index_is_fresh()
                                        {
                                            return Ok(Some(json!([lsp_location])));
                                        }
                                    }
                                }
                            }
                            if is_universal_method(method_name)
                                && let Some(result) = lookup_workspace_definition(
                                    self.coordinator(),
                                    "UNIVERSAL",
                                    method_name,
                                    Some(uri),
                                )
                                && workspace_index_is_fresh()
                            {
                                return Ok(Some(result));
                            }
                            // Fall through for non-self variables
                            break;
                        }
                    }
                }

                let parsed = doc.current_parsed();
                if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
                    let offset = self.pos16_to_offset(doc, line, character);

                    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
                    if workspace_index_is_fresh() {
                        let cursor_on_arrow_method = cursor_in_regex_capture(
                            get_arrow_method_regex()?,
                            &text_around,
                            cursor_in_text,
                            2,
                        ) || cursor_in_regex_capture(
                            get_var_method_regex()?,
                            &text_around,
                            cursor_in_text,
                            2,
                        );
                        if !cursor_on_arrow_method {
                            let current_package =
                                crate::declaration::current_package_at(ast, offset);
                            if let Some(symbol_key) =
                                crate::declaration::symbol_at_cursor_with_source(
                                    ast,
                                    offset,
                                    current_package,
                                    &doc.text,
                                )
                            {
                                let workspace_symbol_key =
                                    super::to_workspace_symbol_key(&symbol_key);
                                let import_source =
                                    self.find_import_source(ast, &workspace_symbol_key.name);
                                let semantic_symbol = semantic_definition_query_symbol(
                                    &workspace_symbol_key,
                                    current_package,
                                    import_source.as_deref(),
                                );
                                if let Some(lsp_location) = self.live_exact_definition_location(
                                    uri,
                                    &semantic_symbol,
                                    offset,
                                ) {
                                    return Ok(Some(json!([lsp_location])));
                                }
                            }
                        }
                    }

                    // Try DeclarationProvider first (it handles function calls properly).
                    // `current_parsed()` is guaranteed `Some` here since `ast`
                    // (above) was derived from it.
                    let empty_parent_map = ParentMap::default();
                    let parent_map = parsed.as_ref().map_or(&empty_parent_map, |p| p.parent_map());
                    let provider = crate::declaration::DeclarationProvider::new(
                        Arc::clone(ast),
                        doc.text_arc.to_string(),
                        uri.to_string(),
                    )
                    .with_parent_map(parent_map)
                    .with_doc_version(doc.version);

                    if let Some(location_links) = provider.find_declaration(offset, doc.version) {
                        // Convert to Location format for definition
                        let result: Vec<Value> = location_links
                            .iter()
                            .map(|link| {
                                let (sel_start_line, sel_start_char) =
                                    self.offset_to_pos16(doc, link.target_selection_range.0);
                                let (sel_end_line, sel_end_char) =
                                    self.offset_to_pos16(doc, link.target_selection_range.1);

                                json!({
                                    "uri": link.target_uri,
                                    "range": {
                                        "start": {
                                            "line": sel_start_line,
                                            "character": sel_start_char,
                                        },
                                        "end": {
                                            "line": sel_end_line,
                                            "character": sel_end_char,
                                        },
                                    },
                                })
                            })
                            .collect();

                        if !result.is_empty() {
                            return Ok(Some(json!(result)));
                        }
                    }

                    // Try workspace index for cross-file definitions using routing policy
                    #[cfg(feature = "workspace")]
                    if workspace_index_is_fresh()
                        && let Some(coordinator) = self.coordinator()
                    {
                        let workspace_index = coordinator.index();
                        // Use symbol_at_cursor to get the symbol key
                        let current_package = crate::declaration::current_package_at(ast, offset);
                        if let Some(symbol_key) = crate::declaration::symbol_at_cursor_with_source(
                            ast,
                            offset,
                            current_package,
                            &doc.text,
                        ) {
                            tracing::debug!(symbol_key = ?symbol_key, "looking for definition");
                            let workspace_symbol_key = super::to_workspace_symbol_key(&symbol_key);

                            let def_locations = find_symbol_key_definition_locations(
                                workspace_index,
                                &workspace_symbol_key,
                            );
                            if !def_locations.is_empty() {
                                tracing::debug!(count = def_locations.len(), "found definition(s)");
                                let lsp_locations: Vec<serde_json::Value> = def_locations
                                    .iter()
                                    .filter_map(|loc| {
                                        let lsp_loc =
                                            crate::workspace_index::lsp_adapter::to_lsp_location(
                                                loc,
                                            )?;
                                        serde_json::to_value(lsp_loc).ok()
                                    })
                                    .collect();
                                if !lsp_locations.is_empty() && workspace_index_is_fresh() {
                                    return Ok(Some(to_json_array(&lsp_locations)));
                                }
                            }

                            if workspace_symbol_key.kind == crate::workspace_index::SymKind::Sub
                                && workspace_symbol_key.sigil.is_none()
                                && let Some(import_source) =
                                    self.find_import_source(ast, &workspace_symbol_key.name)
                                && let Some(def_location) = find_workspace_definition_location(
                                    workspace_index,
                                    &import_source,
                                    &workspace_symbol_key.name,
                                )
                                && let Some(lsp_location) =
                                    crate::workspace_index::lsp_adapter::to_lsp_location(
                                        &def_location,
                                    )
                            {
                                tracing::debug!(
                                    symbol = %workspace_symbol_key.name,
                                    source_pkg = %import_source,
                                    "resolved bare imported symbol through require/import source"
                                );
                                if workspace_index_is_fresh() {
                                    return Ok(Some(json!([lsp_location])));
                                }
                            }
                        }
                    }
                    // No coordinator: fall through to same-file semantic model

                    // Fall back to same-file definition
                    let model = crate::semantic::SemanticModel::build(ast, &doc.text);

                    // Find definition at the position
                    if let Some(definition) = model.definition_at(offset) {
                        let (def_line, def_char) =
                            self.offset_to_pos16(doc, definition.location.start);
                        let (def_end_line, def_end_char) =
                            self.offset_to_pos16(doc, definition.location.end);

                        return Ok(Some(json!([{
                            "uri": uri,
                            "range": {
                                "start": {
                                    "line": def_line,
                                    "character": def_char,
                                },
                                "end": {
                                    "line": def_end_line,
                                    "character": def_end_char,
                                },
                            },
                        }])));
                    }
                }
            }
        }

        Ok(Some(json!([])))
    }

    /// Handle definition request with cancellation support
    pub(crate) fn handle_definition_cancellable(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Convert raw Value ID to typed ID at the boundary.
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        // RAII guard ensures cleanup on all exit paths (early returns, errors, panics)
        let _cleanup_guard = RequestCleanupGuard::from_ref(typed_id.as_ref());

        if let Some(params) = params {
            // Create or get cancellation token for this request
            if let Some(ref tid) = typed_id {
                let token = GLOBAL_CANCELLATION_REGISTRY.get_token(tid).unwrap_or_else(|| {
                    let token = PerlLspCancellationToken::new(
                        tid.clone(),
                        "textDocument/definition".to_string(),
                    );
                    let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone());
                    token
                });

                // Early cancellation check with relaxed read
                if token.is_cancelled_relaxed() {
                    return Err(JsonRpcError {
                        code: REQUEST_CANCELLED,
                        message: "Request cancelled - definition provider".to_string(),
                        data: None,
                    });
                }
            }

            // Delegate to original handler
            self.handle_definition(Some(params))
        } else {
            self.handle_definition(params)
        }
    }

    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    fn definition_semantic_shadow_receipt(&self, params: &Value) -> Option<Value> {
        let uri = req_uri(params).ok()?;
        let (line, character) = req_position(params).ok()?;
        let (symbol, byte_offset, text_around, cursor_in_text, document_generation) =
            self.navigation_runtime_snapshot(uri, line, character)?;
        let fqn_regex = get_fqn_regex().ok()?;
        if matches!(
            fqn_component_at_cursor(fqn_regex, &text_around, cursor_in_text),
            Some(FqnCursorComponent::Prefix)
        ) {
            return None;
        }
        let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);
        if self.workspace_index_stale_for_any_open_document() {
            return None;
        }
        let IndexAccessMode::Full(coordinator) = route_index_access(self.coordinator()) else {
            return None;
        };
        let index = coordinator.index();
        let snapshot_is_current = || {
            document_generation == 0
                || (self.document_generation(uri) == Some(document_generation)
                    && index.indexed_generation(uri) == Some(document_generation))
        };
        if !snapshot_is_current() {
            return None;
        }
        let receipt = index.with_semantic_queries_for_uri(uri, |file_id, queries| {
            let context = QueryContext::new(file_id, None, Some(byte_offset));
            goto_definition_live_exact_or_imported(index.as_ref(), &queries, &symbol, &context)
                .receipt
        })?;
        if !snapshot_is_current() || self.workspace_index_stale_for_any_open_document() {
            return None;
        }
        serde_json::to_value(receipt).ok()
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn definition_runtime_quality_receipt(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let live_provider_result = self.handle_definition(params.clone())?;
        let live_provider_count = lsp_location_count(live_provider_result.as_ref());

        #[cfg(not(all(feature = "workspace", not(target_arch = "wasm32"))))]
        {
            Ok(Some(json!({
                "provider": "definition",
                "live_provider_result": live_provider_result,
                "live_provider_count": live_provider_count,
                "compiler_receipt": null,
                "no_live_behavior_change": true,
                "note": "definition runtime proof unavailable without workspace semantic queries"
            })))
        }

        #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
        {
            let Some(params) = params else {
                return Ok(Some(json!({
                    "provider": "definition",
                    "live_provider_result": live_provider_result,
                    "live_provider_count": live_provider_count,
                    "compiler_receipt": null,
                    "no_live_behavior_change": true,
                    "note": "definition runtime proof missing request params"
                })));
            };

            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;
            let Some((symbol, byte_offset, _, _, _)) =
                self.navigation_runtime_snapshot(uri, line, character)
            else {
                return Ok(Some(json!({
                    "provider": "definition",
                    "live_provider_result": live_provider_result,
                    "live_provider_count": live_provider_count,
                    "compiler_receipt": null,
                    "no_live_behavior_change": true,
                    "note": "definition runtime proof found no symbol at request position"
                })));
            };

            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);
            let compiler_receipt = if self.workspace_index_stale_for_any_open_document() {
                None
            } else {
                match route_index_access(self.coordinator()) {
                    IndexAccessMode::Full(coordinator) => {
                        let index = coordinator.index();
                        index.with_semantic_queries_for_uri(uri, |file_id, queries| {
                        let ctx = QueryContext::new(file_id, None, Some(byte_offset));
                        let mut receipt = goto_definition_live_exact_or_imported(
                            index.as_ref(),
                            &queries,
                            &symbol,
                            &ctx,
                        )
                        .receipt;
                        let compiler_result_count = receipt.new_result.match_count;
                        receipt.notes.push(format!(
                            "definition runtime proof: live_provider_results={live_provider_count}; compiler_fact_candidates={}; compiler_result_count={}; partial live exact/imported cutover",
                            compiler_result_count, compiler_result_count
                        ));
                        receipt
                    })
                    }
                    IndexAccessMode::Partial(_) | IndexAccessMode::None => None,
                }
            };
            let live_cutover = compiler_receipt.is_some();

            Ok(Some(json!({
                "provider": "definition",
                "symbol": symbol,
                "live_provider_result": live_provider_result,
                "live_provider_count": live_provider_count,
                "compiler_receipt": compiler_receipt,
                "no_live_behavior_change": !live_cutover,
                "live_cutover": if live_cutover {
                    Some("partial_exact_imported")
                } else {
                    None
                }
            })))
        }
    }

    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    fn navigation_runtime_snapshot(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<(String, u32, String, usize, u32)> {
        let documents = self.documents_guard();
        let doc = self.get_document(&documents, uri)?;
        let document_generation = doc.current_generation();
        let offset = self.pos16_to_offset(doc, line, character);
        let (symbol, byte_offset) =
            self.navigation_runtime_symbol_from_document(doc, line, character, offset)?;
        let (text_start, text_around) = self.get_text_window_around_offset(&doc.text, offset, 50);
        let cursor_in_text = offset.min(doc.text.len()).saturating_sub(text_start);
        Some((symbol, byte_offset, text_around, cursor_in_text, document_generation))
    }

    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    fn navigation_runtime_symbol_from_document(
        &self,
        doc: &DocumentState,
        line: u32,
        character: u32,
        offset: usize,
    ) -> Option<(String, u32)> {
        #[cfg(not(target_arch = "wasm32"))]
        let parsed = doc.current_parsed();
        if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
            let current_package = crate::declaration::current_package_at(ast, offset);
            if let Some(symbol_key) = crate::declaration::symbol_at_cursor_with_source(
                ast,
                offset,
                current_package,
                &doc.text,
            ) {
                let workspace_symbol_key = super::to_workspace_symbol_key(&symbol_key);
                let import_source = self.find_import_source(ast, &workspace_symbol_key.name);
                let symbol = semantic_definition_query_symbol(
                    &workspace_symbol_key,
                    current_package,
                    import_source.as_deref(),
                );
                let byte_offset = u32::try_from(offset).ok()?;
                return Some((symbol, byte_offset));
            }
        }
        let symbol = token_under_cursor(&doc.text, line as usize, character as usize)?;
        if symbol.is_empty() {
            return None;
        }
        let byte_offset = u32::try_from(offset).ok()?;
        Some((symbol, byte_offset))
    }

    #[cfg(all(feature = "workspace", not(target_arch = "wasm32")))]
    fn live_exact_definition_location(
        &self,
        uri: &str,
        symbol: &str,
        byte_offset: usize,
    ) -> Option<Value> {
        let byte_offset = u32::try_from(byte_offset).ok()?;
        if self.workspace_index_stale_for_any_open_document() {
            return None;
        }
        let workspace_index = self.workspace_index()?;
        let outcome = workspace_index.with_semantic_queries_for_uri(uri, |file_id, queries| {
            let ctx = QueryContext::new(file_id, None, Some(byte_offset));
            goto_definition_live_exact_or_imported(workspace_index.as_ref(), &queries, symbol, &ctx)
        })?;

        if self.workspace_index_stale_for_any_open_document() {
            return None;
        }

        let DefinitionCutoverResult::Exact(candidate) = outcome.result else {
            return None;
        };
        let def_location = workspace_index.semantic_anchor_wire_location(candidate.anchor_id)?;
        // An unconvertible URI yields no definition rather than a fabricated one:
        // this exact path claims source-backed exactness, which a substituted
        // resource cannot support.
        let location = lsp_types::Location::try_from(def_location).ok()?;
        serde_json::to_value(location).ok()
    }

    /// Builds the fallback `doc_map` shared by `handle_type_definition` and
    /// `handle_implementation`'s same-document TOCTOU fix (#3613).
    ///
    /// Both handlers capture the request document's ast/text under one lock
    /// acquisition, then later re-read all open documents via
    /// `documents_text_snapshot()` for the fallback cross-file scan.
    ///
    /// Consistency note: `documents_text_snapshot()` is a fresh, independent
    /// lock acquisition, so in general it observes whatever generation of
    /// each document is live *at this later point* -- not necessarily the
    /// same generation `captured_text` captured. For every *other* open
    /// document that's fine (the provider's cross-file scan is a heuristic,
    /// name-based search with no offset dependency on the earlier capture).
    /// For `uri` itself it is not: the caller's `ast` was parsed from
    /// `captured_text`'s generation, and the provider converts the
    /// request's line/character into a byte offset using
    /// `documents.get(uri)` -- so searching that offset against a *fresher*
    /// re-read of the same uri (if a `didChange` races in between the two
    /// lock acquisitions) would pair a generation-N AST with generation-N+1
    /// text for the same document, the exact single-instance/single-
    /// generation invariant this off-lock pattern must preserve (mirrors
    /// the references.rs fix, #3396 / a95ad72). `uri`'s own entry is
    /// therefore pinned to `captured_text` (the exact generation captured
    /// by the caller) instead of the live map; every other open document
    /// still gets the freshest read.
    ///
    /// The pin is applied by an unconditional `insert` after the snapshot
    /// is collected, not a substitute-in-place during the iteration: a
    /// concurrent `didClose` racing between the caller's up-front capture
    /// and this later re-read removes `uri` from
    /// `documents_text_snapshot()` entirely, so substituting only when `k
    /// == uri` is already present would silently drop `uri` from the map
    /// instead of pinning it -- and the provider's `documents.get(uri)?`
    /// would then return `None` (an empty result) even though a valid
    /// captured snapshot exists. The unconditional `insert` restores `uri`
    /// regardless of whether the live map still has it, closing that
    /// residual TOCTOU window.
    fn pinned_doc_map_for(&self, uri: &str, captured_text: &str) -> HashMap<String, String> {
        // Test-only: pauses here (no-op in production) so a race
        // regression test can apply a real edit (or close) to `uri` before
        // the fallback re-reads `documents_text_snapshot()` below (#3613).
        wait_at_same_doc_fallback_gap();

        let mut doc_map: HashMap<String, String> =
            self.documents_text_snapshot().into_iter().collect();

        // In the text-sync (open/change/close) path, the live map is keyed by
        // `normalize_uri_key` (see text_sync.rs "Store document state with
        // normalized URI"), but `uri` here is the raw request URI as received
        // from the client -- which can differ from its normalized form (e.g.
        // Windows drive-letter casing: `file:///C:/...` vs the normalized
        // `file:///c:/...`). If the two differ AND the document remains open
        // through the snapshot read, `documents_text_snapshot()` above
        // contains this SAME document under its normalized key; inserting the
        // pinned entry under the raw key without first removing that
        // normalized entry would leave the same document present under two
        // keys. A scan that iterates every entry (e.g.
        // `find_package_definition_in_docs`) would then find the same package
        // declaration twice, producing an "ambiguous identity"
        // (`locations.len() > 1`) empty result -- even for a request with no
        // race at all. Remove the normalized entry first so the pinned insert
        // is the only copy of this document in the map (see #3613 for the
        // didClose case where the normalized entry is absent, and #3665 for
        // the rename path edge case).
        let normalized = self.normalize_uri_key(uri);
        if normalized != uri {
            doc_map.remove(&normalized);
        }
        doc_map.insert(uri.to_string(), captured_text.to_string());
        doc_map
    }

    /// Handle textDocument/typeDefinition request
    pub(crate) fn handle_type_definition(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        use crate::features::type_definition::TypeDefinitionProvider;

        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;
            let trace_context = NavigationDecisionTraceContext {
                provider: "type_definition",
                provider_action: "textDocument/typeDefinition",
                uri: uri.to_string(),
                line,
                character,
                include_declaration: None,
            };
            let req_version =
                params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
            if let Some(request_version) = req_version {
                let current_document_version = {
                    let documents = self.documents_guard();
                    self.get_document(&documents, uri).map(|doc| doc.version)
                };
                if let Some(current_document_version) = current_document_version
                    && request_version < current_document_version
                {
                    self.record_type_definition_provider_decision_trace(
                        &trace_context,
                        0,
                        stale_type_definition_fallback_trace(
                            request_version,
                            current_document_version,
                        ),
                    );
                    return Err(Self::content_modified());
                }
            }

            // Acquire minimal data under lock, then drop it
            let (ast, doc_text) = {
                let documents = self.documents_guard();
                let Some(doc) = self.get_document(&documents, uri) else {
                    self.record_type_definition_provider_decision_trace(
                        &trace_context,
                        0,
                        TypeDefinitionFallbackTrace::default(),
                    );
                    return Ok(Some(json!([])));
                };
                let Some(ast) = doc.current_parsed().and_then(|p| p.ast().cloned()) else {
                    self.record_type_definition_provider_decision_trace(
                        &trace_context,
                        0,
                        TypeDefinitionFallbackTrace::default(),
                    );
                    return Ok(Some(json!([])));
                };
                (ast, doc.text_arc.to_string())
            };

            // Build doc_map outside the lock, pinning `uri`'s own entry to
            // the captured generation -- see `pinned_doc_map_for`'s
            // doc-comment for why (#3613).
            let doc_map = self.pinned_doc_map_for(uri, &doc_text);

            let provider = TypeDefinitionProvider::new();
            if let Some(locations) =
                provider.find_type_definition(ast.as_ref(), line, character, uri, &doc_map)
            {
                if locations.len() == 1 {
                    self.record_type_definition_provider_decision_trace(
                        &trace_context,
                        locations.len(),
                        TypeDefinitionFallbackTrace::default(),
                    );
                    return Ok(Some(to_json_array(&locations)));
                }

                self.record_type_definition_ambiguous_identity_trace(
                    &trace_context,
                    locations.len(),
                );
                return Ok(Some(json!([])));
            }
            self.record_type_definition_provider_decision_trace(
                &trace_context,
                0,
                classify_type_definition_fallback_trace_with_documents(
                    &doc_text, line, character, &doc_map,
                ),
            );
        }

        Ok(Some(json!([])))
    }

    fn record_type_definition_provider_decision_trace(
        &self,
        context: &NavigationDecisionTraceContext,
        result_count: usize,
        fallback_trace: TypeDefinitionFallbackTrace,
    ) {
        let acted = result_count > 0;
        let result_count = u64::try_from(result_count).unwrap_or(u64::MAX);
        let fact_source = if acted { "parser_syntax" } else { fallback_trace.fact_source };
        let mut receipt = json!({
            "provider": context.provider,
            "provider_action": context.provider_action,
            "decision": if acted { "acted" } else { fallback_trace.decision },
            "reason": if acted { "source_backed_high_confidence" } else { fallback_trace.reason },
            "uri": context.uri,
            "line": context.line,
            "character": context.character,
            "result_count": result_count,
            "live_provider_result_count": result_count,
            "fact_source": fact_source,
            "confidence": if acted { "high" } else { "low" },
            "freshness": provider_decision_freshness_wire(type_definition_receipt_freshness(
                fact_source,
            )),
            "source_backed": acted,
            "source_backed_state": if acted {
                "open_document_type_definition"
            } else {
                fallback_trace.source_backed_state
            },
            "fallback": if acted { "none" } else { fallback_trace.fallback },
            "fallback_state": if acted { "none" } else { fallback_trace.fallback },
            "dynamic_boundary": if acted { false } else { fallback_trace.dynamic_boundary },
            "trace_only_no_live_behavior_change": if acted {
                true
            } else {
                fallback_trace.trace_only_no_live_behavior_change
            },
            "claim_boundary": "records existing type-definition safe subset only; direct package/class identifiers and constructor receivers may resolve to open-document package definitions while variable receivers, chained method results, function-call results, missing package definitions, generated/no-source facts, unscannable documents, dynamic boundaries, stale facts, low-confidence facts, and ambiguous identities remain fallback or blocked"
        });
        if !acted && let Some(object) = receipt.as_object_mut() {
            object.insert("blocker".to_string(), json!(fallback_trace.blocker));
            if let Some(request_version) = fallback_trace.request_version {
                object.insert("request_version".to_string(), json!(request_version));
            }
            if let Some(current_document_version) = fallback_trace.current_document_version {
                object.insert(
                    "current_document_version".to_string(),
                    json!(current_document_version),
                );
            }
        }

        self.record_provider_decision_trace(context.provider, &receipt);
    }

    fn record_type_definition_ambiguous_identity_trace(
        &self,
        context: &NavigationDecisionTraceContext,
        candidate_count: usize,
    ) {
        let candidate_count = u64::try_from(candidate_count).unwrap_or(u64::MAX);
        let fact_source = "parser_syntax";
        let receipt = json!({
            "provider": context.provider,
            "provider_action": context.provider_action,
            "decision": "fallback",
            "reason": "ambiguous_low_confidence_candidates",
            "blocker": "ambiguous_identity",
            "uri": context.uri,
            "line": context.line,
            "character": context.character,
            "result_count": 0,
            "live_provider_result_count": 0,
            "ambiguous_candidate_count": candidate_count,
            "fact_source": fact_source,
            "confidence": "low",
            "freshness": provider_decision_freshness_wire(type_definition_receipt_freshness(
                fact_source,
            )),
            "source_backed": false,
            "source_backed_state": "ambiguous_type_definition_identity",
            "fallback": "no_result",
            "fallback_state": "no_result",
            "dynamic_boundary": false,
            "trace_only_no_live_behavior_change": false,
            "claim_boundary": "blocks ambiguous type-definition identities; direct package/class identifiers and constructor receivers may resolve only when they identify one open-document package definition, while duplicate package declarations, variable receivers, chained method results, function-call results, missing package definitions, generated/no-source facts, dynamic boundaries, stale facts, low-confidence facts, and unsupported identities remain fallback or blocked"
        });

        self.record_provider_decision_trace(context.provider, &receipt);
    }

    /// Handle textDocument/implementation request
    pub(crate) fn handle_implementation(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            // Acquire minimal data under lock, then drop it
            let (ast, doc_text) = {
                let documents = self.documents_guard();
                let Some(doc) = self.get_document(&documents, uri) else {
                    return Ok(Some(json!([])));
                };
                let Some(ast) = doc.current_parsed().and_then(|p| p.ast().cloned()) else {
                    return Ok(Some(json!([])));
                };
                (ast, doc.text_arc.to_string())
            };

            #[cfg(feature = "workspace")]
            {
                // Wait for the workspace index to finish building before querying it.
                // Without this, an implementation request while the index is in Building
                // state routes to Partial and returns no cross-file implementors.
                // Mirrors the pattern used by completion (#3069) and workspace/symbol (#1514).
                let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);

                // Build doc_map outside the lock, pinning `uri`'s own entry
                // to the captured generation -- mirrors
                // `handle_type_definition` above; see `pinned_doc_map_for`'s
                // doc-comment for why (#3613, and the references.rs fix,
                // #3396 / a95ad72).
                let doc_map = self.pinned_doc_map_for(uri, &doc_text);

                // Sample after readiness wait and doc snapshot; do not call while
                // holding `documents_guard()` (#5016 / #6199 deadlock lesson).
                let workspace_index_stale = self.workspace_index_stale_for_any_open_document();

                // Use routing policy - only provide workspace index in Full mode.
                // When any open document is ahead of the index, skip the index tier
                // and rely on the open-document AST scan only (#5016).
                let workspace_index = if workspace_index_stale {
                    tracing::debug!(
                        "Implementation: skipping stale workspace index tier, using open-doc scan only"
                    );
                    None
                } else {
                    let access_mode = route_index_access(self.coordinator());
                    if let IndexAccessMode::Full(coordinator) = access_mode {
                        Some(coordinator.index().clone())
                    } else {
                        // Partial/None: same-file analysis only
                        None
                    }
                };

                let provider = ImplementationProvider::new(workspace_index);
                let locations =
                    provider.find_implementations(ast.as_ref(), line, character, uri, &doc_map);
                return Ok(Some(to_json_array(&locations)));
            }

            #[cfg(not(feature = "workspace"))]
            {
                let _ = (ast, doc_text, line, character, uri); // Suppress unused warnings
            }
        }

        Ok(Some(json!([])))
    }

    /// Test-only entrypoint for LSP `textDocument/typeDefinition`.
    ///
    /// Exposes the internal [`Self::handle_type_definition`] handler for
    /// integration tests (#3613 same-document TOCTOU regression coverage).
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn test_handle_type_definition(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_type_definition(params)
    }

    /// Test-only entrypoint for LSP `textDocument/implementation`.
    ///
    /// Exposes the internal [`Self::handle_implementation`] handler for
    /// integration tests (#3613 same-document TOCTOU regression coverage).
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn test_handle_implementation(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_implementation(params)
    }

    /// Test-only: arm [`wait_at_same_doc_fallback_gap`] for one deterministic
    /// pause.
    ///
    /// The next call into `handle_type_definition` or `handle_implementation`
    /// that reaches the same-document fallback gap will send on `reached`
    /// and then block on `resume.recv()` until the test signals it to
    /// continue. Consumed (armed exactly once) per call -- see #3613.
    ///
    /// Recovers a poisoned `NAVIGATION_SAME_DOC_FALLBACK_GAP` the same way
    /// `wait_at_same_doc_fallback_gap` does. Without this, `if let Ok(...) =
    /// ...lock()` would silently no-op after any prior poisoning (e.g. a
    /// deliberate-poison test running earlier in the same process), leaving
    /// the hook never armed and a caller's `reached_rx.recv()` blocking
    /// forever waiting for a signal that will never come.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn test_set_navigation_same_doc_fallback_gap_hook(
        &self,
        reached: std::sync::mpsc::Sender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    ) {
        let _ = self;
        let mut hook = match NAVIGATION_SAME_DOC_FALLBACK_GAP.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *hook = Some((reached, resume));
    }

    /// Non-blocking definition handler with fallback
    ///
    /// Production definition dispatch is a transparent adapter over the
    /// canonical handler, so this compatibility handler is compiled only for
    /// the test-fallbacks path (#5108).
    #[cfg(any(test, feature = "test-fallbacks"))]
    pub(crate) fn on_definition(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let uri = params.pointer("/textDocument/uri").and_then(|v| v.as_str()).unwrap_or("");
        let line = params.pointer("/position/line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let ch =
            params.pointer("/position/character").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let text = self.buffer_text(uri).unwrap_or_default();
        let module = token_under_cursor(&text, line, ch).filter(|s| s.contains("::"));

        if let Some(m) = module
            && let Some(path) = self.resolve_module_path_with_uri(&m, Some(&text), Some(uri))
        {
            let loc = location_from_path(&path);
            return Ok(serde_json::json!([loc]));
        }

        // Fallback: try existing analysis
        // For now, just return empty array
        Ok(serde_json::json!([]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serde_freshness_spelling(variant: ProviderDecisionFreshness) -> Option<String> {
        serde_json::to_value(variant).ok().and_then(|value| value.as_str().map(str::to_owned))
    }

    /// The receipt wire spelling must track the enum's serde `snake_case`,
    /// including `not_applicable` rather than the human-readable
    /// `"not applicable"` / capitalized `"Fresh"`.
    #[test]
    fn provider_decision_freshness_wire_matches_canonical_serde_snake_case() {
        for variant in [
            ProviderDecisionFreshness::Fresh,
            ProviderDecisionFreshness::Stale,
            ProviderDecisionFreshness::Unknown,
            ProviderDecisionFreshness::NotApplicable,
        ] {
            assert_eq!(
                Some(provider_decision_freshness_wire(variant).to_string()),
                serde_freshness_spelling(variant),
                "{variant:?} receipt wire spelling drifted from ProviderDecisionFreshness serde"
            );
        }
        assert_eq!(
            provider_decision_freshness_wire(ProviderDecisionFreshness::NotApplicable),
            "not_applicable"
        );
        assert_ne!(provider_decision_freshness_wire(ProviderDecisionFreshness::Fresh), "Fresh");
    }

    /// Exhaustive oracle for goto-definition receipt freshness (#14162).
    #[test]
    fn goto_definition_receipt_freshness_is_derived_from_result_and_index_staleness() {
        assert_eq!(
            goto_definition_receipt_freshness(1, false),
            ProviderDecisionFreshness::Fresh,
            "a location from live or freshness-gated facts is current"
        );
        assert_eq!(
            goto_definition_receipt_freshness(1, true),
            ProviderDecisionFreshness::Fresh,
            "a location under a stale index still came from live document facts"
        );
        assert_eq!(
            goto_definition_receipt_freshness(0, false),
            ProviderDecisionFreshness::Fresh,
            "an empty answer over current sources is a trustworthy negative"
        );
        assert_eq!(
            goto_definition_receipt_freshness(0, true),
            ProviderDecisionFreshness::Unknown,
            "an empty answer over a stale index must not claim freshness"
        );
    }

    /// Counter-assertion: a hardcode of either polarity fails (#14162).
    #[test]
    fn goto_definition_receipt_freshness_is_not_a_constant() {
        assert_ne!(
            goto_definition_receipt_freshness(0, true),
            goto_definition_receipt_freshness(0, false),
            "empty-answer freshness must vary with workspace-index staleness"
        );
        assert_ne!(
            goto_definition_receipt_freshness(1, true),
            goto_definition_receipt_freshness(0, true),
            "freshness must vary by whether a location was returned under one stale index"
        );
        assert_eq!(
            provider_decision_freshness_wire(goto_definition_receipt_freshness(1, true)),
            "fresh"
        );
        assert_eq!(
            provider_decision_freshness_wire(goto_definition_receipt_freshness(0, true)),
            "unknown"
        );
    }

    #[test]
    fn type_definition_receipt_freshness_is_derived_from_the_answering_fact_source() {
        assert_eq!(
            type_definition_receipt_freshness("parser_syntax"),
            ProviderDecisionFreshness::Fresh,
            "open-document parser facts are current for the request"
        );
        assert_eq!(
            type_definition_receipt_freshness("dynamic_boundary"),
            ProviderDecisionFreshness::Fresh,
            "a dynamic boundary classified from the current buffer is current"
        );
        assert_eq!(
            type_definition_receipt_freshness("fallback"),
            ProviderDecisionFreshness::Fresh,
            "a fallback scan of current open documents is current"
        );
        assert_eq!(
            type_definition_receipt_freshness("request_version"),
            ProviderDecisionFreshness::Stale,
            "a request behind the live document version is stale"
        );
        assert_eq!(
            type_definition_receipt_freshness("not_a_known_source"),
            ProviderDecisionFreshness::Unknown,
            "an unrecognized source must fail closed"
        );
    }

    /// Counter-assertion for type-definition: acted/ambiguous parser facts and
    /// a stale request cannot share one hardcoded polarity.
    #[test]
    fn type_definition_receipt_freshness_is_not_a_constant() {
        assert_ne!(
            type_definition_receipt_freshness("parser_syntax"),
            type_definition_receipt_freshness("request_version"),
            "freshness must distinguish current open-document facts from a stale request"
        );
        assert_eq!(
            provider_decision_freshness_wire(type_definition_receipt_freshness("parser_syntax")),
            "fresh"
        );
        assert_eq!(
            provider_decision_freshness_wire(type_definition_receipt_freshness("request_version")),
            "stale"
        );
    }

    #[test]
    fn navigation_receipt_freshness_stays_in_the_canonical_vocabulary() {
        let canonical: Vec<String> = [
            ProviderDecisionFreshness::Fresh,
            ProviderDecisionFreshness::Stale,
            ProviderDecisionFreshness::Unknown,
            ProviderDecisionFreshness::NotApplicable,
        ]
        .into_iter()
        .filter_map(serde_freshness_spelling)
        .collect();
        assert_eq!(
            canonical.len(),
            4,
            "ProviderDecisionFreshness serde must yield four snake_case spellings"
        );

        let emitted = [
            goto_definition_receipt_freshness(0, false),
            goto_definition_receipt_freshness(0, true),
            goto_definition_receipt_freshness(1, false),
            goto_definition_receipt_freshness(1, true),
            type_definition_receipt_freshness("parser_syntax"),
            type_definition_receipt_freshness("dynamic_boundary"),
            type_definition_receipt_freshness("fallback"),
            type_definition_receipt_freshness("request_version"),
            type_definition_receipt_freshness("unknown_source"),
        ];
        for freshness in emitted {
            let value = provider_decision_freshness_wire(freshness);
            assert!(
                canonical.iter().any(|canonical| canonical == value),
                "{freshness:?} emitted {value:?}, outside ProviderDecisionFreshness"
            );
        }
    }

    fn goto_definition_request_receipt(
        server: &LspServer,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<(Option<Value>, Value), Box<dyn std::error::Error>> {
        let result = server.test_handle_definition(Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        })))?;
        let explanation = server
            .handle_execute_command(Some(json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": "goto_definition"}]
            })))?
            .ok_or("missing explain-provider-decision response")?;
        let receipt =
            explanation.get("request_receipt").cloned().ok_or("missing request_receipt")?;
        Ok((result, receipt))
    }

    /// End-to-end counter-assertion that the goto-definition receipt's
    /// `freshness` is wired to the derivation rather than emitted as a literal
    /// (#14162).
    ///
    /// One server, both index states, both result polarities:
    /// - empty over a current index → `fresh` (fails `empty => unknown`)
    /// - live answer under a stale index → `fresh` (fails `stale index => unknown`)
    /// - empty under that same stale index → `unknown` (fails hardcoded `fresh`)
    #[cfg(feature = "workspace")]
    #[test]
    fn handle_definition_derives_receipt_freshness_from_what_it_answered_from()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let main_uri = "file:///workspace/freshness-def-main.pl";
        let main_text = "package Foo;\nsub bar { return 1; }\npackage main;\nFoo::bar();\n";
        let unrelated_uri = "file:///workspace/freshness-def-unrelated.pl";
        let unrelated_text = "package Unrelated;\nsub helper {}\n";

        server.test_apply_did_open(main_uri, main_text, 1)?;
        server.test_apply_did_open(unrelated_uri, unrelated_text, 1)?;
        server
            .test_index_file_in_building_state(main_uri, main_text)
            .map_err(std::io::Error::other)?;
        server
            .test_index_file_in_building_state(unrelated_uri, unrelated_text)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();
        assert!(
            !server.workspace_index_stale_for_any_open_document(),
            "the fixture starts with a current workspace index"
        );

        // Cursor on the `Foo` prefix of `Foo::bar` (line 3, character 1).
        let (prefix_fresh_index, prefix_fresh_receipt) =
            goto_definition_request_receipt(&server, main_uri, 3, 1)?;
        assert!(
            prefix_fresh_index.as_ref().and_then(Value::as_array).is_some_and(Vec::is_empty)
                || prefix_fresh_index.is_none(),
            "a package-prefix cursor must yield an empty answer; got {prefix_fresh_index:?}"
        );
        assert_eq!(prefix_fresh_receipt.get("result_count").and_then(Value::as_u64), Some(0));
        assert_eq!(
            prefix_fresh_receipt.get("freshness").and_then(Value::as_str),
            Some("fresh"),
            "an empty answer over a current index is a trustworthy negative"
        );

        server
            .test_replace_document_without_index(
                unrelated_uri,
                "package Unrelated;\nsub renamed {}\n",
                2,
            )
            .map_err(std::io::Error::other)?;
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "the edited unrelated buffer must stale the workspace index"
        );

        // Cursor on `bar` in `Foo::bar()` (line 3, character 5): live same-file
        // facts still answer, so the receipt stays fresh.
        let (live, live_receipt) = goto_definition_request_receipt(&server, main_uri, 3, 5)?;
        assert!(
            live.as_ref().and_then(Value::as_array).is_some_and(|locations| !locations.is_empty()),
            "same-file Foo::bar should still resolve under a stale index: {live:?}"
        );
        assert_eq!(
            live_receipt.get("freshness").and_then(Value::as_str),
            Some("fresh"),
            "an answer read from live document facts is current despite a stale index"
        );

        let (prefix_stale_index, prefix_stale_receipt) =
            goto_definition_request_receipt(&server, main_uri, 3, 1)?;
        assert!(
            prefix_stale_index.as_ref().and_then(Value::as_array).is_some_and(Vec::is_empty)
                || prefix_stale_index.is_none(),
            "a package-prefix cursor must stay empty under a stale index; got {prefix_stale_index:?}"
        );
        assert_eq!(prefix_stale_receipt.get("result_count").and_then(Value::as_u64), Some(0));
        assert_eq!(
            prefix_stale_receipt.get("freshness").and_then(Value::as_str),
            Some("unknown"),
            "an empty answer over a stale index must not claim freshness"
        );

        assert_ne!(
            live_receipt.get("freshness"),
            prefix_stale_receipt.get("freshness"),
            "freshness must discriminate a live answer from an empty stale-index answer"
        );
        assert_ne!(
            prefix_fresh_receipt.get("freshness"),
            prefix_stale_receipt.get("freshness"),
            "empty-answer freshness must vary with the index state actually observed"
        );

        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn fqn_component_classifier_matches_navigation_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let regex = get_fqn_regex()?;
        assert_eq!(fqn_component_at_cursor(regex, "Foo::bar", 1), Some(FqnCursorComponent::Prefix));
        assert_eq!(
            fqn_component_at_cursor(regex, "Foo::bar", 5),
            Some(FqnCursorComponent::Final { package: "Foo".to_string(), name: "bar".to_string() })
        );
        assert_eq!(fqn_component_at_cursor(regex, "Foo::bar", 9), None);
        Ok(())
    }

    /// The predicate `rename.rs` and `references.rs` share (#14757).
    ///
    /// Each caller kept its own copy of this match until the two were lifted
    /// here. The cases below are the union of what those copies answered, so a
    /// divergence in either direction is a red test rather than a provider that
    /// quietly misbehaves for one surface only.
    ///
    /// The document is deliberately multi-line and every offset is off line one:
    /// the predicate derives its own line window, and the callers that used to do
    /// that themselves must not lose the whole-line requirement in the move.
    ///
    /// Each case pins the component it classifies to before asserting the
    /// predicate's answer. Without that, an offset can silently land on a
    /// different arm than its comment claims -- `new` in `Some::Module->new()`
    /// looks like a final component but carries no `::`, so it classifies as
    /// `None` -- and the case then proves nothing about the arm it names.
    #[test]
    fn shared_off_symbol_predicate_answers_for_rename_and_references()
    -> Result<(), Box<dyn std::error::Error>> {
        let regex = get_fqn_regex()?;
        let text = "use Some::Module;\nSome::Module->new();\nAlpha::target();\n";
        let line2 = text.find("Some::Module->new").ok_or("fixture line 2")?;
        let line3 = text.find("Alpha::target").ok_or("fixture line 3")?;
        let arm = |offset: usize, line_start: usize| {
            let (_, line_text) = crate::util::line_window_around_offset(text, offset);
            fqn_component_at_cursor(regex, line_text, offset - line_start)
        };

        // Arrow receiver: the qualified-name match stops at the `->`, so `Module`
        // is that match's *final* component while the resolved symbol is the
        // method `new`. Off the named symbol -- the wrong-symbol edit this
        // predicate exists to refuse. A predicate keyed on `Prefix` alone
        // answers `false` here.
        assert!(matches!(arm(line2 + 6, line2), Some(FqnCursorComponent::Final { .. })));
        assert!(
            cursor_is_off_named_symbol(text, line2 + 6, Some("new")),
            "a cursor on the arrow receiver is off the method the key names"
        );
        // The method itself carries no `::`, so it is not inside a qualified
        // match at all. The predicate must not refuse it -- this is the one
        // position on this line rename has to keep working.
        assert!(arm(line2 + 14, line2).is_none());
        assert!(
            !cursor_is_off_named_symbol(text, line2 + 14, Some("new")),
            "a cursor on the method names the symbol being acted on"
        );

        // Package prefix of a qualified call: never names the callable.
        assert!(matches!(arm(line3 + 1, line3), Some(FqnCursorComponent::Prefix)));
        assert!(
            cursor_is_off_named_symbol(text, line3 + 1, Some("target")),
            "a cursor on a package prefix is off the sub it resolves to"
        );
        // Final component agreeing with the resolved name: on the symbol.
        // Refusing every `Final` would break rename here.
        assert!(matches!(arm(line3 + 8, line3), Some(FqnCursorComponent::Final { .. })));
        assert!(
            !cursor_is_off_named_symbol(text, line3 + 8, Some("target")),
            "a cursor on the final component names the sub"
        );

        // Not inside a `::`-qualified match at all: not a question this
        // predicate answers, so it must not refuse. `references.rs` reaches this
        // for every unqualified cursor.
        assert!(
            !cursor_is_off_named_symbol("my $x = 1;\n", 4, Some("x")),
            "an unqualified cursor is not classified as off the symbol"
        );

        // `rename.rs` alone reaches an unresolved cursor: `references.rs` only
        // calls the predicate with a resolved bare sub key. A final component
        // with nothing to disagree with is not a disagreement, so the union arm
        // must stay `false` -- refusing here would refuse rename wherever
        // resolution came back empty on a qualified line.
        assert!(
            !cursor_is_off_named_symbol(text, line3 + 8, None),
            "with no resolved symbol a final component reports no disagreement"
        );
        // A prefix stays off the symbol even unresolved: it never names a
        // callable regardless of what resolution found.
        assert!(
            cursor_is_off_named_symbol(text, line3 + 1, None),
            "a prefix component is off the symbol independently of resolution"
        );
        Ok(())
    }

    /// Regression: a stale workspace index must not re-enable the wrong jump the
    /// package-prefix guard exists to prevent.
    ///
    /// With the cursor on `Foo` in `Foo::bar`, `DeclarationProvider` and
    /// `symbol_at_cursor_with_source` both extract the LAST component (`bar`)
    /// regardless of cursor position, so falling through to them navigates to
    /// `sub bar` — a confidently wrong target. `handle_definition_inner`
    /// therefore returns `Ok(None)` for a prefix cursor.
    ///
    /// That guard used to live inside the workspace-index freshness gate, so an
    /// unrelated edited buffer with a stale index entry skipped the whole block
    /// and let the prefix cursor reach `DeclarationProvider`. This asserts the
    /// guard is evaluated on the stale path too: the answer must be empty, never
    /// a location on the `bar` line.
    #[cfg(feature = "workspace")]
    #[test]
    fn definition_on_package_prefix_stays_empty_while_workspace_index_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let main_uri = "file:///workspace/prefix-main.pl";
        let main_text = "package Foo;\nsub bar { return 1; }\npackage main;\nFoo::bar();\n";
        let unrelated_uri = "file:///workspace/prefix-unrelated.pl";
        let unrelated_text = "package Unrelated;\nsub helper {}\n";

        server.test_apply_did_open(main_uri, main_text, 1)?;
        server.test_apply_did_open(unrelated_uri, unrelated_text, 1)?;

        // Make an *unrelated* open buffer stale: indexed at generation 0, then
        // edited to generation 1 without re-indexing. The caller document is
        // untouched, so only the any-open-document gate can see this.
        let coordinator = server
            .index_coordinator
            .as_ref()
            .ok_or("test server must have an index coordinator")?;
        coordinator
            .index()
            .index_file_with_generation(
                url::Url::parse(unrelated_uri)?,
                unrelated_text.to_string(),
                0,
            )
            .map_err(std::io::Error::other)?;
        server
            .test_replace_document_without_index(
                unrelated_uri,
                "package Unrelated;\nsub renamed {}\n",
                2,
            )
            .map_err(std::io::Error::other)?;

        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "the edited unrelated buffer must put the workspace index in the stale state \
             this regression is about"
        );

        // Cursor on the `Foo` prefix of `Foo::bar` (line 3, character 1).
        let result = server.test_handle_definition(Some(json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 3, "character": 1 }
        })))?;

        let locations = result
            .as_ref()
            .and_then(|value| value.as_array())
            .map_or_else(Vec::new, |array| array.to_vec());
        let bar_line = 1;
        assert!(
            !locations.iter().any(|location| {
                location.pointer("/range/start/line").and_then(Value::as_u64) == Some(bar_line)
            }),
            "a prefix cursor must never resolve to the final component `bar` \
             (line {bar_line}); got {result:?}"
        );
        assert!(
            locations.is_empty(),
            "a package-prefix cursor must yield an empty answer, not a guessed target; \
             got {result:?}"
        );

        Ok(())
    }

    /// Regression (#5016 item 2): stale workspace index must not run definition
    /// semantic shadow queries even when the request document's generation matches.
    #[cfg(feature = "workspace")]
    #[test]
    fn definition_semantic_shadow_skips_stale_workspace_index_tier()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let main_uri = "file:///workspace/shadow-main.pl";
        let main_text = "package Foo;\nsub bar { return 1; }\npackage main;\nFoo::bar();\n";
        let unrelated_uri = "file:///workspace/shadow-unrelated.pl";
        let unrelated_text = "package Unrelated;\nsub helper {}\n";

        server.test_apply_did_open(main_uri, main_text, 1)?;
        server.test_apply_did_open(unrelated_uri, unrelated_text, 1)?;
        server
            .test_index_file_in_building_state(main_uri, main_text)
            .map_err(std::io::Error::other)?;
        server
            .test_index_file_in_building_state(unrelated_uri, unrelated_text)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        let fresh = server.test_handle_definition(Some(json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 3, "character": 5 }
        })))?;
        assert!(
            fresh.as_ref().and_then(Value::as_array).is_some_and(|locations| !locations.is_empty()),
            "fresh index should resolve Foo::bar call target: {fresh:?}"
        );
        let fresh_explanation = server
            .handle_execute_command(Some(json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": "goto_definition"}]
            })))?
            .ok_or("missing explain-provider-decision response")?;
        assert!(
            fresh_explanation
                .get("request_receipt")
                .and_then(|receipt| receipt.get("semantic_shadow_receipt"))
                .is_some(),
            "fresh index should persist definition semantic shadow receipt"
        );

        server
            .test_replace_document_without_index(
                unrelated_uri,
                "package Unrelated;\nsub renamed {}\n",
                2,
            )
            .map_err(std::io::Error::other)?;
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "edited unrelated buffer must stale the workspace index"
        );

        let _ = server.test_handle_definition(Some(json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 3, "character": 5 }
        })))?;
        let stale_explanation = server
            .handle_execute_command(Some(json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": "goto_definition"}]
            })))?
            .ok_or("missing explain-provider-decision response")?;
        assert!(
            stale_explanation
                .get("request_receipt")
                .and_then(|receipt| receipt.get("semantic_shadow_receipt"))
                .is_none(),
            "stale workspace index must not persist definition semantic shadow receipt"
        );

        Ok(())
    }

    /// Serializes tests in this module that touch
    /// `NAVIGATION_SAME_DOC_FALLBACK_GAP`, mirroring `toctou_hook_lock` in
    /// `tests/navigation_same_document_toctou_regression_tests.rs`: any call
    /// into `handle_type_definition`/`handle_implementation` unconditionally
    /// drains the hook slot via `wait_at_same_doc_fallback_gap`, so two
    /// tests touching it concurrently (this crate's own guidance is
    /// `--test-threads=2`, not 1) could steal each other's armed hook.
    /// Self-heals from a poisoned lock, matching `timing::capture::test_lock`.
    fn same_doc_fallback_gap_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Verifies that `handle_implementation` executes the workspace
    /// index-readiness wait when indexing is in progress (#3095).
    ///
    /// The wait short-circuits immediately because the coordinator is Ready
    /// by default, but the line must execute to satisfy patch coverage.
    #[cfg(feature = "workspace")]
    #[test]
    fn test_wait_guard_fires_in_handle_implementation_when_indexing_in_progress() {
        let _serial = same_doc_fallback_gap_test_lock();

        let server = LspServer::new();
        let uri = "file:///test-impl-race.pl";
        let open_result = server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "package Foo;\nsub run { }\n",
            }
        })));
        assert!(open_result.is_ok(), "didOpen failed: {open_result:?}");
        // Simulate the race window: flag is set but coordinator is already Ready.
        // The wait exits immediately on the first Ready check.
        server.test_simulate_indexing_start();
        let result = server.handle_implementation(Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 4 }
        })));
        assert!(result.is_ok(), "handle_implementation must not error: {result:?}");
    }

    /// Regression (#5016): when the workspace index is stale relative to an open
    /// document, `handle_implementation` must not return implementors from the
    /// outdated index tier (open-document AST scan may still answer).
    #[cfg(feature = "workspace")]
    #[test]
    fn implementation_skips_stale_workspace_index_tier() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let base_uri = "file:///workspace/stale_impl_base.pm";
        let derived_uri = "file:///workspace/stale_impl_derived.pm";
        let base_text = "package StaleImpl::Base;\nsub new { bless {}, shift }\n1;\n";
        let derived_v1 = "package StaleImpl::Derived;\nuse parent 'StaleImpl::Base';\n1;\n";
        let derived_v2 = "package StaleImpl::Derived;\n1;\n";

        server.test_apply_did_open(base_uri, base_text, 1)?;
        server.test_apply_did_open(derived_uri, derived_v1, 1)?;
        server
            .test_index_file_in_building_state(base_uri, base_text)
            .map_err(std::io::Error::other)?;
        server
            .test_index_file_in_building_state(derived_uri, derived_v1)
            .map_err(std::io::Error::other)?;
        server.test_simulate_indexing_complete();

        // Cursor on the `Base` package identifier.
        let fresh = server.handle_implementation(Some(json!({
            "textDocument": { "uri": base_uri },
            "position": { "line": 0, "character": 19 }
        })))?;
        let fresh_locations = fresh.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        assert!(
            fresh_locations.iter().any(|loc| {
                loc.get("targetUri")
                    .and_then(|u| u.as_str())
                    .is_some_and(|uri| uri.contains("stale_impl_derived"))
            }),
            "fresh workspace index should return Derived implementor: {fresh_locations:?}"
        );

        server
            .test_replace_document_without_index(derived_uri, derived_v2, 2)
            .map_err(std::io::Error::other)?;
        assert!(
            server.workspace_index_stale_for_any_open_document(),
            "test setup must leave the workspace index stale relative to open documents"
        );

        let stale = server.handle_implementation(Some(json!({
            "textDocument": { "uri": base_uri },
            "position": { "line": 0, "character": 19 }
        })))?;
        let stale_locations = stale.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        assert!(
            !stale_locations.iter().any(|loc| {
                loc.get("targetUri")
                    .and_then(|u| u.as_str())
                    .is_some_and(|uri| uri.contains("stale_impl_derived"))
            }),
            "stale workspace index must not return removed parent relationship: {stale_locations:?}"
        );

        Ok(())
    }

    /// Verifies `wait_at_same_doc_fallback_gap`'s poison-recovery path
    /// (#3613): a previously panicked holder of
    /// `NAVIGATION_SAME_DOC_FALLBACK_GAP` must not silently disable the
    /// synchronization point for a later caller. Deliberately poisons the
    /// static mutex (a thread panics while holding its lock), then confirms
    /// the recovery branch (`Err(poisoned) => poisoned.into_inner().take()`)
    /// still lets an armed hook fire instead of the `.lock().ok()` pattern
    /// this replaces, which would turn the poisoned `Err` into `None` and
    /// silently skip the pause.
    ///
    /// The deliberate `panic!` (to poison the mutex) and the `.expect()`
    /// calls (to fail loudly, with a diagnosing message, if the recovery
    /// path regresses) are the point of this test, not banned production
    /// patterns creeping in -- narrowly allowed here the same way
    /// `LazyLock` regex initializers are elsewhere in this codebase.
    ///
    /// Takes `same_doc_fallback_gap_test_lock()` for the same reason
    /// `test_wait_guard_fires_in_handle_implementation_when_indexing_in_progress`
    /// above does: any concurrently running test that reaches
    /// `wait_at_same_doc_fallback_gap` could otherwise steal the hook this
    /// test arms. Calls `clear_poison()` at the end so a poisoned mutex from
    /// this deliberate test doesn't linger for the rest of the process --
    /// every access site already recovers poison correctly, so this is
    /// hygiene, not a correctness requirement.
    #[test]
    #[allow(clippy::panic, clippy::expect_used)]
    fn test_wait_at_same_doc_fallback_gap_recovers_from_poisoned_mutex() {
        let _serial = same_doc_fallback_gap_test_lock();

        // Poison the static by panicking while holding its lock on another
        // thread. `thread::spawn` catches the panic and reports it via
        // `join()`'s `Err`, so this does not abort the test process.
        let poison_result = std::thread::spawn(|| {
            let _guard = NAVIGATION_SAME_DOC_FALLBACK_GAP.lock();
            panic!("deliberately poisoning the hook mutex for #3613 test coverage");
        })
        .join();
        assert!(poison_result.is_err(), "the poisoning thread must have panicked");
        assert!(NAVIGATION_SAME_DOC_FALLBACK_GAP.lock().is_err(), "the mutex must now be poisoned");

        // Arm the hook directly, recovering the poisoned guard the same way
        // `wait_at_same_doc_fallback_gap` does -- this unit test lives in
        // the same module, so it can reach the private static directly.
        let (reached_tx, reached_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        {
            let mut hook = match NAVIGATION_SAME_DOC_FALLBACK_GAP.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            *hook = Some((reached_tx, resume_rx));
        }

        // The function under test must recover the still-poisoned mutex and
        // still fire the hook -- not silently no-op like `.lock().ok()` would.
        let handler = std::thread::spawn(wait_at_same_doc_fallback_gap);
        reached_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("poison recovery must still let the hook fire `reached`");
        resume_tx.send(()).expect("resume channel must still be open");
        handler.join().expect(
            "wait_at_same_doc_fallback_gap must not panic after recovering a poisoned lock",
        );

        // Hygiene: clear the poison this test deliberately introduced so it
        // doesn't linger for the rest of the process. Every access site
        // already recovers poison correctly (that's what this test proves),
        // so this is not required for correctness -- it just keeps the
        // mutex's poisoned flag from being permanently true after this test
        // runs, matching "no test leaving the mutex poisoned".
        NAVIGATION_SAME_DOC_FALLBACK_GAP.clear_poison();
    }
}
