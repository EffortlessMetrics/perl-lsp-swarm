//! Adapter from provider diagnostics to typed quick-fix machine context (#8396).

use super::types::{QuickFixDiagnostic, QuickFixMetadata};
use crate::providers::diagnostics::Diagnostic;
use crate::providers::diagnostics::quick_fix_context::{
    ParseErrorFixKind, parse_error_fix_kind_for_diagnostic,
};

/// Typed view of a diagnostic for code-action applicability decisions.
pub struct DiagnosticContext<'a> {
    diagnostic: &'a Diagnostic,
}

impl<'a> DiagnosticContext<'a> {
    #[must_use]
    pub fn new(diagnostic: &'a Diagnostic) -> Self {
        Self { diagnostic }
    }

    #[must_use]
    pub fn parse_error_fix_kind(&self) -> Option<ParseErrorFixKind> {
        parse_error_fix_kind_for_diagnostic(
            self.diagnostic.code.as_deref(),
            &self.diagnostic.message,
            self.diagnostic.parse_error_fix_kind,
        )
    }

    #[must_use]
    pub fn quick_fix_metadata(
        &self,
        printf_metadata: Option<QuickFixMetadata>,
    ) -> Option<QuickFixMetadata> {
        if let Some(metadata) = printf_metadata {
            return Some(metadata);
        }
        self.parse_error_fix_kind().map(QuickFixMetadata::ParseErrorFix)
    }
}

impl QuickFixDiagnostic {
    #[must_use]
    pub(super) fn parse_error_fix_kind(&self) -> Option<ParseErrorFixKind> {
        self.metadata.as_ref().and_then(|metadata| match metadata {
            QuickFixMetadata::ParseErrorFix(kind) => Some(*kind),
            QuickFixMetadata::PrintfFormatArity { .. } => None,
        })
    }
}
