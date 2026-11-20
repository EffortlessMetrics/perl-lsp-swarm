//! Same-pattern capture definition and references from retained regex analysis (#7030).

use perl_parser_core::{RegexAnalysisAvailability, RegexAnalysisRecord, RegexAnalysisTable};
use perl_regex::analyzer::{
    CaptureAnalysis, CaptureId, PatternControlFact, PatternControlId, PatternControlKind,
    PatternControlResolution,
};
use perl_regex::validator::RegexRange;

/// Inclusive-exclusive byte span in the immutable source snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceByteSpan {
    /// Span start byte offset.
    pub start: usize,
    /// Span end byte offset.
    pub end: usize,
}

/// One definition answer when the client supports `LocationLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexCaptureDefinitionLink {
    /// Origin selection range (typically the reference operand).
    pub origin_selection: SourceByteSpan,
    /// Enclosing target range (declaration group).
    pub target_range: SourceByteSpan,
    /// Target selection range (capture name or group).
    pub target_selection: SourceByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternFocus {
    Capture(CaptureId),
    Control(PatternControlId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureIdentity {
    Named { family_index: usize },
    Number(u32),
}

/// Whether `offset` lies in a retained regex pattern body for this generation.
#[must_use]
pub fn record_at_pattern_body_offset<'a>(
    source: &str,
    table: &'a RegexAnalysisTable,
    offset: usize,
) -> Option<&'a RegexAnalysisRecord> {
    if !table.source_matches(source) {
        return None;
    }
    let record = table.find_at_offset(offset)?;
    let range = record.pattern_range()?;
    (range.start <= offset && offset < range.end).then_some(record)
}

/// Resolve in-pattern capture definitions at `offset`, or fail closed.
#[must_use]
pub fn definition_links_at(
    source: &str,
    table: &RegexAnalysisTable,
    offset: usize,
) -> Option<Vec<RegexCaptureDefinitionLink>> {
    let record = eligible_record(source, table, offset)?;
    let focus = pattern_focus(record, offset)?;
    let pattern = record.pattern.as_ref()?;
    match focus {
        PatternFocus::Control(fact_id) => definition_from_control(record, pattern, fact_id),
        PatternFocus::Capture(id) => {
            let identity = identity_for_declaration(&pattern.controls.captures, id)?;
            definition_from_declaration(record, &pattern.controls.captures, identity)
        }
    }
}

/// Resolve in-pattern capture references at `offset`, or fail closed.
#[must_use]
pub fn reference_spans_at(
    source: &str,
    table: &RegexAnalysisTable,
    offset: usize,
    include_declaration: bool,
) -> Option<Vec<SourceByteSpan>> {
    let record = eligible_record(source, table, offset)?;
    let focus = pattern_focus(record, offset)?;
    let pattern = record.pattern.as_ref()?;
    let identity = match focus {
        PatternFocus::Capture(id) => identity_for_declaration(&pattern.controls.captures, id)?,
        PatternFocus::Control(fact_id) => {
            let fact = pattern.controls.facts.iter().find(|f| f.id == fact_id)?;
            identity_for_resolved_reference(&pattern.controls.captures, fact)?
        }
    };
    Some(collect_reference_spans(
        record,
        &pattern.controls.captures,
        &pattern.controls.facts,
        identity,
        include_declaration,
    ))
}

fn eligible_record<'a>(
    source: &str,
    table: &'a RegexAnalysisTable,
    offset: usize,
) -> Option<&'a RegexAnalysisRecord> {
    if !table.source_matches(source) {
        return None;
    }
    let record = table.find_at_offset(offset)?;
    let pattern_range = record.pattern_range()?;
    if offset < pattern_range.start || offset >= pattern_range.end {
        return None;
    }
    if record.availability != RegexAnalysisAvailability::Analyzed {
        return None;
    }
    let pattern = record.pattern.as_ref()?;
    if !navigation_exact(record, pattern) {
        return None;
    }
    Some(record)
}

fn navigation_exact(
    record: &RegexAnalysisRecord,
    pattern: &perl_parser_core::RetainedRegexPatternAnalysis,
) -> bool {
    pattern.controls.status.is_complete()
        && pattern.controls.captures.status.is_complete()
        && pattern.structural.completeness.is_complete()
        && !pattern.structural.malformed
        && pattern.structural.exhausted.is_none()
        && pattern.controls.facts.iter().all(|fact| fact_resolution_is_navigable(&fact.resolution))
        && record
            .geometry
            .as_ref()
            .is_some_and(|geometry| geometry.pattern.range.end <= record.full_range.end)
}

