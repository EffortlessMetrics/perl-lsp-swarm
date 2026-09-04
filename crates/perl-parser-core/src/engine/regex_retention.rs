//! Canonical parser entry points that retain generation-bound regex analysis.
//!
//! The legacy [`crate::Parser::parse_with_recovery`] API remains available for
//! compatibility. These entry points suppress its legacy per-operator scans,
//! retain parser-owned whole-operator geometry during the parse, derive lexical
//! feature/source-UTF-8 state from the completed AST, and analyze each pattern
//! exactly once against the immutable final source.

use std::{
    cell::RefCell,
    ops::Deref,
    sync::{Arc, atomic::AtomicBool},
};

use perl_ast::{Node, NodeKind, SourceLocation};
use perl_pragma::CompileTimePragmaEnvironment;
use perl_regex::{
    analyzer::{CaptureDiagnosticCode, CaptureLanguageProfile, FeatureState, RegexLanguageProfile},
    validator::{RegexDiagnosticClass, RegexDiagnosticCode},
};

use crate::{ParseError, ParseOutput, Parser, Token};
// Regex-family geometry is reached through the engine's `quote_parser` alias, which is
// where `main` keeps it: the scanner lives in a private module inside `syntax::quote` so
// the existing RIPR suppression on that path covers the seam, and is re-exported from
// there. This layer consumes that surface rather than declaring a second one.
use crate::engine::quote_parser::{
    RegexFamilyGeometry, RegexFamilyOperator, extract_regex_family_geometry,
};
use crate::syntax::regex_analysis::{
    RegexAnalysisAvailability, RegexAnalysisRecord, RegexAnalysisTable,
};

const MAX_RECOVERY_GEOMETRY_STARTS: usize = 4_096;

thread_local! {
    static ACTIVE_GEOMETRY_SESSIONS: RefCell<Vec<PendingGeometrySession>> = const {
        RefCell::new(Vec::new())
    };
}

/// Parser output plus canonical regex-family evidence for the same source snapshot.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RegexParseOutput {
    /// Recovery-aware parser output.
    pub parse_output: ParseOutput,
    /// Source-digest-bound regex-family analysis table.
    pub regex_analysis: RegexAnalysisTable,
}

impl Deref for RegexParseOutput {
    type Target = ParseOutput;

    fn deref(&self) -> &Self::Target {
        &self.parse_output
    }
}

impl RegexParseOutput {
    /// Consume the wrapper and return both owned output planes.
    #[must_use]
    pub fn into_parts(self) -> (ParseOutput, RegexAnalysisTable) {
        (self.parse_output, self.regex_analysis)
    }
}

/// Parse source with recovery and retain one canonical regex analysis table.
#[must_use]
pub fn parse_source_with_regex_analysis(source: &str) -> RegexParseOutput {
    let session = PendingGeometryGuard::begin(source);
    let mut parser = Parser::new(source);
    let parse_output = parser.parse_with_recovery();
    finish_output(
        source,
        parse_output,
        session.finish().unwrap_or_else(PendingGeometrySession::empty),
    )
}

/// Parse source with cooperative cancellation and retain canonical regex analysis.
#[must_use]
pub fn parse_source_with_cancellation_and_regex_analysis(
    source: &str,
    cancellation_flag: Arc<AtomicBool>,
) -> RegexParseOutput {
    let session = PendingGeometryGuard::begin(source);
    let mut parser = Parser::new_with_cancellation(source, cancellation_flag);
    let parse_output = parser.parse_with_recovery();
    finish_output(
        source,
        parse_output,
        session.finish().unwrap_or_else(PendingGeometrySession::empty),
    )
}

/// Parse a caller-supplied token stream and retain canonical regex analysis.
///
/// This is the integration seam used by incremental/fresh equivalence proof.
#[must_use]
pub fn parse_tokens_with_regex_analysis(tokens: Vec<Token>, source: &str) -> RegexParseOutput {
    let session = PendingGeometryGuard::begin(source);
    let mut parser = Parser::from_tokens(tokens, source);
    let parse_output = parser.parse_with_recovery();
    finish_output(
        source,
        parse_output,
        session.finish().unwrap_or_else(PendingGeometrySession::empty),
    )
}

