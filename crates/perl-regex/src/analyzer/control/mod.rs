mod model;
mod parse;
mod resolve;

pub use model::{
    PatternBoundary, PatternBoundaryKind, PatternControlAnalysis, PatternControlAnalysisStatus,
    PatternControlDiagnostic, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlFact, PatternControlId, PatternControlKind, PatternControlResolution,
    PatternControlUnresolvedReason, PatternExtendedMode, PatternModeState, PatternReferenceSyntax,
};

use crate::{
    analyzer::{CaptureLanguageProfile, CaptureMode, EffectiveModifiers, ExtendedMode},
    syntax::event::{
        RegexEmbeddedCodeKind, RegexEventBudget, RegexEventKind, RegexExtendedMode, RegexGroupKind,
        RegexModeState, parse_regex_events,
    },
    validator::RegexAnalysisBudget,
};

use model::map_source_range;
use parse::{
    RawControl, ResolutionRequest, parse_escape_control, parse_special_group_control,
    parse_star_control, unsupported_control,
};
use resolve::{diagnostic_for_resolution, resolve_request};

pub(crate) fn analyze_pattern_controls(
    pattern: &str,
    source_start: usize,
    modifiers: EffectiveModifiers,
    profile: CaptureLanguageProfile,
) -> PatternControlAnalysis {
    let captures = super::capture::analyze_captures(pattern, modifiers, profile);
    let stream = parse_regex_events(pattern, initial_mode(modifiers));
    let mut raw = Vec::<(RawControl, PatternModeState)>::new();
    let mut boundaries = Vec::<PatternBoundary>::new();
    let mut dynamic_positions = Vec::<usize>::new();
    let mut structural_positions = Vec::<usize>::new();
    let mut covered_until = 0usize;

    for event in &stream.events {
        if event.range.start < covered_until {
            continue;
        }
        match event.kind {
            RegexEventKind::Escape => {
                if let Some(parsed) = parse_escape_control(pattern, event.range.start) {
                    covered_until = parsed.range.end;
                    raw.push((parsed, public_mode(event.mode)));
                }
            }
            RegexEventKind::GroupOpen(RegexGroupKind::Special) => {
                if let Some(parsed) = parse_special_group_control(pattern, event.range.start) {
                    covered_until = parsed.range.end;
                    if parsed.boundary == Some(PatternBoundaryKind::UnsupportedControl) {
                        structural_positions.push(parsed.range.start);
                    }
                    raw.push((parsed, public_mode(event.mode)));
                } else {
                    let parsed = unsupported_control(pattern, event.range, "(?");
                    structural_positions.push(parsed.range.start);
                    raw.push((parsed, public_mode(event.mode)));
                }
            }
            RegexEventKind::GroupOpen(RegexGroupKind::Capturing | RegexGroupKind::NonCapturing)
                if pattern.get(event.range.start..).is_some_and(|rest| rest.starts_with("(*")) =>
            {
                let parsed = parse_star_control(pattern, event.range.start);
                covered_until = parsed.range.end;
                // The shared event stream does not model star-control structure: it
                // reports every `(*...)` form as a plain group open, and which of the
                // two group kinds it picks is not a property of the control itself.
                // Match on the source prefix instead, and keep later capture-number
                // resolution conservative until that authority models these directly.
                structural_positions.push(parsed.range.start);
                raw.push((parsed, public_mode(event.mode)));
            }
            RegexEventKind::EmbeddedCode { kind, .. } => {
                let (kind, effect, boundary) = match kind {
                    RegexEmbeddedCodeKind::Immediate => (
                        PatternControlKind::ImmediateEmbeddedCode,
                        PatternControlEffect::DynamicExecution,
                        PatternBoundaryKind::EmbeddedCodeExecution,
                    ),
                    RegexEmbeddedCodeKind::Deferred => (
                        PatternControlKind::DeferredRuntimePattern,
                        PatternControlEffect::DynamicPattern,
                        PatternBoundaryKind::RuntimePattern,
                    ),
                };
                if boundary == PatternBoundaryKind::RuntimePattern {
                    dynamic_positions.push(event.range.start);
                }
                let diagnostic = match boundary {
                    PatternBoundaryKind::EmbeddedCodeExecution => {
                        Some(PatternControlDiagnosticCode::EmbeddedCodeBoundary)
                    }
                    PatternBoundaryKind::RuntimePattern => {
                        Some(PatternControlDiagnosticCode::DynamicPatternBoundary)
                    }
                    _ => None,
                };
                raw.push((
                    RawControl {
                        kind,
                        range: event.range,
                        operand_range: None,
                        request: ResolutionRequest::None,
                        effect,
                        boundary: Some(boundary),
                        diagnostic,
                    },
                    public_mode(event.mode),
                ));
            }
            RegexEventKind::Interpolation => {
                dynamic_positions.push(event.range.start);
                raw.push((
                    RawControl {
                        kind: PatternControlKind::SourceInterpolation,
                        range: event.range,
                        operand_range: None,
                        request: ResolutionRequest::None,
                        effect: PatternControlEffect::DynamicPattern,
                        boundary: Some(PatternBoundaryKind::SourceInterpolation),
                        diagnostic: Some(PatternControlDiagnosticCode::DynamicPatternBoundary),
                    },
                    public_mode(event.mode),
                ));
            }
            RegexEventKind::Malformed(_) => {
                structural_positions.push(event.range.start);
                boundaries.push(PatternBoundary {
                    kind: PatternBoundaryKind::StructuralUncertainty,
                    range: event.range,
                    source_range: map_source_range(event.range, source_start),
                });
            }
            _ => {}
        }
    }

    raw.sort_by_key(|(fact, _)| (fact.range.start, fact.range.end));
    dynamic_positions.sort_unstable();
    dynamic_positions.dedup();
    structural_positions.sort_unstable();
    structural_positions.dedup();

    let mut facts = Vec::with_capacity(raw.len());
    let mut diagnostics = Vec::new();
    for (index, (item, local_mode)) in raw.into_iter().enumerate() {
        let resolution = resolve_request(
            pattern,
            &captures,
            &item.request,
            item.range,
            &dynamic_positions,
            &structural_positions,
        );
        let diagnostic = item.diagnostic.or_else(|| diagnostic_for_resolution(&resolution));
        if let Some(code) = diagnostic {
            let range = item.operand_range.unwrap_or(item.range);
            diagnostics.push(PatternControlDiagnostic::new(code, range, source_start));
        }
        if let Some(kind) = item.boundary {
            boundaries.push(PatternBoundary {
                kind,
                range: item.range,
                source_range: map_source_range(item.range, source_start),
            });
        }
        facts.push(PatternControlFact {
            id: PatternControlId(index),
            kind: item.kind,
            range: item.range,
            source_range: map_source_range(item.range, source_start),
            operand_range: item.operand_range,
            source_operand_range: item
                .operand_range
                .and_then(|range| map_source_range(range, source_start)),
            local_mode,
            modifiers,
            profile,
            resolution,
            effect: item.effect,
        });
    }

    boundaries.sort_by_key(|boundary| (boundary.range.start, boundary.range.end, boundary.kind));
    boundaries.dedup_by(|left, right| left.kind == right.kind && left.range == right.range);
    diagnostics
        .sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end, diagnostic.code));
    diagnostics.dedup_by(|left, right| left.code == right.code && left.range == right.range);

    let source_mapping_complete =
        facts.iter().all(|fact| {
            fact.source_range.is_some()
                && (fact.operand_range.is_none() || fact.source_operand_range.is_some())
        }) && boundaries.iter().all(|boundary| boundary.source_range.is_some());
    let exhausted = stream.exhausted.map(map_budget).or(captures.status.exhausted);
    let unsupported = facts.iter().any(|fact| {
        matches!(fact.effect, PatternControlEffect::Unsupported)
            || matches!(&fact.kind, PatternControlKind::Unsupported { .. })
    }) || boundaries
        .iter()
        .any(|boundary| boundary.kind == PatternBoundaryKind::UnsupportedControl);
    let dynamic_execution =
        facts.iter().any(|fact| matches!(fact.effect, PatternControlEffect::DynamicExecution));
    let dynamic_pattern = !dynamic_positions.is_empty() || captures.status.dynamic;
    let structural_uncertainty = !structural_positions.is_empty();
    let malformed = stream.malformed || captures.status.malformed;

    PatternControlAnalysis {
        modifiers,
        profile,
        captures,
        facts,
        boundaries,
        diagnostics,
        status: PatternControlAnalysisStatus {
            dynamic_pattern,
            dynamic_execution,
            unsupported,
            structural_uncertainty,
            malformed,
            exhausted,
            source_mapping_complete,
        },
    }
}

fn public_mode(mode: RegexModeState) -> PatternModeState {
    let extended = match mode.extended {
        RegexExtendedMode::Off => PatternExtendedMode::Off,
        RegexExtendedMode::Extended => PatternExtendedMode::Extended,
        RegexExtendedMode::ExtraExtended => PatternExtendedMode::ExtraExtended,
    };
    PatternModeState { extended, captures_by_default: mode.captures_by_default }
}

fn initial_mode(modifiers: EffectiveModifiers) -> RegexModeState {
    let extended = match modifiers.extended {
        ExtendedMode::Off => RegexExtendedMode::Off,
        ExtendedMode::Extended => RegexExtendedMode::Extended,
        ExtendedMode::ExtraExtended { .. } => RegexExtendedMode::ExtraExtended,
    };
    RegexModeState {
        extended,
        captures_by_default: matches!(modifiers.captures, CaptureMode::CapturingByDefault),
    }
}

fn map_budget(budget: RegexEventBudget) -> RegexAnalysisBudget {
    match budget {
        RegexEventBudget::Events => RegexAnalysisBudget::Events,
        RegexEventBudget::Nesting => RegexAnalysisBudget::Nesting,
        RegexEventBudget::Steps => RegexAnalysisBudget::Steps,
    }
}
