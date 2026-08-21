use crate::{
    analyzer::{CaptureAnalysis, CaptureId, CaptureLanguageProfile, EffectiveModifiers},
    validator::{RegexAnalysisBudget, RegexDiagnosticClass, RegexRange},
};

/// Stable identifier for one pattern-control fact in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PatternControlId(pub(super) usize);

impl PatternControlId {
    /// Return the zero-based fact index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Source spelling used by a capture reference or subpattern call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternReferenceSyntax {
    /// Traditional numeric escape such as `\\1`.
    PlainNumeric,
    /// `\\g{...}` or an equivalent `g` form.
    GReference,
    /// `\\k<name>`, `\\k'name'`, or `\\k{name}`.
    KReference,
    /// Python-compatible `(?P=name)` backreference.
    PythonBackreference,
    /// Parenthesized subpattern call such as `(?1)` or `(?&name)`.
    SubpatternCall,
    /// Conditional predicate such as `(?(1)...)`.
    Conditional,
}

/// Pattern-control or reference construct recognized by static analysis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternControlKind {
    /// `\\K` resets the reported full-match start.
    KeepAnchor,
    /// Absolute numbered backreference.
    NumericBackreference {
        /// Referenced capture number.
        number: u32,
        /// Source spelling family.
        syntax: PatternReferenceSyntax,
    },
    /// Named backreference.
    NamedBackreference {
        /// Referenced capture name.
        name: String,
        /// Source spelling family.
        syntax: PatternReferenceSyntax,
    },
    /// Relative backreference.
    RelativeBackreference {
        /// Signed relative capture offset.
        offset: i32,
        /// Source spelling family.
        syntax: PatternReferenceSyntax,
    },
    /// Whole-pattern recursion, such as `(?R)` or `(?0)`.
    WholePatternRecursion,
    /// Absolute numbered subpattern call.
    NumberedSubpatternCall {
        /// Called capture number.
        number: u32,
    },
    /// Named subpattern call.
    NamedSubpatternCall {
        /// Called capture name.
        name: String,
    },
    /// Relative subpattern call.
    RelativeSubpatternCall {
        /// Signed relative capture offset.
        offset: i32,
    },
    /// Capture-participation conditional using a number.
    CaptureConditionalNumber {
        /// Tested capture number.
        number: u32,
    },
    /// Capture-participation conditional using a name.
    CaptureConditionalName {
        /// Tested capture name.
        name: String,
    },
    /// Conditional whose predicate is recursion state.
    RecursionConditional,
    /// Immediate embedded Perl code `(?{ ... })`.
    ImmediateEmbeddedCode,
    /// Optimistic embedded Perl code `(*{ ... })`.
    OptimisticEmbeddedCode,
    /// Deferred runtime-supplied regex text `(??{ ... })`.
    DeferredRuntimePattern,
    /// Source interpolation or another parser-observed dynamic island.
    SourceInterpolation,
    /// Recognized syntax outside the modeled static subset.
    Unsupported {
        /// Bounded source spelling used to identify the construct.
        spelling: String,
    },
}

impl PatternControlKind {
    /// Stable machine token for the fact family, excluding payload values.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::KeepAnchor => "keep_anchor",
            Self::NumericBackreference { .. } => "numeric_backreference",
            Self::NamedBackreference { .. } => "named_backreference",
            Self::RelativeBackreference { .. } => "relative_backreference",
            Self::WholePatternRecursion => "whole_pattern_recursion",
            Self::NumberedSubpatternCall { .. } => "numbered_subpattern_call",
            Self::NamedSubpatternCall { .. } => "named_subpattern_call",
            Self::RelativeSubpatternCall { .. } => "relative_subpattern_call",
            Self::CaptureConditionalNumber { .. } => "capture_conditional_number",
            Self::CaptureConditionalName { .. } => "capture_conditional_name",
            Self::RecursionConditional => "recursion_conditional",
            Self::ImmediateEmbeddedCode => "immediate_embedded_code",
            Self::OptimisticEmbeddedCode => "optimistic_embedded_code",
            Self::DeferredRuntimePattern => "deferred_runtime_pattern",
            Self::SourceInterpolation => "source_interpolation",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

/// Semantic role a pattern-control fact plays for downstream consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternControlEffect {
    /// Changes the reported full-match range without changing capture identity.
    ReportedMatchStart,
    /// Reads one or more capture declarations.
    CaptureRead,
    /// Calls another statically identified regex group.
    SubpatternCall,
    /// Selects a branch from capture or recursion state.
    ConditionalControl,
    /// Executes Perl while matching but does not supply new pattern text.
    DynamicExecution,
    /// Can supply new pattern structure at runtime.
    DynamicPattern,
    /// Recognized but deliberately outside the modeled subset.
    Unsupported,
}

