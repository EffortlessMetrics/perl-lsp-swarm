use crate::analyzer::{
    CaptureAnalysis, CaptureId, CaptureNumberConfidence, CaptureProfileConfidence,
    CaptureSourceConfidence,
};
use crate::validator::RegexRange;

use super::model::{
    PatternControlDiagnosticCode, PatternControlResolution, PatternControlUnresolvedReason,
};
use super::parse::ResolutionRequest;

pub(super) fn resolve_request(
    pattern: &str,
    captures: &CaptureAnalysis,
    request: &ResolutionRequest,
    fact_range: RegexRange,
    dynamic_positions: &[usize],
    structural_positions: &[usize],
) -> PatternControlResolution {
    let (targets, missing_reason, ambiguous_plain) = match request {
        ResolutionRequest::None => return PatternControlResolution::NotApplicable,
        ResolutionRequest::Number { number, ambiguous_plain_escape } => (
            capture_targets_by_number(pattern, captures, *number),
            PatternControlUnresolvedReason::MissingCaptureNumber,
            *ambiguous_plain_escape,
        ),
        ResolutionRequest::Name(name) => (
            capture_targets_by_name(pattern, captures, name),
            PatternControlUnresolvedReason::MissingCaptureName,
            false,
        ),
        ResolutionRequest::Relative(offset) => {
            return resolve_relative(
                pattern,
                captures,
                *offset,
                fact_range,
                dynamic_positions,
                structural_positions,
            );
        }
    };

    if targets.is_empty() {
        if !dynamic_positions.is_empty() {
            return PatternControlResolution::DynamicUnknown { known_targets: targets };
        }
        if !structural_positions.is_empty() || captures.status.malformed {
            return PatternControlResolution::StructuralUnknown { known_targets: targets };
        }
        if ambiguous_plain {
            return PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::AmbiguousNumericEscape,
            );
        }
        return PatternControlResolution::Unresolved(missing_reason);
    }

    let last_relevant = targets.iter().fold(fact_range.start, |last, id| {
        captures
            .declarations
            .get(id.index())
            .map_or(last, |declaration| last.max(declaration.group_range.start))
    });
    if dynamic_positions.iter().any(|position| *position <= last_relevant) {
        return PatternControlResolution::DynamicUnknown { known_targets: targets };
    }
    if structural_positions.iter().any(|position| *position <= last_relevant) {
        return PatternControlResolution::StructuralUnknown { known_targets: targets };
    }
    resolution_from_candidate_confidence(captures, targets)
}

fn resolve_relative(
    pattern: &str,
    captures: &CaptureAnalysis,
    offset: i32,
    fact_range: RegexRange,
    dynamic_positions: &[usize],
    structural_positions: &[usize],
) -> PatternControlResolution {
    if dynamic_positions.iter().any(|position| *position < fact_range.start) {
        return PatternControlResolution::DynamicUnknown { known_targets: Vec::new() };
    }
    if structural_positions.iter().any(|position| *position < fact_range.start) {
        return PatternControlResolution::StructuralUnknown { known_targets: Vec::new() };
    }

    let preceding = captures
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.group_range.start < fact_range.start
                && !is_star_control_capture(pattern, declaration.group_range.start)
        })
        .collect::<Vec<_>>();
    if preceding.iter().any(|declaration| declaration.number.is_none()) {
        let dynamic = preceding.iter().any(|declaration| {
            declaration.confidence.number == CaptureNumberConfidence::DynamicUnknown
        });
        return if dynamic {
            PatternControlResolution::DynamicUnknown { known_targets: Vec::new() }
        } else {
            PatternControlResolution::StructuralUnknown { known_targets: Vec::new() }
        };
    }

    let next_number = preceding
        .iter()
        .filter_map(|declaration| declaration.number)
        .max()
        .unwrap_or(0)
        .checked_add(1);
    let Some(next_number) = next_number else {
        return PatternControlResolution::StructuralUnknown { known_targets: Vec::new() };
    };
    let target = if offset < 0 {
        i64::from(next_number) + i64::from(offset)
    } else {
        i64::from(next_number) + i64::from(offset) - 1
    };
    let Ok(target) = u32::try_from(target) else {
        return PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::MissingCaptureNumber,
        );
    };
    if target == 0 {
        return PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::MissingCaptureNumber,
        );
    }
    let targets = capture_targets_by_number(pattern, captures, target);
    if targets.is_empty() {
        if dynamic_positions.iter().any(|position| *position > fact_range.start) {
            return PatternControlResolution::DynamicUnknown { known_targets: targets };
        }
        return PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::MissingCaptureNumber,
        );
    }
    let last_relevant = targets.iter().fold(fact_range.start, |last, id| {
        captures
            .declarations
            .get(id.index())
            .map_or(last, |declaration| last.max(declaration.group_range.start))
    });
    if dynamic_positions.iter().any(|position| *position <= last_relevant) {
        return PatternControlResolution::DynamicUnknown { known_targets: targets };
    }
    if structural_positions.iter().any(|position| *position <= last_relevant) {
        return PatternControlResolution::StructuralUnknown { known_targets: targets };
    }
    resolution_from_candidate_confidence(captures, targets)
}

