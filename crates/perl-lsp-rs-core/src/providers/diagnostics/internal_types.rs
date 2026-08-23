//! Internal diagnostic types for perl-lsp-diagnostics.
//!
//! These types are the working types used by this crate's linting machinery.
//! The canonical public API types (`DiagnosticCode`, `DiagnosticSeverity`, `DiagnosticTag`)
//! are re-exported from `perl-diagnostics::codes::`.

use std::fmt;

use perl_diagnostics::codes::DiagnosticSeverity;
use perl_diagnostics::{ByteSpan, InvalidByteSpan};

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
    /// Whether the producer has a currently available safe remediation.
    pub fixable: bool,
}

/// Failure converting the migration-only provider diagnostic into the canonical type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticConversionError {
    /// The provider diagnostic did not carry a code.
    MissingCode,
    /// The provider code is not a registered built-in code.
    UnknownCode(String),
    /// The primary byte range is reversed.
    InvalidRange(InvalidByteSpan),
    /// A related-information byte range is reversed.
    InvalidRelatedRange {
        /// Zero-based related-information entry index.
        index: usize,
        /// The rejected byte span.
        source: InvalidByteSpan,
    },
}

impl fmt::Display for DiagnosticConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCode => formatter.write_str("diagnostic code is missing"),
            Self::UnknownCode(code) => write!(formatter, "unknown diagnostic code `{code}`"),
            Self::InvalidRange(source) => write!(formatter, "invalid diagnostic range: {source}"),
            Self::InvalidRelatedRange { index, source } => {
                write!(formatter, "invalid related-information range at index {index}: {source}")
            }
        }
    }
}

impl std::error::Error for DiagnosticConversionError {}

/// Fallible migration from the internal working type to the canonical
/// `perl_diagnostics::Diagnostic` (#2213, #4946).
///
/// The migration validates every byte span and refuses to manufacture a
/// plausible built-in code when the working diagnostic has no recognized
/// identity. The broader lossless built-in/external identity model remains
/// owned by #9931.
impl TryFrom<Diagnostic> for perl_diagnostics::Diagnostic {
    type Error = DiagnosticConversionError;

    fn try_from(inner: Diagnostic) -> Result<Self, Self::Error> {
        let Diagnostic {
            range,
            severity,
            code,
            message,
            related_information,
            tags,
            suggestion: _,
            fixable: _,
        } = inner;

        let code_text = code.ok_or(DiagnosticConversionError::MissingCode)?;
        let code = parse_diagnostic_code(&code_text)
            .ok_or_else(|| DiagnosticConversionError::UnknownCode(code_text.clone()))?;
        let range = ByteSpan::try_from(range).map_err(DiagnosticConversionError::InvalidRange)?;
        let related_information = related_information
            .into_iter()
            .enumerate()
            .map(|(index, related)| {
                let location = ByteSpan::try_from(related.location).map_err(|source| {
                    DiagnosticConversionError::InvalidRelatedRange { index, source }
                })?;
                Ok(perl_diagnostics::RelatedInformation::new(related.message, location))
            })
            .collect::<Result<Vec<_>, DiagnosticConversionError>>()?;

        let mut diagnostic = perl_diagnostics::Diagnostic::new(code, severity, range, message);
        if !related_information.is_empty() {
            diagnostic.related_information = Some(related_information);
        }
        if !tags.is_empty() {
            diagnostic.tags = Some(tags);
        }
        Ok(diagnostic)
    }
}

/// Parse a diagnostic code string into the canonical `DiagnosticCode` enum.
fn parse_diagnostic_code(s: &str) -> Option<perl_diagnostics::codes::DiagnosticCode> {
    use perl_diagnostics::codes::DiagnosticCode;

    DiagnosticCode::parse_code(s).or(match s {
        "parse_error" => Some(DiagnosticCode::ParseError),
        "syntax_error" => Some(DiagnosticCode::SyntaxError),
        "unexpected_eof" => Some(DiagnosticCode::UnexpectedEof),
        "missing_strict" => Some(DiagnosticCode::MissingStrict),
        "missing_warnings" => Some(DiagnosticCode::MissingWarnings),
        "unused_variable" => Some(DiagnosticCode::UnusedVariable),
        "undefined_variable" => Some(DiagnosticCode::UndefinedVariable),
        "variable_shadowing" => Some(DiagnosticCode::VariableShadowing),
        "variable_redeclared" => Some(DiagnosticCode::VariableRedeclaration),
        _ => None,
    })
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
            fixable: false,
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
    use super::{Diagnostic, DiagnosticConversionError, RelatedInformation};
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

    #[test]
    fn canonical_conversion_rejects_reversed_primary_range() {
        let diagnostic =
            Diagnostic::new((12, 8), DiagnosticSeverity::Error, "bad").with_code("PL001");

        assert!(matches!(
            perl_diagnostics::Diagnostic::try_from(diagnostic),
            Err(DiagnosticConversionError::InvalidRange(_))
        ));
    }

    #[test]
    fn canonical_conversion_rejects_unknown_code_instead_of_defaulting() {
        let diagnostic =
            Diagnostic::new((8, 12), DiagnosticSeverity::Error, "bad").with_code("native.unknown");

        assert!(matches!(
            perl_diagnostics::Diagnostic::try_from(diagnostic),
            Err(DiagnosticConversionError::UnknownCode(code)) if code == "native.unknown"
        ));
    }
}