impl PatternControlEffect {
    /// Stable machine token for receipts and downstream adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReportedMatchStart => "reported_match_start",
            Self::CaptureRead => "capture_read",
            Self::SubpatternCall => "subpattern_call",
            Self::ConditionalControl => "conditional_control",
            Self::DynamicExecution => "dynamic_execution",
            Self::DynamicPattern => "dynamic_pattern",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Why a statically requested capture target could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternControlUnresolvedReason {
    /// No source-backed capture has the requested number.
    MissingCaptureNumber,
    /// No source-backed capture has the requested name.
    MissingCaptureName,
    /// The operand spelling is malformed or outside the accepted reference grammar.
    InvalidOperand,
    /// A multi-digit traditional escape may instead be an octal escape.
    AmbiguousNumericEscape,
    /// Matching declarations exist but are incompatible with the supplied language profile.
    ProfileIncompatible,
}

impl PatternControlUnresolvedReason {
    /// Stable machine token for diagnostics and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCaptureNumber => "missing_capture_number",
            Self::MissingCaptureName => "missing_capture_name",
            Self::InvalidOperand => "invalid_operand",
            Self::AmbiguousNumericEscape => "ambiguous_numeric_escape",
            Self::ProfileIncompatible => "profile_incompatible",
        }
    }
}

/// Resolution of a capture reference, subpattern call, or capture conditional.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternControlResolution {
    /// The construct does not target a capture declaration.
    NotApplicable,
    /// Every statically applicable target is known.
    Resolved {
        /// Target declarations in source order. Branch-reset and duplicate-name
        /// forms may legitimately contain more than one declaration.
        targets: Vec<CaptureId>,
    },
    /// Known declarations are retained, but version or source-profile state is incomplete.
    ProfileDependent {
        /// Currently known target declarations.
        known_targets: Vec<CaptureId>,
    },
    /// Runtime pattern text can add or renumber candidate declarations.
    DynamicUnknown {
        /// Statically visible candidates before applying the dynamic boundary.
        known_targets: Vec<CaptureId>,
    },
    /// Malformed or unsupported structure prevents an exact target set.
    StructuralUnknown {
        /// Statically visible candidates before applying structural uncertainty.
        known_targets: Vec<CaptureId>,
    },
    /// The complete static prefix establishes no valid target.
    Unresolved(PatternControlUnresolvedReason),
}

impl PatternControlResolution {
    /// Whether the target set is exact under the supplied source/profile context.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::NotApplicable | Self::Resolved { .. })
    }
}

/// Effective extended-whitespace mode at a fact's source position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternExtendedMode {
    /// Extended mode is disabled.
    Off,
    /// `/x` semantics are active.
    Extended,
    /// `/xx` semantics are active.
    ExtraExtended,
}

/// Effective local modes carried by one pattern-control fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PatternModeState {
    /// Local extended-whitespace mode.
    pub extended: PatternExtendedMode,
    /// Whether an ordinary group captures by default at this position.
    pub captures_by_default: bool,
}

