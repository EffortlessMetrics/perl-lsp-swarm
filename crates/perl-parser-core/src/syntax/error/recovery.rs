//! Error recovery for the Perl parser
//!
//! This module implements error recovery strategies to continue parsing
//! even when syntax errors are encountered. This is essential for IDE
//! scenarios where code is often incomplete or temporarily invalid.
//!
//! # Progress Invariant
//!
//! All recovery operations guarantee forward progress: every recovery attempt
//! must consume at least one token or exit. This prevents infinite loops when
//! the parser cannot make sense of the input.
//!
//! # Budget Awareness
//!
//! Recovery operations respect the `ParseBudget` limits to prevent runaway
//! parsing on adversarial input. When budget is exhausted, recovery returns
//! immediately with an appropriate error node.

use super::{BudgetTracker, ParseBudget};
use perl_ast_v2::Node;
use perl_lexer::TokenType;
use perl_position_tracking::Range;

/// Parser-owned source anchor for a diagnostic.
///
/// Downstream consumers should use this semantic accessor instead of matching
/// public [`super::ParseError`] variants to reconstruct location fields. The
/// parser can then add variants without forcing old consumers to guess that an
/// unknown error belongs at byte zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseDiagnosticAnchor {
    /// The parser owns one exact byte offset in the source.
    Exact(usize),
    /// The diagnostic belongs at the current end of the source.
    EndOfInput,
    /// The diagnostic has no defensible source anchor.
    NoSource,
}

/// A [`ParseDiagnosticAnchor`] resolved against concrete source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolvedParseDiagnosticAnchor {
    /// Exact in-bounds UTF-8 byte boundary.
    Exact(usize),
    /// End-of-input anchor, resolved to the supplied source length.
    EndOfInput(usize),
    /// No source location is available.
    NoSource,
    /// The parser reported an exact offset outside the supplied source.
    InvalidOffset {
        /// Parser-reported offset.
        reported: usize,
        /// Concrete source length used for validation.
        source_len: usize,
    },
    /// The parser reported an in-bounds offset inside a UTF-8 code point.
    InvalidUtf8Boundary {
        /// Parser-reported offset.
        reported: usize,
        /// Concrete source length used for validation.
        source_len: usize,
    },
    /// The parsed source snapshot and current source are not the same subject.
    StaleSource {
        /// Byte length of the source snapshot that produced the error.
        parsed_len: usize,
        /// Byte length of the current source supplied by the consumer.
        current_len: usize,
    },
}

impl ParseDiagnosticAnchor {
    /// Resolve this semantic anchor against concrete source text.
    ///
    /// Exact offsets are never silently clamped. Out-of-range offsets and
    /// offsets inside a multibyte UTF-8 code point remain distinct typed states
    /// so a consumer cannot convert parser corruption into a plausible range.
    #[must_use]
    pub fn resolve(self, source: &str) -> ResolvedParseDiagnosticAnchor {
        match self {
            Self::Exact(reported) if reported > source.len() => {
                ResolvedParseDiagnosticAnchor::InvalidOffset {
                    reported,
                    source_len: source.len(),
                }
            }
            Self::Exact(reported) if !source.is_char_boundary(reported) => {
                ResolvedParseDiagnosticAnchor::InvalidUtf8Boundary {
                    reported,
                    source_len: source.len(),
                }
            }
            Self::Exact(offset) => ResolvedParseDiagnosticAnchor::Exact(offset),
            Self::EndOfInput => ResolvedParseDiagnosticAnchor::EndOfInput(source.len()),
            Self::NoSource => ResolvedParseDiagnosticAnchor::NoSource,
        }
    }

    /// Resolve an anchor for a current source only when it is the same source
    /// snapshot that produced the parse error.
    ///
    /// This exact comparison is the source-subject guard. It deliberately does
    /// not infer sameness from equal lengths or a caller-supplied generation
    /// number. Consumers that already hold generation identity should still use
    /// it to avoid unnecessary comparisons, then call this method at the final
    /// authority boundary.
    #[must_use]
    pub fn resolve_for_current(
        self,
        parsed_source: &str,
        current_source: &str,
    ) -> ResolvedParseDiagnosticAnchor {
        if matches!(self, Self::NoSource) {
            return ResolvedParseDiagnosticAnchor::NoSource;
        }
        if parsed_source != current_source {
            return ResolvedParseDiagnosticAnchor::StaleSource {
                parsed_len: parsed_source.len(),
                current_len: current_source.len(),
            };
        }
        self.resolve(parsed_source)
    }
}

