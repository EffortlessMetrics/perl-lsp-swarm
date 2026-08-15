//! Diagnostic types: [`ByteSpan`], [`Diagnostic`], and [`RelatedInformation`].
//!
//! This module contains the transport-neutral diagnostic message types used by
//! analyzers and adapters. Source locations are half-open UTF-8 byte spans;
//! line, column, URI, and negotiated position-encoding policy belong to the
//! consuming transport or source-snapshot layer.

use std::fmt;
use std::ops::Range;

// Unified types — canonical definitions are in codes module.
pub use crate::codes::{DiagnosticSeverity, DiagnosticTag};

/// An invalid half-open UTF-8 byte span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidByteSpan {
    start: usize,
    end: usize,
}

impl InvalidByteSpan {
    /// Return the rejected start offset.
    pub const fn start(self) -> usize {
        self.start
    }

    /// Return the rejected end offset.
    pub const fn end(self) -> usize {
        self.end
    }
}

impl fmt::Display for InvalidByteSpan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "byte span start {} exceeds end {}", self.start, self.end)
    }
}

impl std::error::Error for InvalidByteSpan {}

/// A validated half-open UTF-8 byte interval `[start, end)`.
///
/// `ByteSpan` validates only interval ordering. Whether the offsets are within
/// a particular source snapshot and fall on UTF-8 scalar boundaries must be
/// checked by the consumer that owns that exact source.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteSpan {
    start: usize,
    end: usize,
}

impl ByteSpan {
    /// The deliberate empty span at byte zero used by compatibility defaults.
    ///
    /// New diagnostic producers should construct the exact span they observed
    /// rather than relying on this compatibility value.
    pub const EMPTY: Self = Self { start: 0, end: 0 };

    /// Construct a validated half-open byte span.
    pub const fn new(start: usize, end: usize) -> Result<Self, InvalidByteSpan> {
        if start <= end { Ok(Self { start, end }) } else { Err(InvalidByteSpan { start, end }) }
    }

    /// Return the inclusive start byte offset.
    pub const fn start(self) -> usize {
        self.start
    }

    /// Return the exclusive end byte offset.
    pub const fn end(self) -> usize {
        self.end
    }

    /// Return the byte length of the interval.
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Return whether the interval is zero-width.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Return whether the half-open interval contains `offset`.
    pub const fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Return whether this interval fully contains `other`.
    pub const fn contains_span(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Return whether two non-empty half-open intervals overlap.
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Return the non-empty intersection of two half-open intervals.
    pub const fn intersection(self, other: Self) -> Option<Self> {
        let start = if self.start >= other.start { self.start } else { other.start };
        let end = if self.end <= other.end { self.end } else { other.end };

        if start < end { Some(Self { start, end }) } else { None }
    }

    /// Convert this span into a standard byte range.
    pub fn to_range(self) -> Range<usize> {
        self.start..self.end
    }
}

impl TryFrom<(usize, usize)> for ByteSpan {
    type Error = InvalidByteSpan;

    fn try_from((start, end): (usize, usize)) -> Result<Self, Self::Error> {
        Self::new(start, end)
    }
}

impl TryFrom<Range<usize>> for ByteSpan {
    type Error = InvalidByteSpan;

    fn try_from(range: Range<usize>) -> Result<Self, Self::Error> {
        Self::new(range.start, range.end)
    }
}

impl From<ByteSpan> for Range<usize> {
    fn from(span: ByteSpan) -> Self {
        span.to_range()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ByteSpan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("ByteSpan", 2)?;
        state.serialize_field("start", &self.start)?;
        state.serialize_field("end", &self.end)?;
        state.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ByteSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct ByteSpanRepresentation {
            start: usize,
            end: usize,
        }

        let representation =
            <ByteSpanRepresentation as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(representation.start, representation.end).map_err(serde::de::Error::custom)
    }
}

/// A diagnostic message with a byte span and semantic metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Diagnostic {
    /// The diagnostic code (for example, `PL001`).
    pub code: crate::codes::DiagnosticCode,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Half-open UTF-8 byte span of the diagnostic.
    pub range: ByteSpan,
    /// The message text.
    pub message: String,
    /// Optional related information.
    pub related_information: Option<Vec<RelatedInformation>>,
    /// Optional tags (`Unnecessary`, `Deprecated`, and so on).
    pub tags: Option<Vec<DiagnosticTag>>,
}