/// One source-backed pattern-control fact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternControlFact {
    /// Stable identity within this analysis.
    pub id: PatternControlId,
    /// Construct kind and operand.
    pub kind: PatternControlKind,
    /// Exact body-relative byte range.
    pub range: RegexRange,
    /// Exact original-source range when `source_start + range` did not overflow.
    pub source_range: Option<RegexRange>,
    /// Operand token range, excluding delimiters, when one exists.
    pub operand_range: Option<RegexRange>,
    /// Original-source operand range when it can be mapped exactly.
    pub source_operand_range: Option<RegexRange>,
    /// Effective local `/x`/`xx` and `/n` state at the construct.
    pub local_mode: PatternModeState,
    /// Lossless/effective suffix modifiers supplied for this analysis.
    pub modifiers: EffectiveModifiers,
    /// Perl language and source-UTF-8 profile used for resolution.
    pub profile: CaptureLanguageProfile,
    /// Static capture-target resolution.
    pub resolution: PatternControlResolution,
    /// Downstream semantic role.
    pub effect: PatternControlEffect,
}

/// Dynamic or unsupported boundary exposed by pattern-control analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PatternBoundaryKind {
    /// Immediate or optimistic embedded Perl execution.
    EmbeddedCodeExecution,
    /// Deferred runtime-supplied pattern text.
    RuntimePattern,
    /// Source interpolation or another dynamic source island.
    SourceInterpolation,
    /// Recognized control syntax outside the modeled subset.
    UnsupportedControl,
    /// Malformed/truncated structure prevents an exact interpretation.
    StructuralUncertainty,
}

impl PatternBoundaryKind {
    /// Stable machine token for cross-layer boundary mapping.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedCodeExecution => "embedded_code_execution",
            Self::RuntimePattern => "runtime_pattern",
            Self::SourceInterpolation => "source_interpolation",
            Self::UnsupportedControl => "unsupported_pattern_control",
            Self::StructuralUncertainty => "structural_uncertainty",
        }
    }
}

/// One source-backed dynamic or unsupported boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternBoundary {
    /// Stable boundary kind.
    pub kind: PatternBoundaryKind,
    /// Body-relative boundary range.
    pub range: RegexRange,
    /// Original-source boundary range when exactly mappable.
    pub source_range: Option<RegexRange>,
}

/// Stable diagnostic identity for pattern-control analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PatternControlDiagnosticCode {
    /// A reference operand is malformed.
    InvalidReference,
    /// A complete static pattern has no declaration matching the reference.
    UnresolvedReference,
    /// The requested declaration exists only in a profile-incompatible form.
    ProfileIncompatibleReference,
    /// Pattern text is supplied or changed at runtime.
    DynamicPatternBoundary,
    /// Perl code executes while the regex is evaluated.
    EmbeddedCodeBoundary,
    /// A valid or potentially valid construct is outside the modeled subset.
    UnsupportedControl,
}

impl PatternControlDiagnosticCode {
    /// Stable machine token for catalogs and provider projection.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidReference => "invalid_pattern_reference",
            Self::UnresolvedReference => "unresolved_pattern_reference",
            Self::ProfileIncompatibleReference => "profile_incompatible_pattern_reference",
            Self::DynamicPatternBoundary => "dynamic_pattern_boundary",
            Self::EmbeddedCodeBoundary => "embedded_code_boundary",
            Self::UnsupportedControl => "unsupported_pattern_control",
        }
    }

    const fn class(self) -> RegexDiagnosticClass {
        match self {
            Self::InvalidReference
            | Self::UnresolvedReference
            | Self::ProfileIncompatibleReference => RegexDiagnosticClass::Syntax,
            Self::DynamicPatternBoundary
            | Self::EmbeddedCodeBoundary
            | Self::UnsupportedControl => RegexDiagnosticClass::DynamicBoundary,
        }
    }
}