impl super::ParseError {
    /// Return the parser-owned source anchor for this error.
    ///
    /// This match is intentionally exhaustive inside `perl-parser-core`. Adding
    /// a new [`super::ParseError`] variant therefore requires the parser owner to
    /// decide its source semantics before the crate compiles. Downstream crates
    /// can remain forward-compatible without a wildcard that invents byte zero.
    #[must_use]
    pub const fn diagnostic_anchor(&self) -> ParseDiagnosticAnchor {
        match self {
            Self::UnexpectedEof => ParseDiagnosticAnchor::EndOfInput,
            Self::UnexpectedToken { location, .. }
            | Self::SyntaxError { location, .. }
            | Self::Advisory { location, .. }
            | Self::Recovered { location, .. } => ParseDiagnosticAnchor::Exact(*location),
            Self::LexerError { .. }
            | Self::RecursionLimit
            | Self::InvalidNumber { .. }
            | Self::InvalidString
            | Self::UnclosedDelimiter { .. }
            | Self::InvalidRegex { .. }
            | Self::NestingTooDeep { .. }
            | Self::Cancelled => ParseDiagnosticAnchor::NoSource,
        }
    }

    /// Resolve the parser-owned diagnostic anchor against concrete source text.
    #[must_use]
    pub fn resolved_diagnostic_anchor(&self, source: &str) -> ResolvedParseDiagnosticAnchor {
        self.diagnostic_anchor().resolve(source)
    }

    /// Resolve the anchor for a current source after proving it is the same
    /// source snapshot that produced this parse error.
    #[must_use]
    pub fn resolved_diagnostic_anchor_for_current(
        &self,
        parsed_source: &str,
        current_source: &str,
    ) -> ResolvedParseDiagnosticAnchor {
        self.diagnostic_anchor().resolve_for_current(parsed_source, current_source)
    }
}

/// Error information with recovery context for comprehensive Perl parsing error handling.
///
/// This structure encapsulates all information needed for intelligent error recovery
/// in the Perl parser, enabling continued parsing after syntax errors and providing
/// detailed diagnostic information for IDE integration.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Human-readable error message describing the syntax issue
    pub message: String,
    /// Source range where the error occurred
    pub range: Range,
    /// Expected token kinds
    pub expected: Vec<String>,
    /// Actual token text
    pub found: String,
    /// Optional recovery guidance
    pub recovery_hint: Option<String>,
}

impl ParseError {
    /// Create a new parse error
    pub fn new(message: String, range: Range) -> Self {
        ParseError {
            message,
            range,
            expected: Vec::new(),
            found: String::new(),
            recovery_hint: None,
        }
    }

    /// Add expected tokens
    pub fn with_expected(mut self, expected: Vec<String>) -> Self {
        self.expected = expected;
        self
    }

    /// Add found token
    pub fn with_found(mut self, found: String) -> Self {
        self.found = found;
        self
    }

    /// Add recovery hint
    pub fn with_hint(mut self, hint: String) -> Self {
        self.recovery_hint = Some(hint);
        self
    }
}

/// Synchronization tokens for error recovery
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncPoint {
    /// Semicolon - statement boundary
    Semicolon,
    /// Closing brace - block boundary
    CloseBrace,
    /// Keywords that start statements
    Keyword,
    /// End of file
    Eof,
}

/// Result of a recovery operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryResult {
    /// Recovery succeeded, consumed the given number of tokens.
    Recovered(usize),
    /// Already at a sync point when recovery was called.
    /// The caller must decide whether to consume the sync token.
    /// This prevents infinite loops at call boundaries.
    AtSyncPoint,
    /// Recovery failed due to budget exhaustion.
    BudgetExhausted,
    /// Recovery reached EOF without finding sync point.
    ReachedEof,
}

/// Error recovery strategies
pub trait ErrorRecovery {
    /// Create an error node and recover
    fn create_error_node(
        &mut self,
        message: String,
        expected: Vec<String>,
        partial: Option<Node>,
    ) -> Node;

    /// Synchronize to a recovery point
    fn synchronize(&mut self, sync_points: &[SyncPoint]) -> bool;

    /// Try to recover from an error
    fn recover_with_node(&mut self, error: ParseError) -> Node;

    /// Skip tokens until a sync point.
    ///
    /// # Progress Invariant
    ///
    /// This method guarantees forward progress: it will consume at least one
    /// token on each call (unless already at EOF or a sync point), preventing
    /// infinite recovery loops.
    fn skip_until(&mut self, sync_points: &[SyncPoint]) -> usize;

    /// Budget-aware skip that respects limits.
    ///
    /// # Progress Invariant
    ///
    /// Consumes at least one token per call (unless at sync point, EOF, or budget exhausted).
    fn skip_until_with_budget(
        &mut self,
        sync_points: &[SyncPoint],
        budget: &ParseBudget,
        tracker: &mut BudgetTracker,
    ) -> RecoveryResult;

    /// Check if current token is a sync point
    fn is_sync_point(&self, sync_point: SyncPoint) -> bool;
}

