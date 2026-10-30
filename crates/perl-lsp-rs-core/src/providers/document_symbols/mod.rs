//! Document-symbol provider proof and source-backed live helpers.
//!
//! The runtime owns LSP request handling and fallback. This module owns the
//! compiler-shaped document-symbol facts that are safe to promote live:
//! fresh, high-confidence, source-backed syntax symbols, plus shadow receipts
//! for generated, stale, and dynamic candidates that must stay gated.

use perl_parser_core::ast::Node;
use perl_position_tracking::WireRange;
use perl_semantic_analyzer::symbol::{ScopeKind, Symbol, SymbolExtractor, SymbolKind, SymbolTable};
use perl_semantic_facts::{
    AnchorId, Confidence, Provenance, ProviderFactFreshness, ProviderFactSourceKind,
    ProviderFactTrace, ProviderFallbackState, ProviderSurface,
};
use perl_symbol::{SymbolDecl, extract_symbol_decls};
use perl_workspace::semantic_shadow_compare::{
    SemanticShadowCompareReceipt, ShadowQueryInput, ShadowQueryName, summarize_identities,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// LSP-compatible document symbol produced from source-backed compiler facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DocumentSymbol {
    /// Display name shown by the client.
    pub name: String,
    /// Optional symbol detail text.
    pub detail: String,
    /// LSP `SymbolKind` numeric value.
    pub kind: u32,
    /// Full source-backed range for the symbol.
    pub range: WireRange,
    /// Source-backed selection range for the symbol name.
    pub selection_range: WireRange,
    /// Nested source-backed symbols.
    pub children: Vec<DocumentSymbol>,
    /// Internal assembler ordering fact; never emitted on the LSP wire.
    #[serde(skip)]
    pub(crate) sort_priority: u8,
}

/// Assembler priority shared by source-backed callables and synthesized subtests.
pub(crate) const CALLABLE_DOCUMENT_SYMBOL_PRIORITY: u8 = 3;

// Join the compatibility outline to existing canonical declaration records.
// A conflicting duplicate is retained as None and fails closed; no new span
// model or presentation-parent identity is introduced.
type DeclarationNames<'a> = HashMap<(SymbolKind, usize, usize, &'a str), Option<&'a SymbolDecl>>;

/// Source-backed document-symbol live result.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DocumentSymbolLiveResult {
    /// Symbols safe to return live.
    pub symbols: Vec<DocumentSymbol>,
    /// Fact traces proving source/provenance/confidence/freshness.
    pub fact_traces: Vec<ProviderFactTrace>,
}

/// Build live document symbols from fresh, source-backed parser/compiler facts.
///
/// This helper deliberately excludes virtual generated members, stale facts,
/// dynamic boundaries, and ambiguous/no-source candidates. The runtime keeps
/// its existing fallback behavior when parsing is unavailable.
#[must_use]
pub fn source_backed_document_symbols_from_ast(
    ast: &Node,
    source: &str,
) -> DocumentSymbolLiveResult {
    let extractor = SymbolExtractor::new_with_source(source);
    let symbol_table = extractor.extract(ast);
    let declarations = extract_symbol_decls(ast, None);
    let mut declaration_names: DeclarationNames<'_> = HashMap::new();
    for declaration in &declarations {
        // ADJUST blocks deliberately use a Method node without a name span.
        // Keep that unported block shape on the compatibility path; a named
        // `method ADJUST` has an anchor and participates in the exact join.
        if declaration.kind == SymbolKind::Method
            && declaration.name == "ADJUST"
            && declaration.anchor_span.is_none()
        {
            continue;
        }
        if matches!(
            declaration.kind,
            SymbolKind::Package | SymbolKind::Subroutine | SymbolKind::Method
        ) {
            let key = (
                declaration.kind,
                declaration.full_span.0,
                declaration.full_span.1,
                declaration.name.as_str(),
            );
            declaration_names
                .entry(key)
                .and_modify(|previous| {
                    if previous.is_some_and(|record| record.anchor_span != declaration.anchor_span)
                    {
                        *previous = None;
                    }
                })
                .or_insert(Some(declaration));
        }
    }
    source_backed_document_symbols_from_table(&symbol_table, source, &declaration_names)
}

fn is_namespace_container(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role)
}

fn is_container(symbol: &Symbol) -> bool {
    is_namespace_container(symbol)
        || matches!(symbol.kind, SymbolKind::Subroutine | SymbolKind::Method)
}

/// A framework namespace symbol synthesized by the extractor at an import
/// or call site (e.g. the `EV` class from `use EV`, or the
/// `Plack::Middleware::*` package from `enable 'Session'`). The extractor
/// marks these with a `framework=<Name>` declaration or attribute; real
/// package/class/role declarations carry neither. A synthesized symbol
/// declares no package region: it must not terminate the enclosing
/// statement package's extent or absorb following symbols.
fn is_synthesized_framework(symbol: &Symbol) -> bool {
    symbol.declaration.as_deref().is_some_and(|d| d.starts_with("framework="))
        || symbol.attributes.iter().any(|a| a.starts_with("framework="))
}

/// Extent of a namespace container, as byte offsets.
///
/// A block-form package/class has a `ScopeKind::Package` scope whose
/// location starts at the declaration; that scope location already covers
/// the whole `package Foo { ... }` / `class Foo { ... }` construct, so it is
/// the extent. Statement-form packages create no scope, so their extent is
/// derived from the sorted namespace starts: it runs from the declaration
/// to the next namespace container declared in the same enclosing scope
/// (`scope_id`), or to the end of that enclosing scope (global scope has
/// end 0, which falls back to EOF).
fn namespace_extents(
    symbols: &[Symbol],
    symbol_table: &SymbolTable,
    source: &str,
) -> Vec<(usize, usize)> {
    let mut extents: Vec<(usize, usize)> =
        symbols.iter().map(|s| (s.location.start, s.location.end)).collect();
    let namespace: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| is_namespace_container(s))
        .map(|(idx, _)| idx)
        .collect();
    // A block-form container does not terminate an enclosing statement
    // package: after `package Inner { ... }` the outer package resumes.
    let block_extents: Vec<Option<usize>> = namespace
        .iter()
        .map(|&idx| {
            let start = symbols[idx].location.start;
            symbol_table
                .scopes
                .values()
                .filter(|scope| {
                    scope.kind == ScopeKind::Package
                        && scope.location.start == start
                        && scope.location.end > start
                })
                .map(|scope| scope.location.end)
                .max()
        })
        .collect();
    for (position, &idx) in namespace.iter().enumerate() {
        let symbol = &symbols[idx];
        let start = symbol.location.start;
        if is_synthesized_framework(symbol) {
            // Synthesized framework namespaces own only their import/call
            // span; they are not region boundaries (handled by the
            // next_same_level filter below as well).
            extents[idx] = (start, symbol.location.end.min(source.len()));
            continue;
        }
        if let Some(block_end) = block_extents[position] {
            extents[idx] = (start, block_end.max(symbol.location.end).min(source.len()));
            continue;
        }
        let enclosing_end = symbol_table
            .scopes
            .get(&symbol.scope_id)
            .map(|scope| scope.location.end)
            .filter(|end| *end > start)
            .unwrap_or(source.len());
        let next_same_level = namespace
            .iter()
            .enumerate()
            .filter(|(other_position, _)| block_extents[*other_position].is_none())
            .map(|(_, &other)| other)
            .filter(|&other| other != idx)
            .filter(|&other| !is_synthesized_framework(&symbols[other]))
            .filter(|&other| {
                symbols[other].scope_id == symbol.scope_id && symbols[other].location.start > start
            })
            .map(|other| symbols[other].location.start)
            .min()
            .unwrap_or(enclosing_end);
        let end = enclosing_end.min(next_same_level).max(symbol.location.end).min(source.len());
        extents[idx] = (start, end);
    }
    extents
}

struct PendingSymbol {
    document_symbol: DocumentSymbol,
    start: usize,
    end: usize,
    extent: (usize, usize),
    container: bool,
}

