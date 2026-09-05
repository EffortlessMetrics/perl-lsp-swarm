//! Parser-domain rejection, operational failure, and constructor errors.

use super::range::SourceRange;
use pest::RuleType;
use pest::error::{Error as PestError, ErrorVariant, InputLocation};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Schema identity for serialized [`StrictParseError`] documents.
pub const STRICT_PARSE_ERROR_SCHEMA: &str = "perl-parser-pest.strict_parse_error.v1";

/// Schema identity for serialized [`ParserFailure`] documents.
pub const PARSER_FAILURE_SCHEMA: &str = "perl-parser-pest.parser_failure.v1";

/// Fallible vocabulary construction. These are not parse rejections.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OutcomeError {
    /// `start` is after `end`.
    #[error("source range start {start} is after end {end}")]
    InvertedRange {
        /// Requested start byte.
        start: usize,
        /// Requested exclusive end byte.
        end: usize,
    },
    /// Exclusive end exceeds the original source length.
    #[error("source range [{start}, {end}) exceeds source length {source_len}")]
    OutOfBounds {
        /// Requested start byte.
        start: usize,
        /// Requested exclusive end byte.
        end: usize,
        /// Original source length in bytes.
        source_len: usize,
    },
    /// Start or end is not a UTF-8 character boundary of the original source.
    #[error("source range [{start}, {end}) is not on a UTF-8 character boundary")]
    InvalidUtf8Boundary {
        /// Requested start byte.
        start: usize,
        /// Requested exclusive end byte.
        end: usize,
    },
    /// Recovery ranges overlap or are duplicates after sorting.
    #[error("recovery ranges overlap: {left} and {right}")]
    OverlappingRecovery {
        /// Earlier range after sort.
        left: SourceRange,
        /// Later overlapping range after sort.
        right: SourceRange,
    },
    /// Complete completeness carried diagnostics or recovery ranges.
    #[error("complete outcomes cannot carry diagnostics or recovery ranges")]
    CompleteWithRecovery,
    /// Recovered completeness had neither diagnostics nor recovery ranges.
    #[error("recovered outcomes require diagnostics or recovery ranges")]
    RecoveredWithoutEvidence,
    /// Unsupported completeness had no `unsupported-syntax` diagnostic.
    #[error("unsupported outcomes require an unsupported-syntax diagnostic")]
    UnsupportedWithoutDiagnostic,
}

impl OutcomeError {
    pub(super) fn overlapping(left: SourceRange, right: SourceRange) -> Self {
        Self::OverlappingRecovery { left, right }
    }
}

/// Parser-domain rejection with original Pest context.
///
/// This is not an operational/instrument failure. Byte ranges refer to the
/// caller-supplied original source, not Pest's derived line/column display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Error)]
#[error("{message}")]
#[serde(deny_unknown_fields)]
pub struct StrictParseError {
    #[serde(deserialize_with = "deserialize_strict_schema")]
    schema: String,
    range: SourceRange,
    message: String,
    pest_context: String,
}

/// Operational or instrument failure. Never a parser-domain rejection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Error)]
#[error("{kind}")]
#[serde(deny_unknown_fields)]
pub struct ParserFailure {
    #[serde(deserialize_with = "deserialize_failure_schema")]
    schema: String,
    kind: ParserFailureKind,
}

/// Why parsing did not produce a parser-domain outcome or rejection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Error)]
#[serde(rename_all = "kebab-case")]
pub enum ParserFailureKind {
    /// The parser process panicked.
    #[error("parser panicked: {message}")]
    Panic {
        /// Panic payload rendered as text.
        message: String,
    },
    /// Caller source was not valid UTF-8 for the `&str` parse API.
    #[error("source is not valid UTF-8: {detail}")]
    InvalidUtf8 {
        /// Decoder detail.
        detail: String,
    },
    /// Other instrument/precondition failure.
    #[error("instrument failure: {detail}")]
    Instrument {
        /// Failure detail.
        detail: String,
    },
}

impl StrictParseError {
    /// Construct a rejection already bound to original-source bytes.
    #[must_use]
    pub fn new(
        range: SourceRange,
        message: impl Into<String>,
        pest_context: impl Into<String>,
    ) -> Self {
        Self {
            schema: STRICT_PARSE_ERROR_SCHEMA.to_string(),
            range,
            message: message.into(),
            pest_context: pest_context.into(),
        }
    }

    /// Map a Pest error onto the caller-supplied original source.
    ///
    /// Pest line/column display is preserved only as `pest_context`. The range is
    /// Pest's byte location checked against `original_source`.
    pub fn from_pest<R: RuleType>(
        error: &PestError<R>,
        original_source: &str,
    ) -> Result<Self, OutcomeError> {
        let range = match error.location {
            InputLocation::Pos(pos) => SourceRange::try_over_source(pos, pos, original_source)?,
            InputLocation::Span((start, end)) => {
                SourceRange::try_over_source(start, end, original_source)?
            }
        };
        let message = match &error.variant {
            ErrorVariant::ParsingError { positives, negatives } => {
                format!("parsing error (expected: {positives:?}, unexpected: {negatives:?})")
            }
            ErrorVariant::CustomError { message } => message.clone(),
        };
        Ok(Self::new(range, message, error.to_string()))
    }

    /// Schema identity.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Original-source byte range of the rejection.
    #[must_use]
    pub const fn range(&self) -> SourceRange {
        self.range
    }

    /// Short parser-domain message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Original Pest `Display` context, including Pest's own line/column rendering.
    #[must_use]
    pub fn pest_context(&self) -> &str {
        &self.pest_context
    }
}

impl ParserFailure {
    /// Construct an operational failure.
    #[must_use]
    pub fn new(kind: ParserFailureKind) -> Self {
        Self { schema: PARSER_FAILURE_SCHEMA.to_string(), kind }
    }

    /// Parser panic payload.
    #[must_use]
    pub fn panic(message: impl Into<String>) -> Self {
        Self::new(ParserFailureKind::Panic { message: message.into() })
    }

    /// Invalid UTF-8 instrument failure.
    #[must_use]
    pub fn invalid_utf8(detail: impl Into<String>) -> Self {
        Self::new(ParserFailureKind::InvalidUtf8 { detail: detail.into() })
    }

    /// Generic instrument failure.
    #[must_use]
    pub fn instrument(detail: impl Into<String>) -> Self {
        Self::new(ParserFailureKind::Instrument { detail: detail.into() })
    }

    /// Schema identity.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Failure class.
    #[must_use]
    pub const fn kind(&self) -> &ParserFailureKind {
        &self.kind
    }
}

fn deserialize_strict_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    deserialize_expected_schema(deserializer, STRICT_PARSE_ERROR_SCHEMA)
}

fn deserialize_failure_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    deserialize_expected_schema(deserializer, PARSER_FAILURE_SCHEMA)
}

pub(super) fn deserialize_expected_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
    expected: &'static str,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value == expected {
        Ok(value)
    } else {
        Err(D::Error::custom(format!("unsupported parse-outcome schema {value}")))
    }
}
