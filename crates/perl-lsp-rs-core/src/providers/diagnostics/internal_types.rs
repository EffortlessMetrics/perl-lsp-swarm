//! Internal diagnostic types for perl-lsp-diagnostics.
//!
//! These types are the working types used by this crate's linting machinery.
//! The canonical public API types (`DiagnosticCode`, `DiagnosticSeverity`, `DiagnosticTag`)
//! are re-exported from `perl-diagnostics::codes::`.

use perl_diagnostics::codes::DiagnosticSeverity;

/// Tags for diagnostics (internal alias for the canonical type from codes::).
pub use perl_diagnostics::codes::DiagnosticTag;

/// A diagnostic message (internal working type).
///
/// This is the rich internal type used by the linting machinery.
/// It has string-based codes for compatibility with the diagnostic pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Source code range (start, end) where the issue occurs.
    pub range: (usize, usize),
    /// Severity level of the diagnostic.
    pub severity: DiagnosticSeverity,
    /// Optional diagnostic code for categorization.
    pub code: Option<String>,
    /// Human-readable description of the issue.
    pub message: String,
    /// Additional context and related information.
    pub related_information: Vec<RelatedInformation>,
    /// Tags for categorizing the diagnostic.
    pub tags: Vec<DiagnosticTag>,
    /// Optional short suggestion for how to fix the issue.
    pub suggestion: Option<String>,
}

/// Conversion from the internal working type to the canonical
/// `perl_diagnostics::Diagnostic` (#4946).
///
/// This bridges the two coexisting Diagnostic domain models with a
/// documented ownership contract: the internal type is the working type
/// for linting machinery (string codes, always-present related_info/tags
/// vectors, suggestion field); the canonical type is the public API type
/// (typed DiagnosticCode enum, optional related_info/tags, no suggestion).
///
/// String codes that don't match a known `DiagnosticCode` variant map to
/// `DiagnosticCode::ParseError` (the default), preserving the "fail safe"
/// principle.
impl From<Diagnostic> for perl_diagnostics::Diagnostic {
    fn from(inner: Diagnostic) -> Self {
        let code = inner.code.as_deref().and_then(parse_diagnostic_code).unwrap_or_default();
        let mut diag =
            perl_diagnostics::Diagnostic::new(code, inner.severity, inner.range, inner.message);
        if !inner.related_information.is_empty() {
            diag.related_information = Some(
                inner
                    .related_information
                    .into_iter()
                    .map(|ri| perl_diagnostics::RelatedInformation::new(ri.message, ri.location))
                    .collect(),
            );
        }
        if !inner.tags.is_empty() {
            diag.tags = Some(inner.tags);
        }
        diag
    }
}

/// Parse a diagnostic code string into the canonical `DiagnosticCode` enum.
fn parse_diagnostic_code(s: &str) -> Option<perl_diagnostics::codes::DiagnosticCode> {
    use perl_diagnostics::codes::DiagnosticCode;
    match s {
        "PL001" | "parse_error" => Some(DiagnosticCode::ParseError),
        "PL002" | "syntax_error" => Some(DiagnosticCode::SyntaxError),
        "PL003" | "unexpected_eof" => Some(DiagnosticCode::UnexpectedEof),
        "PL100" | "missing_strict" => Some(DiagnosticCode::MissingStrict),
        "PL101" | "missing_warnings" => Some(DiagnosticCode::MissingWarnings),
        "PL102" | "unused_variable" => Some(DiagnosticCode::UnusedVariable),
        "PL103" | "undefined_variable" => Some(DiagnosticCode::UndefinedVariable),
        "PL104" | "variable_shadowing" => Some(DiagnosticCode::VariableShadowing),
        "PL105" | "variable_redeclared" => Some(DiagnosticCode::VariableRedeclaration),
        _ => None,
    }
}

impl Diagnostic {
    /// Creates a diagnostic with required fields and sensible defaults.
    pub fn new(
        range: (usize, usize),
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            range,
            severity,
            code: None,
            message: message.into(),
            related_information: Vec::new(),
            tags: Vec::new(),
            suggestion: None,
        }
    }

    /// Sets the optional diagnostic code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Sets the optional suggestion text.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Adds related information to this diagnostic.
    pub fn with_related_information(mut self, related_information: RelatedInformation) -> Self {
        self.related_information.push(related_information);
        self
    }

    /// Adds a tag to this diagnostic.
    pub fn with_tag(mut self, tag: DiagnosticTag) -> Self {
        self.tags.push(tag);
        self
    }
}

/// Related information for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInformation {
    /// Location in source code for the related information.
    pub location: (usize, usize),
    /// Description of the related information.
    pub message: String,
}

impl RelatedInformation {
    /// Creates a related information entry.
    pub fn new(location: (usize, usize), message: impl Into<String>) -> Self {
        Self { location, message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, RelatedInformation};
    use perl_diagnostics::codes::{DiagnosticSeverity, DiagnosticTag};

    #[test]
    fn diagnostic_new_initializes_optional_fields() {
        let diagnostic = Diagnostic::new((3, 5), DiagnosticSeverity::Warning, "warn");

        assert_eq!(diagnostic.range, (3, 5));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, None);
        assert_eq!(diagnostic.message, "warn");
        assert!(diagnostic.related_information.is_empty());
        assert!(diagnostic.tags.is_empty());
        assert_eq!(diagnostic.suggestion, None);
    }

    #[test]
    fn diagnostic_builder_methods_attach_optional_data() {
        let related = RelatedInformation::new((20, 24), "this is related");

        let diagnostic = Diagnostic::new((10, 16), DiagnosticSeverity::Error, "bad")
            .with_code("E001")
            .with_suggestion("do the right thing")
            .with_related_information(related.clone())
            .with_tag(DiagnosticTag::Deprecated);

        assert_eq!(diagnostic.code, Some(String::from("E001")));
        assert_eq!(diagnostic.suggestion, Some(String::from("do the right thing")));
        assert_eq!(diagnostic.related_information, vec![related]);
        assert_eq!(diagnostic.tags, vec![DiagnosticTag::Deprecated]);
    }

    #[test]
    fn related_information_new_sets_fields() {
        let related = RelatedInformation::new((8, 12), "hint");

        assert_eq!(related.location, (8, 12));
        assert_eq!(related.message, "hint");
    }
}