/// Retain canonical regex analysis around a parse the caller drives itself.
///
/// The whole-parse entry points above own both the parse and its recovery policy.
/// A long-lived host such as the language server already owns that policy — it
/// drives [`Parser::parse`] directly so it can keep its own failure handling — but
/// it still needs the one canonical regex table for the same source snapshot.
///
/// Wrapping the caller's parse in this session gives it exactly that, without a
/// second parse and without a second regex authority:
///
/// ```
/// use perl_parser_core::{Parser, RetainedRegexSession};
///
/// let source = "my $re = qr/(a+)+b/;";
/// let session = RetainedRegexSession::begin(source);
/// let mut parser = Parser::new(source);
/// let mut ast = parser.parse().expect("clean parse");
/// let table = session.finish(Some(&mut ast));
///
/// assert_eq!(table.records.len(), 1);
/// assert!(table.source_matches(source));
/// ```
///
/// While the session is active the parser's legacy per-operator scan is suppressed
/// exactly as it is for the whole-parse entry points, so the retained table is the
/// only regex evidence produced for that parse.
///
/// # Binding the session to one source
///
/// The session borrows the source it began with and uses that same text to build
/// the table, so the retained geometry and the table's digest cannot come from two
/// different documents. That matters more than it looks: geometry is recorded
/// against the parser's source by length, so a *different* document of the same
/// length would otherwise contribute geometry to a table stamped with this
/// document's digest — mis-anchored ranges that still pass every freshness check
/// downstream. Holding the borrow makes that unrepresentable rather than merely
/// validated.
///
/// The session is thread-local. Dropping it without calling
/// [`RetainedRegexSession::finish`] releases it and retains nothing.
#[derive(Debug)]
pub struct RetainedRegexSession<'source> {
    guard: PendingGeometryGuard,
    source: &'source str,
}

impl<'source> RetainedRegexSession<'source> {
    /// Begin retaining parser-owned geometry for a parse of `source`.
    #[must_use]
    pub fn begin(source: &'source str) -> Self {
        Self { guard: PendingGeometryGuard::begin(source), source }
    }

    /// Finish the session and build the canonical table for the parsed `ast`.
    ///
    /// The table is built against the source this session began with, and binds its
    /// digest so a later consumer can prove freshness.
    ///
    /// `ast` is `None` when the caller's parse produced no usable tree. That case
    /// returns an empty source-bound table rather than a fabricated clean one: no
    /// record is not the same claim as no finding, and the caller can still tell the
    /// two apart through [`RegexAnalysisTable::source_matches`].
    ///
    /// A session finished out of order — while a session begun after it is still
    /// active — retains nothing rather than consuming the other session's geometry.
    /// An empty table is a claim this layer can honestly make; another document's
    /// spans stamped with this document's digest is not.
    ///
    /// The AST's compatibility flags (`has_embedded_code`) are refreshed from the
    /// table so they remain a projection of the canonical analysis rather than an
    /// independent scan result.
    ///
    /// # Caller contract
    ///
    /// `ast` must be the tree produced by parsing the source this session borrows.
    /// That is the one part of the binding this type cannot enforce, so it is stated
    /// rather than assumed.
    ///
    /// Geometry admission is bound to the exact buffer, so a foreign tree cannot
    /// contribute another parse's spans. It can still influence the result in two
    /// narrower ways: [`collect_ast_geometry`] may re-extract geometry from *this*
    /// source at a foreign node's offsets when the ranges and pattern text happen to
    /// be compatible, and the compile-time pragma environment — which supplies the
    /// language/feature profile every record is analyzed under — is built from the
    /// tree it is given. A foreign tree therefore yields analysis of this source
    /// under another document's pragma state.
    ///
    /// Making that structural would need a parse result that carries its own session
    /// identity, which is a wider API change than this seam; it is tracked separately.
    #[must_use]
    pub fn finish(self, ast: Option<&mut Node>) -> RegexAnalysisTable {
        let source = self.source;
        let pending = self.guard.finish().unwrap_or_else(PendingGeometrySession::empty);
        let Some(ast) = ast else {
            return RegexAnalysisTable::for_source(source);
        };
        let table = build_table(source, ast, pending);
        apply_ast_compatibility_flags(ast, &table);
        table
    }
}