/// One typed pattern-control diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternControlDiagnostic {
    /// Stable diagnostic identity.
    pub code: PatternControlDiagnosticCode,
    /// Operational diagnostic class.
    pub class: RegexDiagnosticClass,
    /// Body-relative diagnostic range.
    pub range: RegexRange,
    /// Original-source diagnostic range when exactly mappable.
    pub source_range: Option<RegexRange>,
}

impl PatternControlDiagnostic {
    pub(super) fn new(
        code: PatternControlDiagnosticCode,
        range: RegexRange,
        source_start: usize,
    ) -> Self {
        Self {
            code,
            class: code.class(),
            range,
            source_range: map_source_range(range, source_start),
        }
    }

    /// Render the current human-readable message.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self.code {
            PatternControlDiagnosticCode::InvalidReference => {
                "Invalid regex capture reference or subpattern-call operand"
            }
            PatternControlDiagnosticCode::UnresolvedReference => {
                "Regex capture reference does not resolve to a static declaration"
            }
            PatternControlDiagnosticCode::ProfileIncompatibleReference => {
                "Regex capture reference targets a declaration unavailable in this Perl profile"
            }
            PatternControlDiagnosticCode::DynamicPatternBoundary => {
                "Regex pattern text is supplied or changed at runtime"
            }
            PatternControlDiagnosticCode::EmbeddedCodeBoundary => {
                "Regex evaluation contains embedded Perl execution"
            }
            PatternControlDiagnosticCode::UnsupportedControl => {
                "Regex pattern-control construct is outside the modeled static subset"
            }
        }
    }
}

/// Completeness and mapping status for pattern-control analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PatternControlAnalysisStatus {
    /// Pattern text can be supplied or changed at runtime.
    pub dynamic_pattern: bool,
    /// Perl code can execute while the regex is evaluated.
    pub dynamic_execution: bool,
    /// At least one recognized control construct is outside the modeled subset.
    pub unsupported: bool,
    /// The current event/capture model cannot prove exact structural identity.
    pub structural_uncertainty: bool,
    /// Malformed or truncated structure was observed.
    pub malformed: bool,
    /// Deterministic event production stopped at a declared budget.
    pub exhausted: Option<RegexAnalysisBudget>,
    /// Every body-relative range mapped to original source without overflow.
    pub source_mapping_complete: bool,
}

impl PatternControlAnalysisStatus {
    /// Whether this result is exact enough for downstream static consumers.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        !self.dynamic_pattern
            && !self.dynamic_execution
            && !self.unsupported
            && !self.structural_uncertainty
            && !self.malformed
            && self.exhausted.is_none()
            && self.source_mapping_complete
    }
}

/// Canonical captures, pattern-control facts, boundaries, diagnostics, and status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternControlAnalysis {
    /// Lossless/effective suffix modifiers used for the analysis.
    pub modifiers: EffectiveModifiers,
    /// Perl language and source-UTF-8 profile used for the analysis.
    pub profile: CaptureLanguageProfile,
    /// Capture declarations and numbering used to resolve facts.
    pub captures: CaptureAnalysis,
    /// Control/reference facts in deterministic source order.
    pub facts: Vec<PatternControlFact>,
    /// Dynamic/unsupported boundaries in source order.
    pub boundaries: Vec<PatternBoundary>,
    /// Pattern-control diagnostics in source order.
    pub diagnostics: Vec<PatternControlDiagnostic>,
    /// Local completeness, recovery, budget, and mapping status.
    pub status: PatternControlAnalysisStatus,
}