fn pattern_focus(record: &RegexAnalysisRecord, offset: usize) -> Option<PatternFocus> {
    let pattern = record.pattern.as_ref()?;
    for fact in &pattern.controls.facts {
        let Some(source_range) = fact.source_range else {
            continue;
        };
        if span_contains(
            SourceByteSpan { start: source_range.start, end: source_range.end },
            offset,
        ) {
            return Some(PatternFocus::Control(fact.id));
        }
    }
    for declaration in &pattern.controls.captures.declarations {
        if declaration
            .name_range
            .and_then(|range| mapped_body_span(record, range))
            .is_some_and(|span| span_contains(span, offset))
        {
            return Some(PatternFocus::Capture(declaration.id));
        }
        if span_contains(mapped_body_span(record, declaration.group_range)?, offset) {
            return Some(PatternFocus::Capture(declaration.id));
        }
    }
    None
}

fn definition_from_control(
    record: &RegexAnalysisRecord,
    pattern: &perl_parser_core::RetainedRegexPatternAnalysis,
    fact_id: PatternControlId,
) -> Option<Vec<RegexCaptureDefinitionLink>> {
    let fact = pattern.controls.facts.iter().find(|f| f.id == fact_id)?;
    if !fact_is_capture_reference(fact) {
        return None;
    }
    let targets = resolution_target_ids(&fact.resolution)?;
    let origin = reference_span(record, fact)?;
    let mut links = Vec::new();
    for target in targets {
        let Some(declaration) = pattern.controls.captures.declarations.get(target.index()) else {
            continue;
        };
        let target_range = mapped_body_span(record, declaration.group_range)?;
        let target_selection = declaration
            .name_range
            .and_then(|range| mapped_body_span(record, range))
            .unwrap_or(target_range);
        links.push(RegexCaptureDefinitionLink {
            origin_selection: origin,
            target_range,
            target_selection,
        });
    }
    if links.is_empty() { None } else { Some(links) }
}

fn definition_from_declaration(
    record: &RegexAnalysisRecord,
    captures: &CaptureAnalysis,
    identity: CaptureIdentity,
) -> Option<Vec<RegexCaptureDefinitionLink>> {
    let mut links = Vec::new();
    for declaration in &captures.declarations {
        if !declaration_in_identity(captures, identity, declaration.id) {
            continue;
        }
        let target_range = mapped_body_span(record, declaration.group_range)?;
        let target_selection = declaration
            .name_range
            .and_then(|range| mapped_body_span(record, range))
            .unwrap_or(target_range);
        links.push(RegexCaptureDefinitionLink {
            origin_selection: target_selection,
            target_range,
            target_selection,
        });
    }
    if links.is_empty() { None } else { Some(links) }
}

fn identity_for_resolved_reference(
    captures: &CaptureAnalysis,
    fact: &PatternControlFact,
) -> Option<CaptureIdentity> {
    if !fact_is_capture_reference(fact) {
        return None;
    }
    let targets = resolution_target_ids(&fact.resolution)?;
    identity_for_declaration(captures, *targets.first()?)
}

fn collect_reference_spans(
    record: &RegexAnalysisRecord,
    captures: &CaptureAnalysis,
    facts: &[PatternControlFact],
    identity: CaptureIdentity,
    include_declaration: bool,
) -> Vec<SourceByteSpan> {
    let mut spans = Vec::new();
    if include_declaration {
        for declaration in &captures.declarations {
            if !declaration_in_identity(captures, identity, declaration.id) {
                continue;
            }
            if let Some(span) = mapped_body_span(record, declaration.group_range) {
                spans.push(span);
            }
        }
    }
    for fact in facts {
        if !fact_is_capture_reference(fact) {
            continue;
        }
        let Some(targets) = resolution_target_ids(&fact.resolution) else {
            continue;
        };
        if !targets.iter().any(|id| declaration_in_identity(captures, identity, *id)) {
            continue;
        }
        if let Some(span) = reference_span(record, fact) {
            spans.push(span);
        }
    }
    spans.sort_by_key(|span| (span.start, span.end));
    spans.dedup();
    spans
}

fn identity_for_declaration(captures: &CaptureAnalysis, id: CaptureId) -> Option<CaptureIdentity> {
    let declaration = captures.declarations.get(id.index())?;
    if let Some(name) = &declaration.name {
        for (index, family) in captures.named_families.iter().enumerate() {
            if family.name == *name {
                return Some(CaptureIdentity::Named { family_index: index });
            }
        }
        return None;
    }
    declaration.number.map(CaptureIdentity::Number)
}

fn declaration_in_identity(
    captures: &CaptureAnalysis,
    identity: CaptureIdentity,
    id: CaptureId,
) -> bool {
    match identity {
        CaptureIdentity::Named { family_index } => captures
            .named_families
            .get(family_index)
            .is_some_and(|family| family.declarations.contains(&id)),
        CaptureIdentity::Number(number) => captures
            .declarations
            .get(id.index())
            .is_some_and(|declaration| declaration.number == Some(number)),
    }
}