impl Default for Diagnostic {
    fn default() -> Self {
        Self {
            code: crate::codes::DiagnosticCode::default(),
            severity: DiagnosticSeverity::default(),
            range: ByteSpan::EMPTY,
            message: String::new(),
            related_information: None,
            tags: None,
        }
    }
}

impl Diagnostic {
    /// Create a diagnostic with the given code, severity, span, and message.
    pub fn new(
        code: crate::codes::DiagnosticCode,
        severity: DiagnosticSeverity,
        range: ByteSpan,
        message: impl Into<String>,
    ) -> Self {
        Self { code, severity, range, message: message.into(), ..Default::default() }
    }

    /// Validate a byte interval and create a diagnostic.
    pub fn try_new(
        code: crate::codes::DiagnosticCode,
        severity: DiagnosticSeverity,
        start: usize,
        end: usize,
        message: impl Into<String>,
    ) -> Result<Self, InvalidByteSpan> {
        Ok(Self::new(code, severity, ByteSpan::new(start, end)?, message))
    }
}

/// Information related to a diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct RelatedInformation {
    /// The message text.
    pub message: String,
    /// Half-open UTF-8 byte span of the related source.
    pub location: ByteSpan,
}

impl Default for RelatedInformation {
    fn default() -> Self {
        Self { message: String::new(), location: ByteSpan::EMPTY }
    }
}

impl RelatedInformation {
    /// Create a new related-information entry.
    pub fn new(message: impl Into<String>, location: ByteSpan) -> Self {
        Self { message: message.into(), location }
    }

    /// Validate a byte interval and create a related-information entry.
    pub fn try_new(
        message: impl Into<String>,
        start: usize,
        end: usize,
    ) -> Result<Self, InvalidByteSpan> {
        Ok(Self::new(message, ByteSpan::new(start, end)?))
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteSpan, InvalidByteSpan};

    #[test]
    fn byte_span_accepts_ordered_and_empty_intervals() -> Result<(), InvalidByteSpan> {
        let non_empty = ByteSpan::new(3, 7)?;
        let empty = ByteSpan::new(5, 5)?;

        assert_eq!(non_empty.start(), 3);
        assert_eq!(non_empty.end(), 7);
        assert_eq!(non_empty.len(), 4);
        assert!(!non_empty.is_empty());
        assert!(empty.is_empty());
        Ok(())
    }

    #[test]
    fn byte_span_rejects_reversed_intervals() {
        let error = ByteSpan::new(7, 3);

        assert_eq!(error, Err(InvalidByteSpan { start: 7, end: 3 }));
    }

    #[test]
    fn byte_span_handles_boundaries_without_arithmetic_overflow() -> Result<(), InvalidByteSpan> {
        let maximum = ByteSpan::new(usize::MAX, usize::MAX)?;
        let whole_prefix = ByteSpan::new(0, usize::MAX)?;

        assert_eq!(maximum.len(), 0);
        assert_eq!(whole_prefix.len(), usize::MAX);
        Ok(())
    }

    #[test]
    fn byte_span_contains_and_intersects_half_open_intervals() -> Result<(), InvalidByteSpan> {
        let outer = ByteSpan::new(2, 10)?;
        let nested = ByteSpan::new(4, 6)?;
        let adjacent = ByteSpan::new(10, 12)?;
        let overlapping = ByteSpan::new(8, 12)?;

        assert!(outer.contains(2));
        assert!(!outer.contains(10));
        assert!(outer.contains_span(nested));
        assert!(!outer.overlaps(adjacent));
        assert_eq!(outer.intersection(adjacent), None);
        assert_eq!(outer.intersection(overlapping), Some(ByteSpan::new(8, 10)?));
        Ok(())
    }

    #[cfg(feature = "serde")]
    #[test]
    fn byte_span_serde_names_fields_and_rejects_reversal() -> Result<(), Box<dyn std::error::Error>>
    {
        let span = ByteSpan::new(3, 7)?;
        let serialized = serde_json::to_string(&span)?;

        assert_eq!(serialized, r#"{"start":3,"end":7}"#);
        assert_eq!(serde_json::from_str::<ByteSpan>(&serialized)?, span);
        assert!(serde_json::from_str::<ByteSpan>(r#"{"start":7,"end":3}"#).is_err());
        Ok(())
    }
}