pub(super) fn map_source_range(range: RegexRange, source_start: usize) -> Option<RegexRange> {
    Some(RegexRange {
        start: source_start.checked_add(range.start)?,
        end: source_start.checked_add(range.end)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_status() -> PatternControlAnalysisStatus {
        PatternControlAnalysisStatus {
            dynamic_pattern: false,
            dynamic_execution: false,
            unsupported: false,
            structural_uncertainty: false,
            malformed: false,
            exhausted: None,
            source_mapping_complete: true,
        }
    }

    #[test]
    fn source_mapping_offsets_a_body_relative_range() {
        let mapped =
            perl_test_must::must_some(map_source_range(RegexRange { start: 2, end: 5 }, 10));
        assert_eq!((mapped.start, mapped.end), (12, 15));
        // A zero offset is the identity, not a special case.
        let identity =
            perl_test_must::must_some(map_source_range(RegexRange { start: 2, end: 5 }, 0));
        assert_eq!((identity.start, identity.end), (2, 5));
    }

    #[test]
    fn source_mapping_returns_none_instead_of_wrapping_on_overflow() {
        // Fabricating a wrapped offset would point a reader at an unrelated byte.
        assert_eq!(map_source_range(RegexRange { start: 1, end: 2 }, usize::MAX), None);
        assert_eq!(
            map_source_range(RegexRange { start: 0, end: usize::MAX }, 1),
            None,
            "the end offset must be checked as well as the start"
        );
    }

    #[test]
    fn a_status_is_complete_only_when_every_boundary_is_clear() {
        assert!(complete_status().is_complete());

        // Each field independently defeats completeness; none may be ignored.
        let mut dynamic = complete_status();
        dynamic.dynamic_pattern = true;
        assert!(!dynamic.is_complete());

        let mut execution = complete_status();
        execution.dynamic_execution = true;
        assert!(!execution.is_complete());

        let mut unsupported = complete_status();
        unsupported.unsupported = true;
        assert!(!unsupported.is_complete());

        let mut structural = complete_status();
        structural.structural_uncertainty = true;
        assert!(!structural.is_complete());

        let mut malformed = complete_status();
        malformed.malformed = true;
        assert!(!malformed.is_complete());

        let mut unmapped = complete_status();
        unmapped.source_mapping_complete = false;
        assert!(!unmapped.is_complete());
    }

    #[test]
    fn fact_ids_expose_their_source_order_index() {
        assert_eq!(PatternControlId(0).index(), 0);
        assert_eq!(PatternControlId(7).index(), 7);
    }

    #[test]
    fn machine_tokens_are_stable_and_distinct() {
        // These strings are consumed by adapters and receipts, so a collision would
        // silently merge two different facts downstream.
        let kinds = [
            PatternControlKind::KeepAnchor.as_str(),
            PatternControlKind::WholePatternRecursion.as_str(),
            PatternControlKind::RecursionConditional.as_str(),
            PatternControlKind::ImmediateEmbeddedCode.as_str(),
            PatternControlKind::OptimisticEmbeddedCode.as_str(),
            PatternControlKind::DeferredRuntimePattern.as_str(),
            PatternControlKind::SourceInterpolation.as_str(),
        ];
        let mut unique = kinds.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), kinds.len(), "kind tokens must not collide");

        let effects = [
            PatternControlEffect::ReportedMatchStart.as_str(),
            PatternControlEffect::CaptureRead.as_str(),
            PatternControlEffect::SubpatternCall.as_str(),
            PatternControlEffect::ConditionalControl.as_str(),
            PatternControlEffect::DynamicExecution.as_str(),
            PatternControlEffect::DynamicPattern.as_str(),
            PatternControlEffect::Unsupported.as_str(),
        ];
        let mut unique_effects = effects.to_vec();
        unique_effects.sort_unstable();
        unique_effects.dedup();
        assert_eq!(unique_effects.len(), effects.len(), "effect tokens must not collide");
    }

    #[test]
    fn reference_carrying_kinds_report_their_operand() {
        assert_eq!(
            PatternControlKind::NumericBackreference {
                number: 3,
                syntax: PatternReferenceSyntax::PlainNumeric,
            }
            .as_str(),
            "numeric_backreference"
        );
        assert_eq!(
            PatternControlKind::NamedBackreference {
                name: "x".to_string(),
                syntax: PatternReferenceSyntax::KReference,
            }
            .as_str(),
            "named_backreference"
        );
    }
}