/// Record parser-owned geometry and suppress the legacy detached-body scan.
///
/// Returns `true` only while a canonical retained-analysis entry point owns the
/// active parse. Ordinary `Parser` callers continue through the compatibility
/// validator path.
pub(crate) fn record_operator_geometry(source: &str, start: usize) -> bool {
    ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        let Some(session) = sessions.last_mut() else {
            return false;
        };
        if !session.owns(source) {
            // A parse of some other buffer. Suppressing the legacy scan for it
            // would lose its findings without retaining anything in exchange, so
            // hand it back to the compatibility path.
            return false;
        }
        if let Some(text) = source.get(start..)
            && let Some(geometry) = extract_regex_family_geometry(text, start)
            && !session.geometries.iter().any(|existing| {
                existing.operator == geometry.operator && existing.full_range == geometry.full_range
            })
        {
            session.geometries.push(geometry);
        }
        true
    })
}

/// Whether a canonical retained-analysis entry point owns the active parse.
///
/// This is the cheap guard callers test before doing any work to satisfy
/// [`record_operator_geometry`]. It reads one thread-local vector's length and
/// never touches the source, so the ordinary parse path pays nothing beyond it.
pub(crate) fn has_active_session() -> bool {
    ACTIVE_GEOMETRY_SESSIONS.with(|sessions| !sessions.borrow().is_empty())
}

fn finish_output(
    source: &str,
    mut parse_output: ParseOutput,
    pending: PendingGeometrySession,
) -> RegexParseOutput {
    let table = build_table(source, &parse_output.ast, pending);

    apply_ast_compatibility_flags(&mut parse_output.ast, &table);
    project_regex_diagnostics(&mut parse_output, &table);

    RegexParseOutput { parse_output, regex_analysis: table }
}

/// Build the canonical table for one already-parsed AST and its exact source.
///
/// This is the single retention body shared by every entry point: the whole-parse
/// entry points above and the caller-driven [`RetainedRegexSession`]. Keeping one
/// body is what stops a second regex authority from appearing for callers that
/// drive their own parse.
fn build_table(
    source: &str,
    ast: &Node,
    mut pending: PendingGeometrySession,
) -> RegexAnalysisTable {
    let environment = CompileTimePragmaEnvironment::build(ast);
    let parser_geometry = pending.geometries.clone();
    let mut unavailable = Vec::new();
    collect_ast_geometry(ast, source, &parser_geometry, &mut pending.geometries, &mut unavailable);
    pending.geometries.sort_by_key(geometry_sort_key);
    pending.geometries.dedup_by(|left, right| {
        left.operator == right.operator && left.full_range == right.full_range
    });
    unavailable.sort_by_key(|candidate| {
        (candidate.range.start, candidate.range.end, candidate.expected.rank())
    });
    unavailable.dedup();
    unavailable.retain(|candidate| {
        !pending.geometries.iter().any(|geometry| {
            candidate.expected.accepts(geometry.operator)
                && contains(candidate.range, geometry.full_range)
        })
    });

    let mut inputs = pending
        .geometries
        .into_iter()
        .map(|geometry| RetentionInput::Geometry(Box::new(geometry)))
        .chain(unavailable.into_iter().map(RetentionInput::Unavailable))
        .collect::<Vec<_>>();
    inputs.sort_by_key(RetentionInput::sort_key);

    let mut table = RegexAnalysisTable::for_source(source);
    for input in inputs {
        match input {
            RetentionInput::Geometry(geometry) => {
                let profile = profile_at(&environment, geometry.pattern.range.start);
                let _record = table.retain_geometry(*geometry, profile);
            }
            RetentionInput::Unavailable(candidate) => {
                let profile = profile_at(&environment, candidate.range.start);
                let _record = table.retain_unavailable(
                    candidate.range,
                    RegexAnalysisAvailability::GeometryUnavailable,
                    profile,
                );
            }
        }
    }

    table
}

