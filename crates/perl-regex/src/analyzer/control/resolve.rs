use crate::analyzer::{
    CaptureAnalysis, CaptureId, CaptureNumberConfidence, CaptureProfileConfidence,
    CaptureSourceConfidence,
};
use crate::validator::RegexRange;

use super::model::{
    PatternControlDiagnosticCode, PatternControlResolution, PatternControlUnresolvedReason,
};
use super::parse::{ResolutionRequest, starts_star_control};

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
        ResolutionRequest::Number { number, ambiguous_plain_escape } => {
            let mut targets = capture_targets_by_number(pattern, captures, *number);
            if *ambiguous_plain_escape {
                targets.retain(|id| {
                    captures
                        .declarations
                        .get(id.index())
                        .is_some_and(|declaration| declaration.group_range.start < fact_range.start)
                });
            }
            (targets, PatternControlUnresolvedReason::MissingCaptureNumber, *ambiguous_plain_escape)
        }
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
                && !starts_star_control(pattern, declaration.group_range.start)
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
        // A later star control or malformed region leaves the forward capture numbering
        // unknown, so a missing target there is not evidence of a missing capture. Fail
        // closed the same way the dynamic branch above does instead of reporting a hard
        // unresolved-reference diagnostic against numbering this analysis cannot claim.
        if structural_positions.iter().any(|position| *position > fact_range.start) {
            return PatternControlResolution::StructuralUnknown { known_targets: targets };
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
    let declarations =
        targets.iter().filter_map(|id| captures.declarations.get(id.index())).collect::<Vec<_>>();
    if declarations
        .iter()
        .all(|declaration| declaration.confidence.profile == CaptureProfileConfidence::Incompatible)
    {
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
    if declarations
        .iter()
        .any(|declaration| declaration.confidence.number == CaptureNumberConfidence::DynamicUnknown)
    {
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
                && !starts_star_control(pattern, declaration.group_range.start)
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
                && !starts_star_control(pattern, declaration.group_range.start)
        })
        .map(|declaration| declaration.id)
        .collect()
}

