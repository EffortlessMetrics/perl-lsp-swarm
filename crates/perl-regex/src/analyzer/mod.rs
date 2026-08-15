mod capture;
mod control;
mod hover;
mod modifier_analysis;
mod modifiers;

pub use capture::{
    CaptureAnalysis, CaptureAnalysisStatus, CaptureConfidence, CaptureDeclaration,
    CaptureDiagnostic, CaptureDiagnosticCode, CaptureGroup, CaptureId, CaptureLanguageProfile,
    CaptureNumberConfidence, CaptureProfileConfidence, CaptureSourceConfidence, CaptureSyntax,
    NamedCaptureFamily,
};
pub use control::{
    PatternBoundary, PatternBoundaryKind, PatternControlAnalysis, PatternControlAnalysisStatus,
    PatternControlDiagnostic, PatternControlDiagnosticCode, PatternControlEffect,
    PatternControlFact, PatternControlId, PatternControlKind, PatternControlResolution,
    PatternControlUnresolvedReason, PatternExtendedMode, PatternModeState, PatternReferenceSyntax,
};
pub use modifier_analysis::{
    CaptureMode, CharacterSetMode, EffectiveModifiers, ExtendedMode, FeatureState,
    ModifierAnalysis, ModifierRequirement, ModifierRequirementKind, ModifierSequence,
    ModifierToken, PerlVersion, RegexLanguageProfile, RegexOperator, RequirementDisposition,
    TransliterationModifiers,
};

pub struct RegexAnalyzer;

impl RegexAnalyzer {
    /// Project named captures through the legacy compatibility shape.
    pub fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
        capture::extract_named_captures(pattern)
    }

    /// Analyze every source-backed capture declaration using explicit suffix
    /// modifiers and source/profile facts.
    #[must_use]
    pub fn analyze_captures(
        pattern: &str,
        modifiers: EffectiveModifiers,
        profile: CaptureLanguageProfile,
    ) -> CaptureAnalysis {
        capture::analyze_captures(pattern, modifiers, profile)
    }

    /// Analyze pattern control, capture references, recursion, conditionals,
    /// embedded code, and local completeness boundaries.
    ///
    /// `pattern` is the regex body alone, and `source_start` is that body's byte offset
    /// in the original source. Every fact carries both its body-relative range and, where
    /// the mapping succeeds, the corresponding original-source range.
    ///
    /// Source mapping is checked rather than fallible: if `source_start` plus a range
    /// would overflow, the affected `source_range` is left `None` and
    /// `status.source_mapping_complete` becomes `false`, instead of returning an error or
    /// reporting a wrapped offset. Body-relative ranges stay exact in that case, so a
    /// caller that needs original-source positions must consult
    /// `status.source_mapping_complete` before trusting them.
    #[must_use]
    pub fn analyze_pattern_controls(
        pattern: &str,
        source_start: usize,
        modifiers: EffectiveModifiers,
        profile: CaptureLanguageProfile,
    ) -> PatternControlAnalysis {
        control::analyze_pattern_controls(pattern, source_start, modifiers, profile)
    }

    /// Analyze a raw modifier sequence using explicit operator and language context.
    #[must_use]
    pub fn analyze_modifiers(
        operator: RegexOperator,
        sequence: ModifierSequence,
        profile: RegexLanguageProfile,
    ) -> ModifierAnalysis {
        modifier_analysis::analyze_modifiers(operator, sequence, profile)
    }

    pub fn hover_text_for_regex(pattern: &str, modifiers: &str) -> String {
        hover::hover_text_for_regex(pattern, modifiers)
    }
}
