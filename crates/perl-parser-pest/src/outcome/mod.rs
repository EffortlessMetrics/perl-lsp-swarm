//! Typed parse outcome, diagnostic, and original-source range vocabulary.
//!
//! This module is substrate only. It does not change
//! [`crate::PureRustPerlParser::parse`] or `parse_with_recovery`, and it does
//! not add `parse_strict`. Constructors exist so later train rows can consume
//! the types without implying that current recovery already accounts for source.
//!
//! Parser-domain completeness, rejection, and operational failure are distinct
//! types and cannot be stored in one another's success path.

mod diagnostic;
mod failure;
mod range;

pub use diagnostic::{ParseDiagnostic, ParseDiagnosticKind, RecoveryAction};
pub use failure::{
    OutcomeError, PARSER_FAILURE_SCHEMA, ParserFailure, ParserFailureKind,
    STRICT_PARSE_ERROR_SCHEMA, StrictParseError,
};
pub use range::{SourceLineColumn, SourceRange};

use failure::deserialize_expected_schema;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

/// Schema identity for serialized [`ParseOutcomeVocabulary`] documents.
pub const PARSE_OUTCOME_SCHEMA: &str = "perl-parser-pest.parse_outcome.v1";

/// Completeness of a parser-domain AST result.
///
/// `Complete`, `Recovered`, and `Unsupported` are mutually exclusive. Rejection
/// and instrument failure are different types ([`StrictParseError`] and
/// [`ParserFailure`]) and never inhabit this enum.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParseCompleteness {
    /// The parser claims a complete representation under its declared contract.
    ///
    /// This variant does not prove source accounting for the current recovery
    /// path; that remains a later train row.
    Complete,
    /// The AST is partial because recovery ran.
    Recovered,
    /// The parser does not claim to support the construct. Not malformed syntax.
    Unsupported,
}

impl ParseCompleteness {
    /// Stable kebab-case machine name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Recovered => "recovered",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Parser-domain AST result with completeness, diagnostics, and recovery ranges.
///
/// The AST payload is generic so this crate can host the vocabulary without a
/// serde contract for [`crate::AstNode`]. Current recovery is not implied to be
/// source-accounted.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutcome<T> {
    ast: T,
    completeness: ParseCompleteness,
    diagnostics: Vec<ParseDiagnostic>,
    recovery_ranges: Vec<SourceRange>,
}

/// Serializable outcome vocabulary without an AST payload.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseOutcomeVocabulary {
    #[serde(deserialize_with = "deserialize_outcome_schema")]
    schema: String,
    completeness: ParseCompleteness,
    diagnostics: Vec<ParseDiagnostic>,
    recovery_ranges: Vec<SourceRange>,
}

/// Parser-domain outcome, parser-domain rejection, or operational failure.
///
/// The three arms cannot be conflated by type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseAttempt<T> {
    /// Completeness-bearing parser-domain result.
    Outcome(ParseOutcome<T>),
    /// Strict parser-domain rejection with original Pest context.
    Rejected(StrictParseError),
    /// Operational/instrument failure.
    Failed(ParserFailure),
}

impl<T> ParseOutcome<T> {
    /// Complete result with no diagnostics and no recovery ranges.
    #[must_use]
    pub fn complete(ast: T) -> Self {
        Self {
            ast,
            completeness: ParseCompleteness::Complete,
            diagnostics: Vec::new(),
            recovery_ranges: Vec::new(),
        }
    }

    /// Fallible constructor that enforces completeness invariants.
    pub fn try_new(
        ast: T,
        completeness: ParseCompleteness,
        diagnostics: Vec<ParseDiagnostic>,
        recovery_ranges: Vec<SourceRange>,
        source: &str,
    ) -> Result<Self, OutcomeError> {
        let diagnostics = ParseDiagnostic::ordered_for_source(diagnostics, source)?;
        let recovery_ranges = SourceRange::sort_and_check_disjoint(recovery_ranges)?;
        for range in &recovery_ranges {
            range.check_over_source(source)?;
        }
        match completeness {
            ParseCompleteness::Complete => {
                if !diagnostics.is_empty() || !recovery_ranges.is_empty() {
                    return Err(OutcomeError::CompleteWithRecovery);
                }
            }
            ParseCompleteness::Recovered => {
                if diagnostics.is_empty() && recovery_ranges.is_empty() {
                    return Err(OutcomeError::RecoveredWithoutEvidence);
                }
            }
            ParseCompleteness::Unsupported => {
                if !diagnostics.iter().any(|diagnostic| {
                    matches!(diagnostic.kind(), ParseDiagnosticKind::UnsupportedSyntax)
                }) {
                    return Err(OutcomeError::UnsupportedWithoutDiagnostic);
                }
            }
        }
        Ok(Self { ast, completeness, diagnostics, recovery_ranges })
    }