pub(super) fn diagnostic_for_resolution(
    resolution: &PatternControlResolution,
) -> Option<PatternControlDiagnosticCode> {
    match resolution {
        PatternControlResolution::Unresolved(PatternControlUnresolvedReason::InvalidOperand) => {
            Some(PatternControlDiagnosticCode::InvalidReference)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{
        CaptureLanguageProfile, EffectiveModifiers, FeatureState, PerlVersion,
        RegexLanguageProfile, capture::analyze_captures,
    };

    fn captures_of(pattern: &str) -> CaptureAnalysis {
        let profile = CaptureLanguageProfile::new(
            RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), FeatureState::Disabled),
            FeatureState::Enabled,
        );
        analyze_captures(pattern, EffectiveModifiers::default(), profile)
    }

    fn at(pattern: &str, needle: &str) -> RegexRange {
        let start = pattern.find(needle).expect("needle present in pattern");
        RegexRange { start, end: start + needle.len() }
    }

    fn resolved_indexes(resolution: &PatternControlResolution) -> Vec<usize> {
        match resolution {
            PatternControlResolution::Resolved { targets } => {
                targets.iter().map(|id| id.index()).collect()
            }
            _ => Vec::new(),
        }
    }

    #[test]
    fn no_request_is_not_applicable_rather_than_unresolved() {
        // `\K` and friends carry no operand; "nothing to resolve" and "resolved to
        // nothing" are different answers and must not be conflated.
        let resolution = resolve_request(
            "abc",
            &captures_of("abc"),
            &ResolutionRequest::None,
            RegexRange { start: 0, end: 1 },
            &[],
            &[],
        );
        assert_eq!(resolution, PatternControlResolution::NotApplicable);
    }

    #[test]
    fn a_numeric_request_resolves_to_the_declaration_with_that_number() {
        let pattern = r"(a)(b)\2";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Number { number: 2, ambiguous_plain_escape: false },
            at(pattern, r"\2"),
            &[],
            &[],
        );
        assert_eq!(resolved_indexes(&resolution), vec![1]);
    }

    #[test]
    fn a_named_request_keeps_every_duplicate_declaration() {
        // Perl permits duplicate names; selecting one arbitrarily would invent a fact.
        let pattern = r"(?<x>a)(?<x>b)\k<x>";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Name("x".to_string()),
            at(pattern, r"\k<x>"),
            &[],
            &[],
        );
        assert_eq!(resolved_indexes(&resolution), vec![0, 1]);
    }

    #[test]
    fn a_missing_target_is_unresolved_with_the_matching_reason() {
        let pattern = r"(a)\9";
        let missing_number = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Number { number: 9, ambiguous_plain_escape: false },
            at(pattern, r"\9"),
            &[],
            &[],
        );
        assert_eq!(
            missing_number,
            PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::MissingCaptureNumber
            )
        );

        let named = r"(a)\k<nope>";
        let missing_name = resolve_request(
            named,
            &captures_of(named),
            &ResolutionRequest::Name("nope".to_string()),
            at(named, r"\k<nope>"),
            &[],
            &[],
        );
        assert_eq!(
            missing_name,
            PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::MissingCaptureName
            )
        );
    }

    #[test]
    fn dynamic_and_structural_positions_fail_a_missing_target_closed() {
        // With runtime or malformed text in play, a missing declaration is not evidence
        // that the capture does not exist, so it must not become a hard error.
        let pattern = r"(a)\9";
        let dynamic = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Number { number: 9, ambiguous_plain_escape: false },
            at(pattern, r"\9"),
            &[0],
            &[],
        );
        assert!(matches!(dynamic, PatternControlResolution::DynamicUnknown { .. }));

        let structural = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Number { number: 9, ambiguous_plain_escape: false },
            at(pattern, r"\9"),
            &[],
            &[0],
        );
        assert!(matches!(structural, PatternControlResolution::StructuralUnknown { .. }));
    }

    #[test]
    fn an_ambiguous_plain_escape_only_accepts_targets_declared_before_it() {
        // `\12` could be capture 12 or capture 1 followed by a literal `2`. A target
        // that opens after the escape cannot be the one meant, so it is dropped, and
        // the ambiguity is reported rather than guessed.
        let pattern = r"\1(a)";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Number { number: 1, ambiguous_plain_escape: true },
            at(pattern, r"\1"),
            &[],
            &[],
        );
        assert_eq!(
            resolution,
            PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::AmbiguousNumericEscape
            )
        );
    }

    #[test]
    fn relative_numbering_counts_from_the_highest_preceding_capture_number() {
        // This pins the `next_number` computation: with two captures open before the
        // reference, `(?-1)` must select the second, not the first. Taking the count
        // instead of the maximum, or the first instead of the last, both break here.
        let pattern = "(a)(b)(?-1)";
        let back_one = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Relative(-1),
            at(pattern, "(?-1)"),
            &[],
            &[],
        );
        assert_eq!(resolved_indexes(&back_one), vec![1]);

        let back_two = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Relative(-2),
            at(pattern, "(?-1)"),
            &[],
            &[],
        );
        assert_eq!(resolved_indexes(&back_two), vec![0]);
    }

    #[test]
    fn a_forward_relative_reference_resolves_to_a_later_capture() {
        let pattern = "(?+1)(a)";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Relative(1),
            at(pattern, "(?+1)"),
            &[],
            &[],
        );
        assert_eq!(resolved_indexes(&resolution), vec![0]);
    }

    #[test]
    fn a_relative_reference_past_either_end_stays_unresolved() {
        let pattern = "(a)(?-9)";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Relative(-9),
            at(pattern, "(?-9)"),
            &[],
            &[],
        );
        assert_eq!(
            resolution,
            PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::MissingCaptureNumber
            )
        );
    }

    #[test]
    fn earlier_dynamic_text_makes_relative_numbering_unknown() {
        // Interpolated text before the reference can open its own groups, so the
        // relative count is no longer knowable from this source.
        let pattern = "(a)(?-1)";
        let resolution = resolve_request(
            pattern,
            &captures_of(pattern),
            &ResolutionRequest::Relative(-1),
            at(pattern, "(?-1)"),
            &[0],
            &[],
        );
        assert!(matches!(resolution, PatternControlResolution::DynamicUnknown { .. }));
    }

    #[test]
    fn diagnostics_are_emitted_only_for_resolutions_that_need_repair() {
        assert_eq!(
            diagnostic_for_resolution(&PatternControlResolution::Resolved { targets: Vec::new() }),
            None
        );
        assert_eq!(diagnostic_for_resolution(&PatternControlResolution::NotApplicable), None);
        assert_eq!(
            diagnostic_for_resolution(&PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::MissingCaptureName
            )),
            Some(PatternControlDiagnosticCode::UnresolvedReference)
        );
        assert_eq!(
            diagnostic_for_resolution(&PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::InvalidOperand
            )),
            Some(PatternControlDiagnosticCode::InvalidReference)
        );
        assert_eq!(
            diagnostic_for_resolution(&PatternControlResolution::Unresolved(
                PatternControlUnresolvedReason::ProfileIncompatible
            )),
            Some(PatternControlDiagnosticCode::ProfileIncompatibleReference)
        );
    }
}