fn sort_pending_indices(indices: &mut [usize], pending: &[Option<PendingSymbol>]) {
    // #18111: (priority, start, end, name) ties must not fall through to the
    // HashMap-derived bucket order of the flattened symbol table; `kind` is
    // the final deterministic tiebreak. Order only — no entry or kind changes.
    indices.sort_by(|&a, &b| {
        let (Some(left), Some(right)) = (&pending[a], &pending[b]) else {
            return std::cmp::Ordering::Equal;
        };
        (
            left.document_symbol.sort_priority,
            left.start,
            left.end,
            left.document_symbol.name.as_str(),
            left.document_symbol.kind,
        )
            .cmp(&(
                right.document_symbol.sort_priority,
                right.start,
                right.end,
                right.document_symbol.name.as_str(),
                right.document_symbol.kind,
            ))
    });
}

fn assemble_document_symbol(
    index: usize,
    pending: &mut [Option<PendingSymbol>],
    children_by_parent: &[Vec<usize>],
) -> Option<DocumentSymbol> {
    let mut node = pending[index].take()?;
    let mut child_indices = children_by_parent[index].clone();
    sort_pending_indices(&mut child_indices, pending);
    let mut children = Vec::new();
    for child_index in child_indices {
        if let Some(child) = assemble_document_symbol(child_index, pending, children_by_parent) {
            children.push(child);
        }
    }
    node.document_symbol.children = children;
    Some(node.document_symbol)
}

fn source_backed_document_symbols_from_table(
    symbol_table: &SymbolTable,
    source: &str,
    declaration_names: &DeclarationNames<'_>,
) -> DocumentSymbolLiveResult {
    let mut all_symbols: Vec<Symbol> = symbol_table
        .symbols
        .values()
        .flat_map(|symbols| symbols.iter().cloned())
        .filter(|symbol| is_source_backed(symbol, source))
        .collect();
    all_symbols.sort_by(|a, b| {
        (a.location.start, a.location.end, a.name.as_str(), a.scope_id).cmp(&(
            b.location.start,
            b.location.end,
            b.name.as_str(),
            b.scope_id,
        ))
    });

    // #18111: a `has` attribute and its synthesized same-name accessor share
    // (name, span, scope); keep only the attribute in the outline. Accessors
    // with a distinct name/span and explicit subs are unaffected. The symbol
    // table itself is untouched, so definition/hover still see the accessor.
    {
        let attribute_keys: HashSet<(String, usize, usize, usize)> = all_symbols
            .iter()
            .filter(|s| s.declaration.as_deref() == Some("has") && s.kind == SymbolKind::scalar())
            .map(|s| (s.name.clone(), s.location.start, s.location.end, s.scope_id))
            .collect();
        if !attribute_keys.is_empty() {
            all_symbols.retain(|s| {
                !(s.declaration.as_deref() == Some("has")
                    && s.kind == SymbolKind::Subroutine
                    && attribute_keys.contains(&(
                        s.name.clone(),
                        s.location.start,
                        s.location.end,
                        s.scope_id,
                    )))
            });
        }
    }

    let extents = namespace_extents(&all_symbols, symbol_table, source);

    let mut fact_traces = Vec::new();
    let mut pending: Vec<Option<PendingSymbol>> = Vec::new();
    for (symbol, extent) in all_symbols.iter().zip(extents.iter()) {
        let range_override = if is_namespace_container(symbol) { Some(*extent) } else { None };
        if let Some(document_symbol) = source_backed_document_symbol(
            symbol,
            source,
            declaration_names,
            &mut fact_traces,
            range_override,
        ) {
            pending.push(Some(PendingSymbol {
                document_symbol,
                start: symbol.location.start,
                end: symbol.location.end,
                extent: *extent,
                container: is_container(symbol),
            }));
        } else {
            // Keep indices aligned with the extent/parent computation.
            pending.push(None);
        }
    }

    // Containers in a deterministic order for tie-breaking equal extents.
    let mut containers: Vec<usize> = pending
        .iter()
        .enumerate()
        .filter(|(_, node)| node.as_ref().is_some_and(|node| node.container))
        .map(|(idx, _)| idx)
        .collect();
    containers.sort_by(|&a, &b| {
        let (Some(left), Some(right)) = (&pending[a], &pending[b]) else {
            return std::cmp::Ordering::Equal;
        };
        (left.extent.0, std::cmp::Reverse(left.extent.1), left.document_symbol.name.as_str()).cmp(
            &(
                right.extent.0,
                std::cmp::Reverse(right.extent.1),
                right.document_symbol.name.as_str(),
            ),
        )
    });
    let mut container_rank: Vec<Option<usize>> = vec![None; pending.len()];
    for (rank, idx) in containers.iter().enumerate() {
        container_rank[*idx] = Some(rank);
    }

    // Parent each symbol under the innermost container containing it.
    // Containers must be fully contained by their parent (with an earlier
    // rank breaking equal-extent ties) so the result is a forest.
    let mut children_by_parent: Vec<Vec<usize>> = vec![Vec::new(); pending.len()];
    let mut roots: Vec<usize> = Vec::new();
    for index in 0..pending.len() {
        let Some(node) = &pending[index] else { continue };
        let (node_start, node_extent, node_container) = (node.start, node.extent, node.container);
        let mut best: Option<usize> = None;
        let mut best_key: Option<(usize, std::cmp::Reverse<usize>, usize)> = None;
        for &candidate in &containers {
            if candidate == index {
                continue;
            }
            let Some(parent) = &pending[candidate] else { continue };
            if parent.extent.0 > node_start || node_start >= parent.extent.1 {
                continue;
            }
            if !node_container && node.end > parent.extent.1 {
                // A leaf that starts inside but extends past the candidate
                // parent is not contained by it; leave it for a wider
                // container or the top level instead of misparenting it.
                continue;
            }
            if node_container {
                if parent.extent.0 > node_extent.0 || node_extent.1 > parent.extent.1 {
                    continue;
                }
                if parent.extent == node_extent && container_rank[candidate] > container_rank[index]
                {
                    continue;
                }
            }
            let key = (
                parent.extent.1 - parent.extent.0,
                std::cmp::Reverse(parent.extent.0),
                container_rank[candidate].unwrap_or(usize::MAX),
            );
            if best_key.is_none_or(|current| key < current) {
                best_key = Some(key);
                best = Some(candidate);
            }
        }
        match best {
            Some(parent) => children_by_parent[parent].push(index),
            None => roots.push(index),
        }
    }

    // Top level stays in source order; only children use the priority sort.
    roots.sort_by(|&a, &b| {
        let (Some(left), Some(right)) = (&pending[a], &pending[b]) else {
            return std::cmp::Ordering::Equal;
        };
        (left.start, left.end, left.document_symbol.name.as_str()).cmp(&(
            right.start,
            right.end,
            right.document_symbol.name.as_str(),
        ))
    });
    let mut output = Vec::new();
    for root in roots {
        if let Some(symbol) = assemble_document_symbol(root, &mut pending, &children_by_parent) {
            output.push(symbol);
        }
    }

    DocumentSymbolLiveResult { symbols: output, fact_traces }
}

fn source_backed_document_symbol(
    symbol: &Symbol,
    source: &str,
    declaration_names: &DeclarationNames<'_>,
    fact_traces: &mut Vec<ProviderFactTrace>,
    range_override: Option<(usize, usize)>,
) -> Option<DocumentSymbol> {
    if !is_source_backed(symbol, source) {
        return None;
    }
    let selection_range = symbol_name_range(source, symbol, declaration_names)?;

    fact_traces.push(source_backed_trace());
    Some(DocumentSymbol {
        name: document_symbol_name(symbol),
        detail: document_symbol_detail(symbol),
        kind: document_symbol_kind(symbol),
        range: range_override
            .map(|(start, end)| WireRange::from_byte_offsets(source, start, end))
            .unwrap_or_else(|| symbol_range(source, symbol)),
        selection_range,
        children: Vec::new(),
        sort_priority: document_symbol_priority(symbol),
    })
}

