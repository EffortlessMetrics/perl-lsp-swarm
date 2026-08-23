//! Diagnostics provider (delegated to perl-lsp-providers).

pub mod pull;
pub mod report_identity;
pub use pull::{PullDiagnosticsContext, PullDiagnosticsProvider};
pub use report_identity::{
    DiagnosticProjectionFragment, NotReusable, PullPositionEncoding, PullReportResultId,
    PullReportSubject,
};

// Re-export core diagnostics types from perl-lsp-diagnostics
pub use perl_lsp_rs_core::providers::diagnostics::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, DiagnosticsProvider, RelatedInformation,
};
