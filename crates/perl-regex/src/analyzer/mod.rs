mod capture;
mod hover;
mod modifier_analysis;
mod modifiers;
mod parser;

pub use capture::CaptureGroup;
pub use modifier_analysis::{
    CaptureMode, CharacterSetMode, EffectiveModifiers, ExtendedMode, FeatureState,
    ModifierAnalysis, ModifierRequirement, ModifierRequirementKind, ModifierSequence,
    ModifierToken, PerlVersion, RegexLanguageProfile, RegexOperator, RequirementDisposition,
    TransliterationModifiers,
};

pub struct RegexAnalyzer;

impl RegexAnalyzer {
    pub fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
        capture::extract_named_captures(pattern)
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
