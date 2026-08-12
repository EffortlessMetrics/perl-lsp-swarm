/// Typed diagnostics, facts, ranges, and completeness for batch analysis.
pub mod analysis;

mod batch;
mod code_execution;
mod complexity;
mod config;
mod nested_quantifier;

pub use analysis::{
    EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisCompleteness, RegexDiagnostic,
    RegexDiagnosticClass, RegexDiagnosticCode, RegexFacts, RegexRange,
};
pub use config::RegexValidationConfig;

use crate::error::RegexError;

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

    /// Analyze one regex body and return all typed diagnostics and reusable facts.
    ///
    /// Diagnostic and fact ranges are byte offsets relative to `pattern`.
    #[must_use]
    pub fn analyze(&self, pattern: &str) -> RegexAnalysis {
        batch::analyze(pattern, &self.config)
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
        code_execution::detects_code_execution(pattern)
    }

    pub fn detect_nested_quantifiers(&self, pattern: &str) -> bool {
        nested_quantifier::detect_nested_quantifiers(pattern)
    }

    pub fn find_code_execution(&self, pattern: &str, start_pos: usize) -> Option<RegexFinding> {
        code_execution::find_code_execution(pattern, start_pos).map(|finding| {
            let message = match finding.kind {
                code_execution::EmbeddedCodeKind::Immediate => {
                    "Embedded code execution is not allowed in regex patterns"
                }
                code_execution::EmbeddedCodeKind::Deferred => {
                    "Deferred embedded code execution is not allowed in regex patterns"
                }
            };
            RegexFinding { offset: finding.offset, message }
        })
    }

    pub fn find_nested_quantifier(&self, pattern: &str, start_pos: usize) -> Option<RegexFinding> {
        nested_quantifier::find_nested_quantifier(pattern, start_pos).map(|offset| RegexFinding {
            offset,
            message: "Nested quantifiers may cause catastrophic backtracking",
        })
    }
}
