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
    analyzer::{
        CaptureDiagnosticCode, CaptureLanguageProfile, FeatureState, RegexLanguageProfile,
    },
    validator::{RegexDiagnosticClass, RegexDiagnosticCode},
};

use crate::{ParseError, ParseOutput, Parser, Token};
use crate::syntax::{
    quote_geometry::{
        RegexFamilyGeometry, RegexFamilyOperator, extract_regex_family_geometry,
    },
    regex_analysis::{
        RegexAnalysisAvailability, RegexAnalysisRecord, RegexAnalysisTable, RegexSourceDigest,
    },
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
    finish_output(source, parse_output, session.finish())
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
    finish_output(source, parse_output, session.finish())
}

/// Parse a caller-supplied token stream and retain canonical regex analysis.
///
/// This is the integration seam used by incremental/fresh equivalence proof.
#[must_use]
pub fn parse_tokens_with_regex_analysis(
    tokens: Vec<Token>,
    source: &str,
) -> RegexParseOutput {
    let session = PendingGeometryGuard::begin(source);
    let mut parser = Parser::from_tokens(tokens, source);
    let parse_output = parser.parse_with_recovery();
    finish_output(source, parse_output, session.finish())
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
        if session.source_len != source.len() {
            return false;
        }
        if let Some(text) = source.get(start..)
            && let Some(geometry) = extract_regex_family_geometry(text, start)
            && !session.geometries.iter().any(|existing| {
                existing.operator == geometry.operator
                    && existing.full_range == geometry.full_range
            })
        {
            session.geometries.push(geometry);
        }
        true
    })
}

fn finish_output(
    source: &str,
    mut parse_output: ParseOutput,
    mut pending: PendingGeometrySession,
) -> RegexParseOutput {
    let environment = CompileTimePragmaEnvironment::build(&parse_output.ast);
    let parser_geometry = pending.geometries.clone();
    let mut unavailable = Vec::new();
    collect_ast_geometry(
        &parse_output.ast,
        source,
        &parser_geometry,
        &mut pending.geometries,
        &mut unavailable,
    );
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
        .map(RetentionInput::Geometry)
        .chain(unavailable.into_iter().map(RetentionInput::Unavailable))
        .collect::<Vec<_>>();
    inputs.sort_by_key(RetentionInput::sort_key);

    let mut table = RegexAnalysisTable::for_source(source);
    for input in inputs {
        match input {
            RetentionInput::Geometry(geometry) => {
                let profile = profile_at(&environment, geometry.pattern.range.start);
                let _record = table.retain_geometry(geometry, profile);
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

    apply_ast_compatibility_flags(&mut parse_output.ast, &table);
    project_regex_diagnostics(&mut parse_output, &table);

    RegexParseOutput { parse_output, regex_analysis: table }
}

fn profile_at(
    environment: &CompileTimePragmaEnvironment,
    offset: usize,
) -> CaptureLanguageProfile {
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
    CaptureLanguageProfile::new(
        RegexLanguageProfile::new(None, enhanced_xx),
        source_utf8,
    )
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
                existing.operator == geometry.operator
                    && existing.full_range == geometry.full_range
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
        if best
            .as_ref()
            .is_none_or(|existing: &RegexFamilyGeometry| {
                geometry.full_range.start > existing.full_range.start
            })
        {
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
                    projected.push(ParseError::syntax(
                        diagnostic.message(),
                        diagnostic.range.start,
                    ));
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
                projected.push(ParseError::syntax(
                    diagnostic.message(),
                    source_range.start,
                ));
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
    parse_output
        .diagnostics
        .sort_by_key(|diagnostic| diagnostic.location().unwrap_or(usize::MAX));
    parse_output.budget_usage.errors_emitted =
        parse_output.budget_usage.errors_emitted.saturating_add(added);
}

fn contains(outer: SourceLocation, inner: SourceLocation) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn geometry_sort_key(geometry: &RegexFamilyGeometry) -> (usize, usize, u8) {
    (
        geometry.full_range.start,
        geometry.full_range.end,
        operator_rank(geometry.operator),
    )
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
                RegexFamilyOperator::Transliteration
                    | RegexFamilyOperator::TransliterationAlias
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

#[derive(Debug)]
enum RetentionInput {
    Geometry(RegexFamilyGeometry),
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
    source_len: usize,
    geometries: Vec<RegexFamilyGeometry>,
}

impl PendingGeometrySession {
    fn for_source(source: &str) -> Self {
        Self { source_len: source.len(), geometries: Vec::new() }
    }
}

struct PendingGeometryGuard {
    active: bool,
}

impl PendingGeometryGuard {
    fn begin(source: &str) -> Self {
        ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
            sessions.borrow_mut().push(PendingGeometrySession::for_source(source));
        });
        Self { active: true }
    }

    fn finish(mut self) -> PendingGeometrySession {
        let pending = ACTIVE_GEOMETRY_SESSIONS.with(|sessions| sessions.borrow_mut().pop());
        self.active = false;
        pending.unwrap_or(PendingGeometrySession { source_len: 0, geometries: Vec::new() })
    }
}

impl Drop for PendingGeometryGuard {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_GEOMETRY_SESSIONS.with(|sessions| {
                let _ = sessions.borrow_mut().pop();
            });
        }
    }
}