    /// Recovered result. Requires diagnostics or recovery ranges.
    pub fn try_recovered(
        ast: T,
        diagnostics: Vec<ParseDiagnostic>,
        recovery_ranges: Vec<SourceRange>,
        source: &str,
    ) -> Result<Self, OutcomeError> {
        Self::try_new(ast, ParseCompleteness::Recovered, diagnostics, recovery_ranges, source)
    }

    /// Unsupported result. Requires at least one unsupported-syntax diagnostic.
    pub fn try_unsupported(
        ast: T,
        diagnostics: Vec<ParseDiagnostic>,
        recovery_ranges: Vec<SourceRange>,
        source: &str,
    ) -> Result<Self, OutcomeError> {
        Self::try_new(ast, ParseCompleteness::Unsupported, diagnostics, recovery_ranges, source)
    }

    /// AST payload.
    #[must_use]
    pub const fn ast(&self) -> &T {
        &self.ast
    }

    /// Consume the outcome and return the AST payload.
    #[must_use]
    pub fn into_ast(self) -> T {
        self.ast
    }

    /// Completeness class.
    #[must_use]
    pub const fn completeness(&self) -> ParseCompleteness {
        self.completeness
    }

    /// Deterministically ordered diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Sorted disjoint recovery ranges.
    #[must_use]
    pub fn recovery_ranges(&self) -> &[SourceRange] {
        &self.recovery_ranges
    }

    /// Serializable vocabulary record. Does not include the AST.
    #[must_use]
    pub fn vocabulary(&self) -> ParseOutcomeVocabulary {
        ParseOutcomeVocabulary {
            schema: PARSE_OUTCOME_SCHEMA.to_string(),
            completeness: self.completeness,
            diagnostics: self.diagnostics.clone(),
            recovery_ranges: self.recovery_ranges.clone(),
        }
    }
}

impl ParseOutcomeVocabulary {
    /// Schema identity.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Completeness class.
    #[must_use]
    pub const fn completeness(&self) -> ParseCompleteness {
        self.completeness
    }

    /// Deterministically ordered diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Sorted disjoint recovery ranges.
    #[must_use]
    pub fn recovery_ranges(&self) -> &[SourceRange] {
        &self.recovery_ranges
    }

    /// Reattach an AST payload after schema-aware deserialization.
    pub fn try_into_outcome<T>(
        self,
        ast: T,
        source: &str,
    ) -> Result<ParseOutcome<T>, OutcomeError> {
        ParseOutcome::try_new(
            ast,
            self.completeness,
            self.diagnostics,
            self.recovery_ranges,
            source,
        )
    }
}

impl<T> ParseAttempt<T> {
    /// Completeness-bearing parser-domain result.
    #[must_use]
    pub fn outcome(outcome: ParseOutcome<T>) -> Self {
        Self::Outcome(outcome)
    }

    /// Parser-domain rejection.
    #[must_use]
    pub fn rejected(error: StrictParseError) -> Self {
        Self::Rejected(error)
    }

    /// Operational/instrument failure.
    #[must_use]
    pub fn failed(failure: ParserFailure) -> Self {
        Self::Failed(failure)
    }

    /// Parser-domain outcome when this attempt succeeded with completeness.
    #[must_use]
    pub const fn as_outcome(&self) -> Option<&ParseOutcome<T>> {
        match self {
            Self::Outcome(outcome) => Some(outcome),
            Self::Rejected(_) | Self::Failed(_) => None,
        }
    }

    /// Parser-domain rejection when this attempt was rejected.
    #[must_use]
    pub const fn as_rejected(&self) -> Option<&StrictParseError> {
        match self {
            Self::Rejected(error) => Some(error),
            Self::Outcome(_) | Self::Failed(_) => None,
        }
    }

    /// Instrument failure when this attempt failed operationally.
    #[must_use]
    pub const fn as_failed(&self) -> Option<&ParserFailure> {
        match self {
            Self::Failed(failure) => Some(failure),
            Self::Outcome(_) | Self::Rejected(_) => None,
        }
    }
}

fn deserialize_outcome_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    deserialize_expected_schema(deserializer, PARSE_OUTCOME_SCHEMA)
}