fn source_backed_trace() -> ProviderFactTrace {
    ProviderFactTrace::new(
        ProviderSurface::DocumentSymbols,
        ProviderFactSourceKind::ParserSyntax,
        Provenance::ExactAst,
        Confidence::High,
        ProviderFactFreshness::Fresh,
        ProviderFallbackState::Primary,
        None,
        None,
        Some(1),
    )
}

fn is_source_backed(symbol: &Symbol, source: &str) -> bool {
    symbol.location.start <= symbol.location.end && symbol.location.end <= source.len()
}

fn symbol_range(source: &str, symbol: &Symbol) -> WireRange {
    WireRange::from_byte_offsets(source, symbol.location.start, symbol.location.end)
}

/// Project existing canonical package/subroutine/named-method spans. Known missing,
/// conflicting or invalid anchors cannot fall back to source-search inference.
fn symbol_name_range(
    source: &str,
    symbol: &Symbol,
    declaration_names: &DeclarationNames<'_>,
) -> Option<WireRange> {
    // The compatibility extractor reclassifies a framework `package` as
    // Class or Role, while its canonical declaration record stays Package
    // (#17924). A reclassified namespace therefore retries the Package key
    // with the same exact name and full span; the fail-closed handling of a
    // present record (conflicting/missing/invalid anchor) is unchanged.
    // Native class/role declarations have no Package record at the same
    // span, so they still take the compatibility path below.
    let lookup_kinds: &[SymbolKind] = match symbol.kind {
        SymbolKind::Class | SymbolKind::Role => &[symbol.kind, SymbolKind::Package],
        _ => &[symbol.kind],
    };
    for &lookup_kind in lookup_kinds {
        let key = (lookup_kind, symbol.location.start, symbol.location.end, symbol.name.as_str());
        if let Some(declaration) = declaration_names.get(&key) {
            let declaration = (*declaration)?;
            let (start, end) = declaration.anchor_span?;
            if start >= end
                || start < declaration.full_span.0
                || end > declaration.full_span.1
                || source.get(start..end).is_none()
            {
                return None;
            }
            return Some(WireRange::from_byte_offsets(source, start, end));
        }
    }
    // Compatibility for unported declaration families and synthesized symbols
    // (phasers, typeglob callables, framework members, etc.). This bounded slice
    // does not claim to complete their canonical-span migration under #8183.
    Some(legacy_symbol_name_range(source, symbol))
}

/// Compatibility name inference for unported declaration families.
///
/// Per LSP 3.17, `selectionRange` must pinpoint the symbol name alone (e.g. just `greet` in
/// `sub greet { ... }`), while `range` covers the entire construct.  This helper performs a
/// byte-string search for `symbol.name` within the source slice bounded by `symbol.location`.
///
/// # Fallback
/// If the name cannot be located within the bounds (malformed source, empty name, out-of-range
/// location), the function falls back to `symbol_range()` — the full declaration span — so that
/// clients always receive a valid, non-panicking range.
fn legacy_symbol_name_range(source: &str, symbol: &Symbol) -> WireRange {
    if symbol.name.is_empty() {
        return symbol_range(source, symbol);
    }
    let slice = match source.get(symbol.location.start..symbol.location.end) {
        Some(s) => s,
        None => return symbol_range(source, symbol),
    };
    let rel_offset = match slice.find(symbol.name.as_str()) {
        Some(o) => o,
        None => return symbol_range(source, symbol),
    };
    let abs_start = symbol.location.start + rel_offset;
    let abs_end = abs_start + symbol.name.len();
    // Guard: name end must not exceed the symbol's declared bounds.
    if abs_end > symbol.location.end {
        return symbol_range(source, symbol);
    }
    WireRange::from_byte_offsets(source, abs_start, abs_end)
}

fn document_symbol_kind(symbol: &Symbol) -> u32 {
    if symbol.declaration.as_deref() == Some("has") && symbol.kind == SymbolKind::scalar() {
        7
    } else {
        symbol.kind.to_lsp_kind_document_symbol()
    }
}

fn document_symbol_name(symbol: &Symbol) -> String {
    if symbol.declaration.as_deref() == Some("has") {
        symbol.name.clone()
    } else if let Some(sigil) = symbol.kind.sigil() {
        format!("{}{}", sigil, symbol.name)
    } else {
        symbol.name.clone()
    }
}

fn document_symbol_detail(symbol: &Symbol) -> String {
    if symbol.declaration.as_deref() == Some("has") && symbol.kind == SymbolKind::scalar() {
        if !symbol.attributes.is_empty() {
            symbol.attributes.join(", ")
        } else {
            symbol.documentation.clone().unwrap_or_default()
        }
    } else if symbol.declaration.as_deref() == Some("has") {
        symbol.documentation.clone().unwrap_or_default()
    } else {
        symbol.declaration.as_deref().unwrap_or("").to_string()
    }
}

fn document_symbol_priority(symbol: &Symbol) -> u8 {
    if symbol.declaration.as_deref() == Some("has") && symbol.kind == SymbolKind::scalar() {
        0
    } else if symbol.declaration.as_deref() == Some("has") {
        2
    } else if matches!(symbol.kind, SymbolKind::Package | SymbolKind::Class | SymbolKind::Role) {
        1
    } else if symbol.kind.is_callable() {
        CALLABLE_DOCUMENT_SYMBOL_PRIORITY
    } else {
        4
    }
}

/// Legacy document-symbol identity considered by the shadow proof.
///
/// This is not a live LSP response type. The identity should be stable enough
/// to compare the existing provider result against compiler-fact candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocumentSymbolShadowLegacy {
    /// Stable identity for deterministic receipt comparison.
    pub identity: String,
}

/// Compiler-fact candidate considered by the document-symbol shadow proof.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DocumentSymbolShadowCandidate {
    /// Stable identity for deterministic receipt comparison.
    pub identity: String,
    /// Fact source that produced the candidate.
    pub source: ProviderFactSourceKind,
    /// Semantic provenance for the candidate.
    pub provenance: Provenance,
    /// Confidence in the candidate.
    pub confidence: Confidence,
    /// Freshness of the candidate relative to the request.
    pub freshness: ProviderFactFreshness,
    /// Whether the candidate is shadowed, fallback, or blocked.
    pub fallback_state: ProviderFallbackState,
    /// Optional source hash for fact freshness proof.
    pub source_hash: Option<String>,
    /// Optional semantic anchor for the candidate.
    pub anchor_id: Option<AnchorId>,
    /// Optional producer model version.
    pub model_version: Option<u32>,
}

/// Document-symbol shadow proof result.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DocumentSymbolShadowResult {
    /// Legacy symbols returned by the existing runtime provider path.
    pub legacy_symbols: Vec<DocumentSymbolShadowLegacy>,
    /// Shadow receipt comparing legacy symbols with compiler-fact candidates.
    pub receipt: SemanticShadowCompareReceipt,
}

/// Compare legacy document-symbol output against compiler-fact candidates.
///
/// This function is intentionally shadow-only: it returns the original legacy
/// symbols unchanged and emits a receipt that records source, provenance,
/// confidence, freshness, and fallback/blocker state for candidate facts.
#[must_use]
pub fn document_symbol_source_shadow(
    legacy_symbols: Vec<DocumentSymbolShadowLegacy>,
    compiler_candidates: Vec<DocumentSymbolShadowCandidate>,
    symbol: &str,
) -> DocumentSymbolShadowResult {
    let old_result = summarize_identities(Some(
        legacy_symbols.iter().map(|symbol| symbol.identity.clone()).collect(),
    ));
    let new_result =
        summarize_identities(Some(document_symbol_answer_identities(&compiler_candidates)));
    let notes = vec![document_symbol_shadow_note(&legacy_symbols, &compiler_candidates)];
    let fact_source_traces =
        compiler_candidates.iter().map(document_symbol_candidate_trace).collect();

    let receipt = SemanticShadowCompareReceipt::from_summaries_with_fact_source_traces(
        ShadowQueryName::DocumentSymbols,
        ShadowQueryInput { symbol: symbol.to_string() },
        old_result,
        new_result,
        notes,
        fact_source_traces,
    );

    DocumentSymbolShadowResult { legacy_symbols, receipt }
}