fn profile_at(environment: &CompileTimePragmaEnvironment, offset: usize) -> CaptureLanguageProfile {
    let state = environment.map().state_at(offset);
    let enhanced_xx = if state.has_feature("enhanced_xx") {
        FeatureState::Enabled
    } else {
        FeatureState::Disabled
    };
    let source_utf8 = if state.utf8 {
        FeatureState::Enabled
    } else if state.encoding.is_some() {
        FeatureState::Unknown
    } else {
        FeatureState::Disabled
    };
    CaptureLanguageProfile::new(RegexLanguageProfile::new(None, enhanced_xx), source_utf8)
}

fn collect_ast_geometry(
    node: &Node,
    source: &str,
    parser_geometry: &[RegexFamilyGeometry],
    collected: &mut Vec<RegexFamilyGeometry>,
    unavailable: &mut Vec<UnavailableCandidate>,
) {
    if let Some(expected) = ExpectedFamily::for_node(&node.kind) {
        if let Some(geometry) = geometry_for_node(node, source, parser_geometry, expected) {
            if !collected.iter().any(|existing| {
                existing.operator == geometry.operator && existing.full_range == geometry.full_range
            }) {
                collected.push(geometry);
            }
        } else {
            unavailable.push(UnavailableCandidate { range: node.location, expected });
        }
    }
    node.for_each_child(|child| {
        collect_ast_geometry(child, source, parser_geometry, collected, unavailable);
    });
}

fn geometry_for_node(
    node: &Node,
    source: &str,
    parser_geometry: &[RegexFamilyGeometry],
    expected: ExpectedFamily,
) -> Option<RegexFamilyGeometry> {
    if let Some(exact) = parser_geometry.iter().find(|geometry| {
        geometry.full_range == node.location
            && expected.accepts(geometry.operator)
            && pattern_is_compatible(&node.kind, geometry)
    }) {
        return Some(exact.clone());
    }

    if let Some(contained) = parser_geometry
        .iter()
        .filter(|geometry| {
            expected.accepts(geometry.operator)
                && contains(node.location, geometry.full_range)
                && pattern_is_compatible(&node.kind, geometry)
        })
        .max_by_key(|geometry| geometry.full_range.start)
    {
        return Some(contained.clone());
    }

    if let Some(text) = source.get(node.location.start..)
        && let Some(geometry) = extract_regex_family_geometry(text, node.location.start)
        && expected.accepts(geometry.operator)
        && geometry.full_range.end <= node.location.end
        && pattern_is_compatible(&node.kind, &geometry)
    {
        return Some(geometry);
    }

    scan_recovered_geometry(node, source, expected)
}

fn scan_recovered_geometry(
    node: &Node,
    source: &str,
    expected: ExpectedFamily,
) -> Option<RegexFamilyGeometry> {
    let text = source.get(node.location.start..node.location.end)?;
    let mut examined = 0usize;
    let mut best = None;
    for (relative, ch) in text.char_indices() {
        if !matches!(ch, '/' | 'm' | 'q' | 's' | 't' | 'y') {
            continue;
        }
        examined = examined.saturating_add(1);
        if examined > MAX_RECOVERY_GEOMETRY_STARTS {
            break;
        }
        let start = node.location.start.checked_add(relative)?;
        let Some(candidate_text) = source.get(start..) else {
            continue;
        };
        let Some(geometry) = extract_regex_family_geometry(candidate_text, start) else {
            continue;
        };
        if geometry.full_range.end > node.location.end
            || !expected.accepts(geometry.operator)
            || !pattern_is_compatible(&node.kind, &geometry)
        {
            continue;
        }
        if best.as_ref().is_none_or(|existing: &RegexFamilyGeometry| {
            geometry.full_range.start > existing.full_range.start
        }) {
            best = Some(geometry);
        }
    }
    best
}

