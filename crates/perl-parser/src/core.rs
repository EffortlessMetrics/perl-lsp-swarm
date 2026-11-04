//! Facade over `perl-parser-core`.
//!
//! Canonical parser kernel implementation lives in `perl-parser-core`.

pub use perl_parser_core::ast::{Node, NodeKind, SourceLocation};
pub use perl_parser_core::error::{
    ErrorCategory, ErrorClass, ParseBudgetExhaustionDimension, ParseCancellationIdentity,
    ParseCatastrophicCause, ParseCompletion, ParseCompletionClass, ParseCompletionRange,
    ParseCompletionStage, ParseCompletionValidationError, ParseDiagnosticAnchor, ParseError,
    ParseOutput, ParseResult, ParseStopCause, ResolvedParseDiagnosticAnchor,
};
pub use perl_parser_core::parser::Parser;

pub use perl_parser_core::engine;
pub use perl_parser_core::engine::{error, parser, position};
