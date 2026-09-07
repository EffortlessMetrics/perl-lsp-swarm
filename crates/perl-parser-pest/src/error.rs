//! Canonical error union for `perl-parser-pest`'s public parsing API.
//!
//! [`ParseError`] is the single fallible-return error type for this crate. It
//! has exactly two arms, and the two are never interconvertible by type:
//!
//! - [`ParseError::Rejected`] wraps [`crate::StrictParseError`] — a
//!   parser-domain rejection. Pest declined the input; this is not an
//!   instrument failure.
//! - [`ParseError::Failed`] wraps [`crate::ParserFailure`] — an
//!   operational/instrument failure (parser panic, invalid UTF-8, or an
//!   internal AST-builder invariant violation). Never a parser-domain
//!   rejection, and never produced by malformed-but-otherwise-well-formed
//!   Perl source on its own.
//!
//! Both wrapped types are schema-versioned (`#[serde(deny_unknown_fields)]`
//! plus an explicit schema-string check on deserialize), so an old or
//! unrecognized serialized payload fails loudly on deserialization rather
//! than being silently misread as the wrong arm.
//!
//! # Normalization and ranges
//!
//! [`crate::PureRustPerlParser::parse`] rewrites the caller-supplied source
//! before Pest ever sees it (for example `$$name` becomes `${$name}`, and
//! `= ~expr` becomes `= bitnot(expr)`). Pest parses that *normalized* string,
//! not the caller's original source, so every [`crate::StrictParseError`]
//! produced by `parse()` carries a [`crate::SourceRange`] into the normalized
//! text. That range coincides with an offset into the caller's original
//! source only when normalization was a no-op for the affected region. Do not
//! treat a `Rejected` range from `parse()` as a caller-source byte offset
//! without first confirming normalization did not shift it.

use crate::outcome::{ParserFailure, StrictParseError};
use serde::{Deserialize, Serialize};

/// Single fallible-return error type for this crate's public parsing API.
///
/// See the [module docs](self) for the normalization/range caveat that
/// applies to `Rejected` values produced by
/// [`crate::PureRustPerlParser::parse`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum ParseError {
    /// Parser-domain rejection. Not an instrument failure.
    ///
    /// When produced by [`crate::PureRustPerlParser::parse`], the carried
    /// range refers to the *normalized* source Pest actually parsed; see the
    /// [module docs](self).
    #[error(transparent)]
    Rejected(StrictParseError),
    /// Operational/instrument failure. Never a parser-domain rejection.
    #[error(transparent)]
    Failed(ParserFailure),
}

impl From<StrictParseError> for ParseError {
    fn from(error: StrictParseError) -> Self {
        Self::Rejected(error)
    }
}

impl From<ParserFailure> for ParseError {
    fn from(failure: ParserFailure) -> Self {
        Self::Failed(failure)
    }
}

impl From<&str> for ParseError {
    /// Route a bare diagnostic string into a typed instrument failure.
    ///
    /// Every internal callsite that reaches this conversion (AST-builder
    /// helpers reporting a shape they did not expect from a Pest parse tree
    /// that already succeeded) is an internal invariant violation, never a
    /// source-domain rejection — so it is always classified as
    /// [`ParseError::Failed`].
    fn from(message: &str) -> Self {
        Self::Failed(ParserFailure::instrument(message))
    }
}

impl From<String> for ParseError {
    /// Route a bare diagnostic string into a typed instrument failure. See
    /// the `impl From<&str> for ParseError` docs above.
    fn from(message: String) -> Self {
        Self::Failed(ParserFailure::instrument(message))
    }
}

impl ParseError {
    /// Construct a parser-domain rejection.
    #[must_use]
    pub fn rejected(error: StrictParseError) -> Self {
        Self::Rejected(error)
    }

    /// Construct an operational/instrument failure.
    #[must_use]
    pub fn failed(failure: ParserFailure) -> Self {
        Self::Failed(failure)
    }

    /// The rejection, when this error is a parser-domain rejection.
    #[must_use]
    pub const fn as_rejected(&self) -> Option<&StrictParseError> {
        match self {
            Self::Rejected(error) => Some(error),
            Self::Failed(_) => None,
        }
    }

    /// The failure, when this error is an operational/instrument failure.
    #[must_use]
    pub const fn as_failed(&self) -> Option<&ParserFailure> {
        match self {
            Self::Failed(failure) => Some(failure),
            Self::Rejected(_) => None,
        }
    }
}