fn pattern_is_compatible(kind: &NodeKind, geometry: &RegexFamilyGeometry) -> bool {
    match kind {
        NodeKind::Regex { pattern, .. } => {
            pattern == &geometry.pattern.text || pattern.contains(&geometry.pattern.text)
        }
        NodeKind::Match { pattern, .. } | NodeKind::Substitution { pattern, .. } => {
            pattern == &geometry.pattern.text
        }
        NodeKind::Transliteration { search, .. } => search == &geometry.pattern.text,
        _ => false,
    }
}

fn apply_ast_compatibility_flags(node: &mut Node, table: &RegexAnalysisTable) {
    let expected = ExpectedFamily::for_node(&node.kind);
    let embedded = expected
        .and_then(|family| record_for_node(table, node.location, family))
        .is_some_and(RegexAnalysisRecord::has_embedded_code);

    match &mut node.kind {
        NodeKind::Regex { has_embedded_code, .. }
        | NodeKind::Match { has_embedded_code, .. }
        | NodeKind::Substitution { has_embedded_code, .. } => {
            *has_embedded_code = embedded;
        }
        _ => {}
    }
    node.for_each_child_mut(|child| apply_ast_compatibility_flags(child, table));
}

fn record_for_node(
    table: &RegexAnalysisTable,
    range: SourceLocation,
    expected: ExpectedFamily,
) -> Option<&RegexAnalysisRecord> {
    if let Some(exact) = table.records.iter().find(|record| {
        record.full_range == range
            && record.operator.is_some_and(|operator| expected.accepts(operator))
    }) {
        return Some(exact);
    }
    table
        .records
        .iter()
        .filter(|record| {
            record.operator.is_some_and(|operator| expected.accepts(operator))
                && contains(range, record.full_range)
        })
        .max_by_key(|record| record.full_range.start)
}

fn project_regex_diagnostics(parse_output: &mut ParseOutput, table: &RegexAnalysisTable) {
    let mut projected = Vec::new();
    for record in &table.records {
        if let Some(modifiers) = &record.modifiers {
            for diagnostic in &modifiers.diagnostics {
                if diagnostic.class == RegexDiagnosticClass::Syntax {
                    projected
                        .push(ParseError::syntax(diagnostic.message(), diagnostic.range.start));
                }
            }
        }

        let Some(pattern) = &record.pattern else {
            continue;
        };
        for diagnostic in &pattern.structural.diagnostics {
            let Some(source_range) = record.map_pattern_range(diagnostic.range) else {
                continue;
            };
            if diagnostic.class == RegexDiagnosticClass::RiskAdvisory {
                if diagnostic.code == RegexDiagnosticCode::NestedQuantifierRisk {
                    projected.push(ParseError::nested_quantifier_advisory(source_range.start));
                } else {
                    projected.push(ParseError::Advisory {
                        message: diagnostic.message(),
                        location: source_range.start,
                    });
                }
            } else if diagnostic.class == RegexDiagnosticClass::Syntax
                || diagnostic.class == RegexDiagnosticClass::PolicyLimit
            {
                projected.push(ParseError::syntax(diagnostic.message(), source_range.start));
            }
        }

        for diagnostic in &pattern.controls.captures.diagnostics {
            let Some(source_range) = record.map_pattern_range(diagnostic.range) else {
                continue;
            };
            let message = match diagnostic.code {
                CaptureDiagnosticCode::InvalidName => "Invalid regex capture name",
                CaptureDiagnosticCode::RequiresPerlVersion => {
                    "Regex capture syntax requires a newer Perl version"
                }
                CaptureDiagnosticCode::RequiresSourceUtf8 => {
                    "Regex capture name requires source UTF-8 semantics"
                }
                _ => "Invalid regex capture declaration",
            };
            projected.push(ParseError::syntax(message, source_range.start));
        }

        for diagnostic in &pattern.controls.diagnostics {
            if diagnostic.class != RegexDiagnosticClass::Syntax {
                continue;
            }
            let Some(source_range) = diagnostic.source_range else {
                continue;
            };
            projected.push(ParseError::syntax(diagnostic.message(), source_range.start));
        }
    }

    projected.sort_by_key(|diagnostic| diagnostic.location().unwrap_or(usize::MAX));
    projected.dedup();
    let mut added = 0usize;
    for diagnostic in projected {
        if !parse_output.diagnostics.contains(&diagnostic) {
            parse_output.diagnostics.push(diagnostic);
            added = added.saturating_add(1);
        }
    }
    parse_output.diagnostics.sort_by_key(|diagnostic| diagnostic.location().unwrap_or(usize::MAX));
    parse_output.budget_usage.errors_emitted =
        parse_output.budget_usage.errors_emitted.saturating_add(added);
}

