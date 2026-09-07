//! Canonical error union for `perl-parser-pest`'s public parsing API.
//!
//! [`ParseError`] is the single error type returned by this crate's *parsing*
//! APIs — every fallible function in [`crate::pure_rust_parser`] and
//! [`crate::pratt_parser`]. It has exactly two arms, and the two are never
//! interconvertible by type:
//!
//! - [`ParseError::Rejected`] wraps [`crate::StrictParseError`] — a
//!   parser-domain rejection. Pest declined the input; this is not an
//!   instrument failure.
//! - [`ParseError::Failed`] wraps [`crate::ParserFailure`] — an
//!   operational/instrument failure. Never a parser-domain rejection, and
//!   never produced by malformed-but-otherwise-well-formed Perl source on
//!   its own.
//!
//! It is deliberately not the crate's *only* error type. Vocabulary
//! construction — [`crate::SourceRange`], [`crate::ParseOutcome`], and
//! [`crate::StrictParseError::from_pest`] — returns [`crate::OutcomeError`],
//! which reports an invalid request to build a value and is not a parse
//! result at all. Parsing errors and vocabulary-construction errors stay
//! separate types on purpose.
//!
//! # Which failure kinds `parse()` can actually produce
//!
//! [`crate::ParserFailureKind`] also models `Panic` and `InvalidUtf8`, but
//! [`crate::PureRustPerlParser::parse`] produces neither. It takes `&str`, so
//! its input is already valid UTF-8, and it does not catch unwinds — a
//! panic in the parser unwinds past this type rather than becoming a
//! `Failed` value, and callers that need to contain one (such as
//! `perl-parser-comparison`'s harness) still wrap the call in
//! [`std::panic::catch_unwind`] themselves. In practice every `Failed` that
//! `parse()` returns carries [`crate::ParserFailureKind::Instrument`],
//! reporting an internal AST-builder invariant violation.
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
//! `= ~expr` becomes `= bitnot(expr)`), which shifts every byte after a
//! rewrite. Pest's offsets therefore index the rewritten buffer, and are
//! translated back through that rewrite before they reach
//! [`ParseError::Rejected`]. A rejection's [`crate::SourceRange`] is an offset
//! into the source the caller passed in — which is what
//! [`crate::StrictParseError`] documents, and what a consumer highlighting or
//! slicing the caller's own text needs.
//!
//! One boundary is worth naming: when the reported offset falls *inside* a
//! rewritten region, it resolves to the start of the span that region was
//! rewritten from. Those bytes have no finer-grained original to point at, and
//! the span start is the token the caller actually wrote.
//!
//! Pest's own rendering is retained verbatim in
//! [`crate::StrictParseError::pest_context`] and still describes the rewritten
//! buffer. It is diagnostic context, never the range authority.

use crate::outcome::{ParserFailure, StrictParseError};
use serde::{Deserialize, Serialize};

/// Error type returned by this crate's public parsing API.
///
/// See the [module docs](self) for how this relates to
/// [`crate::OutcomeError`], and for the normalization/range handling that
/// applies to `Rejected` values produced by
/// [`crate::PureRustPerlParser::parse`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum ParseError {
    /// Parser-domain rejection. Not an instrument failure.
    ///
    /// The carried range is an offset into the source the caller supplied,
    /// even when `parse()` rewrote that source before Pest saw it; see the
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
