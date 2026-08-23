//! LSP formatting provider for Perl.
//!
//! Native formatting is the default. External Perl::Tidy remains an explicit
//! compatibility adapter. Callers that need to distinguish no-change from
//! refusal or failure should consume [`FormattingDecision`].

#[allow(clippy::module_inception)]
mod formatting;

pub use formatting::{
    FormatPosition, FormatRange, FormatTextEdit, FormattedDocument, FormattingDecision,
    FormattingError, FormattingOptions, FormattingProvider, PerlTidyConfig,
};
