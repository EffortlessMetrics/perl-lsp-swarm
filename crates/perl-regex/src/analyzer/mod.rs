mod capture;
mod capture_compat;
mod hover;
mod modifier_analysis;
mod modifiers;
mod parser;

pub use capture::{
    CaptureAnalysis, CaptureAnalysisStatus, CaptureConfidence, CaptureDeclaration,
    CaptureDiagnostic, CaptureDiagnosticCode, CaptureGroup, CaptureId, CaptureLanguageProfile,
    CaptureNumberConfidence, CaptureProfileConfidence, CaptureSourceConfidence, CaptureSyntax,
    NamedCaptureFamily,
};
pub use modifier_analysis::{
    CaptureMode, CharacterSetMode, EffectiveModifiers, ExtendedMode, FeatureState,
    ModifierAnalysis, ModifierRequirement, ModifierRequirementKind, ModifierSequence, ModifierToken,
    PerlVersion, RegexLanguageProfile, RegexOperator, RequirementDisposition,
    TransliterationModifiers,
};

pub struct RegexAnalyzer;

impl RegexAnalyzer {
    /// Project named captures through the legacy compatibility shape.
    pub fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
        capture_compat::extract_named_captures(pattern)
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
