//! Deprecated UTF-16 compatibility surface for [`perl_tidy`].
//!
//! New callers should use the pure configuration, exact byte requests and
//! source-bound edit plans in `perl-tidy`. This module retains the former
//! mode-bearing configuration and DTOs while delegating all parsing, layout,
//! support classification and native identity to that package. It never
//! discovers or starts an external formatter.
#![allow(deprecated)]

#[path = "native/config.rs"]
mod config;
#[path = "native/doc.rs"]
mod doc;
#[path = "native/edit_application.rs"]
mod edit_application;
#[path = "native/implementation.rs"]
mod implementation;
#[path = "native/outcome.rs"]
mod outcome;
#[path = "native/projection.rs"]
mod projection;
#[path = "native/result.rs"]
mod result;

pub use config::*;
pub use doc::FormatDoc;
pub use edit_application::{EditApplicationError, EditSpec, PositionEncoding, apply_edits_exact};
pub use implementation::{NativeFormatter, PerlFormatter};
pub use outcome::*;
pub use result::{
    FormatDiagnostic, FormatDiagnosticSeverity, FormatResult, TextEdit, TextPosition, TextRange,
};

pub use perl_tidy::{
    COUNTER_CLOCK_TAG, COUNTER_SCHEMA_V1, FinalNewlinePolicy,
    MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1, NativePipelineCounters, PipelineCollectorScope,
    PolicyOutcome, SCALING_ABSOLUTE_SLACK_V1, SCALING_RATIO_BOUND_V1, TerminalChange,
    TerminalNewlineEvidence, TerminalRun, TerminalSequence, apply_terminal_sequence_policy,
    exceeds_replacement_envelope_v1, inferred_line_ending, inferred_line_ending_at,
    source_convention, trailing_run,
};