fn contains(outer: SourceLocation, inner: SourceLocation) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn geometry_sort_key(geometry: &RegexFamilyGeometry) -> (usize, usize, u8) {
    (geometry.full_range.start, geometry.full_range.end, operator_rank(geometry.operator))
}

fn operator_rank(operator: RegexFamilyOperator) -> u8 {
    match operator {
        RegexFamilyOperator::BareMatch => 0,
        RegexFamilyOperator::Match => 1,
        RegexFamilyOperator::QuoteRegex => 2,
        RegexFamilyOperator::Substitution => 3,
        RegexFamilyOperator::Transliteration => 4,
        RegexFamilyOperator::TransliterationAlias => 5,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedFamily {
    Regex,
    Match,
    Substitution,
    Transliteration,
}

impl ExpectedFamily {
    fn for_node(kind: &NodeKind) -> Option<Self> {
        match kind {
            NodeKind::Regex { .. } => Some(Self::Regex),
            NodeKind::Match { .. } => Some(Self::Match),
            NodeKind::Substitution { .. } => Some(Self::Substitution),
            NodeKind::Transliteration { .. } => Some(Self::Transliteration),
            _ => None,
        }
    }

    const fn accepts(self, operator: RegexFamilyOperator) -> bool {
        match self {
            Self::Regex | Self::Match => matches!(
                operator,
                RegexFamilyOperator::BareMatch
                    | RegexFamilyOperator::Match
                    | RegexFamilyOperator::QuoteRegex
            ),
            Self::Substitution => matches!(operator, RegexFamilyOperator::Substitution),
            Self::Transliteration => matches!(
                operator,
                RegexFamilyOperator::Transliteration | RegexFamilyOperator::TransliterationAlias
            ),
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Regex => 0,
            Self::Match => 1,
            Self::Substitution => 2,
            Self::Transliteration => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnavailableCandidate {
    range: SourceLocation,
    expected: ExpectedFamily,
}

/// One ordering slot: either recovered geometry or a known gap where geometry was expected.
///
/// `RegexFamilyGeometry` carries the full operator/pattern/replacement/modifier spans and is
/// roughly an order of magnitude larger than `UnavailableCandidate`, so it is boxed to keep
/// the enum's footprint proportional to the common case. These values only pass through a
/// sort and a single consuming loop, so the extra indirection is not on a hot path.
#[derive(Debug)]
enum RetentionInput {
    Geometry(Box<RegexFamilyGeometry>),
    Unavailable(UnavailableCandidate),
}

impl RetentionInput {
    fn sort_key(&self) -> (usize, usize, u8) {
        match self {
            Self::Geometry(geometry) => geometry_sort_key(geometry),
            Self::Unavailable(candidate) => (
                candidate.range.start,
                candidate.range.end,
                candidate.expected.rank().saturating_add(16),
            ),
        }
    }
}

#[derive(Debug)]
struct PendingGeometrySession {
    /// Identity of the guard that pushed this entry, so a session finished out of
    /// order cannot pop and consume a different session's geometry.
    id: u64,
    /// Address of the exact buffer this session was begun for.
    ///
    /// Length alone is not identity. Two documents of the same length would
    /// otherwise both satisfy the admission test, so a parse of one could
    /// contribute geometry to a table built from the other — spans anchored in
    /// text nobody analyzed, carrying a digest that still matches. Comparing the
    /// buffer address as well means only the parse this session actually wraps
    /// can contribute.
    source_ptr: usize,
    source_len: usize,
    geometries: Vec<RegexFamilyGeometry>,
}

impl PendingGeometrySession {
    fn for_source(id: u64, source: &str) -> Self {
        Self {
            id,
            source_ptr: source.as_ptr() as usize,
            source_len: source.len(),
            geometries: Vec::new(),
        }
    }

    fn empty() -> Self {
        Self { id: 0, source_ptr: 0, source_len: 0, geometries: Vec::new() }
    }

    /// Whether `source` is the exact buffer this session was begun for.
    fn owns(&self, source: &str) -> bool {
        self.source_ptr == source.as_ptr() as usize && self.source_len == source.len()
    }
}

/// Source of session identities. Thread-local, so no two live sessions on a thread
/// share an id and the counter never needs to be synchronized.
fn next_session_id() -> u64 {
    thread_local! {
        static NEXT_SESSION_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    }
    NEXT_SESSION_ID.with(|next| {
        let id = next.get();
        next.set(id.saturating_add(1));
        id
    })
}

#[derive(Debug)]
struct PendingGeometryGuard {
    active: bool,
    id: u64,
}

impl PendingGeometryGuard {
    fn begin(source: &str) -> Self {
        let id = next_session_id();
        ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
            sessions.borrow_mut().push(PendingGeometrySession::for_source(id, source));
        });
        Self { active: true, id }
    }

    /// Pop this guard's own session, or `None` when it is not the active one.
    ///
    /// Returning `None` rather than the stack top is the whole point: an
    /// out-of-order finish would otherwise hand this guard a *different* parse's
    /// geometry, which the caller would then anchor against its own source.
    fn finish(mut self) -> Option<PendingGeometrySession> {
        self.active = false;
        ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            match sessions.last() {
                Some(top) if top.id == self.id => sessions.pop(),
                // Not ours. Leave it for its owner rather than consuming it.
                _ => None,
            }
        })
    }
}

impl Drop for PendingGeometryGuard {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
                let mut sessions = sessions.borrow_mut();
                // Same ownership rule as `finish`: only retire our own entry.
                if sessions.last().is_some_and(|top| top.id == self.id) {
                    let _ = sessions.pop();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    #[test]
    fn containment_is_inclusive_at_both_edges_and_rejects_overhang() {
        let outer = location(4, 10);
        assert!(contains(outer, outer), "a range contains itself");
        assert!(contains(outer, location(5, 9)));
        assert!(contains(outer, location(4, 6)), "sharing the start still fits");
        assert!(contains(outer, location(8, 10)), "sharing the end still fits");
        assert!(!contains(outer, location(3, 6)), "overhanging the start does not fit");
        assert!(!contains(outer, location(8, 11)), "overhanging the end does not fit");
        assert!(!contains(outer, location(11, 12)));
    }

    #[test]
    fn geometry_ordering_is_by_position_first_and_operator_only_to_break_ties() {
        // Records must come back in source order; the operator rank exists only to make
        // two operators claiming the exact same span deterministic, never to reorder
        // across positions.
        assert!(
            operator_rank(RegexFamilyOperator::BareMatch)
                < operator_rank(RegexFamilyOperator::Match)
        );
        assert!(
            operator_rank(RegexFamilyOperator::Match)
                < operator_rank(RegexFamilyOperator::QuoteRegex)
        );
        assert!(
            operator_rank(RegexFamilyOperator::Transliteration)
                < operator_rank(RegexFamilyOperator::TransliterationAlias)
        );

        let ranks = [
            RegexFamilyOperator::BareMatch,
            RegexFamilyOperator::Match,
            RegexFamilyOperator::QuoteRegex,
            RegexFamilyOperator::Substitution,
            RegexFamilyOperator::Transliteration,
            RegexFamilyOperator::TransliterationAlias,
        ]
        .map(operator_rank);
        let mut unique = ranks.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ranks.len(), "operator ranks must be a total order");
    }

    #[test]
    fn an_expected_family_accepts_only_its_own_operators() {
        // A transliteration must never be satisfied by a match operator, or the
        // "not regex" boundary would silently disappear.
        assert!(ExpectedFamily::Regex.accepts(RegexFamilyOperator::BareMatch));
        assert!(ExpectedFamily::Regex.accepts(RegexFamilyOperator::QuoteRegex));
        assert!(!ExpectedFamily::Regex.accepts(RegexFamilyOperator::Substitution));
        assert!(!ExpectedFamily::Regex.accepts(RegexFamilyOperator::Transliteration));

        assert!(ExpectedFamily::Substitution.accepts(RegexFamilyOperator::Substitution));
        assert!(!ExpectedFamily::Substitution.accepts(RegexFamilyOperator::Match));

        assert!(ExpectedFamily::Transliteration.accepts(RegexFamilyOperator::Transliteration));
        assert!(
            ExpectedFamily::Transliteration.accepts(RegexFamilyOperator::TransliterationAlias),
            "y/// is the same family as tr///"
        );
        assert!(!ExpectedFamily::Transliteration.accepts(RegexFamilyOperator::BareMatch));
    }

    #[test]
    fn expected_family_ranks_are_distinct() {
        let ranks = [
            ExpectedFamily::Regex,
            ExpectedFamily::Match,
            ExpectedFamily::Substitution,
            ExpectedFamily::Transliteration,
        ]
        .map(ExpectedFamily::rank);
        let mut unique = ranks.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ranks.len());
    }

    #[test]
    fn geometry_recording_is_inert_outside_a_canonical_parse() {
        // Ordinary `Parser::parse_with_recovery` callers must keep the legacy
        // compatibility path. With no active session the hook reports "not handled"
        // so the caller falls through to the validator instead of silently losing
        // its diagnostics.
        assert!(!record_operator_geometry("my $x = /a/;", 8));
    }

    #[test]
    fn the_session_guard_tracks_the_session_and_costs_no_source_scan() {
        // The parser tests this before validating the source as UTF-8, so it must agree
        // with `record_operator_geometry` about whether a session is active. If it ever
        // returned `true` with no session, ordinary parses would pay a full O(source)
        // validation per regex body for a hook that then declines.
        assert!(!has_active_session());
        {
            let session = PendingGeometryGuard::begin("my $x = /a/;");
            assert!(has_active_session());
            let _ = session.finish();
        }
        assert!(!has_active_session(), "the guard must clear when the session unwinds");
    }

    #[test]
    fn a_geometry_session_is_scoped_to_its_own_source_and_unwinds_cleanly() {
        let source = "my $x = /a/;";
        {
            let session = PendingGeometryGuard::begin(source);
            // Inside the session the hook owns the operator scan.
            assert!(record_operator_geometry(source, 8));
            // A different source length cannot belong to this session; accepting it
            // would bind geometry to bytes the table was not built from.
            assert!(!record_operator_geometry("my $x = /ab/;", 8));
            let pending = session.finish().expect("the guard owns the active session");
            assert_eq!(pending.source_len, source.len());
        }
        // The session is popped on drop, so the next ordinary parse is unaffected.
        assert!(!record_operator_geometry(source, 8));
    }

    #[test]
    fn repeated_geometry_at_one_span_is_recorded_once() {
        let source = "my $x = /a/;";
        let session = PendingGeometryGuard::begin(source);
        assert!(record_operator_geometry(source, 8));
        assert!(record_operator_geometry(source, 8));
        let pending = session.finish().expect("the guard owns the active session");
        assert_eq!(
            pending.geometries.len(),
            1,
            "the same operator at the same span must not be retained twice"
        );
    }
}