/// Parser extensions for error recovery
pub trait ParserErrorRecovery {
    /// Parse with error recovery enabled
    fn parse_with_recovery(&mut self) -> (Node, Vec<ParseError>);

    /// Try to parse, returning an error node on failure
    fn try_parse<F>(&mut self, parse_fn: F) -> Node
    where
        F: FnOnce(&mut Self) -> Option<Node>;

    /// Parse a list with recovery on each element
    fn parse_list_with_recovery<F>(
        &mut self,
        parse_element: F,
        separator: TokenType,
        terminator: TokenType,
    ) -> Vec<Node>
    where
        F: Fn(&mut Self) -> Node;
}

/// Recovery-aware statement parsing
pub trait StatementRecovery {
    /// Parse statement with recovery
    fn parse_statement_with_recovery(&mut self) -> Node;

    /// Parse expression with recovery
    fn parse_expression_with_recovery(&mut self) -> Node;

    /// Parse block with recovery
    fn parse_block_with_recovery(&mut self) -> Node;
}

#[cfg(test)]
mod diagnostic_anchor_tests {
    use super::{ParseDiagnosticAnchor, ResolvedParseDiagnosticAnchor};
    use crate::syntax::error::{ParseError, RecoveryKind, RecoverySite};

    #[test]
    fn exact_current_variants_keep_their_parser_owned_offsets() {
        let cases = [
            ParseError::UnexpectedToken {
                expected: "expression".into(),
                found: ";".into(),
                location: 11,
            },
            ParseError::SyntaxError { message: "invalid".into(), location: 12 },
            ParseError::Advisory { message: "warning".into(), location: 13 },
            ParseError::Recovered {
                site: RecoverySite::InfixRhs,
                kind: RecoveryKind::MissingOperand,
                location: 14,
            },
        ];

        assert_eq!(cases[0].diagnostic_anchor(), ParseDiagnosticAnchor::Exact(11));
        assert_eq!(cases[1].diagnostic_anchor(), ParseDiagnosticAnchor::Exact(12));
        assert_eq!(cases[2].diagnostic_anchor(), ParseDiagnosticAnchor::Exact(13));
        assert_eq!(cases[3].diagnostic_anchor(), ParseDiagnosticAnchor::Exact(14));
    }

    #[test]
    fn eof_and_no_source_are_not_byte_zero_aliases() {
        assert_eq!(ParseError::UnexpectedEof.diagnostic_anchor(), ParseDiagnosticAnchor::EndOfInput);

        let no_source = [
            ParseError::LexerError { message: "bad byte".into() },
            ParseError::RecursionLimit,
            ParseError::InvalidNumber { literal: "1x".into() },
            ParseError::InvalidString,
            ParseError::UnclosedDelimiter { delimiter: ')' },
            ParseError::InvalidRegex { message: "bad regex".into() },
            ParseError::NestingTooDeep { depth: 5, max_depth: 4 },
            ParseError::Cancelled,
        ];
        for error in no_source {
            assert_eq!(error.diagnostic_anchor(), ParseDiagnosticAnchor::NoSource);
            assert_ne!(
                error.resolved_diagnostic_anchor("source"),
                ResolvedParseDiagnosticAnchor::Exact(0)
            );
        }
    }

    #[test]
    fn resolution_preserves_eof_and_rejects_invalid_offsets() {
        let source = "aéz";
        assert_eq!(
            ParseError::UnexpectedEof.resolved_diagnostic_anchor(source),
            ResolvedParseDiagnosticAnchor::EndOfInput(source.len())
        );
        let outside = ParseError::syntax("outside", source.len() + 1);
        assert_eq!(
            outside.resolved_diagnostic_anchor(source),
            ResolvedParseDiagnosticAnchor::InvalidOffset {
                reported: source.len() + 1,
                source_len: source.len(),
            }
        );
        let middle_of_e_acute = ParseError::syntax("inside code point", 2);
        assert_eq!(
            middle_of_e_acute.resolved_diagnostic_anchor(source),
            ResolvedParseDiagnosticAnchor::InvalidUtf8Boundary {
                reported: 2,
                source_len: source.len(),
            }
        );
        assert_eq!(
            ParseError::syntax("at eof", source.len()).resolved_diagnostic_anchor(source),
            ResolvedParseDiagnosticAnchor::Exact(source.len())
        );
    }

    #[test]
    fn current_source_resolution_rejects_stale_same_length_text() {
        let error = ParseError::syntax("anchored", 1);
        assert_eq!(
            error.resolved_diagnostic_anchor_for_current("abc", "axc"),
            ResolvedParseDiagnosticAnchor::StaleSource {
                parsed_len: 3,
                current_len: 3,
            }
        );
        assert_eq!(
            error.resolved_diagnostic_anchor_for_current("abc", "abc"),
            ResolvedParseDiagnosticAnchor::Exact(1)
        );
    }
}
