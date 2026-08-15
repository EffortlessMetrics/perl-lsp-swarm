/// Typed diagnostics, facts, ranges, and completeness for batch analysis.
pub mod analysis;

mod batch;
mod code_execution;
mod complexity;
mod config;
mod nested_quantifier;

#[cfg(test)]
mod analysis_contract_tests;

pub use analysis::{
    EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisBudget,
    RegexAnalysisCompleteness, RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode,
    RegexDynamicRegionFact, RegexDynamicRegionKind, RegexFacts, RegexRange,
};
pub use config::RegexValidationConfig;

use crate::{analyzer::EffectiveModifiers, error::RegexError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexFinding {
    pub offset: usize,
    pub message: &'static str,
}

pub struct RegexValidator {
    config: RegexValidationConfig,
}

impl Default for RegexValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexValidator {
    pub fn new() -> Self {
        Self { config: RegexValidationConfig::default() }
    }

    pub fn with_config(config: RegexValidationConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &RegexValidationConfig {
        &self.config
    }

    /// Analyze one regex body with default suffix-modifier state.
    ///
    /// Diagnostic and fact ranges are byte offsets relative to `pattern`.
    #[must_use]
    pub fn analyze(&self, pattern: &str) -> RegexAnalysis {
        self.analyze_with_modifiers(pattern, EffectiveModifiers::default())
    }

    /// Analyze one regex body using explicit effective suffix modifiers.
    ///
    /// Inline modifier scopes are applied by the shared structural event stream.
    /// Diagnostic and fact ranges are byte offsets relative to `pattern`.
    #[must_use]
    pub fn analyze_with_modifiers(
        &self,
        pattern: &str,
        modifiers: EffectiveModifiers,
    ) -> RegexAnalysis {
        batch::analyze(pattern, &self.config, modifiers)
    }

    /// Validate through the historical fail-fast compatibility contract.
    ///
    /// This lossy adapter preserves the old category priority while mapping the
    /// selected typed diagnostic to [`RegexError::Syntax`].
    pub fn validate(&self, pattern: &str, start_pos: usize) -> Result<(), RegexError> {
        let analysis = self.analyze(pattern);
        if let Some(diagnostic) = batch::first_compatibility_diagnostic(&analysis) {
            return Err(RegexError::syntax(
                diagnostic.message(),
                start_pos.saturating_add(diagnostic.range.start),
            ));
        }
        Ok(())
    }

    pub fn detects_code_execution(&self, pattern: &str) -> bool {
        !self.analyze(pattern).facts.embedded_code.is_empty()
    }

    pub fn detect_nested_quantifiers(&self, pattern: &str) -> bool {
        !self.analyze(pattern).facts.nested_quantifiers.is_empty()
    }

    pub fn find_code_execution(&self, pattern: &str, start_pos: usize) -> Option<RegexFinding> {
        self.analyze(pattern).facts.embedded_code.first().map(|finding| RegexFinding {
            offset: start_pos.saturating_add(finding.range.start),
            message: match finding.kind {
                EmbeddedCodeKind::Immediate => {
                    "Embedded code execution is not allowed in regex patterns"
                }
                EmbeddedCodeKind::Deferred => {
                    "Deferred embedded code execution is not allowed in regex patterns"
                }
            },
        })
    }

    pub fn find_nested_quantifier(&self, pattern: &str, start_pos: usize) -> Option<RegexFinding> {
        self.analyze(pattern).facts.nested_quantifiers.first().map(|range| RegexFinding {
            offset: start_pos.saturating_add(range.start),
            message: "Nested quantifiers may cause catastrophic backtracking",
        })
    }
}