fn resolution_from_candidate_confidence(
    captures: &CaptureAnalysis,
    targets: Vec<CaptureId>,
) -> PatternControlResolution {
    let declarations = targets
        .iter()
        .filter_map(|id| captures.declarations.get(id.index()))
        .collect::<Vec<_>>();
    if declarations.iter().all(|declaration| {
        declaration.confidence.profile == CaptureProfileConfidence::Incompatible
    }) {
        return PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::ProfileIncompatible,
        );
    }
    if declarations.iter().any(|declaration| {
        declaration.confidence.source == CaptureSourceConfidence::Recovered
            || declaration.confidence.number == CaptureNumberConfidence::StructuralUnknown
    }) {
        return PatternControlResolution::StructuralUnknown { known_targets: targets };
    }
    if declarations.iter().any(|declaration| {
        declaration.confidence.number == CaptureNumberConfidence::DynamicUnknown
    }) {
        return PatternControlResolution::DynamicUnknown { known_targets: targets };
    }
    if declarations.iter().any(|declaration| {
        declaration.confidence.profile == CaptureProfileConfidence::ProfileDependent
    }) {
        return PatternControlResolution::ProfileDependent { known_targets: targets };
    }
    PatternControlResolution::Resolved { targets }
}

fn capture_targets_by_number(
    pattern: &str,
    captures: &CaptureAnalysis,
    number: u32,
) -> Vec<CaptureId> {
    captures
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.number == Some(number)
                && !is_star_control_capture(pattern, declaration.group_range.start)
        })
        .map(|declaration| declaration.id)
        .collect()
}

fn capture_targets_by_name(
    pattern: &str,
    captures: &CaptureAnalysis,
    name: &str,
) -> Vec<CaptureId> {
    captures
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.name.as_deref() == Some(name)
                && !is_star_control_capture(pattern, declaration.group_range.start)
        })
        .map(|declaration| declaration.id)
        .collect()
}

fn is_star_control_capture(pattern: &str, start: usize) -> bool {
    pattern.get(start..).is_some_and(|rest| rest.starts_with("(*"))
}

pub(super) fn diagnostic_for_resolution(
    resolution: &PatternControlResolution,
) -> Option<PatternControlDiagnosticCode> {
    match resolution {
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::InvalidOperand,
        ) => Some(PatternControlDiagnosticCode::InvalidReference),
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::MissingCaptureNumber
            | PatternControlUnresolvedReason::MissingCaptureName,
        ) => Some(PatternControlDiagnosticCode::UnresolvedReference),
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::ProfileIncompatible,
        ) => Some(PatternControlDiagnosticCode::ProfileIncompatibleReference),
        PatternControlResolution::Unresolved(
            PatternControlUnresolvedReason::AmbiguousNumericEscape,
        )
        | PatternControlResolution::NotApplicable
        | PatternControlResolution::Resolved { .. }
        | PatternControlResolution::ProfileDependent { .. }
        | PatternControlResolution::DynamicUnknown { .. }
        | PatternControlResolution::StructuralUnknown { .. } => None,
    }
}