fn document_symbol_candidate_trace(candidate: &DocumentSymbolShadowCandidate) -> ProviderFactTrace {
    ProviderFactTrace::new(
        ProviderSurface::DocumentSymbols,
        candidate.source,
        candidate.provenance,
        candidate.confidence,
        candidate.freshness,
        candidate.fallback_state,
        candidate.source_hash.clone(),
        candidate.anchor_id,
        candidate.model_version,
    )
}

fn document_symbol_answer_identities(
    compiler_candidates: &[DocumentSymbolShadowCandidate],
) -> Vec<String> {
    compiler_candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.fallback_state,
                ProviderFallbackState::Primary
                    | ProviderFallbackState::Shadow
                    | ProviderFallbackState::Fallback
            )
        })
        .map(|candidate| candidate.identity.clone())
        .collect()
}

fn document_symbol_shadow_note(
    legacy_symbols: &[DocumentSymbolShadowLegacy],
    compiler_candidates: &[DocumentSymbolShadowCandidate],
) -> String {
    let blocked_count = compiler_candidates
        .iter()
        .filter(|candidate| candidate.fallback_state == ProviderFallbackState::Blocked)
        .count();
    format!(
        "document-symbol shadow proof: legacy_candidates={}; compiler_fact_candidates={}; blocked_candidates={}; no live document-symbol behavior change",
        legacy_symbols.len(),
        compiler_candidates.len(),
        blocked_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use perl_tdd_support::must;
    use perl_workspace::semantic_shadow_compare::ShadowCompareVerdict;

    #[test]
    fn name_anchor_control_omits_missing_or_invalid_canonical_names() {
        let source = "sub ub { 'ub' }";
        for anchor in [None, Some((99, 101)), Some((4, 4)), Some((6, 4))] {
            let mut ast = must(Parser::new(source).parse());
            let baseline = source_backed_document_symbols_from_ast(&ast, source);
            assert_eq!(baseline.symbols.len(), 1);
            assert_eq!(baseline.symbols[0].name, "ub");
            let mut changed = false;
            ast.for_each_child_mut(|node| {
                if let perl_parser_core::ast::NodeKind::Subroutine { name_span, .. } =
                    &mut node.kind
                {
                    *name_span = anchor
                        .map(|(start, end)| perl_parser_core::ast::SourceLocation { start, end });
                    changed = true;
                }
            });
            assert!(changed, "the real parsed declaration must be mutated");
            let result = source_backed_document_symbols_from_ast(&ast, source);
            assert!(
                result.symbols.is_empty(),
                "invalid name anchor {anchor:?} must not be guessed"
            );
            assert!(result.fact_traces.is_empty(), "omitted names must not claim exact facts");
        }
    }

    #[test]
    fn name_anchor_control_retains_unported_phase_and_typeglob_symbols() {
        for (source, expected) in [("BEGIN { 1 }", "BEGIN"), ("*helper = sub { 2 };", "helper")] {
            let ast = must(Parser::new(source).parse());
            let result = source_backed_document_symbols_from_ast(&ast, source);
            assert_eq!(result.symbols.len(), 1);
            assert_eq!(result.symbols[0].name, expected);
            assert!(!result.fact_traces.is_empty());
        }
    }

    #[test]
    fn source_backed_document_symbols_emit_fresh_primary_traces()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package Foo;\n\nsub greet {\n    return 1;\n}\n";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);

        assert!(!result.symbols.is_empty(), "expected source-backed symbols");
        assert!(
            result.symbols.iter().any(|symbol| symbol.name == "Foo"),
            "package symbol should be present: {:?}",
            result.symbols
        );
        assert!(
            result.fact_traces.iter().any(|trace| {
                trace.source == ProviderFactSourceKind::ParserSyntax
                    && trace.provenance == Provenance::ExactAst
                    && trace.confidence == Confidence::High
                    && trace.freshness == ProviderFactFreshness::Fresh
                    && trace.fallback_state == ProviderFallbackState::Primary
            }),
            "source-backed live result must carry fresh primary parser-syntax traces"
        );
        Ok(())
    }

    /// Regression guard for #1519: `package Foo;\nsub greet {}` must produce greet
    /// exactly ONCE — as a child of Foo, not also duplicated at the top level.
    ///
    /// The global scope has `location.start = 0` and `package Foo;` at the top of a
    /// file also has `location.start = 0`, so the children-search absorbs global-scope
    /// siblings (greet) as children of Foo.  The de-dup pass then removes greet from
    /// the top-level list.  Both effects together give the correct LSP outline.
    #[test]
    fn source_backed_document_symbols_no_duplication_statement_package()
    -> Result<(), Box<dyn std::error::Error>> {
        let source =
            "package Foo;\n\nsub greet {\n    return 1;\n}\nsub farewell {\n    return 0;\n}\n";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);

        // greet and farewell must each appear exactly once in the full tree.
        let all_names: Vec<String> = {
            let mut names = Vec::new();
            for sym in &result.symbols {
                names.push(sym.name.clone());
                for child in &sym.children {
                    names.push(child.name.clone());
                }
            }
            names
        };
        assert_eq!(
            all_names.iter().filter(|n| n.as_str() == "greet").count(),
            1,
            "greet must appear exactly once (no top-level duplication); tree names: {:?}",
            all_names
        );
        assert_eq!(
            all_names.iter().filter(|n| n.as_str() == "farewell").count(),
            1,
            "farewell must appear exactly once (no top-level duplication); tree names: {:?}",
            all_names
        );

        // Foo must be at the top level.
        assert!(
            result.symbols.iter().any(|s| s.name == "Foo"),
            "Foo package must appear at top level; got: {:?}",
            result.symbols
        );

        // greet and farewell must NOT appear at top level (they are children of Foo).
        assert!(
            !result.symbols.iter().any(|s| s.name == "greet"),
            "greet must not appear at top level (it is a child of Foo); got: {:?}",
            result.symbols
        );

        // greet must be a child of Foo with source-backed ranges.
        let foo = result.symbols.iter().find(|s| s.name == "Foo").ok_or("no Foo")?;
        let greet = foo
            .children
            .iter()
            .find(|c| c.name == "greet")
            .ok_or("greet must be a child of Foo")?;
        assert!(
            greet.range.start.line <= greet.range.end.line,
            "greet must carry source-backed range: {:?}",
            greet
        );
        Ok(())
    }

    fn tree_names(symbols: &[DocumentSymbol]) -> Vec<String> {
        let mut names = Vec::new();
        for sym in symbols {
            names.push(sym.name.clone());
            names.extend(tree_names(&sym.children));
        }
        names
    }

    fn top_names(symbols: &[DocumentSymbol]) -> Vec<&str> {
        symbols.iter().map(|s| s.name.as_str()).collect()
    }

    fn child_names(symbol: &DocumentSymbol) -> Vec<&str> {
        symbol.children.iter().map(|s| s.name.as_str()).collect()
    }

    fn assert_ranges_contained(symbols: &[DocumentSymbol]) {
        for sym in symbols {
            for child in &sym.children {
                let parent_start = (sym.range.start.line, sym.range.start.character);
                let parent_end = (sym.range.end.line, sym.range.end.character);
                let child_start = (child.range.start.line, child.range.start.character);
                let child_end = (child.range.end.line, child.range.end.character);
                assert!(
                    parent_start <= child_start && child_end <= parent_end,
                    "child {} {:?} must be contained in parent {} {:?}",
                    child.name,
                    child.range,
                    sym.name,
                    sym.range
                );
            }
            assert_ranges_contained(&sym.children);
        }
    }

    #[test]
    fn document_symbols_parent_by_extent_multiple_statement_packages()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package Alpha;\nsub a_fn { 1 }\npackage Beta;\nsub b_fn { 2 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert_eq!(top_names(&result.symbols), vec!["Alpha", "Beta"], "tree: {:?}", result.symbols);
        let alpha = result.symbols.iter().find(|s| s.name == "Alpha").ok_or("no Alpha")?;
        let beta = result.symbols.iter().find(|s| s.name == "Beta").ok_or("no Beta")?;
        assert_eq!(child_names(alpha), vec!["a_fn"]);
        assert_eq!(child_names(beta), vec!["b_fn"]);
        let names = tree_names(&result.symbols);
        for expected in ["Alpha", "Beta", "a_fn", "b_fn"] {
            assert_eq!(
                names.iter().filter(|n| n.as_str() == expected).count(),
                1,
                "{expected} once: {names:?}"
            );
        }
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    #[test]
    fn document_symbols_parent_by_extent_nested_block_package()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package Outer;\nsub outer_fn { 1 }\n{\n    package Inner;\n    sub inner_fn { 2 }\n}\nsub outer_fn2 { 3 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert_eq!(top_names(&result.symbols), vec!["Outer"], "tree: {:?}", result.symbols);
        let outer = result.symbols.iter().find(|s| s.name == "Outer").ok_or("no Outer")?;
        assert_eq!(
            child_names(outer),
            vec!["Inner", "outer_fn", "outer_fn2"],
            "outer children: {:?}",
            outer.children
        );
        let inner = outer.children.iter().find(|s| s.name == "Inner").ok_or("no Inner")?;
        assert_eq!(child_names(inner), vec!["inner_fn"]);
        let names = tree_names(&result.symbols);
        for expected in ["Outer", "Inner", "outer_fn", "inner_fn", "outer_fn2"] {
            assert_eq!(
                names.iter().filter(|n| n.as_str() == expected).count(),
                1,
                "{expected} once: {names:?}"
            );
        }
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    #[test]
    fn document_symbols_parent_by_extent_role_container() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "package Speakable;\nuse Moose::Role;\nrequires 'speak';\nsub helper { 1 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert_eq!(top_names(&result.symbols), vec!["Speakable"], "tree: {:?}", result.symbols);
        let role = result.symbols.iter().find(|s| s.name == "Speakable").ok_or("no Speakable")?;
        let children = child_names(role);
        assert!(
            children.contains(&"helper"),
            "helper under Speakable: {children:?} tree {:?}",
            result.symbols
        );
        // `requires 'speak'` may be represented under its bare name if at all.
        let names = tree_names(&result.symbols);
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "helper").count(),
            1,
            "helper once: {names:?}"
        );
        if names.iter().any(|n| n == "speak") {
            assert!(children.contains(&"speak"), "speak under Speakable: {children:?}");
        }
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    #[test]
    fn document_symbols_parent_by_extent_symbols_before_first_package()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "sub early { 1 }\npackage Later;\nsub later_fn { 2 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert_eq!(
            top_names(&result.symbols),
            vec!["early", "Later"],
            "tree: {:?}",
            result.symbols
        );
        let later = result.symbols.iter().find(|s| s.name == "Later").ok_or("no Later")?;
        assert_eq!(child_names(later), vec!["later_fn"]);
        let names = tree_names(&result.symbols);
        for expected in ["early", "Later", "later_fn"] {
            assert_eq!(
                names.iter().filter(|n| n.as_str() == expected).count(),
                1,
                "{expected} once: {names:?}"
            );
        }
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    /// #18111: top-level and child outline order must be deterministic.
    ///
    /// The fixture combines the two probe shapes that permuted across
    /// processes before the deterministic sorts: a global variable and a sub
    /// declared before the first package (symbols that used to follow the
    /// symbol-table HashMap bucket order at the top level), and two
    /// Class::Tiny attribute batches whose members all share one
    /// `use Class::Tiny` span, so the child sort keys tie beyond
    /// (priority, start, end) and used to keep HashMap insertion order.
    /// Every iteration rebuilds the table from a fresh parse, so the symbol
    /// table's HashMaps re-seed; the exact pinned order must hold each time.
    #[test]
    fn document_symbols_order_deterministic_multiple_packages_shared_class_tiny_span()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $global_thing = 1;\nsub before_pkg { 2 }\npackage Alpha;\nuse Class::Tiny qw(zeta alpha middle);\nsub method1 { 1 }\npackage Beta;\nuse Class::Tiny qw(zulu able mike);\nsub method2 { 2 }\n";
        // LSP SymbolKind numeric values below: 5 = Class, 7 = Property,
        // 12 = Function, 13 = Variable.
        let expected_top: Vec<(&str, u32)> =
            vec![("$global_thing", 13), ("before_pkg", 12), ("Alpha", 5), ("Beta", 5)];
        let expected_children: Vec<(&str, Vec<(&str, u32)>)> = vec![
            ("Alpha", vec![("alpha", 7), ("middle", 7), ("zeta", 7), ("method1", 12)]),
            ("Beta", vec![("able", 7), ("mike", 7), ("zulu", 7), ("method2", 12)]),
        ];
        let mut pinned: Option<Vec<DocumentSymbol>> = None;
        for _ in 0..32 {
            let ast = must(Parser::new(source).parse());
            let result = source_backed_document_symbols_from_ast(&ast, source);
            let top_identities: Vec<(&str, u32)> =
                result.symbols.iter().map(|s| (s.name.as_str(), s.kind)).collect();
            assert_eq!(
                top_identities, expected_top,
                "top-level order and kinds must be pinned: {:?}",
                result.symbols
            );
            for (package, children) in &expected_children {
                let symbol = result
                    .symbols
                    .iter()
                    .find(|s| s.name == *package)
                    .ok_or_else(|| format!("no {package} symbol"))?;
                let child_identities: Vec<(&str, u32)> =
                    symbol.children.iter().map(|c| (c.name.as_str(), c.kind)).collect();
                assert_eq!(
                    child_identities, *children,
                    "{package} children must be pinned: shared-span Class::Tiny attributes sort by name, then the method: {:?}",
                    symbol.children
                );
            }
            assert_ranges_contained(&result.symbols);
            if let Some(previous) = &pinned {
                assert_eq!(
                    &result.symbols, previous,
                    "the complete outline must be identical for every fresh HashMap seeding"
                );
            }
            pinned = Some(result.symbols);
        }
        Ok(())
    }

    /// #18111: `kind` is the final child-order tiebreak. When
    /// (priority, start, end, name) tie exactly, the lower LSP `SymbolKind`
    /// number must sort first regardless of the input (HashMap bucket) order;
    /// a stable sort without the `kind` comparison would keep the input
    /// order instead.
    #[test]
    fn child_sort_kind_tiebreak_orders_full_key_tie_from_either_input_order() {
        let tied = |kind: u32| PendingSymbol {
            document_symbol: DocumentSymbol {
                name: "twin".to_string(),
                detail: String::new(),
                kind,
                range: perl_position_tracking::WireRange {
                    start: perl_position_tracking::WirePosition { line: 0, character: 0 },
                    end: perl_position_tracking::WirePosition { line: 1, character: 6 },
                },
                selection_range: perl_position_tracking::WireRange {
                    start: perl_position_tracking::WirePosition { line: 0, character: 4 },
                    end: perl_position_tracking::WirePosition { line: 0, character: 8 },
                },
                children: Vec::new(),
                sort_priority: CALLABLE_DOCUMENT_SYMBOL_PRIORITY,
            },
            start: 0,
            end: 20,
            extent: (0, 20),
            container: false,
        };
        // LSP SymbolKind values: 7 = Property, 12 = Function.
        let pending = vec![Some(tied(12)), Some(tied(7))];
        let mut forward = vec![0, 1];
        sort_pending_indices(&mut forward, &pending);
        assert_eq!(forward, vec![1usize, 0], "kind 7 (Property) must precede kind 12 (Function)");
        let mut reversed = vec![1usize, 0];
        sort_pending_indices(&mut reversed, &pending);
        assert_eq!(
            reversed,
            vec![1usize, 0],
            "the `kind` tiebreak, not the input order, must decide a full-key tie"
        );
    }

    /// Review regression (#18184): `use EV` synthesizes a framework class
    /// symbol at the import site. It declares no package region, so it
    /// must not end P's extent or absorb the following sub.
    #[test]
    fn document_symbols_framework_import_does_not_steal_package_members()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package P;\nuse EV;\nsub helper { 1 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let package = result.symbols.iter().find(|s| s.name == "P").ok_or("no P")?;
        assert!(
            child_names(package).contains(&"helper"),
            "helper must stay under P: {:?}",
            result.symbols
        );
        assert!(
            !child_names(package).contains(&"EV") || {
                let ev = package.children.iter().find(|s| s.name == "EV").ok_or("EV vanished")?;
                ev.children.is_empty()
            },
            "synthetic EV under P must own nothing: {:?}",
            result.symbols
        );
        let names = tree_names(&result.symbols);
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "helper").count(),
            1,
            "helper once: {names:?}"
        );
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    /// Review regression (#18184): the Plack middleware package
    /// synthesized by `enable 'Session'` carries its framework marker in
    /// attributes, not declaration. It must not steal P's members either.
    #[test]
    fn document_symbols_plack_enable_does_not_steal_package_members()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package P;\nuse Plack::Builder;\nbuilder {\n    enable 'Session';\n};\nsub helper { 1 }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let package = result.symbols.iter().find(|s| s.name == "P").ok_or("no P")?;
        assert!(
            child_names(package).contains(&"helper"),
            "helper must stay under P: {:?}",
            result.symbols
        );
        let names = tree_names(&result.symbols);
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "helper").count(),
            1,
            "helper once: {names:?}"
        );
        for symbol in &result.symbols {
            if symbol.name == "Plack::Middleware::Session" {
                assert!(
                    symbol.children.iter().all(|c| c.name != "helper"),
                    "synthetic middleware must not own helper: {:?}",
                    symbol.children
                );
            }
        }
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    /// Review regression (#18184): locals of a `method` belong under the
    /// method, not beside it as class members.
    #[test]
    fn document_symbols_method_locals_nest_under_method() -> Result<(), Box<dyn std::error::Error>>
    {
        let source =
            "class C {\n    method f {\n        my $x = 1;\n    }\n    method g {\n    }\n}\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert_eq!(top_names(&result.symbols), vec!["C"], "tree: {:?}", result.symbols);
        let class = result.symbols.iter().find(|s| s.name == "C").ok_or("no C")?;
        let children = child_names(class);
        assert!(children.contains(&"f"), "f under C: {children:?}");
        assert!(children.contains(&"g"), "g under C: {children:?}");
        assert!(
            !children.iter().any(|name| name.contains('x')),
            "no local directly under C: {children:?}"
        );
        let method = class.children.iter().find(|s| s.name == "f").ok_or("no f")?;
        let locals = child_names(method);
        assert!(
            locals.iter().any(|name| name.contains('x')),
            "local under f: {locals:?} tree {:?}",
            result.symbols
        );
        let names = tree_names(&result.symbols);
        assert_eq!(names.iter().filter(|n| n.contains('x')).count(), 1, "local once: {names:?}");
        assert_ranges_contained(&result.symbols);
        Ok(())
    }

    #[test]
    fn document_symbol_shadow_traces_explicit_syntax_fact() -> Result<(), Box<dyn std::error::Error>>
    {
        let legacy = legacy_symbol("package:Foo:0:0");
        let result = document_symbol_source_shadow(
            vec![legacy],
            vec![shadow_candidate(
                "package:Foo:0:0",
                ProviderFactSourceKind::ParserSyntax,
                Provenance::ExactAst,
                Confidence::High,
                ProviderFactFreshness::Fresh,
                ProviderFallbackState::Shadow,
            )],
            "Foo",
        );

        assert_eq!(result.legacy_symbols.len(), 1);
        assert_eq!(result.receipt.query, ShadowQueryName::DocumentSymbols);
        assert_eq!(result.receipt.verdict, ShadowCompareVerdict::Same);
        assert_eq!(result.receipt.old_result.match_count, 1);
        assert_eq!(result.receipt.new_result.match_count, 1);

        let trace = first_trace(&result)?;
        assert_eq!(trace.surface, ProviderSurface::DocumentSymbols);
        assert_eq!(trace.source, ProviderFactSourceKind::ParserSyntax);
        assert_eq!(trace.provenance, Provenance::ExactAst);
        assert_eq!(trace.confidence, Confidence::High);
        assert_eq!(trace.freshness, ProviderFactFreshness::Fresh);
        assert_eq!(trace.fallback_state, ProviderFallbackState::Shadow);
        Ok(())
    }

    #[test]
    fn document_symbol_shadow_labels_generated_candidates() -> Result<(), Box<dyn std::error::Error>>
    {
        let result = document_symbol_source_shadow(
            Vec::new(),
            vec![shadow_candidate(
                "generated:Foo::reader:virtual",
                ProviderFactSourceKind::FrameworkAdapter,
                Provenance::FrameworkSynthesis,
                Confidence::Medium,
                ProviderFactFreshness::Fresh,
                ProviderFallbackState::Shadow,
            )],
            "reader",
        );

        assert_eq!(result.receipt.verdict, ShadowCompareVerdict::Improved);
        assert_eq!(result.receipt.old_result.match_count, 0);
        assert_eq!(result.receipt.new_result.match_count, 1);

        let trace = first_trace(&result)?;
        assert_eq!(trace.surface, ProviderSurface::DocumentSymbols);
        assert_eq!(trace.source, ProviderFactSourceKind::FrameworkAdapter);
        assert_eq!(trace.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(trace.confidence, Confidence::Medium);
        assert_eq!(trace.freshness, ProviderFactFreshness::Fresh);
        assert_eq!(trace.fallback_state, ProviderFallbackState::Shadow);
        Ok(())
    }

    #[test]
    fn document_symbol_shadow_blocks_dynamic_boundaries() -> Result<(), Box<dyn std::error::Error>>
    {
        let result = document_symbol_source_shadow(
            Vec::new(),
            vec![shadow_candidate(
                "dynamic:Foo::AUTOLOAD",
                ProviderFactSourceKind::DynamicBoundary,
                Provenance::DynamicBoundary,
                Confidence::High,
                ProviderFactFreshness::Fresh,
                ProviderFallbackState::Blocked,
            )],
            "AUTOLOAD",
        );

        assert_eq!(result.receipt.verdict, ShadowCompareVerdict::Same);
        assert_eq!(result.receipt.new_result.match_count, 0);

        let trace = first_trace(&result)?;
        assert_eq!(trace.source, ProviderFactSourceKind::DynamicBoundary);
        assert_eq!(trace.provenance, Provenance::DynamicBoundary);
        assert_eq!(trace.fallback_state, ProviderFallbackState::Blocked);
        Ok(())
    }

    #[test]
    fn document_symbol_shadow_blocks_stale_compiler_facts() -> Result<(), Box<dyn std::error::Error>>
    {
        let result = document_symbol_source_shadow(
            Vec::new(),
            vec![shadow_candidate(
                "stale:Foo::old_sub",
                ProviderFactSourceKind::CompilerFact,
                Provenance::SemanticAnalyzer,
                Confidence::Low,
                ProviderFactFreshness::Stale,
                ProviderFallbackState::Blocked,
            )],
            "old_sub",
        );

        assert_eq!(result.receipt.verdict, ShadowCompareVerdict::Same);
        assert_eq!(result.receipt.new_result.match_count, 0);

        let trace = first_trace(&result)?;
        assert_eq!(trace.source, ProviderFactSourceKind::CompilerFact);
        assert_eq!(trace.confidence, Confidence::Low);
        assert_eq!(trace.freshness, ProviderFactFreshness::Stale);
        assert_eq!(trace.fallback_state, ProviderFallbackState::Blocked);
        Ok(())
    }

    fn legacy_symbol(identity: &str) -> DocumentSymbolShadowLegacy {
        DocumentSymbolShadowLegacy { identity: identity.to_string() }
    }

    fn shadow_candidate(
        identity: &str,
        source: ProviderFactSourceKind,
        provenance: Provenance,
        confidence: Confidence,
        freshness: ProviderFactFreshness,
        fallback_state: ProviderFallbackState,
    ) -> DocumentSymbolShadowCandidate {
        DocumentSymbolShadowCandidate {
            identity: identity.to_string(),
            source,
            provenance,
            confidence,
            freshness,
            fallback_state,
            source_hash: Some("fixture-source-sha".to_string()),
            anchor_id: Some(AnchorId(1)),
            model_version: Some(1),
        }
    }

    fn first_trace(
        result: &DocumentSymbolShadowResult,
    ) -> Result<&ProviderFactTrace, Box<dyn std::error::Error>> {
        result
            .receipt
            .fact_source_traces
            .first()
            .ok_or_else(|| "expected document-symbol fact-source trace".into())
    }

    #[test]
    fn test_symbol_name_range_subroutine() -> Result<(), Box<dyn std::error::Error>> {
        let source = "sub foo { }";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);
        let sub_symbol = result
            .symbols
            .iter()
            .find(|s| s.name == "foo")
            .ok_or("expected 'foo' subroutine symbol")?;

        // "foo" starts at character 4 (after "sub ")
        // Expected: selectionRange points to "foo", not "sub foo"
        let _name_start_byte = source.find("foo").ok_or("name 'foo' not found in source")?;

        // selection_range should span just "foo", not the entire "sub foo { }"
        assert!(
            sub_symbol.selection_range.start.line == 0,
            "selectionRange should start at line 0, got: {:?}",
            sub_symbol.selection_range.start
        );
        assert!(
            sub_symbol.selection_range.end.line == 0,
            "selectionRange should end at line 0, got: {:?}",
            sub_symbol.selection_range.end
        );

        // Verify selection_range is smaller than range
        let range_char_span = sub_symbol.range.end.character - sub_symbol.range.start.character;
        let sel_char_span =
            sub_symbol.selection_range.end.character - sub_symbol.selection_range.start.character;
        assert!(
            sel_char_span < range_char_span,
            "selectionRange ({}) should be smaller than range ({})",
            sel_char_span,
            range_char_span
        );

        Ok(())
    }

    #[test]
    fn test_symbol_name_range_package() -> Result<(), Box<dyn std::error::Error>> {
        let source = "package MyPkg;";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);
        let pkg_symbol = result
            .symbols
            .iter()
            .find(|s| s.name == "MyPkg")
            .ok_or("expected 'MyPkg' package symbol")?;

        // "MyPkg" starts after "package "
        let _name_start_byte = source.find("MyPkg").ok_or("name 'MyPkg' not found in source")?;

        // selection_range should span just "MyPkg", not the entire "package MyPkg;"
        assert!(
            pkg_symbol.selection_range.start.line == 0,
            "selectionRange should start at line 0, got: {:?}",
            pkg_symbol.selection_range.start
        );

        // Verify selection_range is smaller than range
        let range_char_span = pkg_symbol.range.end.character - pkg_symbol.range.start.character;
        let sel_char_span =
            pkg_symbol.selection_range.end.character - pkg_symbol.selection_range.start.character;
        assert!(
            sel_char_span < range_char_span,
            "selectionRange ({}) should be smaller than range ({})",
            sel_char_span,
            range_char_span
        );

        Ok(())
    }

    fn find_named<'a>(symbols: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
        for symbol in symbols {
            if symbol.name == name {
                return Some(symbol);
            }
            if let Some(found) = find_named(&symbol.children, name) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn framework_package_name_selection_keeps_package_anchor()
    -> Result<(), Box<dyn std::error::Error>> {
        // #17924: the compatibility extractor reclassifies a framework
        // package as Class/Role, but the outline selection must stay on the
        // declared name (chars 8-11), not slide into the `package` keyword
        // (chars 4-7, the first substring occurrence of "age").
        // (framework module, expected wire kind: Class = 5, Role = 11)
        for (module, expected_kind) in [
            ("Moo", 5),
            ("Moose", 5),
            ("Moo::Role", 11),
            ("Moose::Role", 11),
            ("Role::Tiny", 11),
            ("Class::Tiny", 5),
        ] {
            let source = format!("package age {{ use {module}; }}");
            let ast = must(Parser::new(&source).parse());
            let result = source_backed_document_symbols_from_ast(&ast, &source);
            let symbol = find_named(&result.symbols, "age")
                .ok_or_else(|| format!("expected 'age' symbol for {module}"))?;
            assert_eq!(
                (symbol.selection_range.start.line, symbol.selection_range.start.character),
                (0, 8),
                "{module}: selection must start at the declared name"
            );
            assert_eq!(
                (symbol.selection_range.end.line, symbol.selection_range.end.character),
                (0, 11),
                "{module}: selection must end after the declared name"
            );
            assert_eq!(
                symbol.kind, expected_kind,
                "{module}: wire kind stays the reclassified Class/Role kind"
            );
        }
        Ok(())
    }

    #[test]
    fn plain_package_name_selection_keeps_package_anchor() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "package age;";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let symbol = find_named(&result.symbols, "age").ok_or("expected 'age' package symbol")?;
        assert_eq!(
            (symbol.selection_range.start.character, symbol.selection_range.end.character),
            (8, 11)
        );
        assert_eq!(symbol.kind, 2, "plain package keeps Module kind");
        Ok(())
    }

    #[test]
    fn test_symbol_name_range_scalar_variable() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $counter = 0;";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);
        // DocumentSymbol.name for a scalar variable includes the sigil (e.g. "$counter").
        let var_symbol = result
            .symbols
            .iter()
            .find(|s| s.name == "$counter")
            .ok_or("expected '$counter' variable symbol")?;

        // "counter" starts after "my $"
        let _name_start_byte =
            source.find("counter").ok_or("name 'counter' not found in source")?;

        // selection_range should span just "counter", not "$counter"
        assert!(
            var_symbol.selection_range.start.line == 0,
            "selectionRange should start at line 0, got: {:?}",
            var_symbol.selection_range.start
        );

        // Verify selection_range is smaller than or equal to range
        let sel_char_span =
            var_symbol.selection_range.end.character - var_symbol.selection_range.start.character;
        let range_char_span = var_symbol.range.end.character - var_symbol.range.start.character;
        assert!(
            sel_char_span <= range_char_span,
            "selectionRange ({}) should be <= range ({})",
            sel_char_span,
            range_char_span
        );

        Ok(())
    }

    #[test]
    fn test_symbol_name_range_array_variable() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my @items = ();";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);
        // DocumentSymbol.name for an array variable includes the sigil (e.g. "@items").
        let var_symbol = result
            .symbols
            .iter()
            .find(|s| s.name == "@items")
            .ok_or("expected '@items' variable symbol")?;

        // "items" starts after "my @"
        let _name_start_byte = source.find("items").ok_or("name 'items' not found in source")?;

        // selection_range should span just "items", not "@items"
        assert!(
            var_symbol.selection_range.start.line == 0,
            "selectionRange should start at line 0, got: {:?}",
            var_symbol.selection_range.start
        );

        // Verify selection_range is smaller than or equal to range
        let sel_char_span =
            var_symbol.selection_range.end.character - var_symbol.selection_range.start.character;
        let range_char_span = var_symbol.range.end.character - var_symbol.range.start.character;
        assert!(
            sel_char_span <= range_char_span,
            "selectionRange ({}) should be <= range ({})",
            sel_char_span,
            range_char_span
        );

        Ok(())
    }

    #[test]
    fn test_symbol_name_range_moose_attribute() -> Result<(), Box<dyn std::error::Error>> {
        // `use Moose;` is required for the SymbolExtractor to recognise the `has` pattern.
        // Without framework context the `has name => ...` expression is not synthesized into
        // a symbol.  DocumentSymbol.name for a Moose attribute is the bare attribute name
        // (no sigil) per document_symbol_name().
        let source = "package Foo;\nuse Moose;\nhas name => (is => 'ro');\n";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);
        let attr_symbol = result
            .symbols
            .iter()
            .flat_map(|s| std::iter::once(s).chain(s.children.iter()))
            .find(|s| s.name == "name")
            .ok_or("expected 'name' Moose attribute symbol")?;

        // "name" starts after "has " within the has declaration.
        // Source layout: line 0="package Foo;", line 1="use Moose;", line 2="has name => ..."
        let _name_start_byte = source.find("name").ok_or("name 'name' not found in source")?;

        // selection_range should span just "name" at character 4 of line 2 (after "has ").
        assert!(
            attr_symbol.selection_range.start.line == 2,
            "selectionRange should start at line 2 (the 'has name' declaration), got: {:?}",
            attr_symbol.selection_range.start
        );

        // Verify selection_range is smaller than or equal to range
        let sel_char_span =
            attr_symbol.selection_range.end.character - attr_symbol.selection_range.start.character;
        let range_char_span = attr_symbol.range.end.character - attr_symbol.range.start.character;
        assert!(
            sel_char_span <= range_char_span,
            "selectionRange ({}) should be <= range ({})",
            sel_char_span,
            range_char_span
        );

        Ok(())
    }

    #[test]
    fn test_document_symbols_selection_range_vs_range() -> Result<(), Box<dyn std::error::Error>> {
        let source = "package TestPkg;\n\nsub method1 { }\nmy $var = 1;\nhas attr => ();\n";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let result = source_backed_document_symbols_from_ast(&ast, source);

        // For every symbol, selectionRange must be <= range
        // (in byte span, not character span, but we verify via positions)
        for symbol in &result.symbols {
            let range_span = symbol.range.end.line as i32 - symbol.range.start.line as i32;
            let sel_span =
                symbol.selection_range.end.line as i32 - symbol.selection_range.start.line as i32;

            assert!(
                sel_span <= range_span,
                "selectionRange line span ({}) should be <= range line span ({}) for symbol '{}'",
                sel_span,
                range_span,
                symbol.name
            );

            // If same line, check character positions
            if symbol.range.start.line == symbol.range.end.line
                && symbol.selection_range.start.line == symbol.selection_range.end.line
                && symbol.range.start.line == symbol.selection_range.start.line
            {
                let range_char_span = symbol.range.end.character - symbol.range.start.character;
                let sel_char_span =
                    symbol.selection_range.end.character - symbol.selection_range.start.character;

                assert!(
                    sel_char_span <= range_char_span,
                    "selectionRange char span ({}) should be <= range char span ({}) for symbol '{}'",
                    sel_char_span,
                    range_char_span,
                    symbol.name
                );
            }

            // Recursively check children
            check_children_ranges(&symbol.children)?;
        }

        Ok(())
    }

    fn check_children_ranges(
        children: &[DocumentSymbol],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for child in children {
            let range_span = child.range.end.line as i32 - child.range.start.line as i32;
            let sel_span =
                child.selection_range.end.line as i32 - child.selection_range.start.line as i32;

            assert!(
                sel_span <= range_span,
                "child selectionRange line span ({}) should be <= range line span ({}) for symbol '{}'",
                sel_span,
                range_span,
                child.name
            );

            check_children_ranges(&child.children)?;
        }
        Ok(())
    }

    fn flatten_names(symbols: &[DocumentSymbol]) -> Vec<&DocumentSymbol> {
        let mut out = Vec::new();
        for s in symbols {
            out.push(s);
            out.extend(flatten_names(&s.children));
        }
        out
    }

    #[test]
    fn moose_has_attribute_not_duplicated_as_accessor() {
        let source = "package Widget;\nuse Moose;\nhas name => (is => 'ro', isa => 'Str');\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let flat = flatten_names(&result.symbols);
        let named: Vec<_> = flat.iter().filter(|s| s.name == "name").collect();
        assert_eq!(named.len(), 1, "expected exactly one `name`, got: {flat:?}");
        assert_eq!(named[0].kind, 7, "kept symbol must be the attribute (Field)");
        assert!(
            named[0].detail.contains("is=ro") && named[0].detail.contains("isa=Str"),
            "attribute detail preserved, got: {:?}",
            named[0].detail
        );
    }

    #[test]
    fn moose_has_explicit_reader_kept() {
        let source =
            "package Widget;\nuse Moose;\nhas name => (is => 'ro', reader => 'get_name');\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let flat = flatten_names(&result.symbols);
        assert_eq!(flat.iter().filter(|s| s.name == "name").count(), 1);
        assert_eq!(
            flat.iter().filter(|s| s.name == "get_name").count(),
            1,
            "explicit reader kept: {flat:?}"
        );
    }

    #[test]
    fn moose_has_same_name_explicit_reader_collapses_to_attribute() {
        // An explicit reader named exactly like the attribute is the same
        // callable the default accessor would mint, at the same span; the
        // outline keeps the single attribute entry (#18111).
        for source in [
            "package Widget;\nuse Moose;\nhas name => (is => 'ro', reader => 'name');\n",
            "package Widget;\nuse Moose;\nhas name => (is => 'bare', reader => 'name');\n",
        ] {
            let ast = must(Parser::new(source).parse());
            let result = source_backed_document_symbols_from_ast(&ast, source);
            let flat = flatten_names(&result.symbols);
            let named: Vec<_> = flat.iter().filter(|s| s.name == "name").collect();
            assert_eq!(named.len(), 1, "exactly one `name` for {source:?}: {flat:?}");
            assert_eq!(named[0].kind, 7, "kept entry is the attribute for {source:?}");
        }
    }

    #[test]
    fn moose_has_plus_explicit_sub_same_name_different_span_kept() {
        let source =
            "package Widget;\nuse Moose;\nhas name => (is => 'ro');\nsub name { return 1; }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let flat = flatten_names(&result.symbols);
        assert_eq!(
            flat.iter().filter(|s| s.name == "name").count(),
            2,
            "attribute + explicit sub kept: {flat:?}"
        );
    }

    #[test]
    fn moose_has_list_keeps_both_attributes() {
        let source = "package Widget;\nuse Moose;\nhas [qw(a b)] => (is => 'ro');\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let flat = flatten_names(&result.symbols);
        assert_eq!(flat.iter().filter(|s| s.name == "a").count(), 1, "{flat:?}");
        assert_eq!(flat.iter().filter(|s| s.name == "b").count(), 1, "{flat:?}");
    }

    #[test]
    fn duplicate_name_at_different_spans_kept() {
        let source = "sub name { return 1; }\nsub name { return 2; }\n";
        let ast = must(Parser::new(source).parse());
        let result = source_backed_document_symbols_from_ast(&ast, source);
        let flat = flatten_names(&result.symbols);
        assert_eq!(flat.iter().filter(|s| s.name == "name").count(), 2, "{flat:?}");
    }

    #[test]
    fn test_symbol_name_range_fallback() -> Result<(), Box<dyn std::error::Error>> {
        // This test verifies the fallback behavior: if a symbol's name cannot be found
        // in the source slice, the function should return the full symbol_range (not panic).
        // We construct a Symbol with mismatched bounds to trigger this.

        let source = "sub test { }";
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());

        let extractor = SymbolExtractor::new_with_source(source);
        let symbol_table = extractor.extract(&ast);

        // Find a symbol from the table
        let _test_symbol = symbol_table
            .symbols
            .values()
            .flatten()
            .find(|s| s.name == "test")
            .ok_or("expected 'test' symbol")?;

        // The symbol_name_range helper should exist and be callable
        // If the name cannot be found within the location bounds, it should gracefully
        // fall back to the full symbol_range instead of panicking.
        //
        // Since symbol_name_range is a private function, we test its behavior
        // indirectly by calling source_backed_document_symbols_from_ast and verifying
        // that all symbols have valid (non-panicking) ranges.

        let result = source_backed_document_symbols_from_ast(&ast, source);
        assert!(!result.symbols.is_empty(), "should produce symbols without panicking");

        Ok(())
    }
}