fn fact_is_capture_reference(fact: &PatternControlFact) -> bool {
    matches!(
        fact.kind,
        PatternControlKind::NumericBackreference { .. }
            | PatternControlKind::NamedBackreference { .. }
            | PatternControlKind::RelativeBackreference { .. }
            | PatternControlKind::NumberedSubpatternCall { .. }
            | PatternControlKind::NamedSubpatternCall { .. }
            | PatternControlKind::RelativeSubpatternCall { .. }
    )
}

fn mapped_body_span(record: &RegexAnalysisRecord, range: RegexRange) -> Option<SourceByteSpan> {
    let mapped = record.map_pattern_range(range)?;
    Some(SourceByteSpan { start: mapped.start, end: mapped.end })
}

fn reference_span(
    record: &RegexAnalysisRecord,
    fact: &PatternControlFact,
) -> Option<SourceByteSpan> {
    if let Some(operand) = fact.source_operand_range {
        let end = operand.end;
        let start = operand.start;
        if start <= end {
            return Some(SourceByteSpan { start, end });
        }
    }
    fact.source_range.map(|range| SourceByteSpan { start: range.start, end: range.end })
}

fn fact_resolution_is_navigable(resolution: &PatternControlResolution) -> bool {
    matches!(
        resolution,
        PatternControlResolution::NotApplicable
            | PatternControlResolution::Resolved { .. }
            | PatternControlResolution::ProfileDependent { .. }
    )
}

fn resolution_target_ids(resolution: &PatternControlResolution) -> Option<&[CaptureId]> {
    match resolution {
        PatternControlResolution::Resolved { targets } => Some(targets.as_slice()),
        PatternControlResolution::ProfileDependent { known_targets }
            if !known_targets.is_empty() =>
        {
            Some(known_targets.as_slice())
        }
        _ => None,
    }
}

fn span_contains(span: SourceByteSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use perl_parser::Parser;
    use perl_parser_core::{RegexAnalysisTable, RetainedRegexSession};

    use super::*;

    fn parse_table(source: &str) -> Arc<RegexAnalysisTable> {
        let session = RetainedRegexSession::begin(source);
        let mut parser = Parser::new(source);
        match parser.parse() {
            Ok(mut ast) => Arc::new(session.finish(Some(&mut ast))),
            Err(_) => Arc::new(session.finish(None)),
        }
    }

    fn offset_in(source: &str, needle: &str) -> usize {
        source.find(needle).unwrap_or_else(|| panic!("needle {needle:?} missing"))
    }

    #[test]
    fn named_backreference_definition_targets_declaration() {
        let source = r"my $re = qr/(?<name>foo)\k<name>/;";
        let table = parse_table(source);
        let use_offset = offset_in(source, r"\k<name>");
        let links = definition_links_at(source, &table, use_offset).expect("definition");
        assert_eq!(links.len(), 1);
        let name_offset = offset_in(source, "name");
        assert!(links[0].target_selection.start <= name_offset);
        assert!(name_offset < links[0].target_selection.end);
    }

    #[test]
    fn duplicate_named_definitions_return_every_target() {
        let source = r"my $re = qr/(?<x>a)(?<x>b)\k<x>/;";
        let table = parse_table(source);
        let use_offset = offset_in(source, r"\k<x>");
        let links = definition_links_at(source, &table, use_offset).expect("definition");
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn references_include_declaration_when_requested() {
        let source = r"my $re = qr/(?<x>a)\k<x>/;";
        let table = parse_table(source);
        let decl_offset = offset_in(source, "(?<x>a)");
        let with = reference_spans_at(source, &table, decl_offset, true).expect("refs");
        assert!(with.len() >= 2);
        let without = reference_spans_at(source, &table, decl_offset, false).expect("refs");
        assert!(without.len() < with.len());
    }

    #[test]
    fn dynamic_pattern_returns_no_navigation() {
        let source = r"my $part = 'a'; my $re = qr/$part/;";
        let table = parse_table(source);
        let offset = offset_in(source, "$part");
        assert!(definition_links_at(source, &table, offset).is_none());
        assert!(reference_spans_at(source, &table, offset, true).is_none());
    }

    #[test]
    fn two_regexes_with_same_name_stay_isolated() {
        let source = r"my $a = qr/(?<x>a)/; my $b = qr/(?<x>b)\k<x>/;";
        let table = parse_table(source);
        let second_use = source.rfind(r"\k<x>").expect("second use");
        let refs = reference_spans_at(source, &table, second_use, true).expect("refs");
        assert_eq!(refs.len(), 2);
        let first_decl = offset_in(source, "(?<x>a)");
        assert!(!refs.iter().any(|span| span.start <= first_decl && first_decl < span.end));
    }

    #[test]
    fn unrelated_pattern_text_returns_none() {
        let source = r"my $re = qr/foo bar/;";
        let table = parse_table(source);
        let offset = offset_in(source, "bar");
        assert!(definition_links_at(source, &table, offset).is_none());
    }
}
