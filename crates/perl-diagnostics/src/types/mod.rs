//! Diagnostic types: Diagnostic and RelatedInformation.
//!
//! This module contains the diagnostic message types used by LSP.
//!
//! Type unification: `DiagnosticSeverity` and `DiagnosticTag` are re-exported
//! from the `codes` module, ensuring a single canonical type that can be used
//! from either module path.

// Unified types — canonical definitions are in codes module
pub use crate::codes::{DiagnosticSeverity, DiagnosticTag};

/// A diagnostic message with location and metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Diagnostic {
    /// The diagnostic code (e.g., PL001).
    pub code: crate::codes::DiagnosticCode,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Location of the diagnostic.
    pub range: (usize, usize),
    /// The message text.
    pub message: String,
    /// Optional related information.
    pub related_information: Option<Vec<RelatedInformation>>,
    /// Optional tags (Unnecessary, Deprecated, etc.).
    pub tags: Option<Vec<DiagnosticTag>>,
}

impl Diagnostic {
    /// Create a new diagnostic with the given code, severity, range, and message.
    pub fn new(
        code: crate::codes::DiagnosticCode,
        severity: DiagnosticSeverity,
        range: (usize, usize),
        message: impl Into<String>,
    ) -> Self {
        Self { code, severity, range, message: message.into(), ..Default::default() }
    }
}

/// Information related to a diagnostic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct RelatedInformation {
    /// The message text.
    pub message: String,
    /// Location of the related information.
    pub location: (usize, usize),
}

impl RelatedInformation {
    /// Create a new related information entry.
    pub fn new(message: impl Into<String>, location: (usize, usize)) -> Self {
        Self { message: message.into(), location }
    }
}
